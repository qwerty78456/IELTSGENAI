//! Turning a `Passage` into PCM.
//!
//! Gemini 3.8 TTS reads its input word for word and takes at most two voices
//! per request. `plan_passage` turns a passage into requests: consecutive
//! lines of one speaker in one style become one turn, and turns are cut into
//! chunks of at most `MAX_WORDS_PER_REQUEST` words and two speakers. A voice
//! that reads alone (a designed voice, or every voice under `PER_TURN_ALL`)
//! never shares a request. Every chunk uses one voice table, the speakers'
//! own voices, so a speaker sounds the same throughout; inside a request the
//! voices are listed in label order. Chunks are synthesised a few at a time
//! and joined, in order, with short gaps.
//!
//! Short chunks keep every request well inside the API's time and token
//! limits, a failure costs one chunk rather than the passage, and an edit
//! invalidates only its own chunk in the speech cache. A three-voice passage
//! (the HSG interview) is simply chunked so that no chunk has a third voice.
//!
//! Each turn is sent as `speech::speech_text`: documented speech tags
//! (`<sigh>`) are kept and performed, anything the model would read aloud by
//! mistake (unknown tags, `[notes]`, stray pipes) is dropped, and
//! `|backchannels|` survive only in a two-voice request on a model that
//! performs them. A tag is one token wherever a turn is cut and never counts
//! towards the word budget.

use futures_util::{StreamExt, TryStreamExt, stream};

use crate::domain::speech::{self, Token};
use crate::domain::{
    DomainError, Passage, SpeakerConfig, SpeakerRole, Voice, VoiceSource, voice_conflicts,
};

use super::super::audio::{Pcm16, SAMPLE_RATE};
use super::super::config::{TTS_MAX_INPUT_TOKENS, config};
use super::super::llm::{GeminiClient, LlmError, SpeechRequest, SpeechTurn, VoiceAssignment};
use super::cache::{self, Reuse};

/// Gemini limit for multi-speaker synthesis.
pub const MAX_MULTI_SPEAKER_VOICES: usize = 2;
/// Words per speech request: about 80 s of audio at exam pace, which 3.8
/// Flash TTS reads in roughly 30 s (measured 2.5x real time), well inside the
/// minute a normal request stays open and far from the 8,192 input and
/// 16,384 output tokens a request allows.
const MAX_WORDS_PER_REQUEST: usize = 200;
/// Silence between two chunks of one speaker (a long turn cut at a sentence end).
const CHUNK_GAP_MS: u32 = 350;
/// Silence between two speakers' turns read in separate requests.
pub const TURN_GAP_MS: u32 = 250;
/// Chunks of one passage synthesised at the same time; their order is kept.
/// `GeminiClient` caps the speech requests of the whole process on top.
const PARALLEL_CHUNKS: usize = 2;
/// The pace every exam voice keeps. Delivery directions go in each turn's
/// style; the text itself is read verbatim, so they never go there.
pub const EXAM_PACE: &str = "clear, at a steady exam pace";
const ANNOUNCEMENT_STYLE: &str = "slow and clear, like an exam announcer";
/// Decision gate G1/G2 (`docs/voices.md`): true makes every voice read one
/// speaker per request, as designed voices always do.
pub const PER_TURN_ALL: bool = false;
/// Decision gate G5: true gives each turn its speaker's role style; false
/// gives every turn `EXAM_PACE` only.
pub const PER_TURN_STYLE: bool = true;

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error(transparent)]
    Llm(#[from] LlmError),
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("{0} has no voice yet. Choose a voice for it and try again.")]
    NoVoice(String),
    /// Two speakers on one voice, or a voice of the other gender (the
    /// teacher-readable text of `voice_conflicts`).
    #[error("{0}")]
    SharedVoice(String),
    /// Synthesised speech could not be written to `DATA_DIR`.
    #[error("The recording could not be saved: {0}")]
    Storage(String),
    /// Something this server will not do; the text says why.
    #[error("{0}")]
    Refused(String),
}

/// One stretch of one speaker in one style.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Turn {
    speaker: String,
    text: String,
    style: String,
}

/// A passage as speech requests, in order. `gaps_ms[i]` is the silence put
/// before request `i` (0 for the first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassagePlan {
    pub requests: Vec<SpeechRequest>,
    pub gaps_ms: Vec<u32>,
}

/// The style of every turn of a speaker: short and the same throughout, as
/// long or changing styles make a voice drift.
pub fn speaker_style(role: &SpeakerRole) -> String {
    if PER_TURN_STYLE {
        format!("{}, {EXAM_PACE}", role.delivery_style())
    } else {
        EXAM_PACE.to_string()
    }
}

