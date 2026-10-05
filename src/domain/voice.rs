//! Voices: the TTS voice each speaker is read with, and how the speakers of a
//! part get voices of their own.
//!
//! Pure rules over a catalogue slice the caller supplies. The browser runs
//! them to show and change voices instantly; the server runs the same
//! `assign_voices` again before a recording, so both sides agree.

use serde::{Deserialize, Serialize};

use super::error::DomainError;
use super::speaker::{Accent, Gender, SpeakerConfig};

/// Longest voice id accepted anywhere (requests, file names, routes).
pub const MAX_VOICE_ID_CHARS: usize = 100;

/// Id prefixes of voices made with Voice Design. The only place they are
/// spelled out: infrastructure asks `Voice::is_designed_id`.
const DESIGNED_ID_PREFIXES: [&str; 2] = ["voice_", "voicekey_"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VoiceSource {
    /// Google's prebuilt voices: the Extended Voice Library and the classic voices.
    Library,
    /// Made with Voice Design from a description. Belongs to the API key's
    /// Google project and is read one turn at a time.
    Designed,
}

/// A TTS voice. Identity is the `id` alone: names and descriptions may change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Voice {
    /// What the speech request names: "en-gb-advisor-1", "voice_kpd3e297369r".
    pub id: String,
    /// Catalogue display name, or the name a designed voice was given.
    pub name: String,
    pub gender: Gender,
    pub accent: Accent,
    pub source: VoiceSource,
    /// Catalogue description or design prompt; may be empty.
    #[serde(default)]
    pub description: String,
}

impl Voice {
    /// Whether this voice can read a speaker of this gender and accent.
    pub fn fits(&self, gender: Gender, accent: Accent) -> bool {
        self.gender == gender && self.accent == accent
    }

    /// The name shown to teachers; the id when the voice has no name.
    pub fn display_name(&self) -> &str {
        if self.name.trim().is_empty() {
            &self.id
        } else {
            &self.name
        }
    }

    /// 1 to `MAX_VOICE_ID_CHARS` ASCII letters, digits, '-' or '_': safe in a
    /// request, a file name and a URL path.
    pub fn check_id(id: &str) -> Result<(), DomainError> {
        let valid = !id.is_empty()
            && id.len() <= MAX_VOICE_ID_CHARS
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
        if valid {
            Ok(())
        } else {
            Err(DomainError::InvalidRequest(format!(
                "That is not a voice id: use 1 to {MAX_VOICE_ID_CHARS} letters, digits, '-' or '_'"
            )))
        }
    }

    /// An id made by Voice Design ("voice_…", "voicekey_…").
    pub fn is_designed_id(id: &str) -> bool {
        DESIGNED_ID_PREFIXES
            .iter()
            .any(|prefix| id.starts_with(prefix))
    }
}

/// How a speaker got its voice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceChoice {
    /// No voice yet: presets, speakers saved before 0.8, or after a gender or
    /// accent edit. `assign_voices` fills it.
    #[default]
    Auto,
    /// Picked by the app. Kept while the catalogue still has it, it fits and no
    /// other speaker has it; otherwise replaced.
    Assigned(Voice),
    /// Picked by the teacher. Never replaced by the app.
    Chosen(Voice),
}

impl VoiceChoice {
    pub fn voice(&self) -> Option<&Voice> {
        match self {
            VoiceChoice::Auto => None,
            VoiceChoice::Assigned(voice) | VoiceChoice::Chosen(voice) => Some(voice),
        }
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, VoiceChoice::Auto)
    }

    pub fn id(&self) -> Option<&str> {
        self.voice().map(|voice| voice.id.as_str())
    }
}

/// The line-up after `assign_voices`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignedVoices {
    pub speakers: Vec<SpeakerConfig>,
    /// Labels left on `Auto` because no fitting voice was free.
    pub unvoiced: Vec<String>,
}

