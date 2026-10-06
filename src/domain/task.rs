//! Tasks and items: the questions printed on the paper, with their key.

use serde::{Deserialize, Deserializer, Serialize};

use super::format::TaskSpec;

/// Reads a missing *or null* field as its default. Language models write
/// `"shared_options": null` as often as they leave the field out, and an
/// "empty `stem`" as `"stem": null`. What is then missing is the validator's
/// to report (an empty question or option), never a parse failure.
pub(crate) fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tfng {
    #[serde(rename = "T")]
    True,
    #[serde(rename = "F")]
    False,
    #[serde(rename = "NG")]
    NotGiven,
}

impl Tfng {
    pub fn code(self) -> &'static str {
        match self {
            Tfng::True => "T",
            Tfng::False => "F",
            Tfng::NotGiven => "NG",
        }
    }
}

/// A lettered option, either per item (multiple choice) or shared by a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub letter: char,
    #[serde(default, deserialize_with = "null_as_default")]
    pub text: String,
}

/// The key for one item. Serialised as `{"kind": "...", "value": ...}` so the
/// same shape can be requested from the language model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Answer {
    /// One letter (multiple choice, matching, who-mentioned) or several (multiple selection).
    Letters(Vec<char>),
    /// Accepted spellings; the first one is the canonical key.
    Text(Vec<String>),
    Tfng(Tfng),
}

impl Answer {
    pub fn display(&self) -> String {
        match self {
            Answer::Letters(letters) => letters
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            Answer::Text(variants) => variants.join(" / "),
            Answer::Tfng(value) => value.code().to_string(),
        }
    }

    pub fn letters(&self) -> &[char] {
        match self {
            Answer::Letters(letters) => letters,
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub number: u8,
    /// The statement, question or sentence stem; empty for summary gaps.
    #[serde(default, deserialize_with = "null_as_default")]
    pub stem: String,
    /// Per-item options (multiple choice). Empty when the task shares options.
    #[serde(default, deserialize_with = "null_as_default")]
    pub options: Vec<Choice>,
    pub answer: Answer,
    /// Verbatim words from the passage that justify the key.
    #[serde(default, deserialize_with = "null_as_default")]
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub spec: TaskSpec,
    pub instruction: String,
    /// Options listed once above the items (multiple selection, matching, who-mentioned).
    #[serde(default, deserialize_with = "null_as_default")]
    pub shared_options: Vec<Choice>,
    /// Summary paragraph with gaps written as "(26)______".
    #[serde(default)]
    pub summary: Option<String>,
    pub items: Vec<Item>,
}

impl Task {
    pub fn answers(&self) -> impl Iterator<Item = (u8, &Answer)> {
        self.items.iter().map(|item| (item.number, &item.answer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_json_shape_is_model_friendly() {
        let json = serde_json::to_string(&Answer::Tfng(Tfng::NotGiven)).unwrap();
        assert_eq!(json, r#"{"kind":"tfng","value":"NG"}"#);
        let parsed: Answer =
            serde_json::from_str(r#"{"kind":"letters","value":["B","D"]}"#).unwrap();
        assert_eq!(parsed, Answer::Letters(vec!['B', 'D']));
    }

    #[test]
    fn null_lists_and_evidence_read_as_empty() {
        let item: Item = serde_json::from_str(
            r#"{"number": 11, "stem": "Why?", "options": null, "evidence": null,
                "answer": {"kind": "letters", "value": ["A"]}}"#,
        )
        .unwrap();
        assert!(item.options.is_empty() && item.evidence.is_empty());
        let item: Item = serde_json::from_str(
            r#"{"number": 11, "stem": "Why?", "answer": {"kind": "letters", "value": ["A"]}}"#,
        )
        .unwrap();
        assert!(item.options.is_empty());
    }

    #[test]
    fn null_or_missing_stems_and_option_texts_read_as_empty() {
        // The item gemini-3.8-flash returned for a summary gap on 2026-10-06.
        let item: Item = serde_json::from_str(
            r#"{"number": 31, "stem": null, "options": [],
                "answer": {"kind": "text", "value": ["sensors"]}, "evidence": "cheap sensors"}"#,
        )
        .unwrap();
        assert_eq!(item.stem, "");
        let item: Item = serde_json::from_str(
            r#"{"number": 31, "answer": {"kind": "text", "value": ["sensors"]}}"#,
        )
        .unwrap();
        assert_eq!(item.stem, "");
        let choice: Choice = serde_json::from_str(r#"{"letter": "A", "text": null}"#).unwrap();
        assert_eq!(choice.text, "");
        let missing: Choice = serde_json::from_str(r#"{"letter": "B"}"#).unwrap();
        assert_eq!(missing.text, "");
        // Written back as plain strings: saved exams keep their shape.
        assert_eq!(
            serde_json::to_string(&choice).unwrap(),
            r#"{"letter":"A","text":""}"#
        );
    }
}
