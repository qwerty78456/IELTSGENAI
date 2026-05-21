//! Script generation service using Google Gemini API
use dioxus::prelude::*;
use crate::domain::{ListeningSection, SpeakerConfig};

#[cfg(feature = "server")]
use crate::domain::{SpeakerRole, Accent, Gender};

#[cfg(feature = "server")]
use super::{api_config, rate_limiter};

#[cfg(feature = "server")]
mod gemini_types {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct GeminiRequest {
        pub contents: Vec<Content>,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Content {
        pub parts: Vec<Part>,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Part {
        pub text: String,
    }

    #[derive(Deserialize, Serialize, Clone, Debug)]
    pub struct GeminiResponse {
        pub candidates: Vec<Candidate>,
    }

    #[derive(Deserialize, Serialize, Clone, Debug)]
    pub struct Candidate {
        pub content: ContentResponse,
    }

    #[derive(Deserialize, Serialize, Clone, Debug)]
    pub struct ContentResponse {
        pub parts: Vec<PartResponse>,
    }

    #[derive(Deserialize, Serialize, Clone, Debug)]
    pub struct PartResponse {
        pub text: String,
    }
}

#[cfg(feature = "server")]
use gemini_types::*;

/// Generate an IELTS Listening script
#[server]
pub async fn generate_script(
    section: ListeningSection,
    topic: String,
    speakers: Vec<SpeakerConfig>,
) -> Result<String, ServerFnError> {
    
    // Server-side logic only
    #[cfg(feature = "server")]
    {
        // Check rate limit
        if let Err(e) = rate_limiter::check_script_rate_limit() {
            return Err(ServerFnError::new(e));
        }

        let prompt = build_script_prompt(section, &topic, &speakers);

        let api_key = api_config::get_api_key().map_err(|e| ServerFnError::new(e))?;
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-flash-latest:generateContent?key={}",
            api_key
        );

        let request_body = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: prompt,
                }],
            }],
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| ServerFnError::new(format!("Failed to create HTTP client: {}", e)))?;
        let mut retries = 0;
        let max_retries = 3;
        let mut backoff_ms = 1000;

        let response = loop {
            match client.post(&url).json(&request_body).send().await {
                Ok(r) => {
                    if r.status().is_success() {
                        break r;
                    } else if r.status().as_u16() == 429 || r.status().as_u16() == 503 {
                        if retries >= max_retries {
                            return Err(ServerFnError::new("The server is currently experiencing heavy load. Please try again later."));
                        }
                        tracing::warn!("API limit/unavailable (status {}). Retrying in {}ms...", r.status(), backoff_ms);
                        tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
                        retries += 1;
                        backoff_ms *= 2;
                    } else {
                        let status = r.status();
                        let error_text = r.text().await.unwrap_or_default();
                        return Err(ServerFnError::new(format!("API Connection Error ({}): {}", status, error_text)));
                    }
                }
                Err(e) => {
                    if e.is_timeout() {
                        if retries >= max_retries {
                            return Err(ServerFnError::new("Connection timeout. The server is currently experiencing heavy load. Please try again later."));
                        }
                        tracing::warn!("API timeout. Retrying in {}ms...", backoff_ms);
                        tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
                        retries += 1;
                        backoff_ms *= 2;
                    } else {
                        return Err(ServerFnError::new(format!("Network error: {}", e)));
                    }
                }
            }
        };

        let gemini_response: GeminiResponse = response
            .json()
            .await
            .map_err(|e| ServerFnError::new(format!("Failed to parse response: {}", e)))?;

        return gemini_response
            .candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .map(|p| p.text.trim().to_string())
            .ok_or_else(|| ServerFnError::new("No response from API"));
    }

    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("This function should only be called on the server"))
}

#[cfg(feature = "server")]
fn build_script_prompt(section: ListeningSection, topic: &str, speakers: &[SpeakerConfig]) -> String {
    let section_description = match section {
        ListeningSection::Section1 => {
            "Section 1: Transactional Conversation - A two-way conversation between two people in an everyday social context \
            (e.g., booking appointments, asking for information about accommodation, travel arrangements)."
        }
        ListeningSection::Section2 => {
            "Section 2: Guided Monologue - A monologue set in an everyday social context \
            (e.g., a speech about local facilities, a talk about arrangements for meals during a conference)."
        }
        ListeningSection::Section3 => {
            "Section 3: Academic Discussion - A conversation between up to four people set in an educational or training context \
            (e.g., a university tutor and student discussing an assignment, or a group of students planning a research project)."
        }
        ListeningSection::Section4 => {
            "Section 4: Academic Lecture - A monologue on an academic subject \
            (e.g., a university lecture)."
        }
    };

    let duration = match section {
        ListeningSection::Section1 => "2.5 to 3 minutes",
        ListeningSection::Section2 => "2.5 to 3 minutes",
        ListeningSection::Section3 => "3 to 4 minutes",
        ListeningSection::Section4 => "3.5 to 4 minutes",
    };

    let mut speaker_descriptions = String::new();
    for speaker in speakers {
        let gender_str = match speaker.gender {
            Gender::Male => "Male",
            Gender::Female => "Female",
        };
        let accent_str = match speaker.accent {
            Accent::British => "British",
            Accent::American => "American",
            Accent::Australian => "Australian",
            Accent::Canadian => "Canadian",
            Accent::NewZealand => "New Zealand",
        };
        let role_str = match &speaker.role {
            SpeakerRole::Student => "Student",
            SpeakerRole::Professor => "Professor",
            SpeakerRole::Clerk => "Clerk",
            SpeakerRole::Receptionist => "Receptionist",
            SpeakerRole::Guide => "Guide",
            SpeakerRole::Other(custom) => custom.as_str(),
        };
        
        speaker_descriptions.push_str(&format!(
            "- {}: {}, {} accent, Role: {}\n",
            speaker.name, gender_str, accent_str, role_str
        ));
    }

    format!(
        r#"You are an expert IELTS test content creator. Generate a complete listening script for the following IELTS Listening test scenario.

SECTION TYPE: {}

TOPIC/SCENARIO: {}

SPEAKERS:
{}

REQUIREMENTS:
1. Create a complete, natural dialogue/monologue that would last approximately {}
2. The script must be COMPLETE with NO GAPS, NO BLANKS, and NO [FILL IN] markers
3. Include ALL spoken words exactly as they would be heard
4. Make the conversation/speech natural, realistic, and authentic
5. Include appropriate hesitations, filler words, and natural speech patterns where suitable
6. Ensure content difficulty and vocabulary are appropriate for IELTS Listening
7. The script should contain information that could later be used to create test questions (numbers, names, dates, specific details, opinions, main ideas)
8. CRITICAL FORMAT RULE: Each speaking turn MUST begin with the EXACT speaker label from the SPEAKERS list above (e.g. "Speaker A:", "Speaker B:"). Do NOT substitute role names, character names, or any other labels. Use ONLY "Speaker A:", "Speaker B:", etc.
9. Do NOT include any instructions, notes, or commentary - ONLY the spoken script

OUTPUT FORMAT:
Provide ONLY the listening script using the exact speaker labels (Speaker A, Speaker B, etc.) followed by their dialogue. Do not include any other text, explanations, or meta-commentary.

Generate the complete listening script now:"#,
        section_description,
        topic,
        speaker_descriptions,
        duration
    )
}
