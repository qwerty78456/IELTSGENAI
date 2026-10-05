//! The voice catalogue: which Google voices read which accent and gender.
//!
//! The built-in pools (`default_voices.json`, compiled in) list, for every
//! accent and gender, library voices of that region in preference order, plus
//! the announcer. `voices.json` (version 2) holds overrides only: a non-empty
//! list replaces the built-in list of its accent and gender, `announcer`
//! replaces the built-in announcer, and everything else follows the release,
//! so a new release improves the pools without rewriting anyone's file.
//!
//! A 0.7 file (no `version`; one classic voice per gender and accent) is still
//! parsed strictly, so a broken one still stops startup, but it is never used:
//! the classic voices are all General American, which was the bug. Left at
//! the 0.7 defaults it is renamed to `voices.0.7.json` and the version 2
//! template is written; a customised one is left alone and a startup notice
//! says why it is ignored.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::{Accent, Gender, Voice, VoiceSource};

/// The pools and announcer this release ships with.
const BUILTIN: &str = include_str!("default_voices.json");
/// Where a 0.7 file left at its defaults is moved.
const RETIRED_NAME: &str = "voices.0.7.json";
/// Pool order inside an accent.
const GENDERS: [Gender; 2] = [Gender::Female, Gender::Male];

/// The smallest pool that keeps the speakers of a part apart: four for
/// British English (HSG has three British speakers of one gender), three for
/// the other core accents, two for the accents added in 0.8.
pub fn minimum_pool(accent: Accent) -> usize {
    match accent {
        Accent::British => 4,
        Accent::American | Accent::Australian | Accent::Canadian | Accent::NewZealand => 3,
        Accent::Irish | Accent::Scottish | Accent::SouthAfrican | Accent::Indian => 2,
    }
}

/// `voices.json` version 2, and the compiled-in defaults in the same shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VoicesFile {
    version: u32,
    #[serde(default)]
    comment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    announcer: Option<PoolVoice>,
    /// Accent key ("british") to its lists.
    #[serde(default)]
    pools: BTreeMap<String, AccentPools>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccentPools {
    #[serde(default)]
    male: Vec<PoolVoice>,
    #[serde(default)]
    female: Vec<PoolVoice>,
}

impl AccentPools {
    fn list(&self, gender: Gender) -> &[PoolVoice] {
        match gender {
            Gender::Male => &self.male,
            Gender::Female => &self.female,
        }
    }
}

/// One voice in a file: a Gemini voice id, with the catalogue's display name
/// and description for the teacher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PoolVoice {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
}

impl VoicesFile {
    /// What a first run writes: no overrides, and how to add some.
    fn template() -> Self {
        let keys: Vec<&str> = Accent::ALL.iter().map(|a| a.key()).collect();
        Self {
            version: 2,
            comment: format!(
                "Voice overrides. Every accent and gender not listed here uses the voices built into this release. \
                 To replace one list, add for example \"pools\": {{\"british\": {{\"female\": [{{\"id\": \"en-gb-advisor-1\", \"name\": \"Authoritative Advisor 1\"}}]}}}}. \
                 Accent keys: {}. List at least 4 British voices per gender, 3 for the other accents, so that the speakers of a part never share one. \
                 \"announcer\": {{\"id\": \"...\"}} replaces the voice that reads the instructions. Ids are Gemini voice ids (GET /v1beta/voices). Restart after editing.",
                keys.join(", ")
            ),
            announcer: None,
            pools: BTreeMap::new(),
        }
    }
}

/// The 0.7 schema, read only to recognise an old file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct VoiceMappingsV1 {
    male: HashMap<String, String>,
    female: HashMap<String, String>,
    #[serde(default = "v1_announcer")]
    announcer: String,
}

fn v1_announcer() -> String {
    "Charon".to_string()
}

