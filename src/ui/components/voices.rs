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

use crate::application::voices::{
    DesignedVoiceRow, DesignedVoices, VoiceCatalogue, VoiceSample, delete_voice, design_voice,
    designed_voices, voice_preview,
};
use crate::domain::{
    Accent, Gender, PartSpec, Severity, SpeakerConfig, ValidationIssue, Voice, VoiceChoice,
    VoiceDesignRequest, VoiceSource, next_voice, validate_speakers,
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

/// The voice ids of every speaker but the one at `index`.
pub fn voices_of_others(speakers: &[SpeakerConfig], index: usize) -> Vec<String> {
    speakers
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != index)
        .filter_map(|(_, speaker)| speaker.voice_id().map(str::to_string))
        .collect()
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
        VoiceChoice::Chosen(voice) if voice.source == VoiceSource::Designed => (
            format!("Voice: {} \u{b7} designed", voice.display_name()),
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

/// The designed voices of the API key's Google project, in the speaker
/// dialog: those of the speaker's gender with Listen, Use and (for voices
/// this app made, on a local server) a two-click Delete, then a form that
/// designs a new voice of the dialog's gender and accent. The list is asked
/// for when the section is first opened. A new voice plays its sample and is
/// chosen at once; `onchoose` gets every chosen voice, `ondelete` every
/// deleted id.
#[component]
pub fn DesignedVoicesPanel(
    /// The speaker's label, for the default name of a new voice.
    label: String,
    gender: Gender,
    accent: Accent,
    /// The voice the dialog would save.
    current: Option<String>,
    /// Voice ids the other speakers of the part have.
    taken: Vec<String>,
    /// The saved exam a new voice is booked to.
    #[props(default)]
    exam: Option<Uuid>,
    onchoose: EventHandler<Voice>,
    ondelete: EventHandler<String>,
) -> Element {
    let mut open = use_signal(|| false);
    let mut list = use_signal(|| None::<DesignedVoices>);
    let mut loading = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    // "create", or the id of a voice being sampled or deleted.
    let mut busy = use_signal(|| None::<String>);
    let mut confirming = use_signal(|| None::<String>);
    let mut name = use_signal(String::new);
    let mut description = use_signal(String::new);
    let mut sample = use_signal(|| None::<VoiceSample>);
    let show_controls = use_signal(|| false);
    let player = use_hook(|| {
        format!(
            "voice-sample-{}",
            NEXT_PLAYER.fetch_add(1, Ordering::Relaxed)
        )
    });

    let mut load = move || {
        loading.set(true);
        error.set(None);
        spawn(async move {
            let outcome = designed_voices().await;
            loading.set(false);
            match outcome {
                Ok(found) => list.set(Some(found)),
                Err(e) => error.set(Some(format!("Could not list the designed voices: {e}"))),
            }
        });
    };
    let toggle = move |_| {
        let opening = !open();
        open.set(opening);
        if opening && list.peek().is_none() && !*loading.peek() {
            load();
        }
    };

    let default_name = format!(
        "{label}, {} {}",
        accent.label(),
        gender.label().to_lowercase()
    );
    let create_name = default_name.clone();
    let create = move |_| {
        let typed = name();
        let request = VoiceDesignRequest {
            name: if typed.trim().is_empty() {
                create_name.clone()
            } else {
                typed
            },
            description: description(),
            gender,
            accent,
        };
        if let Err(e) = request.validate() {
            error.set(Some(e.to_string()));
            return;
        }
        error.set(None);
        busy.set(Some("create".into()));
        let mut show_controls = show_controls;
        spawn(async move {
            let outcome = design_voice(request, exam).await;
            busy.set(None);
            match outcome {
                Ok(made) => {
                    if let Some(found) = list.write().as_mut() {
                        found.voices.insert(
                            0,
                            DesignedVoiceRow {
                                voice: made.voice.clone(),
                                deletable: true,
                            },
                        );
                    }
                    name.set(String::new());
                    description.set(String::new());
                    show_controls.set(false);
                    sample.set(Some(made.sample));
                    onchoose.call(made.voice);
                }
                Err(e) => error.set(Some(format!("Could not create the voice: {e}"))),
            }
        });
    };

    let found = list();
    let rows: Vec<DesignedVoiceRow> = found
        .as_ref()
        .map(|found| {
            found
                .voices
                .iter()
                .filter(|row| row.voice.gender == gender)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let other_gender = found
        .as_ref()
        .map_or(0, |found| found.voices.len() - rows.len());
    let can_design = found.as_ref().is_some_and(|found| found.can_design);
    let creating = busy().as_deref() == Some("create");
    let mount_player = player.clone();

    rsx! {
        div { class: "designed-voices",
            button {
                class: "customize-toggle",
                r#type: "button",
                onclick: toggle,
                span { "Designed voices" }
                span { class: "toggle-icon", if open() { "\u{25bc}" } else { "\u{25b6}" } }
            }
            if open() {
                if loading() {
                    p { class: "muted", "Loading the designed voices of this API key..." }
                }
                if found.is_some() {
                    if rows.is_empty() {
                        p { class: "muted",
                            "No {gender.label().to_lowercase()} designed voices in the Google project of this API key yet."
                        }
                    }
                    for row in rows {
                        {
                            let id = row.voice.id.clone();
                            let in_use = current.as_deref() == Some(id.as_str());
                            let elsewhere = taken.contains(&id);
                            let pending = confirming().as_deref() == Some(id.as_str());
                            let working = busy().as_deref() == Some(id.as_str());
                            let (listen_id, delete_id, confirm_id) = (id.clone(), id.clone(), id.clone());
                            let voice = row.voice.clone();
                            let replay = player.clone();
                            rsx! {
                                div { class: "designed-voice", key: "{id}",
                                    div { class: "designed-voice-text",
                                        span { class: "voice-name", title: "{row.voice.description}",
                                            "{row.voice.display_name()}"
                                        }
                                        span { class: "muted", "{row.voice.accent.label()}" }
                                    }
                                    div { class: "voice-actions",
                                        button {
                                            class: "voice-button",
                                            r#type: "button",
                                            disabled: busy().is_some(),
                                            title: "Hear Google's sample of this voice (free)",
                                            onclick: move |_| {
                                                let id = listen_id.clone();
                                                error.set(None);
                                                if sample.peek().as_ref().is_some_and(|s| s.voice_id == id) {
                                                    play(replay.clone(), show_controls);
                                                    return;
                                                }
                                                busy.set(Some(id.clone()));
                                                let mut show_controls = show_controls;
                                                spawn(async move {
                                                    let outcome = voice_preview(id, exam).await;
                                                    busy.set(None);
                                                    match outcome {
                                                        Ok(made) => {
                                                            show_controls.set(false);
                                                            sample.set(Some(made));
                                                        }
                                                        Err(e) => error.set(Some(format!("Could not play this voice: {e}"))),
                                                    }
                                                });
                                            },
                                            if working && !pending { "Loading..." } else { "Listen" }
                                        }
                                        button {
                                            class: "voice-button",
                                            r#type: "button",
                                            disabled: in_use || elsewhere,
                                            title: "Read this speaker with this voice; its gender and accent come with it",
                                            onclick: move |_| onchoose.call(voice.clone()),
                                            if in_use {
                                                "In use"
                                            } else if elsewhere {
                                                "Used by another speaker"
                                            } else {
                                                "Use"
                                            }
                                        }
                                        if row.deletable {
                                            if pending {
                                                button {
                                                    class: "voice-button danger",
                                                    r#type: "button",
                                                    disabled: busy().is_some(),
                                                    onclick: move |_| {
                                                        let id = confirm_id.clone();
                                                        confirming.set(None);
                                                        error.set(None);
                                                        busy.set(Some(id.clone()));
                                                        spawn(async move {
                                                            let outcome = delete_voice(id.clone()).await;
                                                            busy.set(None);
                                                            match outcome {
                                                                Ok(()) => {
                                                                    if let Some(found) = list.write().as_mut() {
                                                                        found.voices.retain(|r| r.voice.id != id);
                                                                    }
                                                                    if sample.peek().as_ref().is_some_and(|s| s.voice_id == id) {
                                                                        sample.set(None);
                                                                    }
                                                                    ondelete.call(id);
                                                                }
                                                                Err(e) => error.set(Some(format!("Could not delete the voice: {e}"))),
                                                            }
                                                        });
                                                    },
                                                    if working { "Deleting..." } else { "Confirm delete" }
                                                }
                                                button {
                                                    class: "voice-button",
                                                    r#type: "button",
                                                    onclick: move |_| confirming.set(None),
                                                    "Cancel"
                                                }
                                            } else {
                                                button {
                                                    class: "voice-button",
                                                    r#type: "button",
                                                    disabled: busy().is_some(),
                                                    title: "Delete this voice from the Google project of this API key",
                                                    onclick: move |_| confirming.set(Some(delete_id.clone())),
                                                    "Delete"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if other_gender > 0 {
                        p { class: "muted",
                            "{other_gender} designed voice(s) of the other gender are hidden; change Gender above to see them."
                        }
                    }
                    if can_design {
                        div { class: "designed-voice-form",
                            p { class: "designed-voice-title",
                                "New {gender.label().to_lowercase()} {accent.label()} voice"
                            }
                            input {
                                class: "form-input",
                                r#type: "text",
                                maxlength: "60",
                                placeholder: "{default_name}",
                                value: "{name}",
                                disabled: creating,
                                oninput: move |evt| name.set(evt.value()),
                            }
                            textarea {
                                class: "form-input",
                                rows: "3",
                                maxlength: "500",
                                placeholder: "e.g. {description_example(gender, accent)}",
                                value: "{description}",
                                disabled: creating,
                                oninput: move |evt| description.set(evt.value()),
                            }
                            p { class: "muted",
                                "Describe age, timbre, accent and pace in 1-2 sentences, in English."
                            }
                            p { class: "muted",
                                "Creating a voice takes about 20 s and costs about $0.02; a designed voice reads each of its turns in its own request."
                            }
                            button {
                                class: "voice-button",
                                r#type: "button",
                                disabled: busy().is_some(),
                                onclick: create,
                                if creating { "Creating the voice (about 20 s)..." } else { "Create voice" }
                            }
                        }
                    } else {
                        p { class: "muted",
                            "New voices can only be designed on a copy of the app running on your own computer."
                        }
                    }
                }
                if let Some(message) = error() {
                    p { class: "voice-note", "{message}" }
                }
                if found.is_none() && !loading() && error().is_some() {
                    button {
                        class: "voice-button",
                        r#type: "button",
                        onclick: move |_| load(),
                        "Try again"
                    }
                }
                if let Some(playing) = sample() {
                    audio {
                        key: "{playing.url}",
                        id: "{player}",
                        class: "voice-sample",
                        src: "{playing.url}",
                        preload: "auto",
                        controls: show_controls(),
                        onmounted: move |_| play(mount_player.clone(), show_controls),
                    }
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

/// A sample description for the design form, in the speaker's gender and accent.
fn description_example(gender: Gender, accent: Accent) -> String {
    let person = match gender {
        Gender::Female => "A warm woman in her forties",
        Gender::Male => "A calm man in his fifties",
    };
    format!(
        "{person} with a clear {} accent; measured and unhurried, like a teacher.",
        accent.label().trim_end_matches(" English")
    )
}
