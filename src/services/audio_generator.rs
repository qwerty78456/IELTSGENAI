//! Audio generation service
//! NOTE: This is an MVP implementation that provides formatted scripts for external TTS
//! Production version would integrate with professional TTS services like:
//! - Google Cloud Text-to-Speech (requires OAuth2)
//! - Amazon Polly
//! - Microsoft Azure TTS
//! - ElevenLabs

use crate::domain::{SpeakerConfig, Gender, Accent};

/// Generate SSML-formatted script for TTS services
/// SSML (Speech Synthesis Markup Language) is supported by most TTS providers
pub fn generate_ssml_script(
    script: &str,
    speakers: &[SpeakerConfig],
) -> String {
    let mut ssml = String::from("<?xml version=\"1.0\"?>\n<speak>\n");
    
    // Parse script line by line
    for line in script.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        
        // Check if line starts with a speaker name
        if let Some((speaker_name, dialogue)) = line.split_once(':') {
            let speaker_name = speaker_name.trim();
            let dialogue = dialogue.trim();
            
            // Find speaker config
            if let Some(speaker) = speakers.iter().find(|s| s.name == speaker_name) {
                let lang = accent_to_lang_code(&speaker.accent);
                let gender_voice = match speaker.gender {
                    Gender::Male => "male",
                    Gender::Female => "female",
                };
                
                ssml.push_str(&format!(
                    "  <voice language=\"{}\" gender=\"{}\">\n    <prosody rate=\"medium\" pitch=\"medium\">\n      {}\n    </prosody>\n  </voice>\n",
                    lang, gender_voice, dialogue
                ));
            } else {
                // Default voice if speaker not found
                ssml.push_str(&format!("  <p>{}</p>\n", dialogue));
            }
        } else {
            // No speaker label, treat as narration
            ssml.push_str(&format!("  <p>{}</p>\n", line));
        }
    }
    
    ssml.push_str("</speak>");
    ssml
}

/// Generate audio instructions file for teachers
pub fn generate_audio_instructions(
    script: &str,
    speakers: &[SpeakerConfig],
) -> String {
    let mut instructions = String::from("═══════════════════════════════════════════════════════\n");
    instructions.push_str("  IELTS LISTENING PRACTICE - AUDIO GENERATION GUIDE\n");
    instructions.push_str("═══════════════════════════════════════════════════════\n\n");
    
    instructions.push_str("SPEAKER CONFIGURATION:\n");
    instructions.push_str("─────────────────────────────────────────────────────\n");
    for (i, speaker) in speakers.iter().enumerate() {
        instructions.push_str(&format!(
            "{}. {}\n   - Gender: {:?}\n   - Accent: {:?}\n   - Role: {}\n\n",
            i + 1,
            speaker.name,
            speaker.gender,
            speaker.accent,
            match &speaker.role {
                crate::domain::SpeakerRole::Student => "Student",
                crate::domain::SpeakerRole::Professor => "Professor",
                crate::domain::SpeakerRole::Clerk => "Clerk",
                crate::domain::SpeakerRole::Receptionist => "Receptionist",
                crate::domain::SpeakerRole::Guide => "Guide",
                crate::domain::SpeakerRole::Other(s) => s,
            }
        ));
    }
    
    instructions.push_str("\n\nRECOMMENDED TTS SERVICES:\n");
    instructions.push_str("─────────────────────────────────────────────────────\n");
    instructions.push_str("1. ElevenLabs (https://elevenlabs.io/)\n");
    instructions.push_str("   - High quality, natural voices\n");
    instructions.push_str("   - Supports multiple accents\n");
    instructions.push_str("   - Easy to use\n\n");
    instructions.push_str("2. Google Cloud Text-to-Speech\n");
    instructions.push_str("   - WaveNet voices\n");
    instructions.push_str("   - SSML support\n");
    instructions.push_str("   - Multiple language variants\n\n");
    instructions.push_str("3. Amazon Polly\n");
    instructions.push_str("   - Neural voices\n");
    instructions.push_str("   - Good pricing\n");
    instructions.push_str("   - SSML support\n\n");
    
    instructions.push_str("\n\nHOW TO GENERATE AUDIO:\n");
    instructions.push_str("─────────────────────────────────────────────────────\n");
    instructions.push_str("1. Copy the script below\n");
    instructions.push_str("2. Split by speaker (each speaker's lines separately)\n");
    instructions.push_str("3. Use a TTS service to generate audio for each speaker\n");
    instructions.push_str("4. Use audio editing software (Audacity, Adobe Audition) to:\n");
    instructions.push_str("   - Combine the audio clips\n");
    instructions.push_str("   - Add appropriate pauses between speakers\n");
    instructions.push_str("   - Normalize volume levels\n");
    instructions.push_str("   - Export as MP3 (128-192 kbps recommended)\n\n");
    
    instructions.push_str("\n\nSCRIPT:\n");
    instructions.push_str("═══════════════════════════════════════════════════════\n\n");
    instructions.push_str(script);
    
    instructions
}

/// Helper to map accent to language code for TTS
fn accent_to_lang_code(accent: &Accent) -> &'static str {
    match accent {
        Accent::British => "en-GB",
        Accent::American => "en-US",
        Accent::Australian => "en-AU",
        Accent::Canadian => "en-CA",
        Accent::NewZealand => "en-NZ",
    }
}