impl VoiceMappingsV1 {
    /// What 0.7 wrote on a first run.
    fn shipped() -> Self {
        let map = |pairs: [(&str, &str); 6]| {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        Self {
            male: map([
                ("british", "Puck"),
                ("american", "Orus"),
                ("australian", "Fenrir"),
                ("canadian", "Puck"),
                ("newzealand", "Fenrir"),
                ("default", "Puck"),
            ]),
            female: map([
                ("british", "Zephyr"),
                ("american", "Leda"),
                ("australian", "Aoede"),
                ("canadian", "Zephyr"),
                ("newzealand", "Aoede"),
                ("default", "Zephyr"),
            ]),
            announcer: v1_announcer(),
        }
    }

    /// The strict 0.7 parse: a file 0.7 refused is still refused.
    fn parse(path: &Path, bytes: &[u8]) -> Result<Self, String> {
        let voices: Self = serde_json::from_slice(bytes).map_err(|e| {
            format!(
                "{}: invalid voice JSON at line {}, column {} (check male/female maps and announcer).",
                path.display(),
                e.line(),
                e.column()
            )
        })?;
        for (gender, map) in [("male", &voices.male), ("female", &voices.female)] {
            if !map.contains_key("default")
                || map
                    .iter()
                    .any(|(key, voice)| key.trim().is_empty() || voice.trim().is_empty())
            {
                return Err(format!(
                    "{}: {gender} requires a default voice and nonempty mapping names and values",
                    path.display()
                ));
            }
        }
        if voices.announcer.trim().is_empty() {
            return Err(format!("{}: announcer must not be empty", path.display()));
        }
        Ok(voices)
    }
}

/// Every voice the app may give a speaker, by accent and gender, and the
/// announcer. Ids are unique, and the announcer is in no pool.
#[derive(Debug, Clone)]
pub struct VoiceCatalog {
    /// `Accent::ALL` order, female then male, each pool in preference order.
    voices: Vec<Voice>,
    pools: HashMap<(Accent, Gender), Range<usize>>,
    announcer: Voice,
    /// Accents and genders whose list came from the user's file.
    overridden: Vec<(Accent, Gender)>,
}

impl VoiceCatalog {
    /// The pools and announcer of this release.
    pub fn builtin() -> Self {
        Self::build(&builtin_file(), None, Path::new("default_voices.json"))
            .expect("default_voices.json is valid (checked by the tests)")
    }

    /// Writes the version 2 template unless the file exists.
    pub fn create_missing(path: &Path) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(&VoicesFile::template())
            .map_err(|_| "Cannot serialize the voices template")?;
        super::super::config::create_missing(path, &json)
    }