/// Gives every speaker of a part a voice of its own that fits its gender and
/// accent. Deterministic, and applying it twice changes nothing:
/// 1. a `Chosen` voice is always kept;
/// 2. an `Assigned` voice is kept while the catalogue still lists its id, it
///    fits, and neither a choice nor an earlier speaker holds it;
/// 3. in line-up order, every other speaker gets the first fitting voice in
///    catalogue order that no speaker holds, preferring ids not in
///    `elsewhere` (the voices of the exam's other parts).
///
/// The app never gives two speakers one voice; two teacher choices that clash
/// are left for `voice_conflicts` to report.
pub fn assign_voices(
    speakers: &[SpeakerConfig],
    catalogue: &[Voice],
    elsewhere: &[String],
) -> AssignedVoices {
    let mut taken: Vec<&str> = speakers
        .iter()
        .filter_map(|s| match &s.voice {
            VoiceChoice::Chosen(voice) => Some(voice.id.as_str()),
            _ => None,
        })
        .collect();
    let mut voices: Vec<Option<VoiceChoice>> = speakers
        .iter()
        .map(|s| matches!(s.voice, VoiceChoice::Chosen(_)).then(|| s.voice.clone()))
        .collect();

    for (i, speaker) in speakers.iter().enumerate() {
        if let VoiceChoice::Assigned(voice) = &speaker.voice
            && let Some(listed) = catalogue.iter().find(|v| v.id == voice.id)
            && listed.fits(speaker.gender, speaker.accent)
            && !taken.contains(&listed.id.as_str())
        {
            taken.push(&listed.id);
            voices[i] = Some(VoiceChoice::Assigned(listed.clone()));
        }
    }

    let mut unvoiced = Vec::new();
    for (i, speaker) in speakers.iter().enumerate() {
        if voices[i].is_some() {
            continue;
        }
        let free: Vec<&Voice> = catalogue
            .iter()
            .filter(|v| v.fits(speaker.gender, speaker.accent) && !taken.contains(&v.id.as_str()))
            .collect();
        let pick = free
            .iter()
            .find(|v| !elsewhere.contains(&v.id))
            .or(free.first());
        voices[i] = Some(match pick {
            Some(voice) => {
                taken.push(&voice.id);
                VoiceChoice::Assigned((*voice).clone())
            }
            None => {
                unvoiced.push(speaker.label.clone());
                VoiceChoice::Auto
            }
        });
    }

    let speakers = speakers
        .iter()
        .zip(voices)
        .map(|(speaker, voice)| SpeakerConfig {
            voice: voice.unwrap_or_default(),
            ..speaker.clone()
        })
        .collect();
    AssignedVoices { speakers, unvoiced }
}

/// `assign_voices` for the parts of an exam, in part order. Each part
/// prefers voices no other part uses: the earlier parts as just assigned,
/// the later ones as they stand. The browser and the server assign an exam
/// this way, so the voices the teacher sees are the voices that record.
pub fn assign_exam_voices(
    parts: &[Vec<SpeakerConfig>],
    catalogue: &[Voice],
) -> Vec<AssignedVoices> {
    let mut line_ups = parts.to_vec();
    let mut assigned = Vec::with_capacity(parts.len());
    for i in 0..line_ups.len() {
        let elsewhere: Vec<String> = line_ups
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .flat_map(|(_, speakers)| speakers.iter().filter_map(SpeakerConfig::voice_id))
            .map(str::to_string)
            .collect();
        let part = assign_voices(&line_ups[i], catalogue, &elsewhere);
        line_ups[i] = part.speakers.clone();
        assigned.push(part);
    }
    assigned
}

/// "Another voice":the next voice after speaker `index`'s current one, in
/// catalogue order, that fits its gender and accent and no other speaker of
/// the line-up uses; wraps around. `None` when there is no such voice.
pub fn next_voice(speakers: &[SpeakerConfig], index: usize, catalogue: &[Voice]) -> Option<Voice> {
    let speaker = speakers.get(index)?;
    let current = speaker.voice_id();
    let taken: Vec<&str> = speakers
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != index)
        .filter_map(|(_, s)| s.voice_id())
        .collect();
    let fitting: Vec<&Voice> = catalogue
        .iter()
        .filter(|v| v.fits(speaker.gender, speaker.accent))
        .collect();
    let start = current
        .and_then(|id| fitting.iter().position(|v| v.id == id))
        .map_or(0, |position| position + 1);
    (0..fitting.len())
        .map(|step| fitting[(start + step) % fitting.len()])
        .find(|v| Some(v.id.as_str()) != current && !taken.contains(&v.id.as_str()))
        .cloned()
}

