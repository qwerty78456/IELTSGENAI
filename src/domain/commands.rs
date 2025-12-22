//! Command objects (DTOs)

use super::types::*;
use serde::{Deserialize, Serialize};

/// The input payload to trigger generation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationRequest {
    /// Which section to generate
    pub section: ListeningSection,
    /// The topic of the content
    pub topic: String,
}

impl GenerationRequest {
    /// Validates that the request conforms to domain rules
    pub fn validate(&self) -> Result<(), String> {
        // Topic must not be empty
        if self.topic.trim().is_empty() {
            return Err("Topic cannot be empty".to_string());
        }

        Ok(())
    }

    /// Generates default speaker configurations based on the section type
    pub fn generate_default_speakers(&self) -> Vec<SpeakerConfig> {
        match self.section {
            ListeningSection::Section1 => vec![
                SpeakerConfig {
                    name: "Speaker A".to_string(),
                    gender: Gender::Female,
                    accent: Accent::British,
                    role: SpeakerRole::Receptionist,
                },
                SpeakerConfig {
                    name: "Speaker B".to_string(),
                    gender: Gender::Male,
                    accent: Accent::American,
                    role: SpeakerRole::Other("Customer".to_string()),
                },
            ],
            ListeningSection::Section2 => vec![SpeakerConfig {
                name: "Speaker A".to_string(),
                gender: Gender::Male,
                accent: Accent::British,
                role: SpeakerRole::Guide,
            }],
            ListeningSection::Section3 => vec![
                SpeakerConfig {
                    name: "Speaker A".to_string(),
                    gender: Gender::Female,
                    accent: Accent::Australian,
                    role: SpeakerRole::Student,
                },
                SpeakerConfig {
                    name: "Speaker B".to_string(),
                    gender: Gender::Male,
                    accent: Accent::British,
                    role: SpeakerRole::Professor,
                },
            ],
            ListeningSection::Section4 => vec![SpeakerConfig {
                name: "Speaker A".to_string(),
                gender: Gender::Male,
                accent: Accent::American,
                role: SpeakerRole::Professor,
            }],
        }
    }
}