    /// Reads `voices.json`: version 2 overrides on top of the built-in pools,
    /// or a 0.7 file, which is checked and then ignored. Also returns notices
    /// for the startup log (a 0.7 file, a pool too small to keep speakers
    /// apart). Errors name the file and never echo its values.
    pub fn load(path: &Path) -> Result<(Self, Vec<String>), String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
            format!(
                "{}: invalid voice JSON at line {}, column {}. Fix it, or delete the file to get a new template.",
                path.display(),
                e.line(),
                e.column()
            )
        })?;
        if value.get("version").is_none() {
            let old = VoiceMappingsV1::parse(path, &bytes)?;
            let notice = if old == VoiceMappingsV1::shipped() {
                retire(path)
            } else {
                old_format_notice(path)
            };
            return Ok((Self::builtin(), vec![notice]));
        }
        let file: VoicesFile = serde_json::from_slice(&bytes).map_err(|e| {
            format!(
                "{}: invalid voice JSON at line {}, column {} (version 2 has \"version\", \"comment\", \"announcer\" and \"pools\").",
                path.display(),
                e.line(),
                e.column()
            )
        })?;
        if file.version != 2 {
            return Err(format!(
                "{}: this release reads voices.json version 2. Delete the file to get a new template.",
                path.display()
            ));
        }
        let catalog = Self::build(&builtin_file(), Some(&file), path)?;
        let notices = catalog.small_pools(path);
        Ok((catalog, notices))
    }

    /// Every pooled voice: `Accent::ALL` order, female then male, each pool
    /// in preference order. The announcer is not among them.
    pub fn voices(&self) -> &[Voice] {
        &self.voices
    }

    /// The voices for one accent and gender, in preference order.
    pub fn pool(&self, accent: Accent, gender: Gender) -> &[Voice] {
        self.pools
            .get(&(accent, gender))
            .map_or(&[], |range| &self.voices[range.clone()])
    }

    /// The voice that reads the instructions between parts. Never given to a
    /// speaker; its gender and accent are not checked.
    pub fn announcer(&self) -> &Voice {
        &self.announcer
    }

    /// A pooled voice or the announcer.
    pub fn find(&self, id: &str) -> Option<&Voice> {
        self.voices
            .iter()
            .chain(std::iter::once(&self.announcer))
            .find(|voice| voice.id == id)
    }

    /// `base` with the non-empty lists and the announcer of `overrides` laid
    /// over it. Duplicate ids keep their first place; the announcer leaves
    /// the pools.
    fn build(
        base: &VoicesFile,
        overrides: Option<&VoicesFile>,
        path: &Path,
    ) -> Result<Self, String> {
        let known: Vec<&str> = Accent::ALL.iter().map(|a| a.key()).collect();
        for file in std::iter::once(base).chain(overrides) {
            if file.pools.keys().any(|key| Accent::from_key(key).is_none()) {
                return Err(format!(
                    "{}: pools has an accent this release does not know; use {}.",
                    path.display(),
                    known.join(", ")
                ));
            }
        }
        let announcer = overrides
            .and_then(|file| file.announcer.as_ref())
            .or(base.announcer.as_ref())
            .ok_or_else(|| format!("{}: no announcer", path.display()))?;
        Voice::check_id(&announcer.id)
            .map_err(|_| format!("{}: the announcer's id is not a voice id", path.display()))?;
        let mut seen: Vec<&str> = vec![announcer.id.as_str()];
        let mut voices = Vec::new();
        let mut pools = HashMap::new();
        let mut overridden = Vec::new();
        for accent in Accent::ALL {
            for gender in GENDERS {
                let replaced = overrides
                    .and_then(|file| file.pools.get(accent.key()))
                    .map(|pools| pools.list(gender))
                    .filter(|list| !list.is_empty());
                if replaced.is_some() {
                    overridden.push((accent, gender));
                }
                let list = replaced
                    .or_else(|| base.pools.get(accent.key()).map(|p| p.list(gender)))
                    .unwrap_or_default();
                let start = voices.len();
                for entry in list {
                    Voice::check_id(&entry.id).map_err(|_| {
                        format!(
                            "{}: a voice id in pools.{}.{} is not a voice id (1 to 100 letters, digits, '-' or '_').",
                            path.display(),
                            accent.key(),
                            gender.label().to_lowercase()
                        )
                    })?;
                    if seen.contains(&entry.id.as_str()) {
                        continue;
                    }
                    seen.push(&entry.id);
                    voices.push(Voice {
                        id: entry.id.clone(),
                        name: entry.name.trim().to_string(),
                        gender,
                        accent,
                        source: VoiceSource::Library,
                        description: entry.description.trim().to_string(),
                    });
                }
                pools.insert((accent, gender), start..voices.len());
            }
        }
        Ok(Self {
            voices,
            pools,
            announcer: Voice {
                id: announcer.id.clone(),
                name: announcer.name.trim().to_string(),
                gender: Gender::Male,
                accent: Accent::British,
                source: VoiceSource::Library,
                description: announcer.description.trim().to_string(),
            },
            overridden,
        })
    }

    /// One notice per pool smaller than `minimum_pool`.
    fn small_pools(&self, path: &Path) -> Vec<String> {
        let mut notices = Vec::new();
        for accent in Accent::ALL {
            for gender in GENDERS {
                let (have, need) = (self.pool(accent, gender).len(), minimum_pool(accent));
                if have >= need {
                    continue;
                }
                let cell = format!(
                    "{} {} voice(s)",
                    gender.label().to_lowercase(),
                    accent.label()
                );
                notices.push(if self.overridden.contains(&(accent, gender)) {
                    format!(
                        "{}: only {have} {cell} in pools.{}.{}; list at least {need} so that the speakers of a part never share a voice.",
                        path.display(),
                        accent.key(),
                        gender.label().to_lowercase()
                    )
                } else {
                    format!(
                        "Only {have} {cell} are built in; speakers of one part with that accent and gender may run out of different voices."
                    )
                });
            }
        }
        notices
    }
}

