//! The text client of one run: Gemini or Claude, as the page chose or, with
//! no choice, as `TEXT_PROVIDER` says. A plain enum over the two clients;
//! application code reaches text only through it. It never switches
//! provider: a provider that cannot be used, or a request that fails, is an
//! error for the teacher, never a request to the other one.

use serde::de::DeserializeOwned;

use crate::domain::{TextProvider, Usage};

use super::super::config::config;
use super::{ClaudeClient, GeminiClient, LlmError};

#[derive(Clone)]
pub enum TextClient {
    Gemini(GeminiClient),
    Claude(ClaudeClient),
}

impl TextClient {
    /// The client of `choice`, else of the server's `TEXT_PROVIDER`.
    pub fn from_config(choice: Option<TextProvider>) -> Result<Self, LlmError> {
        match TextProvider::resolve(choice, config().text_provider) {
            TextProvider::Gemini => GeminiClient::from_config().map(Self::Gemini),
            TextProvider::Anthropic => ClaudeClient::from_config().map(Self::Claude),
        }
    }

    /// The model topics, scripts and questions are written with.
    pub fn text_model(&self) -> &str {
        match self {
            Self::Gemini(client) => client.text_model(),
            Self::Claude(client) => client.text_model(),
        }
    }

    /// The model download-name summaries are written with
    /// (`GEMINI_SUMMARY_MODEL` or `ANTHROPIC_SUMMARY_MODEL`).
    pub fn summary_model(&self) -> &str {
        match self {
            Self::Gemini(_) => &config().summary_model,
            Self::Claude(client) => client.summary_model(),
        }
    }

    /// Everything this client (and its clones) has been billed for so far.
    pub fn usage(&self) -> Usage {
        match self {
            Self::Gemini(client) => client.usage(),
            Self::Claude(client) => client.usage(),
        }
    }

    /// Plain text from the text model.
    pub async fn generate_text(&self, prompt: &str) -> Result<String, LlmError> {
        match self {
            Self::Gemini(client) => client.generate_text(prompt).await,
            Self::Claude(client) => client.generate_text(prompt).await,
        }
    }

    /// JSON from the text model, parsed into `T`.
    pub async fn generate_json<T: DeserializeOwned>(&self, prompt: &str) -> Result<T, LlmError> {
        match self {
            Self::Gemini(client) => client.generate_json(prompt).await,
            Self::Claude(client) => client.generate_json(prompt).await,
        }
    }

