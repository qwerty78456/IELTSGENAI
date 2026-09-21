use crate::domain::{ExamFormat, PartSpec};

/// Ask for one scenario sentence that fits the part; `theme` narrows it
/// ("news listening") and may be empty.
pub fn topic_prompt(format: &ExamFormat, part: &PartSpec, theme: &str) -> String {
    let theme_line = if theme.trim().is_empty() {
        String::new()
    } else {
        format!("OVERALL THEME OF THE EXAM: {}\n\n", theme.trim())
    };
    format!(
        "You design listening exams in the format \"{}\".\n\n\
         {theme_line}\
         PART: {} ({})\n\
         GENRE: {}\n\
         LENGTH: {}\n\n\
         Write ONE specific, realistic scenario (one or two sentences) for this part. \
         Name the setting and, for dialogues, who is speaking to whom. Prefer contemporary, \
         factual subjects that allow precise details (numbers, names, dates, terms).\n\n\
         Respond with the scenario only, no heading, no quotes, no commentary.",
        format.name,
        part.title,
        part.passage.label(),
        part.brief,
        part.duration_label()
    )
}
