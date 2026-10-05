//! Designed voices (Voice Design): voices made from a teacher's description
//! and kept in the Google project of the API key.
//!
//! Google's list (`GET /v1beta/voices?type=prompted`) is the truth about
//! which designed voices this key can use; it is kept in memory for
//! `LIST_KEEP` and forgotten after every create or delete. The
//! `designed_voices` table of `jobs.db` remembers the voices this app made:
//! their exact accent (Google only keeps a language tag, and en-GB is both
//! British and Scottish English), and that the app may delete them. Voices
//! made elsewhere in the project (the PO's) are listed and usable, with the
//! accent their language tag gives, and are never deleted here.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use sqlx::sqlite::SqlitePool;

use crate::domain::{Accent, Gender, Voice, VoiceDesignRequest, VoiceSource};

use super::super::config::config;
use super::super::jobs::{JobStore, now_secs};
use super::super::llm::{CatalogVoice, GeminiClient, LlmError, VoiceDesign, VoiceQuery};
use super::samples::{SampleFile, make_sample, remove_sample, store_sample};
use super::synthesize::TtsError;

/// How long a listing of the project's designed voices is reused.
const LIST_KEEP: Duration = Duration::from_secs(60);

/// Creates the `designed_voices` table; run at startup with the other tables.
pub(crate) async fn create_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS designed_voices (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            gender TEXT NOT NULL,
            accent TEXT NOT NULL,
            description TEXT NOT NULL,
            created_at_secs INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The voices this app designed, one row each.
pub struct DesignedVoiceStore {
    pool: SqlitePool,
}

type Row = (String, String, String, String, String);

impl DesignedVoiceStore {
    /// The table of the process-wide database.
    pub async fn global() -> DesignedVoiceStore {
        Self::with_pool(JobStore::global().await.pool().clone())
    }

    pub(crate) fn with_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Remembers a voice this app made (replacing a row with its id).
    pub async fn insert(&self, voice: &Voice) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR REPLACE INTO designed_voices
                (id, name, gender, accent, description, created_at_secs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(&voice.id)
        .bind(&voice.name)
        .bind(gender_key(voice.gender))
        .bind(voice.accent.key())
        .bind(&voice.description)
        .bind(now_secs())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Every voice this app made, newest first. Rows that no longer parse
    /// (none should) are skipped.
    pub async fn all(&self) -> Result<Vec<Voice>, sqlx::Error> {
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, name, gender, accent, description FROM designed_voices
             ORDER BY created_at_secs DESC, id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().filter_map(voice_from).collect())
    }

    /// Whether this app made voice `id`.
    pub async fn contains(&self, id: &str) -> Result<bool, sqlx::Error> {
        let found: Option<(String,)> =
            sqlx::query_as("SELECT id FROM designed_voices WHERE id = ?1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(found.is_some())
    }

    /// Forgets voice `id`; `false` when there was no such row.
    pub async fn remove(&self, id: &str) -> Result<bool, sqlx::Error> {
        let done = sqlx::query("DELETE FROM designed_voices WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }
}

fn gender_key(gender: Gender) -> &'static str {
    match gender {
        Gender::Female => "female",
        Gender::Male => "male",
    }
}

fn parse_gender(text: &str) -> Option<Gender> {
    match text.trim().to_ascii_lowercase().as_str() {
        "female" => Some(Gender::Female),
        "male" => Some(Gender::Male),
        _ => None,
    }
}

fn voice_from((id, name, gender, accent, description): Row) -> Option<Voice> {
    Some(Voice {
        id,
        name,
        gender: parse_gender(&gender)?,
        accent: Accent::from_key(&accent)?,
        source: VoiceSource::Designed,
        description,
    })
}

/// The last listing, for one Google project.
struct Listed {
    project: u64,
    at: Instant,
    voices: Vec<Voice>,
}

static LISTED: Mutex<Option<Listed>> = Mutex::new(None);

/// Makes the next `designed_voices` ask Google again.
pub fn forget_designed_list() {
    if let Ok(mut listed) = LISTED.lock() {
        *listed = None;
    }
}

fn cached_list(project: u64) -> Option<Vec<Voice>> {
    let listed = LISTED.lock().ok()?;
    listed
        .as_ref()
        .filter(|l| l.project == project && l.at.elapsed() < LIST_KEEP)
        .map(|l| l.voices.clone())
}

/// The designed voices of this key's Google project, usable by speakers of
/// their gender and accent. Free (a listing, reused for `LIST_KEEP`).
pub async fn designed_voices(client: &GeminiClient) -> Result<Vec<Voice>, TtsError> {
    let project = client.project_tag();
    if let Some(voices) = cached_list(project) {
        return Ok(voices);
    }
    let listed = client
        .list_voices(&VoiceQuery {
            voice_type: Some("prompted".into()),
            ..VoiceQuery::default()
        })
        .await?;
    let made = match DesignedVoiceStore::global().await.all().await {
        Ok(made) => made,
        Err(e) => {
            tracing::error!("cannot read the designed voices table: {e}");
            Vec::new()
        }
    };
    let voices = merge_designed(&listed, &made);
    if let Ok(mut cache) = LISTED.lock() {
        *cache = Some(Listed {
            project,
            at: Instant::now(),
            voices: voices.clone(),
        });
    }
    Ok(voices)
}

/// Google's listing as voices, in its order. A voice this app made keeps the
/// accent and name it was made with; any other gets the first accent of its
/// language tag (en-GB gives British English). Voices with an unusable id, an
/// unknown gender or a language that is not one of the app's accents are
/// left out: no speaker could use them.
pub(crate) fn merge_designed(listed: &[CatalogVoice], made: &[Voice]) -> Vec<Voice> {
    listed
        .iter()
        .filter(|v| v.voice_type == "prompted" || Voice::is_designed_id(&v.id))
        .filter(|v| Voice::check_id(&v.id).is_ok())
        .filter_map(|listed| {
            let ours = made.iter().find(|m| m.id == listed.id);
            let gender = parse_gender(&listed.gender).or(ours.map(|m| m.gender))?;
            let accent = match ours {
                Some(made) => made.accent,
                None => Accent::from_language_code(&listed.language_code)?,
            };
            let name = Some(listed.display_name.trim())
                .filter(|n| !n.is_empty())
                .map(str::to_string)
                .or_else(|| ours.map(|m| m.name.clone()))
                .unwrap_or_default();
            let description = Some(listed.description.trim())
                .filter(|d| !d.is_empty())
                .map(str::to_string)
                .or_else(|| ours.map(|m| m.description.clone()))
                .unwrap_or_default();
            Some(Voice {
                id: listed.id.clone(),
                name,
                gender,
                accent,
                source: VoiceSource::Designed,
                description,
            })
        })
        .collect()
}

/// The ids of the voices this app made: the only ones it may delete.
pub async fn app_made_voice_ids() -> Vec<String> {
    match DesignedVoiceStore::global().await.all().await {
        Ok(made) => made.into_iter().map(|v| v.id).collect(),
        Err(e) => {
            tracing::error!("cannot read the designed voices table: {e}");
            Vec::new()
        }
    }
}

/// A voice speakers may be given or previewed: one of the server's catalogue
/// (or the announcer), else a designed voice of this key's project. Library
/// ids are answered without asking Google.
pub async fn find_voice(client: &GeminiClient, id: &str) -> Result<Voice, TtsError> {
    Voice::check_id(id)?;
    let catalog = &config().voices;
    if let Some(voice) = catalog.find(id) {
        return Ok(voice.clone());
    }
    let designed = if Voice::is_designed_id(id) {
        designed_voices(client).await?
    } else {
        Vec::new()
    };
    known_voice(id, catalog.voices(), &designed)
        .ok_or_else(|| TtsError::Llm(LlmError::UnknownVoice(id.to_string())))
}

/// `id` among `library` then `designed`.
pub(crate) fn known_voice(id: &str, library: &[Voice], designed: &[Voice]) -> Option<Voice> {
    library
        .iter()
        .chain(designed)
        .find(|voice| voice.id == id)
        .cloned()
}

/// Designs a voice from the teacher's request, remembers it as made here
/// (with its accent) and stores its sample: Google's own, or, if it sent
/// none, a sample made the usual way. The caller records the client's
/// usage. The voice is made for the accent's language tag.
pub async fn design_voice(
    client: &GeminiClient,
    request: &VoiceDesignRequest,
) -> Result<(Voice, SampleFile), TtsError> {
    request.validate()?;
    let request = request.cleaned();
    let created = client
        .create_voice(&VoiceDesign {
            display_name: request.name.clone(),
            gender: request.gender,
            language_code: request.accent.language_code().to_string(),
            description: request.description.clone(),
        })
        .await?;
    let name = Some(created.display_name.trim())
        .filter(|n| !n.is_empty())
        .map_or(request.name.clone(), str::to_string);
    let voice = Voice {
        id: created.id,
        name,
        gender: request.gender,
        accent: request.accent,
        source: VoiceSource::Designed,
        description: request.description,
    };
    // The voice exists at Google from here on, whatever happens to its sample.
    if let Err(e) = DesignedVoiceStore::global().await.insert(&voice).await {
        tracing::error!(voice = %voice.id, "cannot remember the designed voice: {e}");
    }
    forget_designed_list();
    tracing::info!(voice = %voice.id, accent = voice.accent.key(), "designed a voice");
    let sample = match &created.sample {
        Some(pcm) => store_sample(&voice.id, pcm).await?,
        None => make_sample(client, &voice).await?,
    };
    Ok((voice, sample))
}

/// Why voice `id` cannot be deleted here, or `None` when it can: only a
/// designed voice this app made.
pub async fn deletion_refusal(store: &DesignedVoiceStore, id: &str) -> Option<String> {
    if Voice::check_id(id).is_err() || !Voice::is_designed_id(id) {
        return Some("Only designed voices can be deleted.".into());
    }
    match store.contains(id).await {
        Ok(true) => None,
        Ok(false) => Some(
            "Only voices designed in this app can be deleted here; manage the others in Google AI Studio."
                .into(),
        ),
        Err(e) => {
            tracing::error!("cannot read the designed voices table: {e}");
            Some("The list of voices made here could not be read; try again.".into())
        }
    }
}

/// Deletes a designed voice this app made: at Google, its sample, and its
/// row. A refusal that reads "unknown voice" (a 403 or 404) counts as
/// deleted only when the project's list confirms the voice is gone, so a
/// voice Google still holds is never forgotten here.
pub async fn delete_designed_voice(client: &GeminiClient, id: &str) -> Result<(), TtsError> {
    let store = DesignedVoiceStore::global().await;
    if let Some(reason) = deletion_refusal(&store, id).await {
        return Err(TtsError::Refused(reason));
    }
    match client.delete_voice(id).await {
        Ok(()) => {}
        Err(LlmError::UnknownVoice(_)) => {
            let listed = client
                .list_voices(&VoiceQuery {
                    voice_type: Some("prompted".into()),
                    ..VoiceQuery::default()
                })
                .await?;
            if listed.iter().any(|voice| voice.id == id) {
                return Err(TtsError::Refused(
                    "Google would not delete this voice for this API key; it is still in the project. Delete it in Google AI Studio.".into(),
                ));
            }
        }
        Err(e) => return Err(e.into()),
    }
    forget_designed_list();
    remove_sample(id).await;
    if let Err(e) = store.remove(id).await {
        tracing::error!(voice = %id, "cannot forget the deleted voice: {e}");
    }
    tracing::info!(voice = %id, "deleted a designed voice");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;

    async fn table() -> (tempfile::TempDir, DesignedVoiceStore) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("jobs.db"))
            .create_if_missing(true);
        let jobs = JobStore::open_options(options).await.unwrap();
        (dir, DesignedVoiceStore::with_pool(jobs.pool().clone()))
    }

    fn designed(id: &str, gender: Gender, accent: Accent) -> Voice {
        Voice {
            id: id.into(),
            name: format!("Name of {id}"),
            gender,
            accent,
            source: VoiceSource::Designed,
            description: "A calm voice from Glasgow.".into(),
        }
    }

    fn listed(id: &str, gender: &str, language: &str) -> CatalogVoice {
        CatalogVoice {
            id: id.into(),
            display_name: format!("Listed {id}"),
            description: format!("Prompt of {id}"),
            gender: gender.into(),
            language_code: language.into(),
            voice_type: "prompted".into(),
            ..CatalogVoice::default()
        }
    }

    #[tokio::test]
    async fn designed_voices_table_round_trips() {
        let (_dir, store) = table().await;
        assert!(store.all().await.unwrap().is_empty());
        let scottish = designed("voice_abc123", Gender::Female, Accent::Scottish);
        let indian = designed("voice_def456", Gender::Male, Accent::Indian);
        store.insert(&scottish).await.unwrap();
        store.insert(&indian).await.unwrap();
        let mut all = store.all().await.unwrap();
        all.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(all, [scottish.clone(), indian.clone()]);
        assert!(store.contains("voice_abc123").await.unwrap());
        assert!(!store.contains("voice_60zf03beui2x").await.unwrap());

        // Inserting again replaces; removing twice removes once.
        let renamed = Voice {
            name: "Mrs Reid".into(),
            ..scottish.clone()
        };
        store.insert(&renamed).await.unwrap();
        assert_eq!(store.all().await.unwrap().len(), 2);
        assert!(store.remove("voice_abc123").await.unwrap());
        assert!(!store.remove("voice_abc123").await.unwrap());
        assert_eq!(store.all().await.unwrap(), [indian]);
    }

    #[tokio::test]
    async fn delete_refuses_ids_the_app_did_not_create() {
        let (_dir, store) = table().await;
        store
            .insert(&designed(
                "voice_kwq20yi2gjin",
                Gender::Female,
                Accent::British,
            ))
            .await
            .unwrap();
        // The PO's voices exist in the project, but this app did not make them.
        for id in ["voice_60zf03beui2x", "voice_nva3gh8uk4cg"] {
            let reason = deletion_refusal(&store, id).await.unwrap();
            assert!(reason.contains("designed in this app"), "{reason}");
        }
        // Library voices and things that are not voice ids are never deleted.
        for id in ["en-gb-advisor-1", "Zephyr", "../jobs.db", ""] {
            assert!(deletion_refusal(&store, id).await.is_some(), "{id}");
        }
        assert_eq!(deletion_refusal(&store, "voice_kwq20yi2gjin").await, None);
    }

    #[test]
    fn the_apps_accent_wins_over_the_language_tag() {
        let made = [designed("voice_ours1", Gender::Female, Accent::Scottish)];
        let listing = [
            listed("voice_ours1", "female", "en-GB"),
            listed("voice_60zf03beui2x", "female", "en-GB"),
            listed("voice_irish1", "male", "en-IE"),
            // Unusable here: a language the app has no accent for, no gender,
            // an id that is not a voice id.
            listed("voice_french1", "female", "fr-FR"),
            listed("voice_nogender", "", "en-GB"),
            listed("voice/../x", "female", "en-GB"),
        ];
        let voices = merge_designed(&listing, &made);
        let summary: Vec<(&str, Gender, Accent)> = voices
            .iter()
            .map(|v| (v.id.as_str(), v.gender, v.accent))
            .collect();
        assert_eq!(
            summary,
            [
                ("voice_ours1", Gender::Female, Accent::Scottish),
                ("voice_60zf03beui2x", Gender::Female, Accent::British),
                ("voice_irish1", Gender::Male, Accent::Irish),
            ]
        );
        assert!(voices.iter().all(|v| v.source == VoiceSource::Designed));
        assert_eq!(voices[1].name, "Listed voice_60zf03beui2x");
        assert_eq!(voices[1].description, "Prompt of voice_60zf03beui2x");
        // A voice this app made but Google no longer lists is not offered.
        let gone = merge_designed(&listing[1..2], &made);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].id, "voice_60zf03beui2x");
    }

    #[test]
    fn preview_refuses_unknown_ids() {
        let library = super::super::voices::VoiceCatalog::builtin();
        let designed_list = [designed(
            "voice_kwq20yi2gjin",
            Gender::Female,
            Accent::British,
        )];
        let pooled = &library.voices()[0];
        assert_eq!(
            known_voice(&pooled.id, library.voices(), &designed_list).as_ref(),
            Some(pooled)
        );
        assert_eq!(
            known_voice("voice_kwq20yi2gjin", library.voices(), &designed_list),
            Some(designed_list[0].clone())
        );
        // Any other library id, and a designed voice of another project, are refused.
        for unknown in ["en-gb-not-pooled-1", "voice_doesnotexist0000", "Zephyr2"] {
            assert_eq!(
                known_voice(unknown, library.voices(), &designed_list),
                None,
                "{unknown}"
            );
        }
    }
}
