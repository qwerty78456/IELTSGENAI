//! `voices.json`: gender + accent -> Gemini prebuilt voice name.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::{Accent, Gender};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        let to_map = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Self { male: to_map(&male), female: to_map(&female), announcer: default_announcer() }
    }
}

impl VoiceMappings {
    /// Reads the mapping file, writing the defaults there when it is missing.
    /// A malformed file falls back to the defaults with an error in the log.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
                tracing::error!("{} is not valid: {e}; using default voices", path.display());
                Self::default()
            }),
            Err(_) => {
                let defaults = Self::default();
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(json) = serde_json::to_string_pretty(&defaults) {
                    let _ = std::fs::write(path, json);
                }
                defaults
            }
        }
    }

    pub fn voice_for(&self, gender: Gender, accent: Accent) -> String {
        let map = match gender {
            Gender::Male => &self.male,
            Gender::Female => &self.female,
        };
        map.get(accent.key())
            .or_else(|| map.get("default"))
            .cloned()
            .unwrap_or_else(|| match gender {
                Gender::Male => "Puck".to_string(),
                Gender::Female => "Zephyr".to_string(),
            })
    }
}