    /// A few words from the summary model (`summary_model`).
    pub async fn generate_brief(&self, prompt: &str) -> Result<String, LlmError> {
        match self {
            Self::Gemini(client) => client.generate_brief(self.summary_model(), prompt).await,
            Self::Claude(client) => client.generate_brief(prompt).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use futures_util::future::join_all;

    use super::*;
    use crate::domain::{FormatId, Passage, Severity, ValidationIssue};
    use crate::infrastructure::config::{
        DEFAULT_ANTHROPIC_EFFORT, DEFAULT_ANTHROPIC_SUMMARY_MODEL, DEFAULT_ANTHROPIC_TEXT_MODEL,
        DEFAULT_TEXT_MODEL, DEFAULT_THINKING_LEVEL, DEFAULT_TTS_MODEL, windows_environment,
    };
    use crate::infrastructure::prompts;

    #[test]
    fn claude_spend_is_booked_under_the_claude_models() {
        // The text server functions record their spend under `text_model`
        // or `summary_model`; with Claude those are Anthropic's models,
        // never the Gemini settings.
        let client = TextClient::Claude(
            ClaudeClient::new(
                "test-secret-not-a-real-key".into(),
                DEFAULT_ANTHROPIC_TEXT_MODEL.into(),
                DEFAULT_ANTHROPIC_SUMMARY_MODEL.into(),
                DEFAULT_ANTHROPIC_EFFORT.into(),
            )
            .unwrap(),
        );
        assert_eq!(client.text_model(), "claude-sonnet-5-5");
        assert_eq!(client.summary_model(), "claude-haiku-5-5");
        // Nothing sent, nothing to book: the ledger adds no row.
        assert!(client.usage().is_empty());
    }

    /// One fixed scenario per IELTS part, so both providers write the same exam.
    const TOPICS: [&str; 4] = [
        "A man phones a sports centre to ask about joining an adult swimming club.",
        "A volunteer coordinator briefs new volunteers on a weekend river clean-up.",
        "Two students and their tutor discuss a research project on how teenagers use public libraries.",
        "A university lecture on how cities can keep their streets cool in summer heatwaves.",
    ];
    /// A provider that has cost more than this stops the probe.
    const SPEND_CAP_MICRO_USD: u64 = 400_000;

    /// What one provider did with the exam.
    #[derive(Default)]
    struct Tally {
        scripts: usize,
        tasks: usize,
        /// Scripts that did not parse and question replies that were not the JSON asked for.
        parse_failures: usize,
        /// Requests that failed for another reason (busy, refused, timed out...).
        failures: usize,
        errors: usize,
        warnings: usize,
        script_latency: Vec<Duration>,
        task_latency: Vec<Duration>,
        wall: Duration,
    }

    impl Tally {
        fn count(&mut self, issues: &[ValidationIssue]) {
            for issue in issues {
                match issue.severity {
                    Severity::Error => self.errors += 1,
                    Severity::Warning => self.warnings += 1,
                }
            }
        }
    }

    fn seconds(latency: &[Duration]) -> String {
        let max = latency.iter().max().copied().unwrap_or_default();
        let total: Duration = latency.iter().sum();
        let mean = total / u32::try_from(latency.len().max(1)).unwrap_or(1);
        format!("{:.1}/{:.1}", mean.as_secs_f32(), max.as_secs_f32())
    }

    fn within_cap(name: &str, client: &TextClient) {
        let spent = client.usage();
        assert!(
            spent.micro_usd <= SPEND_CAP_MICRO_USD,
            "{name} has cost {} already, over the probe's cap of ${:.2}: stopped",
            spent.cost_text(),
            SPEND_CAP_MICRO_USD as f64 / 1_000_000.0
        );
    }

    /// The whole text of an IELTS exam: the four scripts at once, as
    /// "Generate the whole exam" sends them, then every task of each part.
    async fn write_exam(name: &str, client: &TextClient) -> Tally {
        let format = FormatId::IeltsListening.format();
        let format = &format;
        let mut tally = Tally::default();
        let started = Instant::now();
        let scripts = join_all(
            format
                .parts
                .iter()
                .zip(TOPICS)
                .map(|(part, topic)| async move {
                    let sent = Instant::now();
                    let text = client
                        .generate_text(&prompts::passage_prompt(
                            format,
                            part,
                            topic,
                            &part.default_speakers,
                            true,
                        ))
                        .await;
                    (part, topic, text, sent.elapsed())
                }),
        )
        .await;
        let mut passages = Vec::new();
        for (part, topic, text, latency) in scripts {
            tally.script_latency.push(latency);
            let labels: Vec<String> = part
                .default_speakers
                .iter()
                .map(|s| s.label.clone())
                .collect();
            let parsed = text.map_err(|e| (false, e.to_string())).and_then(|text| {
                Passage::parse(part.number, topic, &text, &labels)
                    .map_err(|e| (true, format!("{e}; it starts: {text:.200}")))
            });
            match parsed {
                Ok(passage) => {
                    let passage = passage.for_speakers(&part.default_speakers);
                    tally.scripts += 1;
                    tally.count(&crate::domain::validate_passage(
                        &passage,
                        part,
                        &part.default_speakers,
                    ));
                    passages.push((part, passage));
                }
                Err((parse, e)) => {
                    if parse {
                        tally.parse_failures += 1;
                    } else {
                        tally.failures += 1;
                    }
                    println!("{name} part {} script failed: {e}", part.number);
                }
            }
        }
        for (part, passage) in &passages {
            for spec in &part.tasks {
                within_cap(name, client);
                let sent = Instant::now();
                let draft = client
                    .generate_json::<prompts::TaskDraftDto>(&prompts::task_prompt(
                        part,
                        spec,
                        passage,
                        &part.default_speakers,
                    ))
                    .await;
                tally.task_latency.push(sent.elapsed());
                match draft {
                    Ok(draft) => {
                        let task = draft.into_task(spec.clone());
                        tally.tasks += 1;
                        tally.count(&crate::domain::validate_task(&task, Some(passage)));
                    }
                    Err(e) => {
                        if matches!(e, LlmError::Malformed(_)) {
                            tally.parse_failures += 1;
                        } else {
                            tally.failures += 1;
                        }
                        println!(
                            "{name} part {} questions {}-{} failed: {e}",
                            part.number, spec.first, spec.last
                        );
                    }
                }
            }
        }
        tally.wall = started.elapsed();
        tally
    }

    /// Writes the whole text of one IELTS exam (four scripts at once, then
    /// every question block) with Gemini and then with Claude, from the
    /// same fixed topics, and prints for each: scripts and tasks written,
    /// replies that did not parse, other failures, the validator's errors
    /// and warnings, latency and cost. Keys from the environment (process,
    /// then Windows), models and levels from the usual settings or their
    /// defaults. Stops once a provider has cost more than $0.40. Costs about
    /// $0.17 in all (measured 2026-10-14): ask before running it. Its name
    /// keeps it out of `live_probe`:
    /// `cargo test --features server --no-default-features text_provider_compare_probe -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn text_provider_compare_probe() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let key = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|k| !k.trim().is_empty())
                .or_else(|| windows_environment().remove(name))
                .unwrap_or_else(|| panic!("{name} in the environment"))
        };
        let setting = |name: &str, default: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| default.to_string())
        };
        let gemini = TextClient::Gemini(
            GeminiClient::new(
                key("GEMINI_API_KEY"),
                setting("GEMINI_TEXT_MODEL", DEFAULT_TEXT_MODEL),
                setting("GEMINI_TTS_MODEL", DEFAULT_TTS_MODEL),
                setting("GEMINI_THINKING_LEVEL", DEFAULT_THINKING_LEVEL),
            )
            .unwrap(),
        );
        let claude = TextClient::Claude(
            ClaudeClient::new(
                key("ANTHROPIC_API_KEY"),
                setting("ANTHROPIC_TEXT_MODEL", DEFAULT_ANTHROPIC_TEXT_MODEL),
                setting("ANTHROPIC_SUMMARY_MODEL", DEFAULT_ANTHROPIC_SUMMARY_MODEL),
                setting("ANTHROPIC_EFFORT", DEFAULT_ANTHROPIC_EFFORT),
            )
            .unwrap(),
        );
        let mut rows = Vec::new();
        for (name, client) in [("Gemini", &gemini), ("Claude", &claude)] {
            let tally = write_exam(name, client).await;
            within_cap(name, client);
            rows.push((name, client, tally));
        }

        println!(
            "\n{:<7} {:<20} {:>7} {:>5} {:>11} {:>8} {:>6} {:>8} {:>15} {:>15} {:>6} {:>9}",
            "",
            "model",
            "scripts",
            "tasks",
            "unparsed",
            "failed",
            "errors",
            "warnings",
            "script s avg/max",
            "task s avg/max",
            "wall s",
            "cost"
        );
        for (name, client, tally) in &rows {
            let spent = client.usage();
            println!(
                "{:<7} {:<20} {:>7} {:>5} {:>11} {:>8} {:>6} {:>8} {:>15} {:>15} {:>6.1} {:>9}",
                name,
                client.text_model(),
                tally.scripts,
                tally.tasks,
                tally.parse_failures,
                tally.failures,
                tally.errors,
                tally.warnings,
                seconds(&tally.script_latency),
                seconds(&tally.task_latency),
                tally.wall.as_secs_f32(),
                spent.cost_text()
            );
        }
        for (name, client, _) in &rows {
            let spent = client.usage();
            println!(
                "{name}: {} requests; tokens {} in ({} cached), {} out, {} thinking",
                spent.requests,
                spent.input_tokens,
                spent.cached_tokens,
                spent.output_tokens,
                spent.thinking_tokens
            );
        }
    }
}
