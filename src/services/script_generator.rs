//! Script generation service using Google Gemini API

use serde::{Deserialize, Serialize};
use crate::domain::{ListeningSection, SpeakerConfig, SpeakerRole, Accent, Gender};
use super::{api_config, rate_limiter};

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    content: ContentResponse,
}

#[derive(Deserialize)]
struct ContentResponse {
    parts: Vec<PartResponse>,
}

#[derive(Deserialize)]
struct PartResponse {
    text: String,
}

/// Generate an IELTS Listening script
pub async fn generate_script(
    section: ListeningSection,
    topic: &str,
    speakers: &[SpeakerConfig],
) -> Result<String, String> {
    // Check rate limit before making API call
    rate_limiter::check_script_rate_limit()?;

    let prompt = build_script_prompt(section, topic, speakers);

    let request_body = GeminiRequest {
        contents: vec![Content {
            parts: vec![Part {
                text: prompt,
            }],
        }],
    };

    #[cfg(target_arch = "wasm32")]
    {
        generate_script_wasm(request_body).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        generate_script_native(request_body).await
    }
}

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
            "- {}: {} {}, {} accent, Role: {}\n",
            speaker.name, gender_str, accent_str, accent_str, role_str
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
8. Format: Use "Speaker Name: [their complete dialogue]" for each speaking turn
9. Do NOT include any instructions, notes, or commentary - ONLY the spoken script

OUTPUT FORMAT:
Provide ONLY the listening script with speaker names and their complete dialogue. Do not include any other text, explanations, or meta-commentary.

Generate the complete listening script now:"#,
        section_description,
        topic,
        speaker_descriptions,
        duration
    )
}

#[cfg(target_arch = "wasm32")]
async fn generate_script_wasm(request_body: GeminiRequest) -> Result<String, String> {
    use gloo_net::http::Request;

    let api_key = api_config::get_api_key()?;
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        api_key
    );

    let response = Request::post(&url)
        .json(&request_body)
        .map_err(|e| format!("Failed to create request: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.ok() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("API request failed with status {}: {}", status, error_text));
    }

    let gemini_response: GeminiResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    gemini_response
        .candidates
        .first()
        .and_then(|c| c.content.parts.first())
        .map(|p| p.text.trim().to_string())
        .ok_or_else(|| "No response from API".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
async fn generate_script_native(request_body: GeminiRequest) -> Result<String, String> {
    let api_key = api_config::get_api_key()?;
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        api_key
    );

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("API request failed with status {}: {}", status, error_text));
    }

    let gemini_response: GeminiResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    gemini_response
        .candidates
        .first()
        .and_then(|c| c.content.parts.first())
        .map(|p| p.text.trim().to_string())
        .ok_or_else(|| "No response from API".to_string())
}
