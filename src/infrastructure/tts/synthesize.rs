//! Turning a `Passage` into PCM.
//!
//! Gemini 3.8 TTS reads its input word for word and takes at most two voices
//! per request. A passage is cut into chunks of consecutive turns, each at
//! most `MAX_WORDS_PER_REQUEST` words and at most two speakers; a chunk is one
//! request (one annotated item per turn) and chunks are joined with a short
//! gap. Short chunks keep every request well inside the API's time and token
//! limits, a failure costs one chunk rather than the passage, and an edit
//! invalidates only its own chunk in the speech cache. A three-voice passage
//! (the HSG interview) is simply chunked so that no chunk has a third voice.

use crate::domain::{Line, Passage, SpeakerConfig};

use super::super::audio::{Pcm16, SAMPLE_RATE};
use super::super::config::{TTS_MAX_INPUT_TOKENS, config};
use super::super::llm::{GeminiClient, LlmError, SpeechRequest, SpeechTurn, VoiceAssignment};
use super::cache;

/// Gemini limit for multi-speaker synthesis.
pub const MAX_MULTI_SPEAKER_VOICES: usize = 2;
/// Words per speech request: about 80 s of audio at exam pace, which 3.8
/// Flash TTS reads in roughly 30 s (measured 2.5x real time), well inside the
/// minute a normal request stays open and far from the 8,192 input and
/// 16,384 output tokens a request allows.
const MAX_WORDS_PER_REQUEST: usize = 200;
/// Silence inserted between chunks.
const CHUNK_GAP_MS: u32 = 350;
/// Delivery directions. The text itself is read verbatim, so they never go there.
const PASSAGE_STYLE: &str = "natural, clear pronunciation at a steady exam pace";
const ANNOUNCEMENT_STYLE: &str = "slow and clear, like an exam announcer";

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error(transparent)]
    Llm(#[from] LlmError),
    #[error("No voice is configured for \"{0}\"")]
    NoVoice(String),
}

/// Synthesises a whole passage. Returns 24 kHz mono PCM.
pub async fn synthesize_passage(
    client: &GeminiClient,
    passage: &Passage,
    speakers: &[SpeakerConfig],
) -> Result<Pcm16, TtsError> {
    let voices = &config().voices;
    let assignments: Vec<VoiceAssignment> = passage
        .speakers_used()
        .iter()
        .map(|label| {
            let speaker = speakers
                .iter()
                .find(|s| &s.label == label)
                .ok_or_else(|| TtsError::NoVoice(label.clone()))?;
            Ok(VoiceAssignment {
                label: label.clone(),
                voice: voices.voice_for(speaker.gender, speaker.accent),
            })
        })
        .collect::<Result<_, TtsError>>()?;

    let mut out = Pcm16::silence(0, SAMPLE_RATE);
    for (index, chunk) in speech_chunks(&passage.lines, MAX_WORDS_PER_REQUEST)
        .into_iter()
        .enumerate()
    {
        let request = chunk_request(&chunk, &assignments)?;
        let pcm = cache::speak(client, &request).await?;
        if index > 0 {
            out.append(&Pcm16::silence(CHUNK_GAP_MS, SAMPLE_RATE));
        }
        out.append(&pcm);
    }
    Ok(out)
}

/// The announcer voice reading an instruction.
pub async fn synthesize_announcement(client: &GeminiClient, text: &str) -> Result<Pcm16, TtsError> {
    let request = SpeechRequest {
        turns: vec![SpeechTurn {
            speaker: None,
            text: text.to_string(),
        }],
        voices: vec![VoiceAssignment {
            label: "Announcer".into(),
            voice: config().voices.announcer.clone(),
        }],
        style: ANNOUNCEMENT_STYLE.into(),
    };
    Ok(cache::speak(client, &request).await?)
}

impl super::super::audio::Announcer for GeminiClient {
    async fn speak(&self, text: &str) -> Result<Pcm16, TtsError> {
        synthesize_announcement(self, text).await
    }
}

