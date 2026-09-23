//! DOCX rendering of the paper, the key and the transcript, beside the
//! Markdown one. Same content, laid out like the HSG paper: a title block, a
//! blank candidate block (name, number, "số phách", mark), each part's tasks
//! with a table of numbered answer boxes, then the answer key and the
//! transcripts on pages of their own. Times New Roman 13 pt throughout.
//!
//! Pure: the package is built in memory with `docx-rs`, so the browser can
//! download it without a server round trip. `answer_layout` matches every
//! `TaskKind`; a new kind fails to compile until it says where its answers go.

use std::io::Cursor;

use docx_rs::{
    AlignmentType, BreakType, Docx, Paragraph, Run, RunFonts, Table, TableCell, TableLayoutType,
    TableRow, VAlignType, WidthType,
};

use crate::domain::{Exam, ExamFormat, KeyEntry, PartSpec, Passage, SpeakerConfig, Task, TaskKind};

pub const DOCX_MIME: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

const FONT: &str = "Times New Roman";
/// Font sizes in half-points.
const BODY_SIZE: usize = 26;
const HEADING_SIZE: usize = 28;
const TITLE_SIZE: usize = 32;
/// A4 text width with docx-rs's default margins, in twentieths of a point.
const TEXT_WIDTH: usize = 8_504;
const BOXES_PER_ROW: u8 = 5;
const UNDERLINE: &str = "________________________________";

/// Where the candidate writes the answers of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnswerLayout {
    /// A table of numbered boxes under the task ("numbered boxes provided").
    Boxes,
    /// A line under each question ("spaces provided").
    LineBelow,
    /// On the gap inside the sentence itself.
    InPlace,
}

/// Every task kind decides this; the match is exhaustive on purpose.
fn answer_layout(kind: &TaskKind) -> AnswerLayout {
    match kind {
        TaskKind::TrueFalseNotGiven
        | TaskKind::WhoMentioned { .. }
        | TaskKind::MultipleSelect { .. }
        | TaskKind::MultipleChoice { .. }
        | TaskKind::SummaryCompletion(_)
        | TaskKind::NoteCompletion(_)
        | TaskKind::Matching { .. } => AnswerLayout::Boxes,
        TaskKind::ShortAnswer(_) => AnswerLayout::LineBelow,
        TaskKind::SentenceCompletion(_) => AnswerLayout::InPlace,
    }
}

/// Whether the numbered stems are printed; a summary or note carries its own
/// gaps, and a multiple selection is answered from the shared options alone.
fn shows_stems(task: &Task) -> bool {
    match &task.spec.kind {
        TaskKind::MultipleSelect { .. } => false,
        TaskKind::SummaryCompletion(_) | TaskKind::NoteCompletion(_) => task.summary.is_none(),
        TaskKind::TrueFalseNotGiven
        | TaskKind::WhoMentioned { .. }
        | TaskKind::MultipleChoice { .. }
        | TaskKind::ShortAnswer(_)
        | TaskKind::SentenceCompletion(_)
        | TaskKind::Matching { .. } => true,
    }
}

fn text(s: &str) -> Run {
    Run::new().add_text(s)
}

fn para(s: &str) -> Paragraph {
    Paragraph::new().add_run(text(s))
}

fn bold(s: &str) -> Paragraph {
    Paragraph::new().add_run(text(s).bold())
}

fn italic(s: &str) -> Paragraph {
    Paragraph::new().add_run(text(s).italic())
}

fn blank() -> Paragraph {
    Paragraph::new()
}

fn heading(s: &str, size: usize, new_page: bool) -> Paragraph {
    Paragraph::new()
        .add_run(text(s).bold().size(size))
        .page_break_before(new_page)
}

fn base_document() -> Docx {
    Docx::new()
        .default_fonts(
            RunFonts::new()
                .ascii(FONT)
                .hi_ansi(FONT)
                .east_asia(FONT)
                .cs(FONT),
        )
        .default_size(BODY_SIZE)
}

fn fixed_table(rows: Vec<TableRow>, columns: usize) -> Table {
    let width = TEXT_WIDTH / columns.max(1);
    Table::new(rows)
        .set_grid(vec![width; columns])
        .width(TEXT_WIDTH, WidthType::Dxa)
        .layout(TableLayoutType::Fixed)
}

