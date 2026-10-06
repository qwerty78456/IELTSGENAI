//! What Gemini billed for a draft: tokens, and their price at the time.
//!
//! Money is kept in whole millionths of a US dollar (µUSD) so sums never
//! drift. A price per million tokens in USD is exactly µUSD per token.

use serde::{Deserialize, Serialize};

/// Tokens and cost of one or more Gemini requests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Requests Gemini answered and billed.
    pub requests: u32,
    /// Speech requests answered from the local cache instead, at no cost.
    pub reused: u32,
    /// Prompt tokens, cached ones included.
    pub input_tokens: u64,
    /// The part of `input_tokens` billed at the cached rate.
    pub cached_tokens: u64,
    /// Visible output: text, or audio for speech.
    pub output_tokens: u64,
    /// Thinking tokens, billed at the output rate.
    pub thinking_tokens: u64,
    /// Price when the requests were made, in µUSD.
    pub micro_usd: u64,
    /// Requests whose model had no known price: counted, but not in `micro_usd`.
    pub unpriced: u32,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.requests += other.requests;
        self.reused += other.reused;
        self.input_tokens += other.input_tokens;
        self.cached_tokens += other.cached_tokens;
        self.output_tokens += other.output_tokens;
        self.thinking_tokens += other.thinking_tokens;
        self.micro_usd += other.micro_usd;
        self.unpriced += other.unpriced;
    }

    pub fn is_empty(&self) -> bool {
        self.requests == 0 && self.reused == 0
    }

    pub fn usd(&self) -> f64 {
        self.micro_usd as f64 / 1_000_000.0
    }

    /// "$0.412", with "+?" when some requests could not be priced.
    pub fn cost_text(&self) -> String {
        let unknown = if self.unpriced > 0 { "+?" } else { "" };
        format!("${:.3}{unknown}", self.usd())
    }
}

/// The steps of a draft that call Gemini.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageStep {
    Topic,
    Script,
    Questions,
    Recording,
    /// Voice samples ("Preview") and designed voices.
    Voices,
    /// The few-word summaries in download file names.
    Naming,
}

impl UsageStep {
    pub const ALL: [UsageStep; 6] = [
        UsageStep::Topic,
        UsageStep::Script,
        UsageStep::Questions,
        UsageStep::Recording,
        UsageStep::Voices,
        UsageStep::Naming,
    ];

    /// Stable key for storage.
    pub fn key(self) -> &'static str {
        match self {
            UsageStep::Topic => "topic",
            UsageStep::Script => "script",
            UsageStep::Questions => "questions",
            UsageStep::Recording => "recording",
            UsageStep::Voices => "voices",
            UsageStep::Naming => "naming",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|step| step.key() == key)
    }

    /// Name in the spend breakdown: "scripts $0.012".
    pub fn label(self) -> &'static str {
        match self {
            UsageStep::Topic => "topics",
            UsageStep::Script => "scripts",
            UsageStep::Questions => "questions",
            UsageStep::Recording => "recording",
            UsageStep::Voices => "voices",
            UsageStep::Naming => "file names",
        }
    }

    /// Shown in the spend breakdown even at $0; the optional steps only once
    /// they have cost something.
    pub fn always_listed(self) -> bool {
        match self {
            UsageStep::Topic | UsageStep::Script | UsageStep::Questions | UsageStep::Recording => {
                true
            }
            UsageStep::Voices | UsageStep::Naming => false,
        }
    }
}

/// Everything one exam has cost so far, regenerations and failed runs
/// included, beside the budget it should stay under.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExamUsage {
    pub topics: Usage,
    pub scripts: Usage,
    pub questions: Usage,
    pub recordings: Usage,
    #[serde(default)]
    pub voices: Usage,
    #[serde(default)]
    pub naming: Usage,
    /// 0 means no budget.
    pub budget_micro_usd: u64,
}

