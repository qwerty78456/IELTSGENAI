//! Voices in the browser: the catalogue every page assigns from, and the
//! voice line of a speaker card ("Listen", "Another voice", "Automatic").
//!
//! Assignment and "Another voice" are the pure domain rules run here, over
//! the catalogue `Navbar` loads once; only a sample that was never recorded
//! costs a server round trip with a Gemini request. Both pages show speakers
//! with `speaker_modal::SpeakerCards`, which uses `VoicePicker`.

use std::sync::atomic::{AtomicU32, Ordering};

use dioxus::prelude::*;
use uuid::Uuid;

use crate::application::voices::{VoiceCatalogue, VoiceSample, voice_preview};
use crate::domain::{
    PartSpec, Severity, SpeakerConfig, ValidationIssue, Voice, VoiceChoice, next_voice,
    validate_speakers,
};

/// The voice catalogue, loaded once by `Navbar` and read by both pages.
#[derive(Clone, Copy)]
pub struct VoiceCatalogueCtx(pub Resource<Result<VoiceCatalogue, String>>);

impl VoiceCatalogueCtx {
    /// The voices speakers can be given, once loaded. Reading subscribes.
    pub fn voices(&self) -> Option<Vec<Voice>> {
        match &*self.0.read() {
            Some(Ok(catalogue)) => Some(catalogue.voices.clone()),
            _ => None,
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(&*self.0.read(), Some(Ok(_)))
    }

    /// Why the catalogue could not be loaded.
    pub fn error(&self) -> Option<String> {
        match &*self.0.read() {
            Some(Err(error)) => Some(error.clone()),
            _ => None,
        }
    }

    pub fn retry(mut self) {
        self.0.restart();
    }
}

/// `speaker` with the voice `choice`. A chosen voice also brings its gender
/// and accent (`SpeakerConfig::with_voice`), so it always fits.
pub fn with_choice(speaker: SpeakerConfig, choice: VoiceChoice) -> SpeakerConfig {
    match choice {
        VoiceChoice::Chosen(voice) => speaker.with_voice(voice),
        voice => SpeakerConfig { voice, ..speaker },
    }
}

/// Who reads each speaker, for a collapsed list: "A: Oliver, B: Grace".
pub fn voices_summary(speakers: &[SpeakerConfig]) -> String {
    speakers
        .iter()
        .map(|speaker| {
            let short = speaker
                .label
                .strip_prefix("Speaker ")
                .unwrap_or(&speaker.label);
            match speaker.voice.voice() {
                Some(voice) => format!("{short}: {}", voice.display_name()),
                None => format!("{short}: no voice yet"),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a teacher should look at in a line-up (two speakers on one voice, a
/// voice of the other gender). Errors are left for the requests to report.
pub fn speaker_warnings(spec: &PartSpec, speakers: &[SpeakerConfig]) -> Vec<ValidationIssue> {
    validate_speakers(spec, speakers)
        .into_iter()
        .filter(|issue| issue.severity == Severity::Warning)
        .collect()
}

/// Gives every sample player on the page its own element id.
static NEXT_PLAYER: AtomicU32 = AtomicU32::new(0);

/// One speaker's voice: its name, "Listen", "Another voice" (never a voice
/// another speaker of `speakers` has) and, after a teacher's choice,
/// "Automatic" to let the app pick again. Changes go to `onchange`; the
/// page writes them into its line-up.
#[component]
pub fn VoicePicker(
    /// Reactive, so a sample that arrives late is kept only if the speaker
    /// still has that voice.
    speaker: ReadSignal<SpeakerConfig>,
    /// Where `speaker` is in `speakers`.
    index: usize,
    speakers: Vec<SpeakerConfig>,
    disabled: bool,
    /// The saved exam a new sample is booked to.
    #[props(default)]
    exam: Option<Uuid>,
    onchange: EventHandler<VoiceChoice>,
) -> Element {
    let catalogue = use_context::<VoiceCatalogueCtx>();
    let mut sample = use_signal(|| None::<VoiceSample>);
    let mut making = use_signal(|| false);
    let mut note = use_signal(|| None::<String>);
    let mut show_controls = use_signal(|| false);
    let player = use_hook(|| {
        format!(
            "voice-sample-{}",
            NEXT_PLAYER.fetch_add(1, Ordering::Relaxed)
        )
    });

    let current = speaker();
    let voices = catalogue.voices();
    let (text, description) = match &current.voice {
        VoiceChoice::Auto => ("Voice: not assigned yet".to_string(), String::new()),
        VoiceChoice::Assigned(voice) => (
            format!("Voice: {} \u{b7} automatic", voice.display_name()),
            voice.description.clone(),
        ),
        VoiceChoice::Chosen(voice) => (
            format!("Voice: {}", voice.display_name()),
            voice.description.clone(),
        ),
    };
    let cell = format!(
        "{} {}",
        current.accent.label(),
        current.gender.label().to_lowercase()
    );
    // Still on Auto with the catalogue here: no fitting voice is free.
    let unvoiced = current.voice.is_auto()
        && voices
            .as_ref()
            .is_some_and(|voices| next_voice(&speakers, index, voices).is_none());
    let playing = sample()
        .filter(|s| Some(s.voice_id.as_str()) == current.voice_id())
        .map(|s| s.url);
    let has_voice = current.voice_id().is_some();
    let chosen = matches!(current.voice, VoiceChoice::Chosen(_));

    let (replay_player, mount_player) = (player.clone(), player.clone());
    let listen = move |_| {
        let Some(voice_id) = speaker.peek().voice_id().map(str::to_string) else {
            return;
        };
        note.set(None);
        if sample
            .peek()
            .as_ref()
            .is_some_and(|s| s.voice_id == voice_id)
        {
            play(replay_player.clone(), show_controls);
            return;
        }
        making.set(true);
        spawn(async move {
            let outcome = voice_preview(voice_id.clone(), exam).await;
            making.set(false);
            // A sample of a voice the speaker no longer has is dropped.
            if speaker.peek().voice_id() != Some(voice_id.as_str()) {
                return;
            }
            match outcome {
                Ok(made) => {
                    show_controls.set(false);
                    sample.set(Some(made));
                }
                Err(e) => note.set(Some(format!("Could not play this voice: {e}"))),
            }
        });
    };

    let another_cell = cell.clone();
    let another = move |_| {
        let Some(voices) = catalogue.voices() else {
            return;
        };
        match next_voice(&speakers, index, &voices) {
            Some(voice) => {
                note.set(None);
                onchange.call(VoiceChoice::Chosen(voice));
            }
            None => note.set(Some(format!(
                "Every {another_cell} voice is already used in this part."
            ))),
        }
    };

    rsx! {
        div { class: "voice-line",
            span { class: "voice-name", title: "{description}", "{text}" }
            div { class: "voice-actions",
                button {
                    class: "voice-button",
                    disabled: disabled || !has_voice || making(),
                    title: "Hear this voice. The first listen records a short sample (a small Gemini charge); after that it is free.",
                    onclick: listen,
                    if making() { "Making a sample..." } else { "Listen" }
                }
                button {
                    class: "voice-button",
                    disabled: disabled || voices.is_none(),
                    title: "Another {cell} voice that no other speaker of this part has",
                    onclick: another,
                    "Another voice"
                }
                if chosen {
                    button {
                        class: "voice-button",
                        disabled,
                        title: "Let the app pick this speaker's voice again",
                        onclick: move |_| {
                            note.set(None);
                            onchange.call(VoiceChoice::Auto);
                        },
                        "Automatic"
                    }
                }
            }
            if unvoiced {
                p { class: "voice-note",
                    "No {cell} voice is free for this speaker; change its accent or gender."
                }
            }
            if let Some(message) = note() {
                p { class: "voice-note", "{message}" }
            }
            if let Some(url) = playing {
                audio {
                    key: "{url}",
                    id: "{player}",
                    class: "voice-sample",
                    src: "{url}",
                    preload: "auto",
                    controls: show_controls(),
                    onmounted: move |_| play(mount_player.clone(), show_controls),
                }
            }
        }
    }
}

/// Plays the sample element `id` from the start. A browser may refuse sound
/// that does not closely follow a click (the sample took a while to make);
/// then the player's own controls are shown instead.
fn play(id: String, mut show_controls: Signal<bool>) {
    spawn(async move {
        let script = format!(
            "const player = document.getElementById({id:?});
             if (!player) {{ return false; }}
             try {{ player.currentTime = 0; await player.play(); return true; }}
             catch (_) {{ return false; }}"
        );
        let played = document::eval(&script)
            .join::<bool>()
            .await
            .unwrap_or(false);
        if !played {
            show_controls.set(true);
        }
    });
}
