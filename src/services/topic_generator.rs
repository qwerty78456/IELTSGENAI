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
    let prompt = match section {
        "Section 1" => {
            "Generate a single, specific scenario description for an IELTS Listening Section 1 practice exercise.\n\n\
            SECTION 1 REQUIREMENTS:\n\
            - A two-way conversation between TWO people in an everyday social context\n\
            - Transactional or service-related situations\n\
            - Information exchange about practical matters\n\n\
            SUITABLE TOPICS:\n\
            - Booking appointments (doctor, hairdresser, driving lessons, hotel)\n\
            - Inquiring about services (gym membership, course enrollment, accommodation)\n\
            - Making reservations (restaurant, theater, travel arrangements)\n\
            - Shopping or rental inquiries (equipment rental, property viewing)\n\
            - Banking or post office transactions\n\n\
            Generate ONE realistic scenario (1-2 sentences) that fits Section 1. \
            Example format: 'A phone conversation between a customer and a receptionist about booking a driving lesson.'\n\n\
            Respond with ONLY the scenario description, no additional text."
        }
        "Section 2" => {
            "Generate a single, specific scenario description for an IELTS Listening Section 2 practice exercise.\n\n\
            SECTION 2 REQUIREMENTS:\n\
            - A monologue (ONE speaker) in an everyday social context\n\
            - Informative or descriptive speech\n\
            - Practical information for a general audience\n\n\
            SUITABLE TOPICS:\n\
            - Describing local facilities (community center, library, sports complex, park)\n\
            - Explaining procedures (how to use a service, safety instructions, orientation)\n\
            - Giving tours (museum, historical site, campus, neighborhood)\n\
            - Announcing events (festival details, schedule changes, opening hours)\n\
            - Providing information (transport services, accommodation options, local attractions)\n\n\
            Generate ONE realistic scenario (1-2 sentences) that fits Section 2. \
            Example format: 'A tour guide describing the facilities and activities available at a new community sports center.'\n\n\
            Respond with ONLY the scenario description, no additional text."
        }
        "Section 3" => {
            "Generate a single, specific scenario description for an IELTS Listening Section 3 practice exercise.\n\n\
            SECTION 3 REQUIREMENTS:\n\
            - A conversation between 2 people in an educational or training context\n\
            - Academic discussion or collaborative planning\n\
            - Exchange of ideas and opinions among students and/or tutors\n\n\
            SUITABLE TOPICS:\n\
            - Student-tutor discussions (assignment feedback, research proposal, dissertation planning)\n\
            - Group project planning (students organizing research, dividing tasks, scheduling)\n\
            - Course discussions (seminar debates, study group planning, presentation preparation)\n\
            - Academic advice sessions (choosing modules, career guidance, study strategies)\n\
            - Training workshops (skill development, peer learning, technique discussion)\n\n\
            Generate ONE realistic scenario (1-2 sentences) that fits Section 3. \
            Example format: 'Three students discussing their group presentation on sustainable energy and dividing the research tasks.'\n\n\
            Respond with ONLY the scenario description, no additional text."
        }
        "Section 4" => {
            "Generate a single, specific scenario description for an IELTS Listening Section 4 practice exercise.\n\n\
            SECTION 4 REQUIREMENTS:\n\
            - A monologue (ONE speaker) on an academic subject\n\
            - University-level lecture or presentation\n\
            - Intellectual content with specialized terminology\n\n\
            SUITABLE TOPICS:\n\
            - Science lectures (biology, chemistry, physics, environmental science)\n\
            - Social sciences (psychology, sociology, anthropology, economics)\n\
            - History and archaeology (civilizations, historical events, cultural developments)\n\
            - Technology and engineering (innovations, systems, developments)\n\
            - Arts and humanities (literature, philosophy, cultural studies)\n\
            - Business and management (organizational behavior, marketing, strategy)\n\n\
            Generate ONE realistic scenario (1-2 sentences) that fits Section 4. \
            Example format: 'A university lecture on the impact of climate change on coral reef ecosystems.'\n\n\
            Respond with ONLY the scenario description, no additional text."
        }
        _ => {
            return Err("Invalid section specified".to_string());
        }
    };

    let request_body = GeminiRequest {
        contents: vec![Content {
            parts: vec![Part { text: prompt.to_string() }],
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