/// One request for a chunk: the voices of the speakers in it, in order of
/// appearance, and one turn per line. A one-voice chunk names no speaker.
fn chunk_request(chunk: &[Line], assignments: &[VoiceAssignment]) -> Result<SpeechRequest, TtsError> {
    let mut voices: Vec<VoiceAssignment> = Vec::new();
    for line in chunk {
        if voices.iter().all(|v| v.label != line.speaker) {
            let assignment = assignments
                .iter()
                .find(|a| a.label == line.speaker)
                .cloned()
                .ok_or_else(|| TtsError::NoVoice(line.speaker.clone()))?;
            voices.push(assignment);
        }
    }
    let one_voice = voices.len() == 1;
    let turns: Vec<SpeechTurn> = chunk
        .iter()
        .map(|line| SpeechTurn {
            speaker: (!one_voice).then(|| line.speaker.clone()),
            text: line.text.clone(),
        })
        .collect();
    let words: usize = chunk.iter().map(|l| l.text.split_whitespace().count()).sum();
    if estimate_tokens(words, turns.len()) >= TTS_MAX_INPUT_TOKENS {
        return Err(TtsError::Llm(LlmError::Malformed(
            "a speech chunk is longer than one request allows".into(),
        )));
    }
    Ok(SpeechRequest {
        turns,
        voices,
        style: PASSAGE_STYLE.into(),
    })
}

/// Rough input-token estimate: about 1.4 tokens per English word plus the
/// per-turn annotation, with headroom.
fn estimate_tokens(words: usize, turns: usize) -> usize {
    (words as f32 * 1.4) as usize + turns * 16 + 64
}

