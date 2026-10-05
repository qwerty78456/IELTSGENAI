use crate::domain::{
    Accent, EXAM_SPEECH_TAGS, ExamFormat, Gender, PartSpec, PassageKind, SpeakerConfig, TaskKind,
};

/// The script prompt. It tells the model what the questions will need so the
/// passage contains enough concrete, distinct material, and that every
/// speaker's names, pronouns, words and spelling fit their gender and
/// accent. An `expressive` script may carry a few of the `EXAM_SPEECH_TAGS`
/// (`docs/voices.md`, gate G3); a plain one carries none. Emotion otherwise
/// comes from wording and punctuation: a per-line delivery style moves the
/// voice (gate G5), so there is none.
pub fn passage_prompt(
    format: &ExamFormat,
    part: &PartSpec,
    topic: &str,
    speakers: &[SpeakerConfig],
    expressive: bool,
) -> String {
    let speaker_lines: Vec<String> = speakers
        .iter()
        .map(|s| format!("- {}", s.describe()))
        .collect();
    let fit_lines: Vec<String> = speakers
        .iter()
        .map(|s| {
            format!(
                "- {}: {}; {} spelling and vocabulary",
                s.label,
                pronouns(s.gender),
                spelling(s.accent)
            )
        })
        .collect();
    let labels: Vec<&str> = speakers.iter().map(|s| s.label.as_str()).collect();
    let shape = match part.passage {
        PassageKind::Conversation { .. } => {
            "A natural two-way conversation with turn-taking, clarifications, corrections and \
             back-channel responses. Both speakers must carry information."
        }
        PassageKind::Interview { .. } => {
            "A broadcast interview: the host opens, introduces each guest by name and role, asks \
             focused questions and closes. Each guest must make several DISTINCT claims of their own, \
             and at least two points must be raised by BOTH guests so that 'who mentioned it' items work."
        }
        PassageKind::Monologue => {
            "A single presenter speaking to an audience, with a clear opening, signposted sections and a close."
        }
        PassageKind::Excerpt => {
            "Part of a longer talk by one speaker. Start mid-flow (no greeting) and stop mid-flow (no farewell), \
             as if the recording were an extract."
        }
    };
    let needs: Vec<String> = part
        .tasks
        .iter()
        .map(|t| format!("- {}", task_needs(&t.kind)))
        .collect();
    let target_words = ((part.min_minutes + part.max_minutes) / 2.0 * 150.0).round() as u32;
    let example_label = labels.first().copied().unwrap_or("Speaker A");
    let (delivery, no_markup) = if expressive {
        (
            delivery_block(example_label),
            "; the only exception is the few speech tags allowed under DELIVERY",
        )
    } else {
        (String::new(), ", and no tags in angle brackets")
    };

    format!(
        "You write scripts for listening exams in the format \"{}\".\n\n\
         PART: {} ({})\n\
         GENRE: {}\n\
         SCENARIO: {}\n\
         TARGET LENGTH: about {} words (spoken at roughly 150 words per minute this is {}).\n\n\
         SPEAKERS:\n{}\n\n\
         SHAPE: {}\n\n\
         THE QUESTIONS WRITTEN LATER WILL NEED:\n{}\n\n\
         SPEAKER FIT:\n{}\n\n\
         {}\
         RULES:\n\
         1. Every speaking turn starts with the exact label from the SPEAKERS list followed by a colon \
            ({}). Never use character names or roles as labels; names may be spoken inside the lines.\n\
         2. A speech model reads the script aloud word for word, so it is complete and holds only \
            spoken words: no gaps, no blanks, no [FILL IN], no notes, no headings, no stage directions \
            or sound descriptions in brackets, parentheses or asterisks{}.\n\
         3. Each speaker is one consistent person who fits their SPEAKERS entry and talks like their \
            role. Any name they are given and every pronoun used for them match their gender (SPEAKER \
            FIT). Speakers who share a gender or an accent get clearly different names and manners.\n\
         4. Vocabulary, idiom and spelling fit each speaker's English (SPEAKER FIT): British, \
            Australian, New Zealand, Irish, Scottish, South African and Indian English speakers use \
            British spelling and words (colour, centre, organise; flat, mobile, queue, petrol); American \
            and Canadian English speakers use American spelling and words (color, center, organize; \
            apartment, cell phone, line, gas). Never mention an accent and never spell words to \
            imitate one; the voice carries the accent.\n\
         5. Natural spoken English at the level of an advanced learner exam: contractions, \
            some hesitation, paraphrase and correction, but dense in checkable facts.\n\
         6. Do not repeat the same fact twice unless the exam shape above asks for it.\n\
         7. Output ONLY the script lines, nothing before or after.",
        format.name,
        part.title,
        part.passage.label(),
        part.brief,
        topic.trim(),
        target_words,
        part.duration_label(),
        speaker_lines.join("\n"),
        shape,
        needs.join("\n"),
        fit_lines.join("\n"),
        delivery,
        labels.join(", "),
        no_markup
    )
}

