//! Markdown rendering of the paper, the key and the transcript. Paste into
//! Word or convert with pandoc; a DOCX exporter can be added beside it.

use crate::domain::{Exam, ExamPart, PartSpec, Passage, SpeakerConfig, Task, TaskKind};

/// The question block as printed on the paper (no answers).
pub fn render_task(task: &Task) -> String {
    let mut out = String::new();
    out.push_str(&format!("**{}**\n\n", task.instruction));
    if !task.shared_options.is_empty() {
        for option in &task.shared_options {
            out.push_str(&format!("- **{}** {}\n", option.letter, option.text));
        }
        out.push('\n');
    }
    if let Some(summary) = &task.summary {
        out.push_str(summary.trim());
        out.push_str("\n\n");
    }
    let bare_items =
        task.summary.is_some() || matches!(task.spec.kind, TaskKind::MultipleSelect { .. });
    if !bare_items {
        for item in &task.items {
            out.push_str(&format!("{}. {}\n", item.number, item.stem.trim()));
            for option in &item.options {
                out.push_str(&format!("   {}. {}\n", option.letter, option.text));
            }
            if matches!(task.spec.kind, TaskKind::ShortAnswer(_)) {
                out.push_str("   ________________________________\n");
            }
        }
        out.push('\n');
    }
    out.push_str(&format!("*Your answers: {}*\n\n", task.spec.range_label()));
    out
}

/// A whole part: header line, then its tasks.
pub fn render_part_paper(spec: &PartSpec, tasks: &[Task]) -> String {
    let mut out = format!(
        "## {}\n\n*Listen to {} {} and do the tasks that follow.*\n\n",
        spec.title,
        spec.passage.label().to_lowercase(),
        spec.playback.label()
    );
    for task in tasks {
        out.push_str(&render_task(task));
    }
    out
}

/// Number / answer table.
pub fn render_key(tasks: &[Task]) -> String {
    let mut out = String::from("| Q | Key |\n|---|-----|\n");
    for task in tasks {
        for (number, answer) in task.answers() {
            out.push_str(&format!("| {number} | {} |\n", answer.display()));
        }
    }
    out
}

/// The transcript with the voice line-up, for the teacher's copy (never
/// the student paper, which names no voice): the words a listener hears,
/// without speech tags.
pub fn render_transcript(passage: &Passage, speakers: &[SpeakerConfig]) -> String {
    let mut out = format!(
        "### Transcript - Part {}\n\n*{}*\n\n",
        passage.part, passage.topic
    );
    for speaker in speakers {
        out.push_str(&format!("- {}\n", speaker.describe_with_voice()));
    }
    out.push('\n');
    for line in &passage.lines {
        let words = line.display_text();
        if !words.is_empty() {
            out.push_str(&format!("**{}:** {words}\n\n", line.speaker));
        }
    }
    out
}

fn render_exam_part(part: &ExamPart) -> String {
    render_part_paper(&part.spec, &part.tasks)
}

/// Paper, key and transcripts of the whole exam in one document.
pub fn render_exam(exam: &Exam) -> String {
    let format = &exam.format;
    let mut out = format!(
        "# {}\n\n*{}* - {} items, {} points, {} minutes of listening. \
         Parts played once are not repeated; at the start of each recording you will hear a sound. \
         You have {} minutes to check your answers at the end.\n\n",
        exam.title,
        format.name,
        format.total_items(),
        format.total_points,
        format.listening_minutes,
        format.check_minutes
    );
    for part in &exam.parts {
        out.push_str(&render_exam_part(part));
    }
    out.push_str("---\n\n# Answer key\n\n| Q | Key |\n|---|-----|\n");
    for entry in exam.answer_key() {
        out.push_str(&format!(
            "| {} | {} |\n",
            entry.number,
            entry.answer.display()
        ));
    }
    out.push_str("\n---\n\n# Transcripts\n\n");
    for part in &exam.parts {
        if let Some(passage) = &part.passage {
            out.push_str(&render_transcript(passage, &part.speakers));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Answer, Choice, ExamFormat, Item, TaskSpec, Tfng};

    #[test]
    fn renders_shared_options_and_key() {
        let task = Task {
            spec: TaskSpec::new(TaskKind::TrueFalseNotGiven, 1, 2),
            instruction: "True, false or not given?".into(),
            shared_options: vec![Choice {
                letter: 'S',
                text: "Samara".into(),
            }],
            summary: None,
            items: vec![
                Item {
                    number: 1,
                    stem: "First.".into(),
                    options: vec![],
                    answer: Answer::Tfng(Tfng::True),
                    evidence: String::new(),
                },
                Item {
                    number: 2,
                    stem: "Second.".into(),
                    options: vec![],
                    answer: Answer::Tfng(Tfng::NotGiven),
                    evidence: String::new(),
                },
            ],
        };
        let paper = render_task(&task);
        assert!(paper.contains("- **S** Samara"));
        assert!(paper.contains("2. Second."));
        assert!(!paper.contains("NG"));
        assert!(render_key(&[task]).contains("| 2 | NG |"));
    }

    #[test]
    fn exam_document_has_all_sections() {
        let exam = Exam::new(ExamFormat::hsg_national(), "Mock 1", "news");
        let doc = render_exam(&exam);
        assert!(doc.contains("# Mock 1"));
        assert!(doc.contains("## Part 4"));
        assert!(doc.contains("# Answer key"));
    }

    #[test]
    fn transcripts_have_no_delivery_tags() {
        use crate::domain::{SpeakerRole, Voice, VoiceSource};

        let labels = ["Speaker A".to_string(), "Speaker B".to_string()];
        let passage = Passage::parse(
            1,
            "Booking",
            "Speaker A: Well <sigh>, I suppose |mhm| so [music].\nSpeaker B: <laugh>\nSpeaker B: Fine <chuckle> by me | really!",
            &labels,
        )
        .unwrap();
        let speakers = vec![
            SpeakerConfig::new(
                "Speaker A",
                crate::domain::Gender::Female,
                crate::domain::Accent::British,
                SpeakerRole::Receptionist,
            )
            .with_voice(Voice {
                id: "en-gb-advisor-1".into(),
                name: "Oliver".into(),
                gender: crate::domain::Gender::Female,
                accent: crate::domain::Accent::British,
                source: VoiceSource::Library,
                description: String::new(),
            }),
            SpeakerConfig::new(
                "Speaker B",
                crate::domain::Gender::Male,
                crate::domain::Accent::American,
                SpeakerRole::Guest,
            ),
        ];
        let transcript = render_transcript(&passage, &speakers);
        for markup in ['<', '>', '|', '[', ']'] {
            assert!(!transcript.contains(markup), "{markup}: {transcript}");
        }
        assert!(transcript.contains("**Speaker A:** Well, I suppose so."));
        assert!(transcript.contains("**Speaker B:** Fine by me really!"));
        assert_eq!(transcript.matches("**Speaker B:**").count(), 1);
        // The transcript names the voice; the paper never lists speakers.
        assert!(
            transcript.contains("- Speaker A: Female, British English, Receptionist; voice Oliver")
        );
        let format = ExamFormat::ielts_listening();
        assert!(!render_part_paper(&format.parts[0], &[]).contains("Oliver"));
    }
}