fn cell(width: usize, paragraphs: Vec<Paragraph>) -> TableCell {
    let mut cell = TableCell::new()
        .width(width, WidthType::Dxa)
        .vertical_align(VAlignType::Top);
    for paragraph in paragraphs {
        cell = cell.add_paragraph(paragraph);
    }
    cell
}

/// The paper's first lines: title and, for a whole exam, the format's numbers.
fn title_block(docx: Docx, title: &str, format: Option<&ExamFormat>) -> Docx {
    let docx = docx.add_paragraph(
        Paragraph::new()
            .add_run(text(title).bold().size(TITLE_SIZE))
            .align(AlignmentType::Center),
    );
    match format {
        Some(format) => docx.add_paragraph(
            Paragraph::new()
                .add_run(text(&format!(
                    "{} - {} items - {} points - {} minutes of listening",
                    format.name,
                    format.total_items(),
                    format.total_points,
                    format.listening_minutes
                )))
                .align(AlignmentType::Center),
        ),
        None => docx,
    }
}

/// Blank fields the school fills in; no candidate data lives in the domain.
fn candidate_block(docx: Docx) -> Docx {
    let half = TEXT_WIDTH / 2;
    let dotted = ".........................................";
    let row = TableRow::new(vec![
        cell(
            half,
            vec![
                para(&format!("Họ và tên: {dotted}")),
                para(&format!("Số báo danh: {dotted}")),
            ],
        ),
        cell(
            half,
            vec![
                para(&format!("Số phách: {dotted}")),
                para(&format!("Điểm: {dotted}")),
            ],
        ),
    ]);
    docx.add_paragraph(blank())
        .add_table(fixed_table(vec![row], 2))
        .add_paragraph(blank())
}

fn part_heading(docx: Docx, spec: &PartSpec) -> Docx {
    docx.add_paragraph(heading(&spec.title, HEADING_SIZE, false))
        .add_paragraph(italic(&format!(
            "Listen to {} {} and do the tasks that follow.",
            spec.passage.label().to_lowercase(),
            spec.playback.label()
        )))
}

/// Numbered boxes `first..=last`, five to a row; the last row is padded so
/// the grid stays rectangular.
fn answer_boxes(first: u8, last: u8) -> Table {
    let width = TEXT_WIDTH / usize::from(BOXES_PER_ROW);
    let numbers: Vec<u8> = (first..=last).collect();
    let rows = numbers
        .chunks(usize::from(BOXES_PER_ROW))
        .map(|chunk| {
            let mut cells: Vec<TableCell> = chunk
                .iter()
                .map(|n| cell(width, vec![para(&format!("{n}.")), blank()]))
                .collect();
            while cells.len() < usize::from(BOXES_PER_ROW) {
                cells.push(cell(width, vec![blank(), blank()]));
            }
            TableRow::new(cells).cant_split()
        })
        .collect();
    fixed_table(rows, usize::from(BOXES_PER_ROW))
}

/// One question block as printed on the paper (no answers).
fn task_blocks(mut docx: Docx, task: &Task) -> Docx {
    docx = docx.add_paragraph(bold(&task.instruction));
    for option in &task.shared_options {
        docx = docx.add_paragraph(para(&format!("{}. {}", option.letter, option.text)));
    }
    if let Some(summary) = &task.summary {
        for line in summary.trim().lines() {
            docx = docx.add_paragraph(para(line));
        }
    }
    let layout = answer_layout(&task.spec.kind);
    if shows_stems(task) {
        for item in &task.items {
            docx = docx.add_paragraph(para(&format!("{}. {}", item.number, item.stem.trim())));
            for option in &item.options {
                docx = docx.add_paragraph(para(&format!("    {}. {}", option.letter, option.text)));
            }
            if layout == AnswerLayout::LineBelow {
                docx = docx.add_paragraph(para(&format!("    {UNDERLINE}")));
            }
        }
    }
    docx = docx.add_paragraph(italic(&format!(
        "Your answers: {}",
        task.spec.range_label()
    )));
    if layout == AnswerLayout::Boxes {
        docx = docx.add_table(answer_boxes(task.spec.first, task.spec.last));
    }
    docx.add_paragraph(blank())
}