fn builtin_file() -> VoicesFile {
    serde_json::from_str(BUILTIN).expect("default_voices.json is valid (checked by the tests)")
}

fn old_format_notice(path: &Path) -> String {
    format!(
        "{} is in the 0.7 format; the built-in 0.8 voices are used. Delete it to get the new template.",
        path.display()
    )
}

/// Moves a 0.7 file left at its defaults out of the way and writes the
/// version 2 template. Never fails startup: if either step fails, the file
/// stays and the notice says it is ignored.
fn retire(path: &Path) -> String {
    let target = path.with_file_name(RETIRED_NAME);
    if std::fs::symlink_metadata(&target).is_ok() || std::fs::rename(path, &target).is_err() {
        return old_format_notice(path);
    }
    match VoiceCatalog::create_missing(path) {
        Ok(()) => format!(
            "{} had the 0.7 default voices: it was renamed to {RETIRED_NAME} and a version 2 template was written. The built-in 0.8 voices are used.",
            path.display()
        ),
        Err(e) => format!(
            "{} had the 0.7 default voices and was renamed to {RETIRED_NAME}; the version 2 template could not be written ({e}). The built-in 0.8 voices are used.",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What 0.7 wrote on a first run (its key order varied).
    const SHIPPED_0_7: &str = r#"{
  "male": {"newzealand": "Fenrir", "default": "Puck", "british": "Puck", "american": "Orus", "canadian": "Puck", "australian": "Fenrir"},
  "female": {"british": "Zephyr", "default": "Zephyr", "australian": "Aoede", "newzealand": "Aoede", "american": "Leda", "canadian": "Zephyr"},
  "announcer": "Charon"
}"#;

    #[test]
    fn malformed_voice_structure_and_empty_names_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("voices.json");
        for json in [
            r#"{"male": {}, "female": {"default": "Zephyr"}}"#,
            r#"{"male": {"default": " "}, "female": {"default": "Zephyr"}}"#,
            r#"{"male": {"default": "Puck"}, "female": {"default": "Zephyr"}, "announcer": ""}"#,
            r#"{"male": {"default": "Puck"}, "female": {"default": "Zephyr"}, "anouncer": "Charon"}"#,
            r#"{"male": "secret-never-print", "female": {}}"#,
            r#"{"version": 3, "pools": {}}"#,
            r#"{"version": 2, "pools": {"british": {"female": [{"id": "secret-never-print", "voice": "x"}]}}}"#,
            r#"{"version": 2, "announcer": {"id": "secret never print"}}"#,
            "{broken",
        ] {
            std::fs::write(&path, json).unwrap();
            let error = VoiceCatalog::load(&path).unwrap_err();
            assert!(error.contains("voices.json"), "{error}");
            assert!(!error.contains("secret"), "{error}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), json);
        }
    }

    #[test]
    fn builtin_pools_meet_minimums() {
        let catalog = VoiceCatalog::builtin();
        for accent in Accent::ALL {
            for gender in GENDERS {
                let pool = catalog.pool(accent, gender);
                assert!(
                    pool.len() >= minimum_pool(accent),
                    "{accent:?} {gender:?}: {}",
                    pool.len()
                );
                assert!(pool.iter().all(|v| v.fits(gender, accent)));
            }
        }
        let mut ids: Vec<&str> = catalog.voices().iter().map(|v| v.id.as_str()).collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total, "ids are unique");
        assert!(ids.iter().all(|id| Voice::check_id(id).is_ok()));
        assert!(!ids.contains(&catalog.announcer().id.as_str()));
        assert!(catalog.voices().iter().all(|v| !v.name.is_empty()));
        assert!(catalog.small_pools(Path::new("voices.json")).is_empty());
        // Ids name their region: British voices are en-gb-*, never a classic.
        assert!(
            catalog
                .pool(Accent::British, Gender::Female)
                .iter()
                .all(|v| v.id.starts_with("en-gb-"))
        );
        assert!(catalog.announcer().id.starts_with("en-gb-"));
        assert_eq!(
            catalog.find(&catalog.announcer().id.clone()).map(|v| &v.id),
            Some(&catalog.announcer().id)
        );
    }

    #[test]
    fn every_accent_has_pools() {
        let file = builtin_file();
        assert_eq!(file.version, 2);
        let keys: Vec<&str> = file.pools.keys().map(String::as_str).collect();
        let mut expected: Vec<&str> = Accent::ALL.iter().map(|a| a.key()).collect();
        expected.sort();
        assert_eq!(keys, expected);
        for (key, pools) in &file.pools {
            assert!(!pools.male.is_empty() && !pools.female.is_empty(), "{key}");
        }
        // Pool order follows Accent::ALL, female first.
        let catalog = VoiceCatalog::builtin();
        assert_eq!(catalog.voices()[0].accent, Accent::British);
        assert_eq!(catalog.voices()[0].gender, Gender::Female);
        assert_eq!(catalog.voices().last().unwrap().accent, Accent::Indian);
    }

    #[test]
    fn first_load_of_the_template_uses_the_builtin_voices() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("voices.json");
        VoiceCatalog::create_missing(&path).unwrap();
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["version"], 2);
        assert!(
            written["comment"]
                .as_str()
                .unwrap()
                .contains("southafrican")
        );
        let (catalog, notices) = VoiceCatalog::load(&path).unwrap();
        assert!(notices.is_empty(), "{notices:?}");
        assert_eq!(catalog.voices(), VoiceCatalog::builtin().voices());
    }

    #[test]
    fn v1_default_file_is_renamed_custom_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("voices.json");
        std::fs::write(&path, SHIPPED_0_7).unwrap();
        let (catalog, notices) = VoiceCatalog::load(&path).unwrap();
        assert_eq!(catalog.voices(), VoiceCatalog::builtin().voices());
        assert!(notices[0].contains(RETIRED_NAME), "{notices:?}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(RETIRED_NAME)).unwrap(),
            SHIPPED_0_7
        );
        let (_, notices) = VoiceCatalog::load(&path).unwrap();
        assert!(notices.is_empty(), "the template is version 2: {notices:?}");

        let custom = SHIPPED_0_7.replace("\"british\": \"Zephyr\"", "\"british\": \"Kore\"");
        let other = dir.path().join("custom.json");
        std::fs::write(&other, &custom).unwrap();
        let (catalog, notices) = VoiceCatalog::load(&other).unwrap();
        assert_eq!(catalog.voices(), VoiceCatalog::builtin().voices());
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains("0.7 format"), "{notices:?}");
        assert!(notices[0].contains("custom.json"));
        assert_eq!(std::fs::read_to_string(&other).unwrap(), custom);

        // A default file whose backup name is taken is left alone too.
        std::fs::write(&path, SHIPPED_0_7).unwrap();
        let (_, notices) = VoiceCatalog::load(&path).unwrap();
        assert!(notices[0].contains("0.7 format"), "{notices:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SHIPPED_0_7);
    }

    #[test]
    fn v2_overrides_replace_only_their_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("voices.json");
        let builtin = VoiceCatalog::builtin();
        let announcer = builtin.announcer().id.clone();
        let json = format!(
            r#"{{"version": 2, "comment": "mine",
                "announcer": {{"id": "en-gb-advisor-1", "name": "Narrator"}},
                "pools": {{"british": {{"female": [
                    {{"id": "en-gb-tutor-4", "name": "First"}},
                    {{"id": "en-gb-tutor-4", "name": "Again"}},
                    {{"id": "en-gb-advisor-1"}},
                    {{"id": "{announcer}"}}
                ]}}, "irish": {{"male": []}}}}}}"#
        );
        std::fs::write(&path, &json).unwrap();
        let (catalog, notices) = VoiceCatalog::load(&path).unwrap();
        // Duplicates keep their first place; the (new) announcer leaves the pool.
        let ids: Vec<&str> = catalog
            .pool(Accent::British, Gender::Female)
            .iter()
            .map(|v| v.id.as_str())
            .collect();
        assert_eq!(ids, ["en-gb-tutor-4", &announcer]);
        assert_eq!(
            catalog.pool(Accent::British, Gender::Female)[0].name,
            "First"
        );
        assert_eq!(catalog.announcer().id, "en-gb-advisor-1");
        assert_eq!(catalog.announcer().name, "Narrator");
        // Every other list, the empty Irish one included, is the built-in one.
        for (accent, gender) in [
            (Accent::British, Gender::Male),
            (Accent::American, Gender::Female),
            (Accent::Irish, Gender::Male),
        ] {
            assert_eq!(catalog.pool(accent, gender), builtin.pool(accent, gender));
        }
        // Two British women are too few for a part of three.
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(notices[0].contains("pools.british.female"));
        assert!(notices[0].contains("at least 4"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), json);
    }

    #[test]
    fn unknown_accent_key_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("voices.json");
        let json = r#"{"version": 2, "pools": {"welsh-secret": {"male": [{"id": "en-gb-x"}]}}}"#;
        std::fs::write(&path, json).unwrap();
        let error = VoiceCatalog::load(&path).unwrap_err();
        assert!(error.contains("voices.json") && error.contains("southafrican"));
        assert!(!error.contains("welsh"), "{error}");
        let bad_id = r#"{"version": 2, "pools": {"british": {"male": [{"id": "../secret"}]}}}"#;
        std::fs::write(&path, bad_id).unwrap();
        let error = VoiceCatalog::load(&path).unwrap_err();
        assert!(error.contains("pools.british.male"), "{error}");
        assert!(!error.contains("secret"), "{error}");
    }

    /// Checks the shipped voices against Google's catalogue, then records a
    /// three-speaker Female British passage. Step 1 is free; step 2 costs
    /// about $0.01. Run on demand:
    /// `cargo test --features server --no-default-features voice_live_probe -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn voice_live_probe() {
        use crate::domain::{Passage, SpeakerConfig, SpeakerRole, assign_voices};
        use crate::infrastructure::llm::{GeminiClient, VoiceQuery};

        let key = std::env::var("GEMINI_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| super::super::super::config::windows_environment().remove("GEMINI_API_KEY"))
            .expect("GEMINI_API_KEY in the environment");
        let tts_model = std::env::var("GEMINI_TTS_MODEL").unwrap_or("gemini-3.8-flash-tts".into());
        let client =
            GeminiClient::new(key, "gemini-3.8-flash".into(), tts_model, "low".into()).unwrap();
        let catalog = VoiceCatalog::builtin();

        // 1. Free: every shipped id exists, with the gender and language of its pool.
        let mut listed = HashMap::new();
        let mut codes: Vec<&str> = Accent::ALL.iter().map(|a| a.language_code()).collect();
        codes.sort();
        codes.dedup();
        for code in codes {
            let query = VoiceQuery {
                language_code: Some(code.into()),
                ..VoiceQuery::default()
            };
            let voices = client.list_voices(&query).await.unwrap();
            assert!(
                voices.iter().all(|v| v.language_code == code),
                "the language filter is honoured"
            );
            println!("{code}: {} voices", voices.len());
            listed.extend(voices.into_iter().map(|v| (v.id.clone(), v)));
        }
        let wanted = catalog
            .voices()
            .iter()
            .chain(std::iter::once(catalog.announcer()));
        for voice in wanted {
            let found = listed
                .get(&voice.id)
                .unwrap_or_else(|| panic!("{} is not in Google's catalogue", voice.id));
            assert_eq!(
                found.language_code,
                voice.accent.language_code(),
                "{}",
                voice.id
            );
            assert_eq!(
                found.gender,
                voice.gender.label().to_lowercase(),
                "{}",
                voice.id
            );
            assert_eq!(found.voice_type, "prebuilt", "{}", voice.id);
        }
        println!(
            "{} pooled voices and the announcer exist",
            catalog.voices().len()
        );

        // 2. About $0.01: three Female British speakers get three voices.
        let speakers: Vec<SpeakerConfig> = ["Speaker A", "Speaker B", "Speaker C"]
            .into_iter()
            .map(|label| {
                SpeakerConfig::new(label, Gender::Female, Accent::British, SpeakerRole::Guest)
            })
            .collect();
        let assigned = assign_voices(&speakers, catalog.voices(), &[]);
        assert!(assigned.unvoiced.is_empty());
        let labels: Vec<String> = speakers.iter().map(|s| s.label.clone()).collect();
        let passage = Passage::parse(
            1,
            "probe",
            "Speaker A: Welcome to the museum.\nSpeaker B: Thank you, it looks lovely.\nSpeaker C: Shall we start upstairs?",
            &labels,
        )
        .unwrap();
        let plan = super::super::synthesize::plan_passage(&passage, &assigned.speakers).unwrap();
        let mut sent: Vec<&str> = plan
            .requests
            .iter()
            .flat_map(|r| r.voices.iter().map(|v| v.voice.as_str()))
            .collect();
        sent.sort();
        sent.dedup();
        assert_eq!(sent.len(), 3, "{sent:?}");
        let dir = std::env::temp_dir();
        for (index, request) in plan.requests.iter().enumerate() {
            let pcm = client.synthesize(request).await.unwrap();
            let file = dir.join(format!("voice-probe-{index}.wav"));
            std::fs::write(&file, pcm.to_wav()).unwrap();
            println!(
                "request {index}: {:?}, {:.1} s -> {}",
                request
                    .voices
                    .iter()
                    .map(|v| format!("{}={}", v.label, v.voice))
                    .collect::<Vec<_>>(),
                f64::from(pcm.duration_ms()) / 1000.0,
                file.display()
            );
        }
        println!("spent: {}", client.usage().cost_text());

        // 3. Optional, about $0.10 for 700 words: one real passage read as the app
        // reads it. PROBE_PASSAGE is a "Speaker A: ..." file, PROBE_SPEAKERS lists
        // gender:accent:role per label ("female:british:host,female:british:guest"),
        // PROBE_OUT the WAV to write. Every chunk is synthesised afresh.
        let (Ok(path), Ok(line_up), Ok(out)) = (
            std::env::var("PROBE_PASSAGE"),
            std::env::var("PROBE_SPEAKERS"),
            std::env::var("PROBE_OUT"),
        ) else {
            return;
        };
        let speakers: Vec<SpeakerConfig> = line_up
            .split(',')
            .enumerate()
            .map(|(index, spec)| {
                let parts: Vec<&str> = spec.split(':').collect();
                let gender = if parts[0] == "male" {
                    Gender::Male
                } else {
                    Gender::Female
                };
                let accent = Accent::from_key(parts[1]).expect("accent key");
                let role = SpeakerRole::from_key(parts[2], parts[2]);
                SpeakerConfig::new(crate::domain::speaker_label(index), gender, accent, role)
            })
            .collect();
        let assigned = assign_voices(&speakers, catalog.voices(), &[]);
        assert!(assigned.unvoiced.is_empty(), "{:?}", assigned.unvoiced);
        let labels: Vec<String> = speakers.iter().map(|s| s.label.clone()).collect();
        let text = std::fs::read_to_string(&path).unwrap();
        let passage = Passage::parse(1, "probe", &text, &labels).unwrap();
        for speaker in &assigned.speakers {
            println!("{} -> {:?}", speaker.describe(), speaker.voice_id());
        }
        // The plan `synthesize_passage` follows, without the speech cache (it
        // needs the server's configuration).
        let before = client.usage();
        let plan = super::super::synthesize::plan_passage(&passage, &assigned.speakers).unwrap();
        let mut pcm = crate::infrastructure::audio::Pcm16::silence(0, 24_000);
        for (request, gap_ms) in plan.requests.iter().zip(&plan.gaps_ms) {
            pcm.append(&crate::infrastructure::audio::Pcm16::silence(
                *gap_ms, 24_000,
            ));
            pcm.append(&client.synthesize(request).await.unwrap());
        }
        std::fs::write(&out, pcm.to_wav()).unwrap();
        let spent = client.usage();
        println!(
            "passage: {:.1} s -> {out}; {} requests, ${:.4}",
            f64::from(pcm.duration_ms()) / 1000.0,
            spent.requests - before.requests,
            (spent.micro_usd - before.micro_usd) as f64 / 1e6
        );
    }
}