/// Cuts lines into request-sized chunks. Consecutive lines by one speaker are
/// merged; a turn longer than `max_words` is split at sentence ends (at word
/// boundaries for a sentence that is itself too long). A chunk never exceeds
/// `max_words` words or `MAX_MULTI_SPEAKER_VOICES` speakers, and the order of
/// the words never changes.
fn speech_chunks(lines: &[Line], max_words: usize) -> Vec<Vec<Line>> {
    let max_words = max_words.max(1);
    let mut chunks: Vec<Vec<Line>> = Vec::new();
    let mut current: Vec<Line> = Vec::new();
    let mut current_words = 0;
    for turn in merge_turns(lines) {
        for piece in split_turn(&turn.text, max_words) {
            let words = piece.split_whitespace().count();
            let new_speaker = current.iter().all(|l| l.speaker != turn.speaker);
            let speakers = current
                .iter()
                .map(|l| &l.speaker)
                .fold(Vec::new(), |mut seen, s| {
                    if !seen.contains(&s) {
                        seen.push(s);
                    }
                    seen
                })
                .len();
            let fits = current_words + words <= max_words
                && !(new_speaker && speakers >= MAX_MULTI_SPEAKER_VOICES);
            if !current.is_empty() && !fits {
                chunks.push(std::mem::take(&mut current));
                current_words = 0;
            }
            match current.last_mut() {
                Some(last) if last.speaker == turn.speaker => {
                    last.text.push(' ');
                    last.text.push_str(&piece);
                }
                _ => current.push(Line {
                    speaker: turn.speaker.clone(),
                    text: piece,
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

/// A turn as pieces of at most `max_words` words, broken after sentence ends
/// where possible. A turn that fits is returned unchanged.
fn split_turn(text: &str, max_words: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= max_words {
        return vec![text.trim().to_string()];
    }
    let mut sentences: Vec<Vec<&str>> = vec![Vec::new()];
    for word in words {
        sentences.last_mut().expect("never empty").push(word);
        let end = word.trim_end_matches(['"', '\'', ')', ']', '”', '’']);
        if end.ends_with(['.', '?', '!']) {
            sentences.push(Vec::new());
        }
    }
    let mut pieces: Vec<String> = Vec::new();
    let mut piece: Vec<&str> = Vec::new();
    for sentence in sentences.into_iter().filter(|s| !s.is_empty()) {
        if !piece.is_empty() && piece.len() + sentence.len() > max_words {
            pieces.push(piece.join(" "));
            piece.clear();
        }
        if sentence.len() > max_words {
            for part in sentence.chunks(max_words) {
                pieces.push(part.join(" "));
            }
        } else {
            piece.extend(sentence);
        }
    }
    if !piece.is_empty() {
        pieces.push(piece.join(" "));
    }
    pieces
}

/// Consecutive lines by the same speaker become one turn.
fn merge_turns(lines: &[Line]) -> Vec<Line> {
    let mut turns: Vec<Line> = Vec::new();
    for line in lines {
        match turns.last_mut() {
            Some(last) if last.speaker == line.speaker => {
                last.text.push(' ');
                last.text.push_str(&line.text);
            }
            _ => turns.push(line.clone()),
        }
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(s: &str, t: &str) -> Line {
        Line {
            speaker: s.into(),
            text: t.into(),
        }
    }

    fn words(chunks: &[Vec<Line>]) -> Vec<String> {
        chunks
            .iter()
            .flatten()
            .flat_map(|l| l.text.split_whitespace().map(str::to_string).collect::<Vec<_>>())
            .collect()
    }

    #[test]
    fn merges_consecutive_turns() {
        let merged = merge_turns(&[line("A", "one"), line("A", "two"), line("B", "three")]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].text, "one two");
    }

    #[test]
    fn short_dialogue_is_one_chunk() {
        let lines = [line("A", "Hi there."), line("B", "Hello."), line("A", "Bye.")];
        let chunks = speech_chunks(&lines, 250);
        assert_eq!(chunks, vec![lines.to_vec()]);
    }

    #[test]
    fn chunks_respect_the_word_budget_and_keep_every_word() {
        let lines: Vec<Line> = (0..12)
            .map(|i| line(if i % 2 == 0 { "A" } else { "B" }, "one two three four five."))
            .collect();
        let chunks = speech_chunks(&lines, 12);
        for chunk in &chunks {
            let count: usize = chunk.iter().map(|l| l.text.split_whitespace().count()).sum();
            assert!(count <= 12, "{chunk:?}");
        }
        assert_eq!(chunks.len(), 6);
        assert_eq!(words(&chunks), words(&[lines]));
    }

    #[test]
    fn a_third_speaker_starts_a_new_chunk() {
        let lines = [
            line("Speaker A", "Welcome."),
            line("Speaker B", "Thanks."),
            line("Speaker C", "Hello."),
            line("Speaker A", "Right."),
        ];
        let chunks = speech_chunks(&lines, 250);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2);
        assert_eq!(chunks[1][0].speaker, "Speaker C");
        for chunk in &chunks {
            let mut speakers: Vec<&str> = chunk.iter().map(|l| l.speaker.as_str()).collect();
            speakers.dedup();
            speakers.sort();
            speakers.dedup();
            assert!(speakers.len() <= MAX_MULTI_SPEAKER_VOICES);
        }
    }

    #[test]
    fn a_long_monologue_is_split_at_sentence_ends() {
        let sentence = "The museum opens at nine and closes at five every day.";
        let lecture = vec![sentence; 30].join(" ");
        let chunks = speech_chunks(&[line("Speaker A", &lecture)], 50);
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
        assert_eq!(pieces.iter().map(|p| p.split_whitespace().count()).sum::<usize>(), 25);
    }

    #[test]
    fn requests_name_speakers_only_with_two_voices() {
        let assignments = vec![
            VoiceAssignment {
                label: "Speaker A".into(),
                voice: "Kore".into(),
            },
            VoiceAssignment {
                label: "Speaker B".into(),
                voice: "Puck".into(),
            },
        ];
        let solo = chunk_request(&[line("Speaker B", "Just me.")], &assignments).unwrap();
        assert_eq!(solo.voices.len(), 1);
        assert_eq!(solo.voices[0].voice, "Puck");
        assert_eq!(solo.turns[0].speaker, None);
        let pair = chunk_request(
            &[line("Speaker B", "You first."), line("Speaker A", "Thanks.")],
            &assignments,
        )
        .unwrap();
        assert_eq!(pair.voices[0].label, "Speaker B");
        assert_eq!(pair.turns[1].speaker.as_deref(), Some("Speaker A"));
        assert!(!pair.turns.iter().any(|t| t.text.contains("Speaker")));
        assert!(chunk_request(&[line("Speaker C", "Who?")], &assignments).is_err());
    }
}
