//! What Gemini has cost: per exam (with its budget) and for the whole server.
//!
//! Every server function that calls Gemini records what its client was billed
//! right after the call, whatever the outcome (`record`), so failed and
//! superseded runs are counted too. The browser only reads the totals.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{ExamUsage, Usage};

/// Spend on this server over two windows, from either page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageTotals {
    pub last_24h: Usage,
    pub last_30_days: Usage,
}

/// Adds what `client` was billed to the ledger under `step`.
#[cfg(feature = "server")]
pub(crate) async fn record(
    step: crate::domain::UsageStep,
    exam: Option<Uuid>,
    model: &str,
    client: &crate::infrastructure::llm::GeminiClient,
) {
    let exam = exam.map(|id| id.to_string());
    crate::infrastructure::usage::record(step, exam.as_deref(), model, &client.usage()).await;
}

/// Everything one exam has cost so far, per step, with the configured budget.
#[server]
pub async fn exam_usage(exam: Uuid) -> Result<ExamUsage, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::{config::config, usage::UsageStore};

    let mut usage = UsageStore::global()
        .await
        .for_exam(&exam.to_string())
        .await
        .map_err(user_error)?;
    usage.budget_micro_usd = config().exam_budget_micro_usd;
    Ok(usage)
}

/// Spend on this server in the last 24 hours and the last 30 days.
#[server]
pub async fn usage_totals() -> Result<UsageTotals, ServerFnError> {
    use crate::application::user_error;
    use crate::infrastructure::{jobs::now_secs, usage::UsageStore};

    let store = UsageStore::global().await;
    let now = now_secs();
    Ok(UsageTotals {
        last_24h: store.since(now - 86_400).await.map_err(user_error)?,
        last_30_days: store.since(now - 30 * 86_400).await.map_err(user_error)?,
    })
}