impl ExamUsage {
    pub fn step_mut(&mut self, step: UsageStep) -> &mut Usage {
        match step {
            UsageStep::Topic => &mut self.topics,
            UsageStep::Script => &mut self.scripts,
            UsageStep::Questions => &mut self.questions,
            UsageStep::Recording => &mut self.recordings,
            UsageStep::Voices => &mut self.voices,
            UsageStep::Naming => &mut self.naming,
        }
    }

    pub fn step(&self, step: UsageStep) -> Usage {
        match step {
            UsageStep::Topic => self.topics,
            UsageStep::Script => self.scripts,
            UsageStep::Questions => self.questions,
            UsageStep::Recording => self.recordings,
            UsageStep::Voices => self.voices,
            UsageStep::Naming => self.naming,
        }
    }

    pub fn total(&self) -> Usage {
        let mut total = Usage::default();
        for step in UsageStep::ALL {
            total.add(&self.step(step));
        }
        total
    }

    pub fn over_budget(&self) -> bool {
        self.budget_micro_usd > 0 && self.total().micro_usd > self.budget_micro_usd
    }

    /// "$0.700", or `None` without a budget.
    pub fn budget_text(&self) -> Option<String> {
        (self.budget_micro_usd > 0)
            .then(|| format!("${:.3}", self.budget_micro_usd as f64 / 1_000_000.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spent(micro_usd: u64) -> Usage {
        Usage {
            requests: 1,
            input_tokens: 100,
            output_tokens: 50,
            micro_usd,
            ..Usage::default()
        }
    }

    #[test]
    fn exam_usage_adds_up_and_checks_the_budget() {
        let mut exam = ExamUsage {
            budget_micro_usd: 700_000,
            ..ExamUsage::default()
        };
        exam.step_mut(UsageStep::Script).add(&spent(300_000));
        exam.step_mut(UsageStep::Recording).add(&spent(400_000));
        assert_eq!(exam.total().requests, 2);
        assert_eq!(exam.total().micro_usd, 700_000);
        assert!(!exam.over_budget(), "exactly at the budget is within it");
        exam.step_mut(UsageStep::Questions).add(&spent(1));
        assert!(exam.over_budget());
        exam.step_mut(UsageStep::Voices).add(&spent(2));
        assert_eq!(exam.voices.micro_usd, 2);
        assert_eq!(exam.total().micro_usd, 700_003);
        exam.step_mut(UsageStep::Naming).add(&spent(4));
        assert_eq!(exam.naming.micro_usd, 4);
        assert_eq!(exam.total().micro_usd, 700_007);
        assert_eq!(exam.budget_text().as_deref(), Some("$0.700"));
        assert!(!ExamUsage::default().over_budget());
    }

    #[test]
    fn cost_text_flags_unpriced_requests() {
        assert_eq!(spent(412_345).cost_text(), "$0.412");
        let unknown = Usage {
            unpriced: 1,
            ..spent(0)
        };
        assert_eq!(unknown.cost_text(), "$0.000+?");
    }

    #[test]
    fn step_keys_round_trip() {
        for step in UsageStep::ALL {
            assert_eq!(UsageStep::from_key(step.key()), Some(step));
        }
        assert_eq!(UsageStep::from_key("nope"), None);
        assert_eq!(UsageStep::from_key("voices"), Some(UsageStep::Voices));
        assert_eq!(UsageStep::from_key("naming"), Some(UsageStep::Naming));
        assert!(UsageStep::Script.always_listed());
        assert!(!UsageStep::Naming.always_listed());
    }

    #[test]
    fn usage_saved_without_voices_or_naming_still_loads() {
        let empty = r#"{"requests":0,"reused":0,"input_tokens":0,"cached_tokens":0,"output_tokens":0,"thinking_tokens":0,"micro_usd":0,"unpriced":0}"#;
        let json = format!(
            r#"{{"topics":{empty},"scripts":{empty},"questions":{empty},"recordings":{empty},"budget_micro_usd":700000}}"#
        );
        let usage: ExamUsage = serde_json::from_str(&json).unwrap();
        assert!(usage.voices.is_empty());
        assert!(usage.naming.is_empty());
        assert_eq!(usage.budget_micro_usd, 700_000);
    }
}
