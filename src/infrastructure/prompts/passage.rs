use crate::domain::{ExamFormat, PartSpec, PassageKind, SpeakerConfig, TaskKind};

/// The script prompt. It tells the model what the questions will need so the
/// passage contains enough concrete, distinct material.
pub fn passage_prompt(format: &ExamFormat, part: &PartSpec, topic: &str, speakers: &[SpeakerConfig]) -> String {
    let speaker_lines: Vec<String> = speakers.iter().map(|s| format!("- {}", s.describe())).collect();
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
    let needs: Vec<String> = part.tasks.iter().map(|t| format!("- {}", task_needs(&t.kind))).collect();
    let target_words = ((part.min_minutes + part.max_minutes) / 2.0 * 150.0).round() as u32;

    format!(
        "You write scripts for listening exams in the format \"{}\".\n\n\
         PART: {} ({})\n\
         GENRE: {}\n\
         SCENARIO: {}\n\
         TARGET LENGTH: about {} words (spoken at roughly 150 words per minute this is {}).\n\n\
         SPEAKERS:\n{}\n\n\
         SHAPE: {}\n\n\
         THE QUESTIONS WRITTEN LATER WILL NEED:\n{}\n\n\
         RULES:\n\
         1. Every speaking turn starts with the exact label from the SPEAKERS list followed by a colon \
            ({}). Never use character names or roles as labels; names may be spoken inside the lines.\n\
         2. The script is complete: no gaps, no blanks, no [FILL IN], no stage directions, no notes.\n\
         3. Natural spoken English at the level of an advanced learner exam: contractions, \
            some hesitation, paraphrase and correction, but dense in checkable facts.\n\
         4. Do not repeat the same fact twice unless the exam shape above asks for it.\n\
         5. Output ONLY the script lines, nothing before or after.",
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
        labels.join(", ")
    )
}

fn task_needs(kind: &TaskKind) -> String {
    match kind {
        TaskKind::TrueFalseNotGiven => "statements that can be confirmed, contradicted, or left genuinely unaddressed".into(),
        TaskKind::WhoMentioned { .. } => "clearly attributable claims: some unique to one guest, some made by both".into(),
        TaskKind::MultipleSelect { choose, options } => {
            format!("a cluster of {options} plausible statements of which exactly {choose} are true")
        }
        TaskKind::MultipleChoice { .. } => "reasons, purposes, exceptions and comparisons that invite close distractors".into(),
        TaskKind::ShortAnswer(limit) => {
            format!("precise terms answerable in {} (technical words, names, quantities)", limit.instruction().to_lowercase())
        }
        TaskKind::SummaryCompletion(limit) => {
            format!("a clear structure that a summary can follow, with key words fitting {}", limit.instruction().to_lowercase())
        }
        TaskKind::NoteCompletion(limit) => {
            format!("names, numbers, dates, addresses and items fitting {}", limit.instruction().to_lowercase())
        }
        TaskKind::SentenceCompletion(limit) => {
            format!("statements whose endings are single expressions fitting {}", limit.instruction().to_lowercase())
        }
        TaskKind::Matching { options } => format!("{options} distinct places, people or features described in turn"),
    }
}