/// What makes a recording wrong: two speakers on one voice, or a voice of the
/// other gender. Teacher-readable, in line-up order; empty when all is well.
pub fn voice_conflicts(speakers: &[SpeakerConfig]) -> Vec<String> {
    let mut conflicts = Vec::new();
    for (i, speaker) in speakers.iter().enumerate() {
        let Some(voice) = speaker.voice.voice() else {
            continue;
        };
        if let Some(first) = speakers[..i]
            .iter()
            .find(|other| other.voice_id() == Some(voice.id.as_str()))
        {
            conflicts.push(format!(
                "{} and {} share the voice {}; give one of them another voice",
                first.label,
                speaker.label,
                voice.display_name()
            ));
        }
        if voice.gender != speaker.gender {
            conflicts.push(format!(
                "{} is {} but the voice {} is {}; choose a {} voice",
                speaker.label,
                speaker.gender.label(),
                voice.display_name(),
                voice.gender.label(),
                speaker.gender.label().to_lowercase()
            ));
        }
    }
    conflicts
}

/// What a speaker edit makes out of date. Always derived by comparing the
/// line-up a script or recording was made for with the current one
/// (`Passage::written_for`, `ExamPart::recorded_for`), never stored as a flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpeakerChange {
    /// The script was written for other people: labels, gender, accent
    /// (vocabulary follows it) or role differ.
    pub script: bool,
    /// The recording was read differently: gender, accent, role (it shapes
    /// the delivery) or voice differ.
    pub recording: bool,
}

