//! Reuse of synthesised speech.
//!
//! Every speech request is keyed by a SHA-256 of what shapes its audio (the
//! TTS model, the voices, and each turn's speaker, style and text) and its
//! PCM is kept as a WAV under `DATA_DIR/audio/cache/`. Rendering an exam again
//! after editing one line pays only for the chunk that changed, a retried job
//! pays only for what failed, and the announcements are paid for once. A hit
//! is recorded as `Usage::reused` at no cost. Files unused for
//! `SPEECH_CACHE_HOURS` are deleted by the hourly clean-up; 0 turns the cache
//! off.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use crate::domain::Usage;

use super::super::audio::Pcm16;
use super::super::config::config;
use super::super::llm::{GeminiClient, SpeechRequest};
use super::synthesize::TtsError;

/// Bump when anything outside the key starts to change the audio. v2: voices
/// are regional library ids and each turn carries its own style.
const KEY_VERSION: &str = "speech-cache-v2";

/// Whether an earlier take of a request may be reused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Reuse {
    #[default]
    Allow,
    /// Synthesise again ("New take") and store the new take in place of the old.
    Refresh,
}

impl Reuse {
    /// `Refresh` for a part the teacher asked a new take of
    /// (`AudioRequest::fresh`), `Allow` otherwise.
    pub fn new_take(fresh: bool) -> Self {
        if fresh { Reuse::Refresh } else { Reuse::Allow }
    }
}

/// Synthesises `request`, or returns the audio made for an identical request
/// earlier (unless `reuse` is `Refresh`).
pub async fn speak(
    client: &GeminiClient,
    request: &SpeechRequest,
    reuse: Reuse,
) -> Result<Pcm16, TtsError> {
    let directory = config()
        .speech_cache_secs()
        .map(|_| config().speech_cache_dir());
    speak_in(directory.as_deref(), client, request, reuse).await
}

async fn speak_in(
    directory: Option<&Path>,
    client: &GeminiClient,
    request: &SpeechRequest,
    reuse: Reuse,
) -> Result<Pcm16, TtsError> {
    let Some(directory) = directory else {
        return Ok(client.synthesize(request).await?);
    };
    let key = key_for(client.tts_model(), request);
    let path = directory.join(format!("{key}.wav"));
    let hit = match cached_path(directory, &key, reuse) {
        Some(cached) => read(cached).await,
        None => None,
    };
    let voices: Vec<String> = request
        .voices
        .iter()
        .map(|v| format!("{}={}", v.label.replace(' ', ""), v.voice))
        .collect();
    tracing::info!(
        key = %key,
        hit = hit.is_some(),
        voices = %voices.join(","),
        turns = request.turns.len(),
        words = request
            .turns
            .iter()
            .map(|t| t.text.split_whitespace().count())
            .sum::<usize>(),
        "speech chunk"
    );
    if let Some(pcm) = hit {
        client.add_usage(&Usage {
            reused: 1,
            ..Usage::default()
        });
        return Ok(pcm);
    }
    // Run on its own task: a chunk Google is already reading is finished and
    // stored even if the job that asked for it fails or is dropped meanwhile,
    // so it is never paid for twice. (Its spend reaches the job's ledger
    // entry only if it ends before the job does.)
    let (client, request) = (client.clone(), request.clone());
    let made = tokio::spawn(async move {
        let pcm = client.synthesize(&request).await?;
        let wav = pcm.to_wav();
        let written = tokio::task::spawn_blocking(move || write(&path, &wav)).await;
        if let Ok(Err(e)) | Err(e) = written.map_err(|e| e.to_string()) {
            tracing::warn!("speech cache not updated: {e}");
        }
        Ok::<_, TtsError>(pcm)
    });
    made.await.map_err(|e| {
        TtsError::Llm(super::super::llm::LlmError::Network(format!(
            "the speech task stopped ({e})"
        )))
    })?
}

/// The cached file to read for `key`, if reading is allowed. `Refresh` never
/// reads: the new take overwrites the entry instead.
fn cached_path(directory: &Path, key: &str, reuse: Reuse) -> Option<PathBuf> {
    match reuse {
        Reuse::Allow => Some(directory.join(format!("{key}.wav"))),
        Reuse::Refresh => None,
    }
}

/// Hex SHA-256 over every input that shapes the audio, NUL-separated.
fn key_for(model: &str, request: &SpeechRequest) -> String {
    let mut hasher = Sha256::new();
    let mut field = |value: &str| {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    };
    field(KEY_VERSION);
    field(model);
    for voice in &request.voices {
        field(&voice.label);
        field(&voice.voice);
    }
    for turn in &request.turns {
        field(turn.speaker.as_deref().unwrap_or(""));
        field(&turn.style);
        field(&turn.text);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The cached audio at `path`, touching it so the clean-up keeps it. A file
/// that no longer decodes is removed and treated as a miss.
async fn read(path: PathBuf) -> Option<Pcm16> {
    tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&path).ok()?;
        match Pcm16::from_wav(&bytes) {
            Ok(pcm) => {
                let _ = std::fs::File::options()
                    .write(true)
                    .open(&path)
                    .and_then(|file| file.set_modified(SystemTime::now()));
                Some(pcm)
            }
            Err(_) => {
                let _ = std::fs::remove_file(&path);
                None
            }
        }
    })
    .await
    .ok()
    .flatten()
}