/// Question / key pairs, two pairs per row so a 40-item key fits one page.
fn key_table(entries: &[KeyEntry]) -> Table {
    let narrow = TEXT_WIDTH / 8;
    let wide = TEXT_WIDTH * 3 / 8;
    let half = entries.len().div_ceil(2);
    let pair = |entry: Option<&KeyEntry>| -> Vec<TableCell> {
        match entry {
            Some(entry) => vec![
                cell(narrow, vec![bold(&entry.number.to_string())]),
                cell(wide, vec![para(&entry.answer.display())]),
            ],
            None => vec![cell(narrow, vec![blank()]), cell(wide, vec![blank()])],
        }
    };
    let mut rows = vec![TableRow::new(vec![
        cell(narrow, vec![bold("Q")]),
        cell(wide, vec![bold("Key")]),
        cell(narrow, vec![bold("Q")]),
        cell(wide, vec![bold("Key")]),
    ])];
    for i in 0..half {
        let mut cells = pair(entries.get(i));
        cells.extend(pair(entries.get(i + half)));
        rows.push(TableRow::new(cells));
    }
    Table::new(rows)
        .set_grid(vec![narrow, wide, narrow, wide])
        .width(TEXT_WIDTH, WidthType::Dxa)
        .layout(TableLayoutType::Fixed)
}

/// The script with the voice line-up, for the teacher's copy.
fn transcript_blocks(mut docx: Docx, passage: &Passage, speakers: &[SpeakerConfig]) -> Docx {
    docx = docx
        .add_paragraph(bold(&format!("Transcript - Part {}", passage.part)))
        .add_paragraph(italic(&passage.topic));
    for speaker in speakers {
        docx = docx.add_paragraph(para(&speaker.describe()));
    }
    docx = docx.add_paragraph(blank());
    for line in &passage.lines {
        docx = docx.add_paragraph(
            Paragraph::new()
                .add_run(text(&format!("{}: ", line.speaker)).bold())
                .add_run(text(&line.text)),
        );
    }
    docx.add_paragraph(blank())
}

fn key_entries(tasks: &[Task]) -> Vec<KeyEntry> {
    tasks
        .iter()
        .flat_map(|task| {
            task.answers().map(|(number, answer)| KeyEntry {
                number,
                answer: answer.clone(),
            })
        })
        .collect()
}

/// Paper, key and transcripts of the whole exam, as a document to build.
pub fn exam_document(exam: &Exam) -> Docx {
    let format = &exam.format;
    let mut docx = title_block(base_document(), &exam.title, Some(format));
    docx = candidate_block(docx);
    docx = docx.add_paragraph(para(&format!(
        "Parts played once are not repeated; at the start of each recording you will hear a sound. \
         You have {} minutes to check your answers at the end.",
        format.check_minutes
    )));
    docx = docx.add_paragraph(blank());
    for part in &exam.parts {
        docx = part_heading(docx, &part.spec);
        for task in &part.tasks {
            docx = task_blocks(docx, task);
        }
    }
    docx = docx
        .add_paragraph(heading("Answer key", HEADING_SIZE, true))
        .add_table(key_table(&exam.answer_key()))
        .add_paragraph(heading("Transcripts", HEADING_SIZE, true));
    for part in &exam.parts {
        if let Some(passage) = &part.passage {
            docx = transcript_blocks(docx, passage, &part.speakers);
        }
    }
    docx
}

/// One part's paper, then its key and (when given) its transcript, each on
/// a new page.
pub fn part_document(
    spec: &PartSpec,
    tasks: &[Task],
    transcript: Option<(&Passage, &[SpeakerConfig])>,
) -> Docx {
    let mut docx = title_block(base_document(), &spec.title, None);
    docx = docx.add_paragraph(italic(&format!(
        "Listen to {} {} and do the tasks that follow.",
        spec.passage.label().to_lowercase(),
        spec.playback.label()
    )));
    for task in tasks {
        docx = task_blocks(docx, task);
    }
    docx = docx
        .add_paragraph(heading("Key", HEADING_SIZE, true))
        .add_table(key_table(&key_entries(tasks)));
    if let Some((passage, speakers)) = transcript {
        docx = docx.add_paragraph(
            Paragraph::new()
                .add_run(Run::new().add_break(BreakType::Page))
                .add_run(text("").bold()),
        );
        docx = transcript_blocks(docx, passage, speakers);
    }
    docx
}

