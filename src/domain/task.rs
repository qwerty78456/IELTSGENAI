//! Tasks and items: the questions printed on the paper, with their key.

use serde::{Deserialize, Serialize};

use super::format::TaskSpec;

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
            Answer::Letters(letters) => letters.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", "),
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
    pub stem: String,
    /// Per-item options (multiple choice). Empty when the task shares options.
    #[serde(default)]
    pub options: Vec<Choice>,
    pub answer: Answer,
    /// Verbatim words from the passage that justify the key.
    #[serde(default)]
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub spec: TaskSpec,
    pub instruction: String,
    /// Options listed once above the items (multiple selection, matching, who-mentioned).
    #[serde(default)]
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
        let parsed: Answer = serde_json::from_str(r#"{"kind":"letters","value":["B","D"]}"#).unwrap();
        assert_eq!(parsed, Answer::Letters(vec!['B', 'D']));
    }
}