/// Writes beside the target and renames, so a reader never sees half a file.
pub(super) fn write(path: &Path, wav: &[u8]) -> Result<(), String> {
    let directory = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(directory)
        .map_err(|e| format!("cannot create {}: {e}", directory.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .map_err(|e| format!("cannot write in {}: {e}", directory.display()))?;
    std::io::Write::write_all(&mut temporary, wav)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|e| format!("cannot store {}: {}", path.display(), e.error))?;
    Ok(())
}

/// Deletes cached audio unused for longer than `max_age`; returns how many files went.
pub fn purge_older_than(directory: &Path, max_age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    let cutoff = SystemTime::now()
        .checked_sub(max_age)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("wav") {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|modified| modified < cutoff);
        if stale && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::llm::{SpeechTurn, VoiceAssignment};

    fn request(text: &str) -> SpeechRequest {
        SpeechRequest {
            turns: vec![SpeechTurn {
                speaker: None,
                text: text.into(),
                style: "calm".into(),
            }],
            voices: vec![VoiceAssignment {
                label: "Speaker A".into(),
                voice: "en-gb-advisor-1".into(),
            }],
        }
    }

    fn offline_client() -> GeminiClient {
        GeminiClient::new(
            "not-a-real-key".into(),
            "gemini-3.8-flash".into(),
            "gemini-3.8-flash-tts".into(),
            "low".into(),
        )
        .unwrap()
    }

    #[test]
    fn keys_are_stable_and_change_with_anything_audible() {
        let base = key_for("gemini-3.8-flash-tts", &request("Hello."));
        assert_eq!(base, key_for("gemini-3.8-flash-tts", &request("Hello.")));
        assert_eq!(base.len(), 64);
        assert_ne!(
            base,
            key_for("gemini-3.8-flash-lite-tts", &request("Hello."))
        );
        assert_ne!(base, key_for("gemini-3.8-flash-tts", &request("Hello!")));
        let mut other_voice = request("Hello.");
        other_voice.voices[0].voice = "en-gb-assistant-2".into();
        assert_ne!(base, key_for("gemini-3.8-flash-tts", &other_voice));
        let mut other_label = request("Hello.");
        other_label.voices[0].label = "Speaker B".into();
        assert_ne!(base, key_for("gemini-3.8-flash-tts", &other_label));
    }

    #[test]
    fn keys_change_with_turn_style_and_version_is_v2() {
        assert_eq!(KEY_VERSION, "speech-cache-v2");
        let mut two = request("Hello.");
        two.turns.push(SpeechTurn {
            speaker: None,
            text: "Again.".into(),
            style: "calm".into(),
        });
        let base = key_for("m", &two);
        let mut restyled = two.clone();
        restyled.turns[1].style = "excited".into();
        assert_ne!(base, key_for("m", &restyled));
        // The same words split differently between turns are another request.
        let mut moved = two.clone();
        moved.turns[0].text = "Hello. Again.".into();
        moved.turns[1].text = String::new();
        assert_ne!(base, key_for("m", &moved));
    }

    #[tokio::test]
    async fn a_hit_costs_nothing_and_needs_no_network() {
        let dir = tempfile::tempdir().unwrap();
        let client = offline_client();
        let wanted = request("Cached words.");
        let stored = Pcm16::silence(250, 24_000);
        let path = dir
            .path()
            .join(format!("{}.wav", key_for(client.tts_model(), &wanted)));
        write(&path, &stored.to_wav()).unwrap();
        let pcm = speak_in(Some(dir.path()), &client, &wanted, Reuse::Allow)
            .await
            .unwrap();
        assert_eq!(pcm, stored);
        let usage = client.usage();
        assert_eq!((usage.reused, usage.requests, usage.micro_usd), (1, 0, 0));
    }

    #[test]
    fn refresh_skips_the_cached_take() {
        let dir = tempfile::tempdir().unwrap();
        let key = key_for("m", &request("Again."));
        assert_eq!(
            cached_path(dir.path(), &key, Reuse::Allow),
            Some(dir.path().join(format!("{key}.wav")))
        );
        assert_eq!(cached_path(dir.path(), &key, Reuse::Refresh), None);
        assert_eq!(Reuse::default(), Reuse::Allow);
        assert_eq!(Reuse::new_take(true), Reuse::Refresh);
        assert_eq!(Reuse::new_take(false), Reuse::Allow);
    }

    #[test]
    fn corrupt_entries_are_misses_and_old_entries_are_purged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.wav");
        std::fs::write(&path, b"not a wav").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(runtime.block_on(read(path.clone())).is_none());
        assert!(!path.exists());

        let fresh = dir.path().join("fresh.wav");
        let stale = dir.path().join("stale.wav");
        for file in [&fresh, &stale] {
            write(file, &Pcm16::silence(10, 24_000).to_wav()).unwrap();
        }
        let long_ago = SystemTime::now() - Duration::from_secs(10 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        assert_eq!(purge_older_than(dir.path(), Duration::from_secs(86_400)), 1);
        assert!(fresh.exists() && !stale.exists());
    }
}
