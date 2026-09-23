//! Renders an `AudioProgram` (tones, pauses, announcements, passages) into one PCM buffer.

use std::collections::HashMap;

use crate::domain::{AudioProgram, AudioSegment};

use super::super::config::config;
use super::super::tts::TtsError;
use super::wav::{Pcm16, SAMPLE_RATE};

const TONE_HZ: f32 = 880.0;
const TONE_MS: u32 = 600;
const TONE_AMPLITUDE: f32 = 0.35;
/// Silence used when no music asset is configured.
const MUSIC_FALLBACK_MS: u32 = 3_000;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("Part {0} has no recording yet")]
    MissingPassage(u8),
    #[error(transparent)]
    Tts(#[from] TtsError),
    #[error("Audio asset problem: {0}")]
    Asset(String),
}

/// Anything that can read an announcement aloud. The Gemini client implements
/// it; tests can implement it with silence.
#[allow(async_fn_in_trait)]
pub trait Announcer {
    async fn speak(&self, text: &str) -> Result<Pcm16, TtsError>;
}

pub struct ProgramAssets {
    pub music: Option<Pcm16>,
}

impl ProgramAssets {
    /// Loads the optional music file named by `MUSIC_PATH`.
    pub fn from_config() -> Result<Self, AudioError> {
        let Some(path) = &config().music_path else {
            return Ok(Self { music: None });
        };
        let bytes = std::fs::read(path)
            .map_err(|e| AudioError::Asset(format!("{}: {e}", path.display())))?;
        let music = Pcm16::from_wav(&bytes)
            .map_err(|e| AudioError::Asset(format!("{}: {e}", path.display())))?;
        if music.sample_rate != SAMPLE_RATE {
            return Err(AudioError::Asset(format!(
                "{} must be {SAMPLE_RATE} Hz",
                path.display()
            )));
        }
        Ok(Self { music: Some(music) })
    }
}

/// Concatenates every segment. `passages` maps part number to its synthesised PCM.
pub async fn render_program<A: Announcer>(
    program: &AudioProgram,
    passages: &HashMap<u8, Pcm16>,
    announcer: &A,
    assets: &ProgramAssets,
) -> Result<Pcm16, AudioError> {
    let mut out = Pcm16::silence(0, SAMPLE_RATE);
    for segment in &program.segments {
        let piece = match segment {
            AudioSegment::Music => match &assets.music {
                Some(music) => music.clone(),
                None => Pcm16::silence(MUSIC_FALLBACK_MS, SAMPLE_RATE),
            },
            AudioSegment::Tone => Pcm16::tone(TONE_HZ, TONE_MS, TONE_AMPLITUDE, SAMPLE_RATE),
            AudioSegment::Silence { ms } => Pcm16::silence(*ms, SAMPLE_RATE),
            AudioSegment::Announcement(text) => announcer.speak(text).await?,
            AudioSegment::Passage { part } => passages
                .get(part)
                .cloned()
                .ok_or(AudioError::MissingPassage(*part))?,
        };
        out.append(&piece);
        out.append(&Pcm16::silence(500, SAMPLE_RATE));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ExamFormat;

    struct SilentAnnouncer;

    impl Announcer for SilentAnnouncer {
        async fn speak(&self, _text: &str) -> Result<Pcm16, TtsError> {
            Ok(Pcm16::silence(1_000, SAMPLE_RATE))
        }
    }

    #[tokio::test]
    async fn renders_every_segment_in_order() {
        let format = ExamFormat::hsg_national();
        let program = AudioProgram::for_format(&format);
        let mut passages = HashMap::new();
        for part in &format.parts {
            passages.insert(part.number, Pcm16::silence(2_000, SAMPLE_RATE));
        }
        let pcm = render_program(
            &program,
            &passages,
            &SilentAnnouncer,
            &ProgramAssets { music: None },
        )
        .await
        .unwrap();
        // Two minutes of checking time alone is 120 s; the whole thing must be longer.
        assert!(pcm.duration_ms() > 120_000 + 4 * 2_000);
        passages.remove(&3);
        let err = render_program(
            &program,
            &passages,
            &SilentAnnouncer,
            &ProgramAssets { music: None },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AudioError::MissingPassage(3)));
    }
}
