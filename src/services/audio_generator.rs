//! Audio generation service using Google Gemini TTS API
use dioxus::prelude::*;
use crate::domain::{SpeakerConfig, ListeningSection};

#[cfg(feature = "server")]
use crate::domain::{Gender, Accent};

#[cfg(feature = "server")]
use super::{api_config, rate_limiter};

#[cfg(feature = "server")]
const TTS_MODEL: &str = "gemini-2.5-pro-preview-tts";

#[cfg(feature = "server")]
mod tts_types {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct TtsRequest {
        pub contents: Vec<Content>,
        #[serde(rename = "generationConfig")]
        pub generation_config: GenerationConfig,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Content {
        pub parts: Vec<Part>,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Part {
        pub text: String,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct GenerationConfig {
        #[serde(rename = "responseModalities")]
        pub response_modalities: Vec<String>,
        #[serde(rename = "speechConfig")]
        pub speech_config: SpeechConfig,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct SpeechConfig {
        #[serde(rename = "multiSpeakerVoiceConfig")]
        pub multi_speaker_voice_config: MultiSpeakerVoiceConfig,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct MultiSpeakerVoiceConfig {
        #[serde(rename = "speakerVoiceConfigs")]
        pub speaker_voice_configs: Vec<SpeakerVoiceConfig>,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct SpeakerVoiceConfig {
        pub speaker: String,
        #[serde(rename = "voiceConfig")]
        pub voice_config: VoiceConfig,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct VoiceConfig {
        #[serde(rename = "prebuiltVoiceConfig")]
        pub prebuilt_voice_config: PrebuiltVoiceConfig,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct PrebuiltVoiceConfig {
        #[serde(rename = "voiceName")]
        pub voice_name: String,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct TtsResponse {
        pub candidates: Vec<Candidate>,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct Candidate {
        pub content: ResponseContent,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct ResponseContent {
        pub parts: Vec<ResponsePart>,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct ResponsePart {
        #[serde(rename = "inlineData")]
        pub inline_data: Option<InlineData>,
    }

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct InlineData {
        #[serde(rename = "mimeType")]
        pub mime_type: String,
        pub data: String, // base64 encoded audio (PCM 16-bit, 24kHz)
    }
}

#[cfg(feature = "server")]
use tts_types::*;

/// Generate audio using Gemini TTS API (Server Function)
#[server]
pub async fn generate_audio(
    script: String,
    speakers: Vec<SpeakerConfig>,
    section: ListeningSection,
) -> Result<Vec<u8>, ServerFnError> {
    
    #[cfg(feature = "server")]
    {
        // Check rate limit
        if let Err(e) = rate_limiter::check_audio_rate_limit() {
            return Err(ServerFnError::new(e));
        }

        let api_key = api_config::get_api_key().map_err(|e| ServerFnError::new(e))?;
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            TTS_MODEL, api_key
        );

        // Determine if we need single or multi-speaker based on section
        let use_single_speaker = matches!(section, ListeningSection::Section2 | ListeningSection::Section4);

        let prompt = if use_single_speaker {
            // For single speaker, just use the script directly without speaker tags
            format!("TTS the following IELTS listening script with natural, clear pronunciation suitable for English language learners:\n\n{}", script)
        } else {
            build_tts_prompt(&script, &speakers)
        };

        let voice_mappings = load_voice_mappings();

        let request_body = if use_single_speaker {
            // Single-speaker API (sections 2 and 4)
            let first_speaker = speakers.first().ok_or_else(|| ServerFnError::new("No speaker config provided"))?;
            let voice_name = select_voice(&voice_mappings, &first_speaker.gender, &first_speaker.accent);

            serde_json::json!({
                "contents": [{
                    "parts": [{"text": prompt}]
                }],
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "speechConfig": {
                        "voiceConfig": {
                            "prebuiltVoiceConfig": {
                                "voiceName": voice_name
                            }
                        }
                    }
                }
            })
        } else {
            // Multi-speaker API (sections 1 and 3)
            let speaker_voice_configs: Vec<SpeakerVoiceConfig> = speakers
                .iter()
                .map(|speaker| SpeakerVoiceConfig {
                    speaker: speaker.name.clone(),
                    voice_config: VoiceConfig {
                        prebuilt_voice_config: PrebuiltVoiceConfig {
                            voice_name: select_voice(&voice_mappings, &speaker.gender, &speaker.accent).to_string(),
                        },
                    },
                })
                .collect();

            serde_json::to_value(TtsRequest {
                contents: vec![Content {
                    parts: vec![Part { text: prompt }],
                }],
                generation_config: GenerationConfig {
                    response_modalities: vec!["AUDIO".to_string()],
                    speech_config: SpeechConfig {
                        multi_speaker_voice_config: MultiSpeakerVoiceConfig {
                            speaker_voice_configs,
                        },
                    },
                },
            }).map_err(|e| ServerFnError::new(format!("Failed to serialize request: {}", e)))?
        };
        
        // Create client with extended timeout (TTS can take several minutes for long scripts)
        // Since this runs in a background job, we can afford to wait longer
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300)) // 5 minutes
            .build()
            .map_err(|e| ServerFnError::new(format!("Failed to create HTTP client: {}", e)))?;

        let mut retries = 0;
        let max_retries = 3;
        let mut backoff_ms = 1000;

        let response = loop {
            match client.post(&url).header("Content-Type", "application/json").json(&request_body).send().await {
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
                            return Err(ServerFnError::new("Audio generation timed out. The script may be too long or the service is busy. Please try again with a shorter script."));
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
        
        let tts_response: TtsResponse = response
            .json()
            .await
            .map_err(|e| ServerFnError::new(format!("Failed to parse response: {}", e)))?;
        
        // Extract base64 audio data
        let audio_data = tts_response
            .candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .and_then(|p| p.inline_data.as_ref())
            .ok_or_else(|| ServerFnError::new("No audio data in response"))?;
        
        // Decode base64
        let audio_bytes = decode_base64(&audio_data.data)
            .map_err(|e| ServerFnError::new(e))?;
        
        Ok(audio_bytes)
    }

    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("This function should only be called on the server"))
}

#[cfg(feature = "server")]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct VoiceMappings {
    pub male: std::collections::HashMap<String, String>,
    pub female: std::collections::HashMap<String, String>,
}

#[cfg(feature = "server")]
impl Default for VoiceMappings {
    fn default() -> Self {
        let mut male = std::collections::HashMap::new();
        male.insert("british".to_string(), "Puck".to_string());
        male.insert("american".to_string(), "Orus".to_string());
        male.insert("australian".to_string(), "Fenrir".to_string());
        male.insert("canadian".to_string(), "Puck".to_string());
        male.insert("newzealand".to_string(), "Fenrir".to_string());
        male.insert("default".to_string(), "Puck".to_string());

        let mut female = std::collections::HashMap::new();
        female.insert("british".to_string(), "Zephyr".to_string());
        female.insert("american".to_string(), "Leda".to_string());
        female.insert("australian".to_string(), "Aoede".to_string());
        female.insert("canadian".to_string(), "Zephyr".to_string());
        female.insert("newzealand".to_string(), "Aoede".to_string());
        female.insert("default".to_string(), "Zephyr".to_string());

        Self { male, female }
    }
}

#[cfg(feature = "server")]
fn load_voice_mappings() -> VoiceMappings {
    let path = std::path::Path::new(r"E:\vmq_data\voices.json");
    if let Ok(contents) = std::fs::read_to_string(path) {
        if let Ok(mappings) = serde_json::from_str::<VoiceMappings>(&contents) {
            return mappings;
        } else {
            tracing::error!("Failed to parse voices.json, falling back to default mappings.");
        }
    } else {
        // Create default mapping file if missing
        let default_mappings = VoiceMappings::default();
        if let Ok(json) = serde_json::to_string_pretty(&default_mappings) {
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = std::fs::write(path, json);
        }
    }
    VoiceMappings::default()
}

#[cfg(feature = "server")]
fn select_voice<'a>(mappings: &'a VoiceMappings, gender: &Gender, accent: &Accent) -> &'a str {
    let accent_str = match accent {
        Accent::British => "british",
        Accent::American => "american",
        Accent::Australian => "australian",
        Accent::Canadian => "canadian",
        Accent::NewZealand => "newzealand",
    };
    
    let map = match gender {
        Gender::Male => &mappings.male,
        Gender::Female => &mappings.female,
    };
    
    map.get(accent_str)
        .or_else(|| map.get("default"))
        .map(|s| s.as_str())
        .unwrap_or(match gender {
            Gender::Male => "Puck",
            Gender::Female => "Zephyr",
        })
}

#[cfg(feature = "server")]
fn build_tts_prompt(script: &str, speakers: &[SpeakerConfig]) -> String {
    let mut prompt = String::from("TTS the following IELTS listening script with natural, clear pronunciation suitable for English language learners:\n\n");
    
    if speakers.len() > 1 {
        prompt.push_str("SPEAKER PROFILES:\n");
        for speaker in speakers {
            let accent_desc = match speaker.accent {
                Accent::British => "British English",
                Accent::American => "American English",
                Accent::Australian => "Australian English",
                Accent::Canadian => "Canadian English",
                Accent::NewZealand => "New Zealand English",
            };
            let role_desc = match &speaker.role {
                crate::domain::SpeakerRole::Student => "Student",
                crate::domain::SpeakerRole::Professor => "Professor",
                crate::domain::SpeakerRole::Clerk => "Clerk",
                crate::domain::SpeakerRole::Receptionist => "Receptionist",
                crate::domain::SpeakerRole::Guide => "Guide",
                crate::domain::SpeakerRole::Other(s) => s,
            };
            prompt.push_str(&format!("{}: {} with {} accent\n", speaker.name, role_desc, accent_desc));
        }
        prompt.push_str("\n");
    }
    
    prompt.push_str("SCRIPT:\n");
    prompt.push_str(script);
    
    prompt
}

#[cfg(feature = "server")]
fn decode_base64(data: &str) -> Result<Vec<u8>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD
        .decode(data)
        .map_err(|e| format!("Base64 decode error: {}", e))
}

/// Convert raw PCM audio to WAV format
#[cfg(feature = "server")]
pub fn pcm_to_wav(pcm_data: &[u8], sample_rate: u32, channels: u16, bits_per_sample: u16) -> Vec<u8> {
    let mut wav = Vec::new();
    
    // RIFF header
    wav.extend_from_slice(b"RIFF");
    let file_size = (36 + pcm_data.len()) as u32;
    wav.extend_from_slice(&file_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    
    // fmt chunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // Chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // Audio format (1 = PCM)
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * channels as u32 * (bits_per_sample as u32 / 8);
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = channels * (bits_per_sample / 8);
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    
    // data chunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm_data.len() as u32).to_le_bytes());
    wav.extend_from_slice(pcm_data);
    
    wav
}