/// Whether a voice must have each of its turns to itself. Designed voices
/// cannot join a two-voice request (Google: synthesise each turn on its own).
pub fn reads_alone(voice: &Voice) -> bool {
    PER_TURN_ALL || voice.source == VoiceSource::Designed || Voice::is_designed_id(&voice.id)
}

/// Whether a TTS model performs `|backchannels|`: 3.8 Flash TTS does, the
/// lite model is not documented to (Google: "works best with
/// gemini-3.8-flash-tts").
pub fn performs_backchannels(model: &str) -> bool {
    !model.contains("lite")
}

/// Synthesises a whole passage with each speaker's own voice. Returns 24 kHz
/// mono PCM. `reuse` decides whether earlier takes of a chunk may be reused.
pub async fn synthesize_passage(
    client: &GeminiClient,
    passage: &Passage,
    speakers: &[SpeakerConfig],
    reuse: Reuse,
) -> Result<Pcm16, TtsError> {
    if let Some(conflict) = voice_conflicts(speakers).into_iter().next() {
        return Err(TtsError::SharedVoice(conflict));
    }
    let plan = plan_passage(passage, speakers, client.tts_model())?;
    // The futures are made up front: a stream that maps with a closure here
    // is not provably `Send` for every lifetime, which `tokio::spawn` needs.
    let requests: Vec<_> = plan
        .requests
        .iter()
        .map(|request| cache::speak(client, request, reuse))
        .collect();
    let pieces: Vec<Pcm16> = stream::iter(requests)
        .buffered(PARALLEL_CHUNKS)
        .try_collect()
        .await?;
    let mut out = Pcm16::silence(0, SAMPLE_RATE);
    for (pcm, gap_ms) in pieces.iter().zip(&plan.gaps_ms) {
        if *gap_ms > 0 {
            out.append(&Pcm16::silence(*gap_ms, SAMPLE_RATE));
        }
        out.append(pcm);
    }
    Ok(out)
}

/// The announcer voice reading an instruction.
pub async fn synthesize_announcement(client: &GeminiClient, text: &str) -> Result<Pcm16, TtsError> {
    let request = SpeechRequest {
        turns: vec![SpeechTurn {
            speaker: None,
            text: text.to_string(),
            style: ANNOUNCEMENT_STYLE.into(),
        }],
        voices: vec![VoiceAssignment {
            label: "Announcer".into(),
            voice: config().voices.announcer().id.clone(),
        }],
    };
    Ok(cache::speak(client, &request, Reuse::Allow).await?)
}

impl super::super::audio::Announcer for GeminiClient {
    async fn speak(&self, text: &str) -> Result<Pcm16, TtsError> {
        synthesize_announcement(self, text).await
    }
}

/// The requests that read `passage` on the TTS `model`, without sending any.
/// Every speaker who speaks needs a voice (`NoVoice` otherwise); the same
/// voice table serves every chunk. A turn with nothing left to say once its
/// markup is cleaned (a lone `[music]`) is skipped.
pub fn plan_passage(
    passage: &Passage,
    speakers: &[SpeakerConfig],
    model: &str,
) -> Result<PassagePlan, TtsError> {
    let mut table: Vec<(&SpeakerConfig, &Voice)> = Vec::new();
    for label in passage.speakers_used() {
        let speaker = speakers
            .iter()
            .find(|s| s.label == label)
            .ok_or_else(|| TtsError::NoVoice(label.clone()))?;
        let voice = speaker
            .voice
            .voice()
            .ok_or_else(|| TtsError::NoVoice(label.clone()))?;
        table.push((speaker, voice));
    }
    table.sort_by(|a, b| a.0.label.cmp(&b.0.label));
    let assignments: Vec<VoiceAssignment> = table
        .iter()
        .map(|(speaker, voice)| VoiceAssignment {
            label: speaker.label.clone(),
            voice: voice.id.clone(),
        })
        .collect();
    let mut turns = Vec::with_capacity(passage.lines.len());
    for line in &passage.lines {
        let (speaker, _) = table
            .iter()
            .find(|(s, _)| s.label == line.speaker)
            .ok_or_else(|| TtsError::NoVoice(line.speaker.clone()))?;
        // Backchannels are kept here and dropped per chunk (`chunk_request`).
        let text = line.speech_text(true);
        if text.is_empty() {
            continue;
        }
        turns.push(Turn {
            speaker: line.speaker.clone(),
            text,
            style: speaker_style(&speaker.role),
        });
    }
    let alone = |label: &str| {
        table
            .iter()
            .any(|(speaker, voice)| speaker.label == label && reads_alone(voice))
    };
    let chunks = speech_chunks(&merge_turns(&turns), MAX_WORDS_PER_REQUEST, alone);
    let backchannels = performs_backchannels(model);
    let requests = chunks
        .iter()
        .map(|chunk| chunk_request(chunk, &assignments, backchannels))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PassagePlan {
        requests,
        gaps_ms: gaps(&chunks),
    })
}