/// The bytes of the `.docx` file (a ZIP package built in memory).
fn pack(docx: Docx) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    docx.build()
        .pack(&mut cursor)
        .expect("writing a DOCX package into memory cannot fail");
    cursor.into_inner()
}

/// The whole exam as a `.docx` file.
pub fn render_exam_docx(exam: &Exam) -> Vec<u8> {
    pack(exam_document(exam))
}

/// One part's paper, key and transcript as a `.docx` file.
pub fn render_part_docx(
    spec: &PartSpec,
    tasks: &[Task],
    transcript: Option<(&Passage, &[SpeakerConfig])>,
) -> Vec<u8> {
    pack(part_document(spec, tasks, transcript))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Answer, Choice, ExamFormat, Item, TaskSpec, Tfng, WordLimit};

    fn document_xml(docx: Docx) -> String {
        String::from_utf8(docx.build().document).unwrap()
    }

    fn letters(n: u8) -> Vec<Choice> {
        (0..n)
            .map(|i| Choice {
                letter: (b'A' + i) as char,
                text: format!("Option {}", i + 1),
            })
            .collect()
    }

    /// One item per number with a kind-appropriate key.
    fn fixture_task(spec: &TaskSpec) -> Task {
        let kind = &spec.kind;
        let shared_options = match kind.option_count() {
            Some(n) if kind.has_shared_options() => letters(n),
            _ => Vec::new(),
        };
        let summary = match kind {
            TaskKind::SummaryCompletion(_) | TaskKind::NoteCompletion(_) => Some(
                (spec.first..=spec.last)
                    .map(|n| format!("Line with gap ({n})______ in it."))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => None,
        };
        let items = (spec.first..=spec.last)
            .map(|n| Item {
                number: n,
                stem: format!("Stem {n}"),
                options: match kind {
                    TaskKind::MultipleChoice { options } => letters(*options),
                    _ => Vec::new(),
                },
                answer: match kind {
                    TaskKind::TrueFalseNotGiven => Answer::Tfng(Tfng::NotGiven),
                    TaskKind::WhoMentioned { .. }
                    | TaskKind::MultipleSelect { .. }
                    | TaskKind::MultipleChoice { .. }
                    | TaskKind::Matching { .. } => Answer::Letters(vec!['A']),
                    TaskKind::ShortAnswer(_)
                    | TaskKind::SummaryCompletion(_)
                    | TaskKind::NoteCompletion(_)
                    | TaskKind::SentenceCompletion(_) => Answer::Text(vec!["word".into()]),
                },
                evidence: String::new(),
            })
            .collect();
        Task {
            spec: spec.clone(),
            instruction: spec.instruction(),
            shared_options,
            summary,
            items,
        }
    }

    fn filled_exam() -> Exam {
        let mut exam = Exam::new(ExamFormat::hsg_national(), "Mock 1", "news");
        for part in &mut exam.parts {
            part.tasks = part.spec.tasks.iter().map(fixture_task).collect();
            let labels: Vec<String> = part.speakers.iter().map(|s| s.label.clone()).collect();
            part.passage = Some(
                Passage::parse(
                    part.spec.number,
                    format!("Topic {}", part.spec.number),
                    "Speaker A: Hello there.\nSpeaker B: Hi.",
                    &labels,
                )
                .unwrap(),
            );
        }
        exam
    }

    #[test]
    fn exam_document_has_all_sections() {
        let built = exam_document(&filled_exam()).build();
        let xml = String::from_utf8(built.document).unwrap();
        for marker in [
            "Mock 1",
            "HSG Quoc gia - Listening - 35 items - 5 points - 30 minutes of listening",
            "Số phách",
            "Part 1",
            "Part 4",
            ">26.</w:t>",
            ">35.</w:t>",
            "Stem 1",
            "Answer key",
            "Transcripts",
            "Transcript - Part 4",
            "Speaker A: ",
        ] {
            assert!(xml.contains(marker), "missing {marker}");
        }
        // The five T/F/NG keys appear in the key table and nowhere else.
        assert_eq!(xml.matches(">NG</w:t>").count(), 5);
        // Times New Roman 13 pt is the document default, set in the styles part.
        let styles = String::from_utf8(built.styles).unwrap();
        assert!(styles.contains("Times New Roman"));
        assert!(styles.contains("w:val=\"26\""));
    }

    #[test]
    fn every_task_kind_renders() {
        let mut specs: Vec<TaskSpec> = [ExamFormat::hsg_national(), ExamFormat::ielts_listening()]
            .iter()
            .flat_map(|f| f.parts.iter().flat_map(|p| p.tasks.clone()))
            .collect();
        specs.push(TaskSpec::new(
            TaskKind::SentenceCompletion(WordLimit::words(2)),
            1,
            3,
        ));
        let mut seen = std::collections::HashSet::new();
        for spec in &specs {
            seen.insert(spec.kind.label());
            let task = fixture_task(spec);
            let xml = document_xml(task_blocks(Docx::new(), &task));
            assert!(xml.contains(&task.instruction), "{}", spec.kind.label());
            match answer_layout(&spec.kind) {
                AnswerLayout::Boxes => assert!(xml.contains("<w:tbl>"), "{}", spec.kind.label()),
                AnswerLayout::LineBelow => {
                    assert!(xml.contains(UNDERLINE), "{}", spec.kind.label())
                }
                AnswerLayout::InPlace => {
                    assert!(!xml.contains("<w:tbl>") && !xml.contains(UNDERLINE))
                }
            }
        }
        assert_eq!(seen.len(), 9, "every task kind is covered: {seen:?}");
    }

    #[test]
    fn answer_boxes_fill_rows_of_five() {
        let xml = document_xml(Docx::new().add_table(answer_boxes(26, 35)));
        assert_eq!(xml.matches("<w:tr>").count(), 2);
        assert_eq!(xml.matches("<w:tc>").count(), 10);
        for n in 26..=35 {
            assert!(xml.contains(&format!(">{n}.</w:t>")));
        }
        let xml = document_xml(Docx::new().add_table(answer_boxes(1, 7)));
        assert_eq!(xml.matches("<w:tr>").count(), 2);
        assert_eq!(xml.matches("<w:tc>").count(), 10);
        assert!(!xml.contains(">8.</w:t>"));
    }

    #[test]
    fn part_document_includes_key_and_transcript() {
        let exam = filled_exam();
        let part = &exam.parts[2];
        let passage = part.passage.as_ref().unwrap();
        let xml = document_xml(part_document(
            &part.spec,
            &part.tasks,
            Some((passage, &part.speakers)),
        ));
        assert!(xml.contains("Part 3"));
        assert!(xml.contains(">Key</w:t>"));
        assert!(xml.contains(">21</w:t>"));
        assert!(xml.contains("Transcript - Part 3"));
        assert!(xml.contains("Hello there."));
        assert!(!xml.contains("Số phách"));
        let without = document_xml(part_document(&part.spec, &part.tasks, None));
        assert!(!without.contains("Transcript - Part 3"));
    }

    /// Manual check helper: `cargo test ... dump_fixture -- --ignored` writes
    /// the fixture exam as a saved-exam JSON and as a DOCX under the temp dir.
    #[test]
    #[ignore]
    fn dump_fixture() {
        let exam = filled_exam();
        let saved = crate::application::exams::SavedExam {
            topics: vec![String::new(); exam.parts.len()],
            exam: exam.clone(),
            recording_job: None,
            recording: None,
            recording_stale: false,
            created_at_secs: 0,
            updated_at_secs: 0,
        };
        let dir = std::env::temp_dir();
        std::fs::write(
            dir.join("fixture-exam.json"),
            serde_json::to_string(&saved).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("fixture-exam.docx"), render_exam_docx(&exam)).unwrap();
    }

    #[test]
    fn packed_bytes_are_a_zip() {
        let bytes = render_exam_docx(&filled_exam());
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        assert!(bytes.len() > 2_000);
        let part = render_part_docx(&ExamFormat::hsg_national().parts[0], &[], None);
        assert_eq!(&part[..4], b"PK\x03\x04");
    }
}
