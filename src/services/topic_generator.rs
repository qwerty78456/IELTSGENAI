//! Topic generation service using Google Gemini API
use dioxus::prelude::*;

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

/// Generate a topic suggestion for IELTS Listening Practice
#[server]
pub async fn generate_topic_suggestion(section: String) -> Result<String, ServerFnError> {
    // Everything inside this function body runs ONLY on the server

    // Logic for prompts
    let prompt = match section.as_str() {
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
            return Err(ServerFnError::new("Invalid section specified"));
        }
    };

    // Check rate limit
    #[cfg(feature = "server")]
    if let Err(e) = rate_limiter::check_topic_rate_limit() {
        return Err(ServerFnError::new(e));
    }

    #[cfg(feature = "server")]
    {
        let api_key = api_config::get_api_key().map_err(|e| ServerFnError::new(e))?;
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-flash-latest:generateContent?key={}",
            api_key
        );

        let request_body = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part { text: prompt.to_string() }],
            }],
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
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