/// The silence before each chunk: `CHUNK_GAP_MS` when one speaker goes on,
/// `TURN_GAP_MS` when another speaker starts; none before the first.
fn gaps(chunks: &[Vec<Turn>]) -> Vec<u32> {
    chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            let Some(previous) = index.checked_sub(1).map(|i| &chunks[i]) else {
                return 0;
            };
            let goes_on = previous.last().map(|t| &t.speaker) == chunk.first().map(|t| &t.speaker);
            if goes_on { CHUNK_GAP_MS } else { TURN_GAP_MS }
        })
        .collect()
}

/// One request for a chunk: the voices of the speakers in it, in label
/// order (as `assignments` is), and one turn per stretch. A one-voice chunk
/// names no speaker. `|backchannels|` stay only in a two-voice chunk and
/// only when `backchannels` (the model performs them); elsewhere a listener
/// would hear nobody answer, so they are dropped.
fn chunk_request(
    chunk: &[Turn],
    assignments: &[VoiceAssignment],
    backchannels: bool,
) -> Result<SpeechRequest, TtsError> {
    for turn in chunk {
        if assignments.iter().all(|a| a.label != turn.speaker) {
            return Err(TtsError::NoVoice(turn.speaker.clone()));
        }
    }
    let voices: Vec<VoiceAssignment> = assignments
        .iter()
        .filter(|a| chunk.iter().any(|t| t.speaker == a.label))
        .cloned()
        .collect();
    let one_voice = voices.len() == 1;
    let keep_backchannels = backchannels && voices.len() == MAX_MULTI_SPEAKER_VOICES;
    let turns: Vec<SpeechTurn> = chunk
        .iter()
        .map(|turn| SpeechTurn {
            speaker: (!one_voice).then(|| turn.speaker.clone()),
            text: if keep_backchannels {
                turn.text.clone()
            } else {
                speech::speech_text(&turn.text, false)
            },
            style: turn.style.clone(),
        })
        .collect();
    let tokens: Vec<Token> = turns.iter().flat_map(|t| speech::tokens(&t.text)).collect();
    let words = tokens.iter().filter(|t| t.spoken).count();
    let markup = tokens.len() - words;
    let style_words: usize = chunk
        .iter()
        .map(|t| t.style.split_whitespace().count())
        .sum();
    if estimate_tokens(words, markup, style_words, turns.len()) >= TTS_MAX_INPUT_TOKENS {
        return Err(TtsError::Llm(LlmError::Malformed(
            "a speech chunk is longer than one request allows".into(),
        )));
    }
    Ok(SpeechRequest { turns, voices })
}

/// Rough input-token estimate: about 1.4 tokens per English word, text and
/// styles alike, a few per speech tag, plus the per-turn annotation, with
/// headroom.
fn estimate_tokens(words: usize, markup: usize, style_words: usize, turns: usize) -> usize {
    ((words + style_words) as f32 * 1.4) as usize + markup * 4 + turns * 16 + 64
}

