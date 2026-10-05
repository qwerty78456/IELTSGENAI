//! Speakers: the people heard in a passage, and the voice each is read with.

use serde::{Deserialize, Serialize};

use super::voice::{Voice, VoiceChoice};

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

/// A speaker's accent. Saved exams name these variants, so a variant is never
/// removed; new ones are only added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Accent {
    British,
    American,
    Australian,
    Canadian,
    NewZealand,
    Irish,
    Scottish,
    SouthAfrican,
    Indian,
}

impl Accent {
    /// Every accent, in UI order.
    pub const ALL: [Accent; 9] = [
        Accent::British,
        Accent::American,
        Accent::Australian,
        Accent::Canadian,
        Accent::NewZealand,
        Accent::Irish,
        Accent::Scottish,
        Accent::SouthAfrican,
        Accent::Indian,
    ];

    /// Human label used in prompts and the UI.
    pub fn label(self) -> &'static str {
        match self {
            Accent::British => "British English",
            Accent::American => "American English",
            Accent::Australian => "Australian English",
            Accent::Canadian => "Canadian English",
            Accent::NewZealand => "New Zealand English",
            Accent::Irish => "Irish English",
            Accent::Scottish => "Scottish English",
            Accent::SouthAfrican => "South African English",
            Accent::Indian => "Indian English",
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
            Accent::Irish => "irish",
            Accent::Scottish => "scottish",
            Accent::SouthAfrican => "southafrican",
            Accent::Indian => "indian",
        }
    }

    pub fn from_key(key: &str) -> Option<Accent> {
        Accent::ALL.into_iter().find(|a| a.key() == key)
    }

    /// BCP-47 language tag of this English ("en-GB"). Scottish English shares
    /// en-GB with British English.
    pub fn language_code(self) -> &'static str {
        match self {
            Accent::British => "en-GB",
            Accent::American => "en-US",
            Accent::Australian => "en-AU",
            Accent::Canadian => "en-CA",
            Accent::NewZealand => "en-NZ",
            Accent::Irish => "en-IE",
            Accent::Scottish => "en-GB",
            Accent::SouthAfrican => "en-ZA",
            Accent::Indian => "en-IN",
        }
    }

    /// The first accent, in `ALL` order, with this language tag (case and
    /// '-' or '_' do not matter), so en-GB gives British English.
    pub fn from_language_code(code: &str) -> Option<Accent> {
        let code = code.trim().replace('_', "-");
        Accent::ALL
            .into_iter()
            .find(|a| a.language_code().eq_ignore_ascii_case(&code))
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

    /// How this role sounds, a few words for the speech style. Short and the
    /// same for every turn, as long or changing styles make voices drift; never
    /// age, gender, accent or a name, which the voice carries.
    pub fn delivery_style(&self) -> &'static str {
        match self {
            SpeakerRole::Student => "friendly and curious",
            SpeakerRole::Professor => "measured and explanatory",
            SpeakerRole::Clerk => "polite and efficient",
            SpeakerRole::Receptionist => "polite and helpful",
            SpeakerRole::Guide => "warm and engaging",
            SpeakerRole::Host => "warm and welcoming",
            SpeakerRole::Expert => "confident and knowledgeable",
            SpeakerRole::Guest => "relaxed and conversational",
            SpeakerRole::Reporter => "clear and informative",
            SpeakerRole::Narrator => "calm and clear",
            SpeakerRole::Other(_) => "natural and conversational",
        }
    }
}

/// One speaker of a passage. `label` is the turn marker used in scripts and
/// in the TTS request ("Speaker A"), never a character name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeakerConfig {
    pub label: String,
    pub gender: Gender,
    pub accent: Accent,
    pub role: SpeakerRole,
    /// The TTS voice. Speakers saved before 0.8 load as `Auto`.
    #[serde(default)]
    pub voice: VoiceChoice,
}

impl SpeakerConfig {
    /// A speaker without a voice yet (`VoiceChoice::Auto`).
    pub fn new(
        label: impl Into<String>,
        gender: Gender,
        accent: Accent,
        role: SpeakerRole,
    ) -> Self {
        Self {
            label: label.into(),
            gender,
            accent,
            role,
            voice: VoiceChoice::Auto,
        }
    }

    /// The teacher picks `voice`. The speaker takes its gender and accent, so
    /// a chosen voice always fits.
    pub fn with_voice(self, voice: Voice) -> Self {
        Self {
            gender: voice.gender,
            accent: voice.accent,
            voice: VoiceChoice::Chosen(voice),
            ..self
        }
    }

    pub fn voice_id(&self) -> Option<&str> {
        self.voice.id()
    }

    /// "Female, British English, Host".
    pub fn profile(&self) -> String {
        format!(
            "{}, {}, {}",
            self.gender.label(),
            self.accent.label(),
            self.role.label()
        )
    }

