//! Voices: the catalogue the browser assigns voices from, voice samples
//! ("Listen"), designed voices (Voice Design), and the server's own
//! assignment before a recording starts.
//!
//! The browser runs `domain::assign_voices` over the catalogue to show every
//! speaker its voice at once; `prepare_speakers` runs the same rule again on
//! the server, so a request that arrives without voices (or with voices this
//! server no longer has) still records with distinct, fitting voices.
//!
//! Designed voices live in the Google project of the API key. Anyone who
//! reaches the server may list and use them; creating or deleting one changes
//! the operator's project, so it is allowed only on a loopback bind (the rule
//! of `settings::local_server`), and only voices this app made are deleted.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{Voice, VoiceDesignRequest};

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

/// A voice just designed, with its sample (Google's own, so free to play).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignedVoice {
    pub voice: Voice,
    pub sample: VoiceSample,
}

/// One designed voice of the project, and whether this server may delete it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignedVoiceRow {
    pub voice: Voice,
    /// Made by this app, and this server is local.
    pub deletable: bool,
}

/// The designed voices speakers can use, and whether new ones can be made here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignedVoices {
    pub voices: Vec<DesignedVoiceRow>,
    /// The server is local (loopback), so voices may be designed and deleted.
    pub can_design: bool,
}

/// Why designing or deleting a voice is refused on this server, if it is.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub fn design_refusal(local: bool) -> Option<&'static str> {
    (!local).then_some(
        "This server is reachable from other computers, so it does not design or delete voices; do that in a copy of the app running on your own computer.",
    )
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