/// Compares two line-ups speaker by speaker (matched by label). Nothing is
/// out of date when `before` is empty: there is nothing to compare with
/// (a script or recording made before 0.8).
///
/// Voices count only when both sides have one. A speaker on `Auto` has no
/// voice yet, so the app assigning one (when the catalogue loads, or after
/// "Automatic") changes nothing that was made; a recording is only ever
/// made with voices, and a teacher's voice edit lands as `Chosen`.
pub fn speaker_change(before: &[SpeakerConfig], after: &[SpeakerConfig]) -> SpeakerChange {
    if before.is_empty() {
        return SpeakerChange::default();
    }
    let everything = SpeakerChange {
        script: true,
        recording: true,
    };
    if before.len() != after.len() {
        return everything;
    }
    let mut change = SpeakerChange::default();
    for speaker in after {
        let Some(earlier) = before.iter().find(|b| b.label == speaker.label) else {
            return everything;
        };
        let person = earlier.gender != speaker.gender
            || earlier.accent != speaker.accent
            || earlier.role != speaker.role;
        let voice = matches!(
            (earlier.voice_id(), speaker.voice_id()),
            (Some(then), Some(now)) if then != now
        );
        change.script |= person;
        change.recording |= person || voice;
    }
    change
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::speaker::SpeakerRole;

    fn voice(id: &str, gender: Gender, accent: Accent) -> Voice {
        Voice {
            id: id.into(),
            name: format!("Name of {id}"),
            gender,
            accent,
            source: VoiceSource::Library,
            description: String::new(),
        }
    }

    /// Four British women, two British men, two American women, in that order.
    fn catalogue() -> Vec<Voice> {
        use Accent::*;
        use Gender::*;
        vec![
            voice("gb-f-1", Female, British),
            voice("gb-f-2", Female, British),
            voice("gb-f-3", Female, British),
            voice("gb-f-4", Female, British),
            voice("gb-m-1", Male, British),
            voice("gb-m-2", Male, British),
            voice("us-f-1", Female, American),
            voice("us-f-2", Female, American),
        ]
    }

    fn speaker(label: &str, gender: Gender, accent: Accent) -> SpeakerConfig {
        SpeakerConfig::new(label, gender, accent, SpeakerRole::Guest)
    }

    fn female_british(label: &str) -> SpeakerConfig {
        speaker(label, Gender::Female, Accent::British)
    }

    fn ids(speakers: &[SpeakerConfig]) -> Vec<Option<&str>> {
        speakers.iter().map(SpeakerConfig::voice_id).collect()
    }

    fn assigned(id: &str) -> VoiceChoice {
        let listed = catalogue().into_iter().find(|v| v.id == id).unwrap();
        VoiceChoice::Assigned(listed)
    }

    #[test]
    fn assign_gives_three_female_british_speakers_three_voices() {
        // The HSG Part 1 case: one gender and accent for everyone used to mean one voice.
        let line_up = ["Speaker A", "Speaker B", "Speaker C"].map(female_british);
        let result = assign_voices(&line_up, &catalogue(), &[]);
        assert!(result.unvoiced.is_empty());
        assert_eq!(
            ids(&result.speakers),
            [Some("gb-f-1"), Some("gb-f-2"), Some("gb-f-3")]
        );
        assert!(voice_conflicts(&result.speakers).is_empty());
        assert!(
            result
                .speakers
                .iter()
                .all(|s| matches!(s.voice, VoiceChoice::Assigned(_)))
        );
    }

    #[test]
    fn assign_keeps_chosen_and_fitting_assigned() {
        let chosen = catalogue()[2].clone();
        let line_up = [
            female_british("Speaker A"),
            SpeakerConfig {
                voice: assigned("gb-f-4"),
                ..female_british("Speaker B")
            },
            female_british("Speaker C").with_voice(chosen.clone()),
        ];
        let result = assign_voices(&line_up, &catalogue(), &[]);
        assert_eq!(
            ids(&result.speakers),
            [Some("gb-f-1"), Some("gb-f-4"), Some("gb-f-3")]
        );
        assert_eq!(result.speakers[2].voice, VoiceChoice::Chosen(chosen));
        assert_eq!(result.speakers[1].voice, assigned("gb-f-4"));
    }

    #[test]
    fn assign_moves_the_later_duplicate() {
        let on_first = |label| SpeakerConfig {
            voice: assigned("gb-f-1"),
            ..female_british(label)
        };
        let result = assign_voices(
            &[on_first("Speaker A"), on_first("Speaker B")],
            &catalogue(),
            &[],
        );
        assert_eq!(ids(&result.speakers), [Some("gb-f-1"), Some("gb-f-2")]);

        // A teacher's choice wins over an earlier app pick of the same voice.
        let line_up = [
            on_first("Speaker A"),
            female_british("Speaker B").with_voice(catalogue()[0].clone()),
        ];
        let result = assign_voices(&line_up, &catalogue(), &[]);
        assert_eq!(ids(&result.speakers), [Some("gb-f-2"), Some("gb-f-1")]);
    }

    #[test]
    fn assign_replaces_wrong_gender_or_accent() {
        let line_up = [
            SpeakerConfig {
                voice: assigned("gb-f-1"),
                ..speaker("Speaker A", Gender::Male, Accent::British)
            },
            SpeakerConfig {
                voice: assigned("gb-f-2"),
                ..speaker("Speaker B", Gender::Female, Accent::American)
            },
        ];
        let result = assign_voices(&line_up, &catalogue(), &[]);
        assert_eq!(ids(&result.speakers), [Some("gb-m-1"), Some("us-f-1")]);
    }

    #[test]
    fn assign_drops_ids_missing_from_catalogue() {
        let retired = voice("gb-f-retired", Gender::Female, Accent::British);
        let line_up = [SpeakerConfig {
            voice: VoiceChoice::Assigned(retired),
            ..female_british("Speaker A")
        }];
        let result = assign_voices(&line_up, &catalogue(), &[]);
        assert_eq!(ids(&result.speakers), [Some("gb-f-1")]);

        // A kept voice takes the catalogue's current name and description.
        let mut renamed = catalogue()[1].clone();
        renamed.name = "Old name".into();
        let line_up = [SpeakerConfig {
            voice: VoiceChoice::Assigned(renamed),
            ..female_british("Speaker A")
        }];
        let result = assign_voices(&line_up, &catalogue(), &[]);
        assert_eq!(result.speakers[0].voice, assigned("gb-f-2"));
    }

    #[test]
    fn assign_prefers_voices_unused_elsewhere() {
        let line_up = ["Speaker A", "Speaker B"].map(female_british);
        let elsewhere = ["gb-f-1".to_string(), "gb-f-3".to_string()];
        let result = assign_voices(&line_up, &catalogue(), &elsewhere);
        assert_eq!(ids(&result.speakers), [Some("gb-f-2"), Some("gb-f-4")]);

        // Used elsewhere is still better than no voice.
        let line_up = ["Speaker A", "Speaker B", "Speaker C"].map(female_british);
        let elsewhere: Vec<String> = ["gb-f-1", "gb-f-2", "gb-f-3"].map(String::from).to_vec();
        let result = assign_voices(&line_up, &catalogue(), &elsewhere);
        assert_eq!(
            ids(&result.speakers),
            [Some("gb-f-4"), Some("gb-f-1"), Some("gb-f-2")]
        );
    }

    #[test]
    fn assign_reports_unvoiced() {
        let line_up = ["Speaker A", "Speaker B", "Speaker C"]
            .map(|label| speaker(label, Gender::Male, Accent::British));
        let result = assign_voices(&line_up, &catalogue(), &[]);
        assert_eq!(result.unvoiced, ["Speaker C"]);
        assert_eq!(
            ids(&result.speakers),
            [Some("gb-m-1"), Some("gb-m-2"), None]
        );
        assert!(result.speakers[2].voice.is_auto());

        let nobody = [speaker("Speaker A", Gender::Male, Accent::Indian)];
        assert_eq!(
            assign_voices(&nobody, &catalogue(), &[]).unvoiced,
            ["Speaker A"]
        );
    }

    #[test]
    fn assign_is_deterministic() {
        let line_up = [
            female_british("Speaker A"),
            speaker("Speaker B", Gender::Male, Accent::British),
            female_british("Speaker C").with_voice(catalogue()[0].clone()),
        ];
        let once = assign_voices(&line_up, &catalogue(), &[]);
        assert_eq!(once, assign_voices(&line_up, &catalogue(), &[]));
        let twice = assign_voices(&once.speakers, &catalogue(), &[]);
        assert_eq!(twice.speakers, once.speakers);
    }

    #[test]
    fn exam_parts_prefer_voices_of_their_own() {
        let part = ["Speaker A", "Speaker B"].map(female_british).to_vec();
        let parts = vec![part.clone(), part];
        let once = assign_exam_voices(&parts, &catalogue());
        assert_eq!(ids(&once[0].speakers), [Some("gb-f-1"), Some("gb-f-2")]);
        // Part 2 avoids Part 1's voices while the pool has others.
        assert_eq!(ids(&once[1].speakers), [Some("gb-f-3"), Some("gb-f-4")]);

        // Assigning again changes nothing: the browser and the server agree.
        let line_ups: Vec<Vec<SpeakerConfig>> = once.iter().map(|p| p.speakers.clone()).collect();
        let twice = assign_exam_voices(&line_ups, &catalogue());
        assert_eq!(twice, once);

        // A later part's chosen voice is avoided by the earlier parts.
        let chosen = vec![female_british("Speaker A").with_voice(catalogue()[0].clone())];
        let parts = vec![vec![female_british("Speaker A")], chosen];
        let result = assign_exam_voices(&parts, &catalogue());
        assert_eq!(ids(&result[0].speakers), [Some("gb-f-2")]);
        assert_eq!(ids(&result[1].speakers), [Some("gb-f-1")]);
    }

    #[test]
    fn next_voice_cycles_and_skips_taken() {
        let line_up = assign_voices(
            &["Speaker A", "Speaker B", "Speaker C"].map(female_british),
            &catalogue(),
            &[],
        )
        .speakers;
        // B holds gb-f-2; A and C hold gb-f-1 and gb-f-3, so only gb-f-4 is free.
        let next = next_voice(&line_up, 1, &catalogue()).unwrap();
        assert_eq!(next.id, "gb-f-4");
        let mut line_up = line_up;
        line_up[1] = line_up[1].clone().with_voice(next);
        // From the last voice it wraps to the start, skipping A's and C's.
        assert_eq!(next_voice(&line_up, 1, &catalogue()).unwrap().id, "gb-f-2");

        // Five presses on B never land on A's or C's voice.
        for _ in 0..5 {
            let next = next_voice(&line_up, 1, &catalogue()).unwrap();
            assert!(next.id != "gb-f-1" && next.id != "gb-f-3");
            line_up[1] = line_up[1].clone().with_voice(next);
        }

        // A speaker on Auto gets the first free voice.
        let fresh = [female_british("Speaker A")];
        assert_eq!(next_voice(&fresh, 0, &catalogue()).unwrap().id, "gb-f-1");

        // Nothing else fits: no next voice.
        let men = assign_voices(
            &["Speaker A", "Speaker B"].map(|l| speaker(l, Gender::Male, Accent::British)),
            &catalogue(),
            &[],
        )
        .speakers;
        assert_eq!(next_voice(&men, 0, &catalogue()), None);
        assert_eq!(next_voice(&men, 5, &catalogue()), None);
    }

    #[test]
    fn voice_conflicts_shared_and_wrong_gender() {
        let shared = catalogue()[0].clone();
        let line_up = [
            female_british("Speaker A").with_voice(shared.clone()),
            female_british("Speaker B"),
            female_british("Speaker C").with_voice(shared),
        ];
        assert_eq!(
            voice_conflicts(&line_up),
            [
                "Speaker A and Speaker C share the voice Name of gb-f-1; give one of them another voice"
            ]
        );

        let line_up = [SpeakerConfig {
            voice: assigned("gb-f-1"),
            ..speaker("Speaker B", Gender::Male, Accent::British)
        }];
        assert_eq!(
            voice_conflicts(&line_up),
            ["Speaker B is Male but the voice Name of gb-f-1 is Female; choose a male voice"]
        );

        let fine = assign_voices(
            &["Speaker A", "Speaker B"].map(female_british),
            &catalogue(),
            &[],
        );
        assert!(voice_conflicts(&fine.speakers).is_empty());
        assert!(voice_conflicts(&[female_british("Speaker A")]).is_empty());
    }

    #[test]
    fn speaker_changes_say_what_goes_stale() {
        let before = assign_voices(
            &[
                female_british("Speaker A"),
                speaker("Speaker B", Gender::Male, Accent::British),
            ],
            &catalogue(),
            &[],
        )
        .speakers;
        assert_eq!(speaker_change(&before, &before), SpeakerChange::default());

        let mut voice_only = before.clone();
        voice_only[0].voice = assigned("gb-f-4");
        assert_eq!(
            speaker_change(&before, &voice_only),
            SpeakerChange {
                script: false,
                recording: true
            }
        );

        let mut role = before.clone();
        role[1].role = SpeakerRole::Expert;
        let both = SpeakerChange {
            script: true,
            recording: true,
        };
        assert_eq!(speaker_change(&before, &role), both);

        let mut gender = before.clone();
        gender[1].gender = Gender::Female;
        assert_eq!(speaker_change(&before, &gender), both);
        let mut accent = before.clone();
        accent[0].accent = Accent::American;
        assert_eq!(speaker_change(&before, &accent), both);
        assert_eq!(speaker_change(&before, &before[..1]), both);
        let mut relabelled = before.clone();
        relabelled[1].label = "Speaker C".into();
        assert_eq!(speaker_change(&before, &relabelled), both);
        assert_eq!(speaker_change(&[], &before), SpeakerChange::default());

        // The teacher's choice of the same voice the app had picked: nothing.
        let mut chosen = before.clone();
        chosen[0] = chosen[0].clone().with_voice(catalogue()[0].clone());
        assert_eq!(speaker_change(&before, &chosen), SpeakerChange::default());

        // Voices count only when both sides have one: the app assigning a
        // voice to a speaker on Auto (the catalogue arriving) is no change,
        // and neither is a speaker sent back to Auto before it is reassigned.
        let unvoiced = [
            female_british("Speaker A"),
            speaker("Speaker B", Gender::Male, Accent::British),
        ];
        assert_eq!(speaker_change(&unvoiced, &before), SpeakerChange::default());
        assert_eq!(speaker_change(&before, &unvoiced), SpeakerChange::default());
        let mut automatic = before.clone();
        automatic[1].voice = VoiceChoice::Auto;
        assert_eq!(
            speaker_change(&before, &automatic),
            SpeakerChange::default()
        );
        // Reassigned to another voice, it is a change again.
        automatic[1].voice = assigned("gb-m-2");
        assert_eq!(
            speaker_change(&before, &automatic),
            SpeakerChange {
                script: false,
                recording: true
            }
        );
    }

    #[test]
    fn voice_ids_are_checked_and_designed_ids_recognised() {
        let longest = "a".repeat(MAX_VOICE_ID_CHARS);
        let too_long = "a".repeat(MAX_VOICE_ID_CHARS + 1);
        for id in [
            "en-gb-advisor-1",
            "Zephyr",
            "voice_kpd3e297369r",
            longest.as_str(),
        ] {
            assert!(Voice::check_id(id).is_ok(), "{id}");
        }
        for id in ["", "../etc", "en gb", "voice/1", "é", too_long.as_str()] {
            assert!(Voice::check_id(id).is_err(), "{id}");
        }
        assert!(Voice::is_designed_id("voice_kpd3e297369r"));
        assert!(Voice::is_designed_id("voicekey_abc"));
        assert!(!Voice::is_designed_id("en-gb-advisor-1"));
        assert!(!Voice::is_designed_id("Zephyr"));
    }
}
