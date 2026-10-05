//! Voices: the catalogue the browser assigns voices from, voice samples
//! ("Listen"), and the server's own assignment before a recording starts.
//!
//! The browser runs `domain::assign_voices` over the catalogue to show every
//! speaker its voice at once; `prepare_speakers` runs the same rule again on
//! the server, so a request that arrives without voices (or with voices this
//! server no longer has) still records with distinct, fitting voices.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::Voice;

/// Where a stored voice sample is streamed from (axum path syntax).
/// `infrastructure::startup` mounts `infrastructure::jobs::serve_voice_sample`
/// here; `voice_preview` returns URLs built with `voice_sample_url`. A plain
/// route for the same reason as `audio::AUDIO_ROUTE`.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub const VOICE_SAMPLE_ROUTE: &str = "/voice-sample/{voice_id}";

/// The URL of a voice's stored sample, for `<audio src>`.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub fn voice_sample_url(voice_id: &str) -> String {
    format!("/voice-sample/{voice_id}")
}

/// Every voice this server gives speakers, and the announcer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceCatalogue {
    /// `Accent::ALL` order, female then male, each pool in preference order:
    /// the order `assign_voices` and `next_voice` pick in.
    pub voices: Vec<Voice>,
    /// Reads the instructions between parts; never given to a speaker.
    pub announcer: Voice,
}

/// A voice's stored sample, ready to play.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSample {
    pub voice_id: String,
    pub url: String,
    pub duration_ms: u32,
}

/// The voice catalogue (built-in pools plus `voices.json` overrides). Free:
/// no Gemini request.
#[server]
pub async fn voice_catalogue() -> Result<VoiceCatalogue, ServerFnError> {
    use crate::infrastructure::config::config;

    let catalog = &config().voices;
    Ok(VoiceCatalogue {
        voices: catalog.voices().to_vec(),
        announcer: catalog.announcer().clone(),
    })
}

/// A short sample of a catalogue voice. Recorded once (one paid request,
/// booked under "voices" to `exam`, if any) and stored, so every later listen
/// is free. Only voices of this server's catalogue are recorded.
#[server]
pub async fn voice_preview(
    voice_id: String,
    exam: Option<Uuid>,
) -> Result<VoiceSample, ServerFnError> {
    use crate::application::{usage, user_error};
    use crate::domain::UsageStep;
    use crate::infrastructure::{config::config, llm::GeminiClient, rate_limiter, tts};

    Voice::check_id(&voice_id).map_err(user_error)?;
    let voice = config()
        .voices
        .find(&voice_id)
        .cloned()
        .ok_or_else(|| ServerFnError::new("This voice is not in this server's voice list"))?;
    let sample = match tts::stored_sample(&voice.id).await {
        Some(sample) => sample,
        None => {
            rate_limiter::check(rate_limiter::Bucket::VoiceSample).map_err(ServerFnError::new)?;
            let client = GeminiClient::from_config().map_err(user_error)?;
            let made = tts::make_sample(&client, &voice).await;
            usage::record(UsageStep::Voices, exam, client.tts_model(), &client).await;
            made.map_err(user_error)?
        }
    };
    Ok(VoiceSample {
        url: voice_sample_url(&voice.id),
        voice_id: voice.id,
        duration_ms: sample.duration_ms,
    })
}

/// Gives every speaker of one part a voice from this server's catalogue
/// (`assign_voices`: teacher choices kept, fitting app picks kept, the rest
/// filled), preferring voices not in `elsewhere`. Runs before a recording
/// request is validated. Errors are teacher-readable.
#[cfg(feature = "server")]
pub(crate) fn prepare_speakers(
    speakers: Vec<crate::domain::SpeakerConfig>,
    elsewhere: &[String],
) -> Result<Vec<crate::domain::SpeakerConfig>, String> {
    use crate::infrastructure::config::config;

    let assigned = crate::domain::assign_voices(&speakers, config().voices.voices(), elsewhere);
    checked(assigned)
}

/// Assigns voices to every part of an exam the way the browser does
/// (`assign_exam_voices`); one line-up per part, in the same order.
#[cfg(feature = "server")]
pub(crate) fn prepare_exam_speakers(
    parts: &[Vec<crate::domain::SpeakerConfig>],
) -> Vec<Result<Vec<crate::domain::SpeakerConfig>, String>> {
    use crate::infrastructure::config::config;

    crate::domain::assign_exam_voices(parts, config().voices.voices())
        .into_iter()
        .map(checked)
        .collect()
}

/// The assigned line-up, or why it cannot be recorded: a speaker no free
/// voice fits, or a clash between the teacher's choices.
#[cfg(feature = "server")]
fn checked(
    assigned: crate::domain::AssignedVoices,
) -> Result<Vec<crate::domain::SpeakerConfig>, String> {
    if let Some(speaker) = assigned
        .unvoiced
        .first()
        .and_then(|label| assigned.speakers.iter().find(|s| &s.label == label))
    {
        return Err(format!(
            "There are not enough different {} {} voices for this part; change one speaker's accent or gender.",
            speaker.gender.label().to_lowercase(),
            speaker.accent.label()
        ));
    }
    if let Some(conflict) = crate::domain::voice_conflicts(&assigned.speakers)
        .into_iter()
        .next()
    {
        return Err(conflict);
    }
    Ok(assigned.speakers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Accent, Gender, SpeakerConfig, SpeakerRole, assign_voices, speaker_label};
    use crate::infrastructure::tts::voices::VoiceCatalog;

    fn line_up(count: usize, gender: Gender, accent: Accent) -> Vec<SpeakerConfig> {
        (0..count)
            .map(|i| SpeakerConfig::new(speaker_label(i), gender, accent, SpeakerRole::Guest))
            .collect()
    }

    #[test]
    fn prepared_speakers_get_distinct_fitting_voices() {
        let catalog = VoiceCatalog::builtin();
        // HSG Part 1: three British women used to share one voice.
        let speakers = line_up(3, Gender::Female, Accent::British);
        let prepared = checked(assign_voices(&speakers, catalog.voices(), &[])).unwrap();
        let ids: std::collections::HashSet<&str> =
            prepared.iter().filter_map(|s| s.voice_id()).collect();
        assert_eq!(ids.len(), 3);
        for speaker in &prepared {
            let voice = speaker.voice.voice().unwrap();
            assert!(voice.fits(Gender::Female, Accent::British), "{voice:?}");
        }
        // What the browser assigned, the server keeps.
        let again = checked(assign_voices(&prepared, catalog.voices(), &[])).unwrap();
        assert_eq!(again, prepared);
    }

    #[test]
    fn too_few_voices_or_clashing_choices_are_refused() {
        let catalog = VoiceCatalog::builtin();
        let pool = catalog.pool(Accent::Indian, Gender::Male).len();
        let crowd = line_up(pool + 1, Gender::Male, Accent::Indian);
        let error = checked(assign_voices(&crowd, catalog.voices(), &[])).unwrap_err();
        assert_eq!(
            error,
            "There are not enough different male Indian English voices for this part; change one speaker's accent or gender."
        );

        let voice = catalog.pool(Accent::British, Gender::Female)[0].clone();
        let clash: Vec<SpeakerConfig> = line_up(2, Gender::Female, Accent::British)
            .into_iter()
            .map(|s| s.with_voice(voice.clone()))
            .collect();
        let error = checked(assign_voices(&clash, catalog.voices(), &[])).unwrap_err();
        assert!(error.contains("share the voice"), "{error}");
    }
}
