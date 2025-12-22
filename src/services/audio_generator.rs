//! Audio generation service using Google Gemini TTS API

use serde::{Deserialize, Serialize};
use crate::domain::{SpeakerConfig, Gender, Accent};

const API_KEY: &str = "AIzaSyBq8ur94FNYK9odYENS4lC5YdS-k0MMdzM";
const TTS_MODEL: &str = "gemini-2.5-flash-preview-tts";

#[derive(Serialize)]
struct TtsRequest {
    contents: Vec<Content>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Serialize)]
struct GenerationConfig {
    #[serde(rename = "responseModalities")]
    response_modalities: Vec<String>,
    #[serde(rename = "speechConfig")]
    speech_config: SpeechConfig,
}

#[derive(Serialize)]
struct SpeechConfig {
    #[serde(rename = "multiSpeakerVoiceConfig")]
    multi_speaker_voice_config: MultiSpeakerVoiceConfig,
}

#[derive(Serialize)]
struct MultiSpeakerVoiceConfig {
    #[serde(rename = "speakerVoiceConfigs")]
    speaker_voice_configs: Vec<SpeakerVoiceConfig>,
}

#[derive(Serialize)]
struct SpeakerVoiceConfig {
    speaker: String,
    #[serde(rename = "voiceConfig")]
    voice_config: VoiceConfig,
}

#[derive(Serialize)]
struct VoiceConfig {
    #[serde(rename = "prebuiltVoiceConfig")]
    prebuilt_voice_config: PrebuiltVoiceConfig,
}

#[derive(Serialize)]
struct PrebuiltVoiceConfig {
    #[serde(rename = "voiceName")]
    voice_name: String,
}

#[derive(Deserialize)]
struct TtsResponse {
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    content: ResponseContent,
}

#[derive(Deserialize)]
struct ResponseContent {
    parts: Vec<ResponsePart>,
}

#[derive(Deserialize)]
struct ResponsePart {
    #[serde(rename = "inlineData")]
    inline_data: Option<InlineData>,
}

#[derive(Deserialize)]
struct InlineData {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String, // base64 encoded audio (PCM 16-bit, 24kHz)
}

/// Select appropriate Gemini voice based on gender and accent
fn select_voice(gender: &Gender, accent: &Accent) -> &'static str {
    // Available voices: Zephyr, Puck, Kore, Fenrir, Leda, Aoede, Charon, etc.
    // See: https://ai.google.dev/gemini-api/docs/speech-generation#voice_options
    match (gender, accent) {
        (Gender::Male, Accent::British) => "Puck",      // Upbeat
        (Gender::Male, Accent::American) => "Kore",     // Firm
        (Gender::Male, _) => "Fenrir",                         // Excitable
        (Gender::Female, Accent::British) => "Zephyr", // Bright
        (Gender::Female, Accent::American) => "Leda",  // Youthful
        (Gender::Female, _) => "Aoede",                        // Breezy
    }
}

/// Build TTS prompt with script and speaker styling
fn build_tts_prompt(script: &str, speakers: &[SpeakerConfig]) -> String {
    let mut prompt = String::from("TTS the following IELTS listening script with natural, clear pronunciation suitable for English language learners:\n\n");
    
    // Add speaker descriptions for context
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

#[cfg(target_arch = "wasm32")]
/// Generate audio using Gemini TTS API (WASM version)
pub async fn generate_audio(
    script: &str,
    speakers: &[SpeakerConfig],
) -> Result<Vec<u8>, String> {
    use gloo_net::http::Request;
    use wasm_bindgen::JsCast;
    use web_sys::Blob;
    
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        TTS_MODEL, API_KEY
    );
    
    let prompt = build_tts_prompt(script, speakers);
    
    // Build speaker voice configs
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
    
    let request_body = TtsRequest {
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
    };
    
    let response = Request::post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .map_err(|e| format!("Failed to serialize request: {}", e))?
        .send()
        .await
        .map_err(|e| format!("API request failed: {}", e))?;
    
    if !response.ok() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("API error {}: {}", response.status(), error_text));
    }
    
    let tts_response: TtsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;
    
    // Extract base64 audio data
    let audio_data = tts_response
        .candidates
        .first()
        .and_then(|c| c.content.parts.first())
        .and_then(|p| p.inline_data.as_ref())
        .ok_or_else(|| "No audio data in response".to_string())?;
    
    // Decode base64
    let audio_bytes = decode_base64(&audio_data.data)?;
    
    Ok(audio_bytes)
}

#[cfg(not(target_arch = "wasm32"))]
/// Generate audio using Gemini TTS API (native version)
pub async fn generate_audio(
    script: &str,
    speakers: &[SpeakerConfig],
) -> Result<Vec<u8>, String> {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        TTS_MODEL, API_KEY
    );
    
    let prompt = build_tts_prompt(script, speakers);
    
    // Build speaker voice configs
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
    
    let request_body = TtsRequest {
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
    };
    
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("API request failed: {}", e))?;
    
    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("API error: {}", error_text));
    }
    
    let tts_response: TtsResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;
    
    // Extract base64 audio data
    let audio_data = tts_response
        .candidates
        .first()
        .and_then(|c| c.content.parts.first())
        .and_then(|p| p.inline_data.as_ref())
        .ok_or_else(|| "No audio data in response".to_string())?;
    
    // Decode base64
    let audio_bytes = decode_base64(&audio_data.data)?;
    
    Ok(audio_bytes)
}

/// Decode base64 string to bytes
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