/// The DELIVERY block of an expressive script: the allowed tags, where they
/// go and how sparingly (`docs/voices.md`, gate G3).
fn delivery_block(example_label: &str) -> String {
    let tags: Vec<String> = EXAM_SPEECH_TAGS.iter().map(|t| format!("<{t}>")).collect();
    format!(
        "DELIVERY:\n\
         - The speech model performs only these tags as sounds: {}. Write a tag exactly like that, in \
           angle brackets and lower case.\n\
         - Use one tag every few turns at most, only where the sound is natural (a sigh before bad \
           news, a laugh at a joke), and never more than one in a turn.\n\
         - Put a tag in the middle of a sentence, after a few words: never as the first word of a turn \
           or of a sentence, and never inside a name, number, date, spelling or other fact a \
           question could test. Example: \"{example_label}: Well, I did ask <sigh> but nobody called \
           me back.\"\n\
         - Never any other tag, no square brackets, no pipe characters, and no words in parentheses \
           or asterisks saying how something is said: the speech model would read them out.\n\
         - Carry the rest of the emotion through wording and punctuation: hesitation (\"well...\", \
           \"um\"), self-correction, exclamations, questions, dashes and commas.\n\n",
        tags.join(" ")
    )
}

fn pronouns(gender: Gender) -> &'static str {
    match gender {
        Gender::Female => "she/her",
        Gender::Male => "he/him",
    }
}

/// The spelling and vocabulary a speaker's English uses.
fn spelling(accent: Accent) -> &'static str {
    match accent {
        Accent::American | Accent::Canadian => "American",
        Accent::British
        | Accent::Australian
        | Accent::NewZealand
        | Accent::Irish
        | Accent::Scottish
        | Accent::SouthAfrican
        | Accent::Indian => "British",
    }
}

fn task_needs(kind: &TaskKind) -> String {
    match kind {
        TaskKind::TrueFalseNotGiven => {
            "statements that can be confirmed, contradicted, or left genuinely unaddressed".into()
        }
        TaskKind::WhoMentioned { .. } => {
            "clearly attributable claims: some unique to one guest, some made by both".into()
        }
        TaskKind::MultipleSelect { choose, options } => {
            format!(
                "a cluster of {options} plausible statements of which exactly {choose} are true"
            )
        }
        TaskKind::MultipleChoice { .. } => {
            "reasons, purposes, exceptions and comparisons that invite close distractors".into()
        }
        TaskKind::ShortAnswer(limit) => {
            format!(
                "precise terms answerable in {} (technical words, names, quantities)",
                limit.instruction().to_lowercase()
            )
        }
        TaskKind::SummaryCompletion(limit) => {
            format!(
                "a clear structure that a summary can follow, with key words fitting {}",
                limit.instruction().to_lowercase()
            )
        }
        TaskKind::NoteCompletion(limit) => {
            format!(
                "names, numbers, dates, addresses and items fitting {}",
                limit.instruction().to_lowercase()
            )
        }
        TaskKind::SentenceCompletion(limit) => {
            format!(
                "statements whose endings are single expressions fitting {}",
                limit.instruction().to_lowercase()
            )
        }
        TaskKind::Matching { options } => {
            format!("{options} distinct places, people or features described in turn")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FormatId, SPEECH_TAGS};

    fn prompt(expressive: bool) -> String {
        let format = FormatId::IeltsListening.format();
        let part = format.part(1).unwrap().clone();
        let speakers = part.default_speakers.clone();
        passage_prompt(
            &format,
            &part,
            "Booking a room at a sports centre",
            &speakers,
            expressive,
        )
    }

    #[test]
    fn plain_scripts_forbid_markup() {
        let plain = prompt(false);
        assert!(plain.contains("no stage directions"), "{plain}");
        assert!(plain.contains("no tags in angle brackets"), "{plain}");
        assert!(!plain.contains("DELIVERY"), "{plain}");
        for tag in SPEECH_TAGS {
            assert!(!plain.contains(&format!("<{tag}>")), "{tag}");
        }
    }

    #[test]
    fn expressive_scripts_offer_the_exam_tags_only() {
        let expressive = prompt(true);
        assert!(expressive.contains("DELIVERY:"), "{expressive}");
        assert!(expressive.contains("no stage directions"), "{expressive}");
        assert!(
            expressive.contains("never as the first word of a turn"),
            "{expressive}"
        );
        assert!(
            !expressive.contains("no tags in angle brackets"),
            "{expressive}"
        );
        for tag in SPEECH_TAGS {
            let listed = expressive.contains(&format!("<{tag}>"));
            assert_eq!(listed, EXAM_SPEECH_TAGS.contains(tag), "{tag}");
        }
        assert!(expressive.contains("Speaker A: Well, I did ask <sigh>"));
        assert!(!expressive.contains("|mhm|"));
    }

    #[test]
    fn speakers_must_fit_gender_and_accent() {
        for expressive in [false, true] {
            let text = prompt(expressive);
            // IELTS Part 1: a British receptionist and an American customer.
            assert!(
                text.contains("- Speaker A: she/her; British spelling and vocabulary"),
                "{text}"
            );
            assert!(
                text.contains("- Speaker B: he/him; American spelling and vocabulary"),
                "{text}"
            );
            assert!(text.contains("every pronoun used for them match their gender"));
            assert!(text.contains("never spell words to imitate one"));
        }
        assert_eq!(spelling(Accent::Canadian), "American");
        assert_eq!(spelling(Accent::Indian), "British");
    }
}
