//! Turning a `Passage` into PCM.
//!
//! Gemini's multi-speaker synthesis takes at most two voices per request. A
//! two-voice passage is sent whole (best prosody); a passage with more voices,
//! such as the HSG interview with a host and two guests, is synthesised one
//! turn at a time and concatenated with short gaps.

use crate::domain::{Line, Passage, SpeakerConfig};

use super::super::audio::{Pcm16, SAMPLE_RATE};
use super::super::config::{TTS_MAX_INPUT_TOKENS, config};
use super::super::llm::{GeminiClient, LlmError, VoiceAssignment};

/// Gemini limit for `multiSpeakerVoiceConfig`.
pub const MAX_MULTI_SPEAKER_VOICES: usize = 2;
/// Silence inserted between turns when synthesising turn by turn.
const TURN_GAP_MS: u32 = 350;

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error(transparent)]
    Llm(#[from] LlmError),
    #[error("No voice is configured for \"{0}\"")]
    NoVoice(String),
}

fn style_prompt(script: &str) -> String {
    format!(
        "Read the following listening-exam script aloud with natural, clear pronunciation at a steady exam pace. \
         Do not read the speaker labels.\n\n{script}"
    )
}

/// Synthesises a whole passage. Returns 24 kHz mono PCM.
pub async fn synthesize_passage(
    client: &GeminiClient,
    passage: &Passage,
    speakers: &[SpeakerConfig],
) -> Result<Pcm16, TtsError> {
    let voices = &config().voices;
    let used = passage.speakers_used();
    let assignments: Vec<VoiceAssignment> = used
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

    let text = if assignments.len() == 1 {
        passage.plain_text()
    } else {
        passage.script_text()
    };
    let fits_one_request = estimate_tokens(&text) < TTS_MAX_INPUT_TOKENS;
    if assignments.len() <= MAX_MULTI_SPEAKER_VOICES && fits_one_request {
        let bytes = client
            .synthesize(&style_prompt(&text), &assignments)
            .await?;
        return Ok(Pcm16::from_le_bytes(&bytes, SAMPLE_RATE));
    }
    if !fits_one_request {
        tracing::info!(
            part = passage.part,
            "script exceeds one TTS request; reading turn by turn"
        );
    }

    let mut out = Pcm16::silence(0, SAMPLE_RATE);
    for (index, turn) in merge_turns(&passage.lines).iter().enumerate() {
        let assignment = assignments
            .iter()
            .find(|a| a.label == turn.speaker)
            .cloned()
            .ok_or_else(|| TtsError::NoVoice(turn.speaker.clone()))?;
        let bytes = client
            .synthesize(&style_prompt(&turn.text), std::slice::from_ref(&assignment))
            .await?;
        if index > 0 {
            out.append(&Pcm16::silence(TURN_GAP_MS, SAMPLE_RATE));
        }
        out.append(&Pcm16::from_le_bytes(&bytes, SAMPLE_RATE));
    }
    Ok(out)
}

/// The announcer voice reading an instruction.
pub async fn synthesize_announcement(client: &GeminiClient, text: &str) -> Result<Pcm16, TtsError> {
    let voices = &config().voices;
    let assignment = VoiceAssignment {
        label: "Announcer".into(),
        voice: voices.announcer.clone(),
    };
    let bytes = client
        .synthesize(
            &format!("Read this exam announcement slowly and clearly:\n\n{text}"),
            std::slice::from_ref(&assignment),
        )
        .await?;
    Ok(Pcm16::from_le_bytes(&bytes, SAMPLE_RATE))
}

impl super::super::audio::Announcer for GeminiClient {
    async fn speak(&self, text: &str) -> Result<Pcm16, TtsError> {
        synthesize_announcement(self, text).await
    }
}

/// Rough token estimate for English prose (about 1.4 tokens per word), with
/// headroom for the style prompt.
fn estimate_tokens(text: &str) -> usize {
    (text.split_whitespace().count() as f32 * 1.4) as usize + 64
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

    #[test]
    fn merges_consecutive_turns() {
        let line = |s: &str, t: &str| Line {
            speaker: s.into(),
            text: t.into(),
        };
        let merged = merge_turns(&[line("A", "one"), line("A", "two"), line("B", "three")]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].text, "one two");
    }
}
