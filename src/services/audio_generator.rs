//! Audio generation service using Google Gemini TTS API
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use crate::domain::{SpeakerConfig, Gender, Accent, ListeningSection};

#[cfg(feature = "server")]
use super::{api_config, rate_limiter};

#[cfg(feature = "server")]
const TTS_MODEL: &str = "gemini-2.5-pro-preview-tts";

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

// Single-speaker voice config (for sections 2 and 4)
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SingleSpeakerVoiceConfig {
    #[serde(rename = "voiceConfig")]
    pub voice_config: VoiceConfig,
}

// Single-speaker speech config
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SingleSpeakerSpeechConfig {
    #[serde(rename = "voiceConfig")]
    pub voice_config: VoiceConfig,
}

/// Generate audio using Gemini TTS API (Server Function)
#[server(GenerateAudio)]
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

        let request_body = if use_single_speaker {
            // Single-speaker API (sections 2 and 4)
            let first_speaker = speakers.first().ok_or_else(|| ServerFnError::new("No speaker config provided"))?;
            let voice_name = select_voice(&first_speaker.gender, &first_speaker.accent);

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
                            voice_name: select_voice(&speaker.gender, &speaker.accent).to_string(),
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

        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await;

        let response = match response {
             Ok(r) => r,
             Err(e) => {
                 if e.is_timeout() {
                     return Err(ServerFnError::new(
                         "Audio generation timed out. The script may be too long or the service is busy. Please try again with a shorter script.".to_string()
                     ));
                 }
                 return Err(ServerFnError::new(format!("Failed to send request: {}", e)));
             }
        };
        
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ServerFnError::new(format!("API error: {}", error_text)));
        }
        
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
fn select_voice(gender: &Gender, accent: &Accent) -> &'static str {
    match (gender, accent) {
        (Gender::Male, Accent::British) => "Puck",      // Upbeat
        (Gender::Male, Accent::American) => "Kore",     // Firm
        (Gender::Male, _) => "Fenrir",                         // Excitable
        (Gender::Female, Accent::British) => "Zephyr", // Bright
        (Gender::Female, Accent::American) => "Leda",  // Youthful
        (Gender::Female, _) => "Aoede",                        // Breezy
    }
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
