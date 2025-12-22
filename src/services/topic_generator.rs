//! Topic generation service using Google Gemini API

use serde::{Deserialize, Serialize};

const API_KEY: &str = "AIzaSyBq8ur94FNYK9odYENS4lC5YdS-k0MMdzM";

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

/// Generate a topic suggestion for IELTS Listening Practice
pub async fn generate_topic_suggestion(section: &str) -> Result<String, String> {
    let prompt = format!(
        "Generate a single, specific scenario description for an IELTS Listening {} practice exercise. \
        The description should be 1-2 sentences and focus on a realistic situation that would be appropriate for this section type.\n\n\
        Section context:\n\
        - Section 1: Everyday social conversations (e.g., booking appointments, inquiring about services, making reservations)\n\
        - Section 2: Monologues in everyday contexts (e.g., describing local facilities, explaining procedures, giving tours)\n\
        - Section 3: Academic discussions (e.g., student-tutor conversations, group project planning, course discussions)\n\
        - Section 4: Academic lectures (e.g., university lectures on various subjects)\n\n\
        Respond with ONLY the scenario description, no additional text or formatting.",
        section
    );

    let request_body = GeminiRequest {
        contents: vec![Content {
            parts: vec![Part { text: prompt }],
        }],
    };

    #[cfg(target_arch = "wasm32")]
    {
        generate_topic_wasm(request_body).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        generate_topic_native(request_body).await
    }
}

#[cfg(target_arch = "wasm32")]
async fn generate_topic_wasm(request_body: GeminiRequest) -> Result<String, String> {
    use gloo_net::http::Request;

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        API_KEY
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
async fn generate_topic_native(request_body: GeminiRequest) -> Result<String, String> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
        API_KEY
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