/// A short sample of a voice of this server's catalogue or a designed voice
/// of this key's project; any other id is refused. Made once and stored, so
/// every later listen is free: a designed voice's sample is Google's own
/// (free), a library voice's is recorded (one paid request, booked under
/// "voices" to `exam`, if any).
#[server]
pub async fn voice_preview(
    voice_id: String,
    exam: Option<Uuid>,
) -> Result<VoiceSample, ServerFnError> {
    use crate::application::{usage, user_error};
    use crate::domain::UsageStep;
    use crate::infrastructure::{
        config::config,
        llm::{GeminiClient, LlmError},
        rate_limiter, tts,
    };

    Voice::check_id(&voice_id).map_err(user_error)?;
    let voice = match config().voices.find(&voice_id) {
        Some(voice) => voice.clone(),
        // A designed voice is known only to the key's project.
        None => {
            let client = GeminiClient::from_config().map_err(user_error)?;
            tts::find_voice(&client, &voice_id)
                .await
                .map_err(|e| match e {
                    tts::TtsError::Llm(LlmError::UnknownVoice(_)) => ServerFnError::new(
                        "This voice is not in this server's voice list or among the designed voices of this API key.",
                    ),
                    // A refused key or an unreachable Google says so.
                    other => user_error(other),
                })?
        }
    };
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

/// Designs a voice from the teacher's description (Voice Design) in the
/// Google project of the API key, keeps its sample and returns both. Takes
/// about 20 s and costs about $0.02, booked under "voices" to `exam`
/// whatever the outcome. Only on a local server; rate-limited.
#[server]
pub async fn design_voice(
    request: VoiceDesignRequest,
    exam: Option<Uuid>,
) -> Result<DesignedVoice, ServerFnError> {
    use crate::application::{settings::local_server, usage, user_error};
    use crate::domain::UsageStep;
    use crate::infrastructure::{llm::GeminiClient, rate_limiter, tts};

    if let Some(reason) = design_refusal(local_server()) {
        return Err(ServerFnError::new(reason));
    }
    request.validate().map_err(user_error)?;
    rate_limiter::check(rate_limiter::Bucket::VoiceDesign).map_err(ServerFnError::new)?;
    let client = GeminiClient::from_config().map_err(user_error)?;
    let made = tts::design_voice(&client, &request).await;
    usage::record(UsageStep::Voices, exam, client.tts_model(), &client).await;
    let (voice, sample) = made.map_err(user_error)?;
    Ok(DesignedVoice {
        sample: VoiceSample {
            voice_id: voice.id.clone(),
            url: voice_sample_url(&voice.id),
            duration_ms: sample.duration_ms,
        },
        voice,
    })
}

/// The designed voices of the API key's Google project, each marked
/// deletable when this app made it and the server is local. Free.
#[server]
pub async fn designed_voices() -> Result<DesignedVoices, ServerFnError> {
    use crate::application::{settings::local_server, user_error};
    use crate::infrastructure::{llm::GeminiClient, tts};

    let client = GeminiClient::from_config().map_err(user_error)?;
    let voices = tts::designed_voices(&client).await.map_err(user_error)?;
    let local = local_server();
    let made_here = if local {
        tts::app_made_voice_ids().await
    } else {
        Vec::new()
    };
    Ok(DesignedVoices {
        voices: voices
            .into_iter()
            .map(|voice| DesignedVoiceRow {
                deletable: made_here.contains(&voice.id),
                voice,
            })
            .collect(),
        can_design: design_refusal(local).is_none(),
    })
}

/// Deletes a designed voice this app made, at Google and here (its sample
/// too). Voices made elsewhere in the project are refused. Only on a local
/// server; rate-limited with designing.
#[server]
pub async fn delete_voice(voice_id: String) -> Result<(), ServerFnError> {
    use crate::application::{settings::local_server, user_error};
    use crate::infrastructure::{llm::GeminiClient, rate_limiter, tts};

    if let Some(reason) = design_refusal(local_server()) {
        return Err(ServerFnError::new(reason));
    }
    Voice::check_id(&voice_id).map_err(user_error)?;
    rate_limiter::check(rate_limiter::Bucket::VoiceDesign).map_err(ServerFnError::new)?;
    let client = GeminiClient::from_config().map_err(user_error)?;
    tts::delete_designed_voice(&client, &voice_id)
        .await
        .map_err(user_error)
}

/// Gives every speaker of one part a voice from this server's catalogue
/// (`assign_voices`: teacher choices kept, fitting app picks kept, the rest
/// filled), preferring voices not in `elsewhere`, and checks that every
/// chosen designed voice is in this key's Google project. Runs before a
/// recording request is validated, so a missing voice fails at the start,
/// not halfway through. Errors are teacher-readable.
#[cfg(feature = "server")]
pub(crate) async fn prepare_speakers(
    speakers: Vec<crate::domain::SpeakerConfig>,
    elsewhere: &[String],
) -> Result<Vec<crate::domain::SpeakerConfig>, String> {
    use crate::infrastructure::config::config;

    let assigned = crate::domain::assign_voices(&speakers, config().voices.voices(), elsewhere);
    let speakers = checked(assigned)?;
    let project = project_voices(std::slice::from_ref(&speakers)).await?;
    match missing_designed(&speakers, &project) {
        Some(problem) => Err(problem),
        None => Ok(speakers),
    }
}

/// Assigns voices to every part of an exam the way the browser does
/// (`assign_exam_voices`); one line-up per part, in the same order, each
/// also checked for designed voices this key's project does not have.
#[cfg(feature = "server")]
pub(crate) async fn prepare_exam_speakers(
    parts: &[Vec<crate::domain::SpeakerConfig>],
) -> Vec<Result<Vec<crate::domain::SpeakerConfig>, String>> {
    use crate::infrastructure::config::config;

    let prepared: Vec<Result<Vec<crate::domain::SpeakerConfig>, String>> =
        crate::domain::assign_exam_voices(parts, config().voices.voices())
            .into_iter()
            .map(checked)
            .collect();
    let line_ups: Vec<Vec<crate::domain::SpeakerConfig>> = prepared
        .iter()
        .filter_map(|part| part.as_ref().ok().cloned())
        .collect();
    let project = project_voices(&line_ups).await;
    prepared
        .into_iter()
        .map(|part| {
            let speakers = part?;
            let project = project.as_ref().map_err(Clone::clone)?;
            match missing_designed(&speakers, project) {
                Some(problem) => Err(problem),
                None => Ok(speakers),
            }
        })
        .collect()
}

/// The ids of the designed voices of this key's project, asked of Google
/// (free, briefly cached) only when a speaker of `line_ups` has a designed
/// voice; empty otherwise.
#[cfg(feature = "server")]
async fn project_voices(
    line_ups: &[Vec<crate::domain::SpeakerConfig>],
) -> Result<Vec<String>, String> {
    use crate::infrastructure::{llm::GeminiClient, tts};

    let needed = line_ups
        .iter()
        .flatten()
        .filter_map(|s| s.voice.voice())
        .any(designed_voice);
    if !needed {
        return Ok(Vec::new());
    }
    let client = GeminiClient::from_config().map_err(|e| e.to_string())?;
    let voices = tts::designed_voices(&client)
        .await
        .map_err(|e| e.to_string())?;
    Ok(voices.into_iter().map(|v| v.id).collect())
}

/// A voice only the Google project it was designed in can use.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
fn designed_voice(voice: &Voice) -> bool {
    voice.source == crate::domain::VoiceSource::Designed || Voice::is_designed_id(&voice.id)
}

/// The first speaker whose designed voice is not among `project` (the
/// designed voice ids of this key's project), as the teacher should read it.
#[cfg_attr(not(feature = "server"), allow(dead_code))]
fn missing_designed(
    speakers: &[crate::domain::SpeakerConfig],
    project: &[String],
) -> Option<String> {
    speakers.iter().find_map(|speaker| {
        let voice = speaker.voice.voice()?;
        (designed_voice(voice) && !project.contains(&voice.id)).then(|| {
            format!(
                "{}'s designed voice \"{}\" is not in the Google project of this API key; choose another voice.",
                speaker.label,
                voice.display_name()
            )
        })
    })
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

    fn designed(id: &str, name: &str) -> Voice {
        Voice {
            id: id.into(),
            name: name.into(),
            gender: Gender::Female,
            accent: Accent::British,
            source: crate::domain::VoiceSource::Designed,
            description: String::new(),
        }
    }

    #[test]
    fn designed_voices_must_be_in_the_keys_project() {
        let catalog = VoiceCatalog::builtin();
        let line_up = line_up(2, Gender::Female, Accent::British);
        let speakers = vec![
            line_up[0].clone(),
            line_up[1]
                .clone()
                .with_voice(designed("voice_kwq20yi2gjin", "Probe teacher")),
        ];
        let prepared = checked(assign_voices(&speakers, catalog.voices(), &[])).unwrap();
        let project = vec!["voice_kwq20yi2gjin".to_string()];
        assert_eq!(missing_designed(&prepared, &project), None);
        assert_eq!(
            missing_designed(&prepared, &[]).unwrap(),
            "Speaker B's designed voice \"Probe teacher\" is not in the Google project of this API key; choose another voice."
        );
        // Library voices are never looked up in the project.
        assert_eq!(missing_designed(&prepared[..1], &[]), None);
    }

    #[test]
    fn only_a_local_server_designs_or_deletes_voices() {
        assert_eq!(design_refusal(true), None);
        let reason = design_refusal(false).unwrap();
        assert!(
            reason.contains("reachable from other computers"),
            "{reason}"
        );
    }
}