    /// One-line description for prompts: "Speaker A: Female, British English, Host".
    /// Never names the voice: a voice name ("Daniel") would turn up in the
    /// script as a character.
    pub fn describe(&self) -> String {
        format!("{}: {}", self.label, self.profile())
    }

    /// For transcripts and the answer key, never the student paper:
    /// "Speaker A: Female, British English, Host; voice Oliver".
    pub fn describe_with_voice(&self) -> String {
        match self.voice.voice() {
            Some(voice) => format!("{}; voice {}", self.describe(), voice.display_name()),
            None => self.describe(),
        }
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
        assert_eq!(
            SpeakerRole::from_key("other", "Customer").label(),
            "Customer"
        );
    }

    #[test]
    fn accent_keys_round_trip_and_name_a_language() {
        for accent in Accent::ALL {
            assert_eq!(Accent::from_key(accent.key()), Some(accent));
            assert!(accent.language_code().starts_with("en-"), "{accent:?}");
            assert!(Accent::from_language_code(accent.language_code()).is_some());
        }
        let keys: std::collections::HashSet<_> = Accent::ALL.iter().map(|a| a.key()).collect();
        let labels: std::collections::HashSet<_> = Accent::ALL.iter().map(|a| a.label()).collect();
        assert_eq!((keys.len(), labels.len()), (9, 9));
        assert_eq!(Accent::from_key("southafrican"), Some(Accent::SouthAfrican));
        assert_eq!(Accent::Irish.language_code(), "en-IE");
        assert_eq!(Accent::Indian.label(), "Indian English");
        // Scottish shares en-GB; the tag gives British English.
        assert_eq!(Accent::from_language_code("en-GB"), Some(Accent::British));
        assert_eq!(
            Accent::from_language_code("en_za"),
            Some(Accent::SouthAfrican)
        );
        assert_eq!(Accent::from_language_code("fr-FR"), None);
    }

    fn library_voice(gender: Gender, accent: Accent) -> Voice {
        Voice {
            id: "en-ie-storyteller-1".into(),
            name: "Oliver".into(),
            gender,
            accent,
            source: crate::domain::voice::VoiceSource::Library,
            description: "Warm, steady".into(),
        }
    }

    #[test]
    fn describe_never_names_the_voice() {
        let speaker = SpeakerConfig::new(
            "Speaker A",
            Gender::Female,
            Accent::British,
            SpeakerRole::Host,
        );
        assert_eq!(
            speaker.describe(),
            "Speaker A: Female, British English, Host"
        );
        assert_eq!(speaker.describe_with_voice(), speaker.describe());
        let voiced = speaker.with_voice(library_voice(Gender::Male, Accent::Irish));
        assert_eq!(voiced.describe(), "Speaker A: Male, Irish English, Host");
        assert!(!voiced.describe().contains("Oliver"));
        assert!(!voiced.describe().contains("en-ie"));
        assert_eq!(
            voiced.describe_with_voice(),
            "Speaker A: Male, Irish English, Host; voice Oliver"
        );
    }

    #[test]
    fn choosing_a_voice_adopts_gender_and_accent() {
        let voice = library_voice(Gender::Male, Accent::Irish);
        let speaker = SpeakerConfig::new(
            "Speaker B",
            Gender::Female,
            Accent::British,
            SpeakerRole::Guest,
        )
        .with_voice(voice.clone());
        assert_eq!(
            (speaker.gender, speaker.accent),
            (Gender::Male, Accent::Irish)
        );
        assert_eq!(speaker.role, SpeakerRole::Guest);
        assert_eq!(speaker.voice, VoiceChoice::Chosen(voice));
        assert_eq!(speaker.voice_id(), Some("en-ie-storyteller-1"));
        assert_eq!(speaker.profile(), "Male, Irish English, Guest");
    }

    #[test]
    fn a_0_7_speaker_config_still_loads() {
        let speaker: SpeakerConfig = serde_json::from_str(
            r#"{"label": "Speaker B", "gender": "Male", "accent": "American", "role": {"Other": "Customer"}}"#,
        )
        .unwrap();
        assert_eq!(speaker.voice, VoiceChoice::Auto);
        assert_eq!(speaker.voice_id(), None);
    }

    #[test]
    fn delivery_styles_are_short_and_never_describe_the_speaker() {
        let mut roles: Vec<SpeakerRole> = SpeakerRole::PRESET_KEYS
            .iter()
            .map(|key| SpeakerRole::from_key(key, ""))
            .collect();
        roles.push(SpeakerRole::Other("Mayor".into()));
        for role in roles {
            let style = role.delivery_style();
            assert!(style.split_whitespace().count() <= 4, "{style}");
            for word in style.split_whitespace() {
                assert!(
                    ![
                        "british", "american", "accent", "male", "female", "man", "woman", "old",
                        "young", "elderly", "mayor",
                    ]
                    .contains(&word),
                    "{style}"
                );
            }
        }
    }
}