/// Cuts turns into request-sized chunks. A turn longer than `max_words` is
/// split at sentence ends (at word boundaries for a sentence that is itself
/// too long). Speech tags never count as words. A chunk never exceeds
/// `max_words` words or
/// `MAX_MULTI_SPEAKER_VOICES` speakers, a speaker for whom `alone` holds
/// never shares one, and the order of the words never changes.
fn speech_chunks(turns: &[Turn], max_words: usize, alone: impl Fn(&str) -> bool) -> Vec<Vec<Turn>> {
    let max_words = max_words.max(1);
    let mut chunks: Vec<Vec<Turn>> = Vec::new();
    let mut current: Vec<Turn> = Vec::new();
    let mut current_words = 0;
    for turn in turns {
        for piece in split_turn(&turn.text, max_words) {
            let words = speech::spoken_words(&piece);
            let new_speaker = current.iter().all(|t| t.speaker != turn.speaker);
            let speakers = current
                .iter()
                .map(|t| &t.speaker)
                .fold(Vec::new(), |mut seen, s| {
                    if !seen.contains(&s) {
                        seen.push(s);
                    }
                    seen
                })
                .len();
            let kept_apart =
                new_speaker && (alone(&turn.speaker) || current.iter().any(|t| alone(&t.speaker)));
            let fits = current_words + words <= max_words
                && !(new_speaker && speakers >= MAX_MULTI_SPEAKER_VOICES)
                && !kept_apart;
            if !current.is_empty() && !fits {
                chunks.push(std::mem::take(&mut current));
                current_words = 0;
            }
            match current.last_mut() {
                Some(last) if last.speaker == turn.speaker && last.style == turn.style => {
                    last.text.push(' ');
                    last.text.push_str(&piece);
                }
                _ => current.push(Turn {
                    text: piece,
                    ..turn.clone()
                }),
            }
            current_words += words;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// A turn as pieces of at most `max_words` spoken words, broken after
/// sentence ends where possible. Speech tags and backchannels are tokens of
/// their own (`speech::tokens`): never cut, never counted, kept with the
/// word before them. A turn that fits is returned unchanged.
fn split_turn(text: &str, max_words: usize) -> Vec<String> {
    let tokens = speech::tokens(text);
    let spoken = |tokens: &[Token]| tokens.iter().filter(|t| t.spoken).count();
    if spoken(&tokens) <= max_words {
        return vec![text.trim().to_string()];
    }
    let join = |tokens: &[Token]| tokens.iter().map(|t| t.text).collect::<Vec<_>>().join(" ");
    let mut sentences: Vec<Vec<Token>> = vec![Vec::new()];
    let mut ended = false;
    for token in tokens {
        // Markup after a sentence end stays with that sentence, so a piece
        // never opens with a tag (where the model tends to drop it).
        if ended && token.spoken {
            sentences.push(Vec::new());
        }
        sentences.last_mut().expect("never empty").push(token);
        if token.spoken {
            let end = token.text.trim_end_matches(['"', '\'', ')', ']', '”', '’']);
            ended = end.ends_with(['.', '?', '!']);
        }
    }
    let mut pieces: Vec<String> = Vec::new();
    let mut piece: Vec<Token> = Vec::new();
    for sentence in sentences.into_iter().filter(|s| !s.is_empty()) {
        let words = spoken(&sentence);
        if !piece.is_empty() && spoken(&piece) + words > max_words {
            pieces.push(join(&piece));
            piece.clear();
        }
        if words > max_words {
            for token in sentence {
                if token.spoken && spoken(&piece) == max_words {
                    pieces.push(join(&piece));
                    piece.clear();
                }
                piece.push(token);
            }
        } else {
            piece.extend(sentence);
        }
    }
    if !piece.is_empty() {
        pieces.push(join(&piece));
    }
    pieces
}

/// Consecutive turns by the same speaker in the same style become one turn.
fn merge_turns(turns: &[Turn]) -> Vec<Turn> {
    let mut merged: Vec<Turn> = Vec::new();
    for turn in turns {
        match merged.last_mut() {
            Some(last) if last.speaker == turn.speaker && last.style == turn.style => {
                last.text.push(' ');
                last.text.push_str(&turn.text);
            }
            _ => merged.push(turn.clone()),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Accent, Gender, Line, VoiceChoice};

    const MODEL: &str = "gemini-3.8-flash-tts";

    fn turn(s: &str, t: &str) -> Turn {
        Turn {
            speaker: s.into(),
            text: t.into(),
            style: "calm".into(),
        }
    }

    fn words_of<'a>(texts: impl Iterator<Item = &'a str>) -> Vec<String> {
        texts
            .flat_map(|t| t.split_whitespace().map(str::to_string).collect::<Vec<_>>())
            .collect()
    }

    fn words(chunks: &[Vec<Turn>]) -> Vec<String> {
        words_of(chunks.iter().flatten().map(|t| t.text.as_str()))
    }

    fn no_one_alone(_: &str) -> bool {
        false
    }

    fn voiced(label: &str, gender: Gender, role: SpeakerRole, id: &str) -> SpeakerConfig {
        let source = if Voice::is_designed_id(id) {
            VoiceSource::Designed
        } else {
            VoiceSource::Library
        };
        SpeakerConfig {
            voice: VoiceChoice::Assigned(Voice {
                id: id.into(),
                name: id.into(),
                gender,
                accent: Accent::British,
                source,
                description: String::new(),
            }),
            ..SpeakerConfig::new(label, gender, Accent::British, role)
        }
    }

    fn passage(lines: &[(&str, &str)]) -> Passage {
        Passage {
            part: 1,
            topic: "test".into(),
            lines: lines
                .iter()
                .map(|(speaker, text)| Line::new(*speaker, *text))
                .collect(),
            written_for: Vec::new(),
        }
    }

    fn plan_words(plan: &PassagePlan) -> Vec<String> {
        words_of(
            plan.requests
                .iter()
                .flat_map(|r| r.turns.iter().map(|t| t.text.as_str())),
        )
    }

    #[test]
    fn merges_consecutive_turns() {
        let merged = merge_turns(&[turn("A", "one"), turn("A", "two"), turn("B", "three")]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].text, "one two");
        // Another style is another stretch, even for the same speaker.
        let mut excited = turn("A", "two");
        excited.style = "excited".into();
        assert_eq!(merge_turns(&[turn("A", "one"), excited]).len(), 2);
    }

    #[test]
    fn short_dialogue_is_one_chunk() {
        let turns = [
            turn("A", "Hi there."),
            turn("B", "Hello."),
            turn("A", "Bye."),
        ];
        let chunks = speech_chunks(&turns, 250, no_one_alone);
        assert_eq!(chunks, vec![turns.to_vec()]);
    }

    #[test]
    fn chunks_respect_the_word_budget_and_keep_every_word() {
        let turns: Vec<Turn> = (0..12)
            .map(|i| {
                turn(
                    if i % 2 == 0 { "A" } else { "B" },
                    "one two three four five.",
                )
            })
            .collect();
        let chunks = speech_chunks(&turns, 12, no_one_alone);
        for chunk in &chunks {
            let count: usize = chunk
                .iter()
                .map(|t| t.text.split_whitespace().count())
                .sum();
            assert!(count <= 12, "{chunk:?}");
        }
        assert_eq!(chunks.len(), 6);
        assert_eq!(words(&chunks), words(&[turns]));
    }

    #[test]
    fn a_third_speaker_starts_a_new_chunk() {
        let turns = [
            turn("Speaker A", "Welcome."),
            turn("Speaker B", "Thanks."),
            turn("Speaker C", "Hello."),
            turn("Speaker A", "Right."),
        ];
        let chunks = speech_chunks(&turns, 250, no_one_alone);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2);
        assert_eq!(chunks[1][0].speaker, "Speaker C");
        for chunk in &chunks {
            let mut speakers: Vec<&str> = chunk.iter().map(|t| t.speaker.as_str()).collect();
            speakers.sort();
            speakers.dedup();
            assert!(speakers.len() <= MAX_MULTI_SPEAKER_VOICES);
        }
    }

    #[test]
    fn a_long_monologue_is_split_at_sentence_ends() {
        let sentence = "The museum opens at nine and closes at five every day.";
        let lecture = vec![sentence; 30].join(" ");
        let chunks = speech_chunks(&[turn("Speaker A", &lecture)], 50, no_one_alone);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert_eq!(chunk.len(), 1);
            assert!(chunk[0].text.ends_with("day."), "{}", chunk[0].text);
            assert!(chunk[0].text.split_whitespace().count() <= 50);
        }
        assert_eq!(words(&chunks).join(" "), lecture);
    }

    #[test]
    fn a_sentence_longer_than_the_budget_is_split_at_words() {
        let run_on = vec!["and"; 25].join(" ");
        let pieces = split_turn(&run_on, 10);
        assert_eq!(pieces.len(), 3);
        assert_eq!(
            pieces
                .iter()
                .map(|p| p.split_whitespace().count())
                .sum::<usize>(),
            25
        );
    }

    #[test]
    fn requests_name_speakers_only_with_two_voices() {
        let assignments = vec![
            VoiceAssignment {
                label: "Speaker A".into(),
                voice: "en-gb-advisor-1".into(),
            },
            VoiceAssignment {
                label: "Speaker B".into(),
                voice: "en-gb-assistant-2".into(),
            },
        ];
        let solo = chunk_request(&[turn("Speaker B", "Just me.")], &assignments, true).unwrap();
        assert_eq!(solo.voices.len(), 1);
        assert_eq!(solo.voices[0].voice, "en-gb-assistant-2");
        assert_eq!(solo.turns[0].speaker, None);
        assert_eq!(solo.turns[0].style, "calm");
        let pair = chunk_request(
            &[
                turn("Speaker B", "You first."),
                turn("Speaker A", "Thanks."),
            ],
            &assignments,
            true,
        )
        .unwrap();
        assert_eq!(pair.turns[1].speaker.as_deref(), Some("Speaker A"));
        assert!(!pair.turns.iter().any(|t| t.text.contains("Speaker")));
        assert!(chunk_request(&[turn("Speaker C", "Who?")], &assignments, true).is_err());
    }

    #[test]
    fn requests_list_voices_in_label_order() {
        let speakers = [
            voiced("Speaker B", Gender::Male, SpeakerRole::Guest, "en-gb-b"),
            voiced("Speaker A", Gender::Female, SpeakerRole::Host, "en-gb-a"),
        ];
        let plan = plan_passage(
            &passage(&[("Speaker B", "You first."), ("Speaker A", "Thank you.")]),
            &speakers,
            MODEL,
        )
        .unwrap();
        assert_eq!(plan.requests.len(), 1);
        let labels: Vec<&str> = plan.requests[0]
            .voices
            .iter()
            .map(|v| v.label.as_str())
            .collect();
        assert_eq!(labels, ["Speaker A", "Speaker B"]);
        assert_eq!(plan.requests[0].voices[1].voice, "en-gb-b");
        // The turns keep the script's order.
        assert_eq!(
            plan.requests[0].turns[0].speaker.as_deref(),
            Some("Speaker B")
        );
    }

    #[test]
    fn plan_uses_one_assignment_for_every_chunk() {
        let speakers = [
            voiced(
                "Speaker A",
                Gender::Female,
                SpeakerRole::Host,
                "en-gb-tutor-4",
            ),
            voiced(
                "Speaker B",
                Gender::Female,
                SpeakerRole::Guest,
                "en-gb-tutor-11",
            ),
            voiced(
                "Speaker C",
                Gender::Female,
                SpeakerRole::Guest,
                "en-gb-tutor-10",
            ),
        ];
        let sentence = "We met at the station and walked to the river together.";
        let long = vec![sentence; 12].join(" ");
        let lines: Vec<(&str, &str)> = (0..9)
            .map(|i| {
                (
                    ["Speaker A", "Speaker B", "Speaker C"][i % 3],
                    long.as_str(),
                )
            })
            .collect();
        let plan = plan_passage(&passage(&lines), &speakers, MODEL).unwrap();
        assert!(plan.requests.len() > 3);
        for request in &plan.requests {
            assert!(request.voices.len() <= MAX_MULTI_SPEAKER_VOICES);
            for assignment in &request.voices {
                let speaker = speakers
                    .iter()
                    .find(|s| s.label == assignment.label)
                    .unwrap();
                assert_eq!(Some(assignment.voice.as_str()), speaker.voice_id());
            }
        }
        let mut sent: Vec<&str> = plan
            .requests
            .iter()
            .flat_map(|r| r.voices.iter().map(|v| v.voice.as_str()))
            .collect();
        sent.sort();
        sent.dedup();
        assert_eq!(sent.len(), 3);
        // A speaker without a voice is refused, not given one.
        let mut unvoiced = speakers.to_vec();
        unvoiced[1].voice = VoiceChoice::Auto;
        assert!(matches!(
            plan_passage(&passage(&lines), &unvoiced, MODEL),
            Err(TtsError::NoVoice(label)) if label == "Speaker B"
        ));
    }

    #[test]
    fn turn_style_is_role_base() {
        let speakers = [
            voiced("Speaker A", Gender::Female, SpeakerRole::Receptionist, "a"),
            voiced(
                "Speaker B",
                Gender::Male,
                SpeakerRole::Other("Caller".into()),
                "b",
            ),
        ];
        let plan = plan_passage(
            &passage(&[("Speaker A", "Good morning."), ("Speaker B", "Hello.")]),
            &speakers,
            MODEL,
        )
        .unwrap();
        let styles: Vec<&str> = plan.requests[0]
            .turns
            .iter()
            .map(|t| t.style.as_str())
            .collect();
        assert_eq!(
            styles,
            [
                "polite and helpful, clear, at a steady exam pace",
                "natural and conversational, clear, at a steady exam pace",
            ]
        );
        for style in styles {
            for word in ["british", "female", "male", "accent", "caller"] {
                assert!(!style.contains(word), "{style}");
            }
        }
    }

    #[test]
    fn custom_voices_read_alone() {
        let speakers = [
            voiced(
                "Speaker A",
                Gender::Female,
                SpeakerRole::Host,
                "voice_kpd3e297369r",
            ),
            voiced(
                "Speaker B",
                Gender::Male,
                SpeakerRole::Guest,
                "en-gb-assistant-2",
            ),
            voiced(
                "Speaker C",
                Gender::Male,
                SpeakerRole::Guest,
                "en-gb-advisor-8",
            ),
        ];
        let lines = [
            ("Speaker A", "Welcome to the show."),
            ("Speaker A", "Today we talk about rivers."),
            ("Speaker B", "Thanks for having me."),
            ("Speaker C", "And me."),
            ("Speaker B", "Rivers matter."),
            ("Speaker A", "Indeed they do."),
        ];
        let plan = plan_passage(&passage(&lines), &speakers, MODEL).unwrap();
        for request in &plan.requests {
            if request.voices.iter().any(|v| v.voice.starts_with("voice_")) {
                assert_eq!(request.voices.len(), 1, "{request:?}");
            }
        }
        // A's two lines are one request; B and C still share one.
        assert_eq!(plan.requests.len(), 3);
        assert_eq!(plan.requests[1].voices.len(), 2);
        assert_eq!(
            plan_words(&plan),
            words_of(lines.iter().map(|(_, text)| *text))
        );
    }

    #[test]
    fn designed_voice_speakers_never_share_a_request() {
        // Google accepted designed voices in two-voice requests (gate G4) but
        // documents one turn per request; the app keeps to the documentation.
        let designed = Voice {
            id: "voice_kwq20yi2gjin".into(),
            name: "Probe teacher".into(),
            gender: Gender::Female,
            accent: Accent::British,
            source: VoiceSource::Designed,
            description: String::new(),
        };
        assert!(reads_alone(&designed));
        let speakers = [
            SpeakerConfig::new(
                "Speaker A",
                Gender::Female,
                Accent::British,
                SpeakerRole::Host,
            )
            .with_voice(designed.clone()),
            voiced(
                "Speaker B",
                Gender::Male,
                SpeakerRole::Guest,
                "en-gb-assistant-2",
            ),
        ];
        let lines = [
            ("Speaker A", "Good morning, everyone."),
            ("Speaker B", "Morning."),
            ("Speaker A", "Today we practise numbers."),
            ("Speaker A", "And dates."),
            ("Speaker B", "Sounds good."),
            ("Speaker B", "Shall we start?"),
            ("Speaker A", "Yes."),
        ];
        let plan = plan_passage(&passage(&lines), &speakers, MODEL).unwrap();
        // A | B | A (two lines, one turn) | B (two lines) | A.
        assert_eq!(plan.requests.len(), 5);
        for request in &plan.requests {
            assert_eq!(request.voices.len(), 1, "{request:?}");
            assert!(request.turns.iter().all(|t| t.speaker.is_none()));
        }
        let voices: Vec<&str> = plan
            .requests
            .iter()
            .map(|r| r.voices[0].voice.as_str())
            .collect();
        assert_eq!(
            voices,
            [
                "voice_kwq20yi2gjin",
                "en-gb-assistant-2",
                "voice_kwq20yi2gjin",
                "en-gb-assistant-2",
                "voice_kwq20yi2gjin"
            ]
        );
        // Every word, in order; turns of different speakers are joined with
        // the turn gap.
        assert_eq!(
            plan_words(&plan),
            words_of(lines.iter().map(|(_, text)| *text))
        );
        assert_eq!(
            plan.gaps_ms,
            [0, TURN_GAP_MS, TURN_GAP_MS, TURN_GAP_MS, TURN_GAP_MS]
        );

        // Even a designed voice whose source was lost reads alone by its id.
        let by_id = Voice {
            source: VoiceSource::Library,
            ..designed
        };
        assert!(reads_alone(&by_id));
    }

    #[test]
    fn library_voices_share_conversational_chunks() {
        let speakers = [
            voiced(
                "Speaker A",
                Gender::Female,
                SpeakerRole::Host,
                "en-gb-advisor-1",
            ),
            voiced(
                "Speaker B",
                Gender::Male,
                SpeakerRole::Guest,
                "en-gb-assistant-2",
            ),
        ];
        let plan = plan_passage(
            &passage(&[
                ("Speaker A", "Hello."),
                ("Speaker B", "Hi."),
                ("Speaker A", "Bye."),
            ]),
            &speakers,
            MODEL,
        )
        .unwrap();
        if PER_TURN_ALL {
            assert!(plan.requests.iter().all(|r| r.voices.len() == 1));
        } else {
            assert_eq!(plan.requests.len(), 1);
            assert_eq!(plan.requests[0].voices.len(), 2);
            assert_eq!(plan.requests[0].turns.len(), 3);
        }
    }

    #[test]
    fn gaps_follow_speaker_changes() {
        let speakers = [
            voiced("Speaker A", Gender::Female, SpeakerRole::Host, "voice_abc"),
            voiced(
                "Speaker B",
                Gender::Male,
                SpeakerRole::Guest,
                "en-gb-assistant-2",
            ),
        ];
        let sentence = "The museum opens at nine and closes at five every day.";
        let lecture = vec![sentence; 30].join(" ");
        let plan = plan_passage(
            &passage(&[
                ("Speaker A", "Welcome."),
                ("Speaker B", lecture.as_str()),
                ("Speaker A", "Thank you."),
            ]),
            &speakers,
            MODEL,
        )
        .unwrap();
        // A | B (two chunks of one long turn) | A.
        assert_eq!(plan.requests.len(), 4);
        assert_eq!(plan.gaps_ms, [0, TURN_GAP_MS, CHUNK_GAP_MS, TURN_GAP_MS]);
        assert_eq!(plan.gaps_ms.len(), plan.requests.len());
    }

    #[test]
    fn tags_are_atomic_and_do_not_count() {
        // Eight words and two tags: the budget of four cuts at the sentence end.
        let text = "One two <short pause> three four. Five six <sigh> seven eight.";
        assert_eq!(
            split_turn(text, 4),
            [
                "One two <short pause> three four.",
                "Five six <sigh> seven eight."
            ]
        );
        // A tag after a sentence end stays with that sentence.
        assert_eq!(
            split_turn("One two three four. <sigh> Five six seven eight.", 4),
            ["One two three four. <sigh>", "Five six seven eight."]
        );
        // A tag is never cut, even where a run-on sentence is cut at words.
        let pieces = split_turn("a b <short pause> c d |oh really?| e", 2);
        assert_eq!(pieces, ["a b <short pause>", "c d |oh really?|", "e"]);
        // Tags do not use up the budget of a chunk.
        let turns = [
            turn("Speaker A", "One <laugh> two <sigh> three."),
            turn("Speaker B", "Four <cough> five."),
        ];
        assert_eq!(speech_chunks(&turns, 5, no_one_alone).len(), 1);
        assert_eq!(speech_chunks(&turns, 4, no_one_alone).len(), 2);
        // They are counted lightly in the token estimate.
        assert!(estimate_tokens(10, 2, 0, 1) > estimate_tokens(10, 0, 0, 1));
        assert!(estimate_tokens(10, 2, 0, 1) < estimate_tokens(12, 0, 0, 1) + 8);
    }

    #[test]
    fn unknown_markup_never_reaches_tts() {
        let speakers = [
            voiced("Speaker A", Gender::Female, SpeakerRole::Host, "en-gb-a"),
            voiced("Speaker B", Gender::Male, SpeakerRole::Guest, "en-gb-b"),
            voiced("Speaker C", Gender::Male, SpeakerRole::Expert, "en-gb-c"),
        ];
        let lines = [
            ("Speaker A", "Welcome <smirk> to the show [music] today."),
            (
                "Speaker B",
                "Thanks |mhm| for <long pause> having me | here <Sigh>, really.",
            ),
            ("Speaker C", "[applause]"),
            ("Speaker C", "And me |uh-huh| too."),
        ];
        let sent = |model: &str| -> Vec<(usize, String)> {
            let plan = plan_passage(&passage(&lines), &speakers, model).unwrap();
            plan.requests
                .iter()
                .flat_map(|r| r.turns.iter().map(|t| (r.voices.len(), t.text.clone())))
                .collect()
        };
        for model in [MODEL, "gemini-3.8-flash-lite-tts"] {
            let turns = sent(model);
            // The lone "[applause]" turn is skipped.
            assert_eq!(turns.len(), 3, "{turns:?}");
            for (_, text) in &turns {
                for unwanted in ["smirk", "[", "]", "long pause", "applause"] {
                    assert!(!text.contains(unwanted), "{model}: {text}");
                }
            }
            assert_eq!(turns[0].1, "Welcome to the show today.");
            assert!(turns[1].1.contains("<sigh>, really."), "{turns:?}");
        }
        // Backchannels: kept in A and B's two-voice request on Flash TTS only;
        // C reads alone, where nobody could answer.
        let flash = sent(MODEL);
        assert_eq!(
            flash[1],
            (
                2,
                "Thanks |mhm| for having me here <sigh>, really.".to_string()
            )
        );
        assert_eq!(flash[2], (1, "And me too.".to_string()));
        let lite = sent("gemini-3.8-flash-lite-tts");
        assert_eq!(lite[1].1, "Thanks for having me here <sigh>, really.");
        assert!(lite.iter().all(|(_, text)| !text.contains('|')));
    }

    #[test]
    fn plain_scripts_are_sent_unchanged() {
        let speakers = [voiced(
            "Speaker A",
            Gender::Female,
            SpeakerRole::Host,
            "en-gb-a",
        )];
        let text = "Good morning, everyone.  Today we look at rivers.";
        let plan = plan_passage(&passage(&[("Speaker A", text)]), &speakers, MODEL).unwrap();
        assert_eq!(plan.requests[0].turns[0].text, text);
    }
}
