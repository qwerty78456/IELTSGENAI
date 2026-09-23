use serde::Deserialize;

use crate::domain::{Choice, Item, PartSpec, Passage, SpeakerConfig, Task, TaskKind, TaskSpec};

/// What the model is asked to return for one task. Same shape as `Task`
/// minus the spec, which the server owns.
#[derive(Debug, Clone, Deserialize)]
pub struct TaskDraftDto {
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub shared_options: Vec<Choice>,
    #[serde(default)]
    pub summary: Option<String>,
    pub items: Vec<Item>,
}

impl TaskDraftDto {
    pub fn into_task(self, spec: TaskSpec) -> Task {
        let instruction = self
            .instruction
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| spec.instruction());
        Task {
            spec,
            instruction,
            shared_options: self.shared_options,
            summary: self.summary,
            items: self.items,
        }
    }
}

/// The question prompt for one task of a part. The passage is embedded in
/// full; keys must be quoted verbatim so the validator can ground them.
pub fn task_prompt(
    part: &PartSpec,
    spec: &TaskSpec,
    passage: &Passage,
    speakers: &[SpeakerConfig],
) -> String {
    let speaker_lines: Vec<String> = speakers
        .iter()
        .map(|s| format!("- {}", s.describe()))
        .collect();
    format!(
        "You write questions for a listening exam. Below is the complete script of {} ({}).\n\n\
         SPEAKERS:\n{}\n\n\
         SCRIPT:\n{}\n\n\
         TASK: {} for questions {} to {} ({} items).\n\
         RUBRIC PRINTED ON THE PAPER: {}\n\n\
         {}\n\n\
         GENERAL RULES:\n\
         - Number the items {} to {} in order of the information in the script.\n\
         - Items test understanding, not memory of trivia; paraphrase the script in stems and options, \
           never copy sentences.\n\
         - Distractors must be plausible and drawn from the script's own content.\n\
         - `evidence` is a VERBATIM quotation from the script (10-25 words) that justifies the key.\n\
         - Return ONLY a JSON object with this shape:\n\
         {{\"instruction\": string or null, \"shared_options\": [{{\"letter\": \"A\", \"text\": \"...\"}}], \
         \"summary\": string or null, \"items\": [{{\"number\": {}, \"stem\": \"...\", \
         \"options\": [{{\"letter\": \"A\", \"text\": \"...\"}}], \"answer\": {{\"kind\": \"...\", \"value\": ...}}, \
         \"evidence\": \"...\"}}]}}\n\
         - `answer.kind` is one of \"letters\" (value: array of one-character strings), \
           \"text\" (value: array of accepted spellings, first is canonical), \"tfng\" (value: \"T\", \"F\" or \"NG\").",
        part.title,
        part.passage.label(),
        speaker_lines.join("\n"),
        passage.script_text(),
        spec.kind.label(),
        spec.first,
        spec.last,
        spec.count(),
        spec.instruction(),
        kind_rules(&spec.kind, spec),
        spec.first,
        spec.last,
        spec.first
    )
}

fn kind_rules(kind: &TaskKind, spec: &TaskSpec) -> String {
    match kind {
        TaskKind::TrueFalseNotGiven => "TASK RULES:\n\
             - Each item is a declarative statement; `options` is empty; `answer` is tfng.\n\
             - Mix T, F and NG. NG means the script says nothing either way; F means it contradicts.\n\
             - Statements paraphrase the script; numbers and names may be kept."
            .into(),
        TaskKind::WhoMentioned { guests } => format!(
            "TASK RULES:\n\
             - `shared_options` lists {guests} guest entries plus one entry for both. Use the initial of each \
               guest's first name (as introduced in the script) as its letter and \"B\" for \"Both of the guests\"; \
               if an initial clashes with B or another initial, use the surname initial instead.\n\
             - Each item is a short noun phrase summarising a point; `options` is empty; `answer` is one letter.\n\
             - At least one item per guest alone and at least one \"Both\"."
        ),
        TaskKind::MultipleSelect { choose, options } => format!(
            "TASK RULES:\n\
             - `shared_options` has exactly {options} statements lettered A onwards about the situation described; \
               exactly {choose} of them are true according to the script and the rest are false or unmentioned.\n\
             - Produce {} items (numbers {} to {}); each item has an empty `stem` and empty `options`; the items' \
               answers together are the {choose} true letters, one letter per item, in alphabetical order.",
            spec.count(),
            spec.first,
            spec.last
        ),
        TaskKind::MultipleChoice { options } => format!(
            "TASK RULES:\n\
             - Each item has a stem (a question or an incomplete sentence) and exactly {options} `options` \
               lettered A onwards; `answer` is one letter.\n\
             - Include stems about purpose (\"was mentioned in order to\"), exceptions (\"all of the following EXCEPT\") \
               and implied meaning, as in the reference paper."
        ),
        TaskKind::ShortAnswer(limit) => format!(
            "TASK RULES:\n\
             - Each item is a question whose answer is words taken exactly from the script, {}.\n\
             - `options` is empty; `answer` is text; every accepted spelling must occur verbatim in the script.",
            limit.instruction()
        ),
        TaskKind::SummaryCompletion(limit) => format!(
            "TASK RULES:\n\
             - `summary` is a paragraph (120-220 words) that paraphrases the script in order, containing gaps written \
               exactly as (n)______ for every number {} to {}.\n\
             - Each item has an empty `stem`, empty `options`, and a text answer that fills gap (n), {}, taken verbatim \
               from the script. The words around each gap must be paraphrased so the answer cannot be found by \
               matching the sentence.",
            spec.first,
            spec.last,
            limit.instruction()
        ),
        TaskKind::NoteCompletion(limit) => format!(
            "TASK RULES:\n\
             - `summary` holds the notes or form as plain text lines with gaps written exactly as (n)______ for every \
               number {} to {}. Use headings and short labels, as on an IELTS paper.\n\
             - Each item has an empty `stem`, empty `options`, and a text answer, {}, taken verbatim from the script.",
            spec.first,
            spec.last,
            limit.instruction()
        ),
        TaskKind::SentenceCompletion(limit) => format!(
            "TASK RULES:\n\
             - Each item's stem is a sentence ending in ______; `answer` is text taken verbatim from the script, {}.",
            limit.instruction()
        ),
        TaskKind::Matching { options } => format!(
            "TASK RULES:\n\
             - `shared_options` has exactly {options} entries lettered A onwards (more options than items).\n\
             - Each item's stem names the thing to match; `options` is empty; `answer` is one letter. Letters may repeat \
               only if the rubric says so; by default each letter is used at most once."
        ),
    }
}
