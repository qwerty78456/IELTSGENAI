//! `voices.json`: gender + accent -> Gemini prebuilt voice name.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::{Accent, Gender};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceMappings {
    pub male: HashMap<String, String>,
    pub female: HashMap<String, String>,
    /// Voice used for announcements between parts.
    #[serde(default = "default_announcer")]
    pub announcer: String,
}

fn default_announcer() -> String {
    "Charon".to_string()
}

impl Default for VoiceMappings {
    fn default() -> Self {
        let male = [
            ("british", "Puck"),
            ("american", "Orus"),
            ("australian", "Fenrir"),
            ("canadian", "Puck"),
            ("newzealand", "Fenrir"),
            ("default", "Puck"),
        ];
        let female = [
            ("british", "Zephyr"),
            ("american", "Leda"),
            ("australian", "Aoede"),
            ("canadian", "Zephyr"),
            ("newzealand", "Aoede"),
            ("default", "Zephyr"),
        ];
        let to_map = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        Self {
            male: to_map(&male),
            female: to_map(&female),
            announcer: default_announcer(),
        }
    }
}

impl VoiceMappings {
    pub fn create_missing(path: &Path) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(&Self::default())
            .map_err(|_| "Cannot serialize default voices")?;
        super::super::config::create_missing(path, &json)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        let voices: Self = serde_json::from_slice(&bytes).map_err(|e| format!(
            "{}: invalid voice JSON at line {}, column {} (check male/female maps and announcer).",
            path.display(), e.line(), e.column()
        ))?;
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

    pub fn voice_for(&self, gender: Gender, accent: Accent) -> String {
        let map = match gender {
            Gender::Male => &self.male,
            Gender::Female => &self.female,
        };
        map.get(accent.key())
            .or_else(|| map.get("default"))
            .cloned()
            .expect("voice mappings validated before server starts")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ] {
            std::fs::write(&path, json).unwrap();
            let error = VoiceMappings::load(&path).unwrap_err();
            assert!(error.contains("voices.json"));
            assert!(!error.contains("secret-never-print"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), json);
        }
    }
}
