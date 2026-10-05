//! Voice samples ("Preview"): one short recording per voice, made once and
//! kept under `DATA_DIR/audio/voices/{id}.wav`, so hearing a voice again is
//! free. The hourly clean-up deletes samples unused for `SAMPLE_KEEP_HOURS`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::domain::{SpeakerRole, Voice};

use super::super::audio::{Pcm16, duration_ms_for_len};
use super::super::config::config;
use super::super::llm::{GeminiClient, SpeechRequest, SpeechTurn, VoiceAssignment};
use super::cache;
use super::synthesize::{TtsError, speaker_style};

/// What every sample says: about 12 seconds built around the sounds that
/// tell accents apart (r after a vowel, "bath" and "car park", t, "Tuesday"
/// and "schedule"), and nothing a teacher could mistake for exam content.
pub const PREVIEW_TEXT: &str = "Right, I'd better check the car park first. Last year we paid for parking on Tuesday, but the new schedule says the castle tour starts after lunch, so we can't be late. Water and a map are included.";
/// A sample unused this long (30 days) is deleted.
pub const SAMPLE_KEEP_HOURS: u64 = 720;

/// A stored sample: its WAV and length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleFile {
    pub path: PathBuf,
    pub duration_ms: u32,
}

/// The sample of voice `id` if one is stored (free); touching it keeps it
/// from the clean-up. `None` for an id that is not a voice id.
pub async fn stored_sample(id: &str) -> Option<SampleFile> {
    stored_sample_in(&config().voice_sample_dir(), id).await
}

/// Records `PREVIEW_TEXT` in `voice` (one paid request of about 12 s of
/// audio) and stores it, replacing an older sample. The caller records
/// the client's usage.
pub async fn make_sample(client: &GeminiClient, voice: &Voice) -> Result<SampleFile, TtsError> {
    Voice::check_id(&voice.id)?;
    let pcm = client.synthesize(&sample_request(voice)).await?;
    store_sample_in(&config().voice_sample_dir(), &voice.id, &pcm).await
}

/// Where the sample of `id` lives in `directory`; `None` for an id that is
/// not a voice id, which keeps file names and routes safe.
pub fn sample_path(directory: &Path, id: &str) -> Option<PathBuf> {
    Voice::check_id(id).ok()?;
    Some(directory.join(format!("{id}.wav")))
}

/// One voice reading `PREVIEW_TEXT` the way a narrator would.
fn sample_request(voice: &Voice) -> SpeechRequest {
    SpeechRequest {
        turns: vec![SpeechTurn {
            speaker: None,
            text: PREVIEW_TEXT.into(),
            style: speaker_style(&SpeakerRole::Narrator),
        }],
        voices: vec![VoiceAssignment {
            label: "Preview".into(),
            voice: voice.id.clone(),
        }],
    }
}

async fn stored_sample_in(directory: &Path, id: &str) -> Option<SampleFile> {
    let path = sample_path(directory, id)?;
    tokio::task::spawn_blocking(move || {
        let len = std::fs::metadata(&path).ok().filter(|m| m.is_file())?.len();
        let _ = std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(SystemTime::now()));
        Some(SampleFile {
            duration_ms: duration_ms_for_len(len),
            path,
        })
    })
    .await
    .ok()
    .flatten()
}

async fn store_sample_in(directory: &Path, id: &str, pcm: &Pcm16) -> Result<SampleFile, TtsError> {
    Voice::check_id(id)?;
    let path = directory.join(format!("{id}.wav"));
    let (target, wav) = (path.clone(), pcm.to_wav());
    tokio::task::spawn_blocking(move || cache::write(&target, &wav))
        .await
        .map_err(|e| TtsError::Storage(e.to_string()))?
        .map_err(TtsError::Storage)?;
    Ok(SampleFile {
        path,
        duration_ms: pcm.duration_ms(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn stored_samples_are_hits() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(stored_sample_in(dir.path(), "en-gb-advisor-1").await, None);
        let made = store_sample_in(dir.path(), "en-gb-advisor-1", &Pcm16::silence(500, 24_000))
            .await
            .unwrap();
        assert_eq!(made.path, dir.path().join("en-gb-advisor-1.wav"));
        let hit = stored_sample_in(dir.path(), "en-gb-advisor-1")
            .await
            .unwrap();
        assert_eq!(hit, made);
        assert_eq!(hit.duration_ms, 500);

        // A hit counts as a use: the clean-up keeps it.
        let long_ago = SystemTime::now() - Duration::from_secs(40 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(&hit.path)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        stored_sample_in(dir.path(), "en-gb-advisor-1")
            .await
            .unwrap();
        let keep = Duration::from_secs(SAMPLE_KEEP_HOURS * 3_600);
        assert_eq!(cache::purge_older_than(dir.path(), keep), 0);

        for bad in ["", "../jobs", "en gb", "voice/x", &"x".repeat(101)] {
            assert_eq!(sample_path(dir.path(), bad), None, "{bad}");
            assert_eq!(stored_sample_in(dir.path(), bad).await, None);
            assert!(
                store_sample_in(dir.path(), bad, &Pcm16::silence(1, 24_000))
                    .await
                    .is_err()
            );
        }
    }

    #[test]
    fn samples_are_read_by_a_narrator_alone() {
        let voice = Voice {
            id: "en-gb-advisor-1".into(),
            name: "Authoritative Advisor 1".into(),
            gender: crate::domain::Gender::Female,
            accent: crate::domain::Accent::British,
            source: crate::domain::VoiceSource::Library,
            description: String::new(),
        };
        let request = sample_request(&voice);
        assert_eq!(request.voices.len(), 1);
        assert_eq!(request.voices[0].voice, "en-gb-advisor-1");
        assert_eq!(
            request.turns[0].style,
            speaker_style(&SpeakerRole::Narrator)
        );
        assert_eq!(request.turns[0].speaker, None);
        assert!(PREVIEW_TEXT.split_whitespace().count() >= 30);
    }
}
