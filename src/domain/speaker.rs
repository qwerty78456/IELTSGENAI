//! Speakers: the voices that carry a passage.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Gender {
    Male,
    Female,
}

impl Gender {
    pub fn label(self) -> &'static str {
        match self {
            Gender::Male => "Male",
            Gender::Female => "Female",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Accent {
    British,
    American,
    Australian,
    Canadian,
    NewZealand,
}

impl Accent {
    pub const ALL: [Accent; 5] = [
        Accent::British,
        Accent::American,
        Accent::Australian,
        Accent::Canadian,
        Accent::NewZealand,
    ];

    /// Human label used in prompts and the UI.
    pub fn label(self) -> &'static str {
        match self {
            Accent::British => "British English",
            Accent::American => "American English",
            Accent::Australian => "Australian English",
            Accent::Canadian => "Canadian English",
            Accent::NewZealand => "New Zealand English",
        }
    }

    /// Stable key used in `voices.json` and form values.
    pub fn key(self) -> &'static str {
        match self {
            Accent::British => "british",
            Accent::American => "american",
            Accent::Australian => "australian",
            Accent::Canadian => "canadian",
            Accent::NewZealand => "newzealand",
        }
    }

    pub fn from_key(key: &str) -> Option<Accent> {
        Accent::ALL.into_iter().find(|a| a.key() == key)
    }
}

/// The role a voice plays inside the passage. Presets cover IELTS and news
/// formats; `Other` is free text chosen by the teacher.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpeakerRole {
    Student,
    Professor,
    Clerk,
    Receptionist,
    Guide,
    Host,
    Expert,
    Guest,
    Reporter,
    Narrator,
    Other(String),
}

impl SpeakerRole {
    /// Keys of the preset roles, in UI order. `other` is handled separately.
    pub const PRESET_KEYS: [&'static str; 10] = [
        "student",
        "professor",
        "clerk",
        "receptionist",
        "guide",
        "host",
        "expert",
        "guest",
        "reporter",
        "narrator",
    ];

    pub fn key(&self) -> &str {
        match self {
            SpeakerRole::Student => "student",
            SpeakerRole::Professor => "professor",
            SpeakerRole::Clerk => "clerk",
            SpeakerRole::Receptionist => "receptionist",
            SpeakerRole::Guide => "guide",
            SpeakerRole::Host => "host",
            SpeakerRole::Expert => "expert",
            SpeakerRole::Guest => "guest",
            SpeakerRole::Reporter => "reporter",
            SpeakerRole::Narrator => "narrator",
            SpeakerRole::Other(_) => "other",
        }
    }

    /// Builds a role from a form key; `custom` is only used for `other`.
    pub fn from_key(key: &str, custom: &str) -> SpeakerRole {
        match key {
            "student" => SpeakerRole::Student,
            "professor" => SpeakerRole::Professor,
            "clerk" => SpeakerRole::Clerk,
            "receptionist" => SpeakerRole::Receptionist,
            "guide" => SpeakerRole::Guide,
            "host" => SpeakerRole::Host,
            "expert" => SpeakerRole::Expert,
            "guest" => SpeakerRole::Guest,
            "reporter" => SpeakerRole::Reporter,
            "narrator" => SpeakerRole::Narrator,
            _ => SpeakerRole::Other(custom.to_string()),
        }
    }

    pub fn label(&self) -> String {
        match self {
            SpeakerRole::Other(custom) if !custom.trim().is_empty() => custom.clone(),
            SpeakerRole::Other(_) => "Other".to_string(),
            preset => {
                let key = preset.key();
                let mut chars = key.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
        }
    }
}

/// Configuration of one voice. `label` is the turn marker used in scripts and
/// in the TTS request ("Speaker A"), never a character name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeakerConfig {
    pub label: String,
    pub gender: Gender,
    pub accent: Accent,
    pub role: SpeakerRole,
}

impl SpeakerConfig {
    pub fn new(label: impl Into<String>, gender: Gender, accent: Accent, role: SpeakerRole) -> Self {
        Self { label: label.into(), gender, accent, role }
    }

    /// One-line description for prompts: "Speaker A: Female, British English, Host".
    pub fn describe(&self) -> String {
        format!("{}: {}, {}, {}", self.label, self.gender.label(), self.accent.label(), self.role.label())
    }
}

/// "Speaker A", "Speaker B", ... for index 0, 1, ...
pub fn speaker_label(index: usize) -> String {
    let letter = (b'A' + (index % 26) as u8) as char;
    format!("Speaker {letter}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_alphabet() {
        assert_eq!(speaker_label(0), "Speaker A");
        assert_eq!(speaker_label(2), "Speaker C");
    }

    #[test]
    fn role_keys_round_trip() {
        for key in SpeakerRole::PRESET_KEYS {
            assert_eq!(SpeakerRole::from_key(key, "").key(), key);
        }
        assert_eq!(SpeakerRole::from_key("other", "Customer").label(), "Customer");
    }
}
