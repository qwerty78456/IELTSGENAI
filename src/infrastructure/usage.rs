//! SQLite ledger of what Gemini billed, in the same database as the jobs.
//!
//! One row per step run (a topic, a script, a question block, a recording
//! job), written whatever the step's outcome: a request that was billed and
//! then failed to parse still cost money. `exam_id` links the row to the
//! saved exam that asked for it (none from the part page). Rows outlive the
//! exam they belong to, so the cost history stays complete.

use sqlx::sqlite::SqlitePool;

use crate::domain::{ExamUsage, Usage, UsageStep};

use super::jobs::{JobStore, now_secs};

/// Creates the `usage` table; run at startup with the other tables.
pub(crate) async fn create_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS usage (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            at_secs INTEGER NOT NULL,
            exam_id TEXT,
            step TEXT NOT NULL,
            model TEXT NOT NULL,
            requests INTEGER NOT NULL,
            reused INTEGER NOT NULL,
            input_tokens INTEGER NOT NULL,
            cached_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            thinking_tokens INTEGER NOT NULL,
            micro_usd INTEGER NOT NULL,
            unpriced INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS usage_exam_id ON usage (exam_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS usage_at_secs ON usage (at_secs)")
        .execute(pool)
        .await?;
    Ok(())
}

const SUMS: &str = "coalesce(sum(requests), 0), coalesce(sum(reused), 0),
    coalesce(sum(input_tokens), 0), coalesce(sum(cached_tokens), 0),
    coalesce(sum(output_tokens), 0), coalesce(sum(thinking_tokens), 0),
    coalesce(sum(micro_usd), 0), coalesce(sum(unpriced), 0)";

type Sums = (i64, i64, i64, i64, i64, i64, i64, i64);

fn usage_from(sums: Sums) -> Usage {
    let (requests, reused, input, cached, output, thinking, micro_usd, unpriced) = sums;
    Usage {
        requests: requests as u32,
        reused: reused as u32,
        input_tokens: input as u64,
        cached_tokens: cached as u64,
        output_tokens: output as u64,
        thinking_tokens: thinking as u64,
        micro_usd: micro_usd as u64,
        unpriced: unpriced as u32,
    }
}

pub struct UsageStore {
    pool: SqlitePool,
}

impl UsageStore {
    /// The ledger of the process-wide database.
    pub async fn global() -> UsageStore {
        Self::with_pool(JobStore::global().await.pool().clone())
    }

    pub(crate) fn with_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Adds one step run. Nothing is written for a run that made no request.
    pub async fn record(
        &self,
        step: UsageStep,
        exam: Option<&str>,
        model: &str,
        usage: &Usage,
    ) -> Result<(), sqlx::Error> {
        if usage.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO usage (at_secs, exam_id, step, model, requests, reused, input_tokens,
                                cached_tokens, output_tokens, thinking_tokens, micro_usd, unpriced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .bind(now_secs())
        .bind(exam)
        .bind(step.key())
        .bind(model)
        .bind(i64::from(usage.requests))
        .bind(i64::from(usage.reused))
        .bind(usage.input_tokens as i64)
        .bind(usage.cached_tokens as i64)
        .bind(usage.output_tokens as i64)
        .bind(usage.thinking_tokens as i64)
        .bind(usage.micro_usd as i64)
        .bind(i64::from(usage.unpriced))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Everything recorded for one exam, per step. The budget is left at 0.
    pub async fn for_exam(&self, exam: &str) -> Result<ExamUsage, sqlx::Error> {
        let mut total = ExamUsage::default();
        for step in UsageStep::ALL {
            let sums: Sums = sqlx::query_as(&format!(
                "SELECT {SUMS} FROM usage WHERE exam_id = ?1 AND step = ?2"
            ))
            .bind(exam)
            .bind(step.key())
            .fetch_one(&self.pool)
            .await?;
            *total.step_mut(step) = usage_from(sums);
        }
        Ok(total)
    }

    /// Everything recorded since `at_secs` (Unix time), from either page.
    pub async fn since(&self, at_secs: i64) -> Result<Usage, sqlx::Error> {
        let sums: Sums = sqlx::query_as(&format!("SELECT {SUMS} FROM usage WHERE at_secs >= ?1"))
            .bind(at_secs)
            .fetch_one(&self.pool)
            .await?;
        Ok(usage_from(sums))
    }
}

/// Records a step run, logging instead of failing: the teacher's result
/// matters more than its bookkeeping.
pub async fn record(step: UsageStep, exam: Option<&str>, model: &str, usage: &Usage) {
    if let Err(e) = UsageStore::global()
        .await
        .record(step, exam, model, usage)
        .await
    {
        tracing::error!("usage ledger not updated ({}): {e}", step.key());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;

    async fn ledger() -> (tempfile::TempDir, UsageStore) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("jobs.db"))
            .create_if_missing(true);
        let jobs = JobStore::open_options(options).await.unwrap();
        (dir, UsageStore::with_pool(jobs.pool().clone()))
    }

    fn spent(requests: u32, output_tokens: u64, micro_usd: u64) -> Usage {
        Usage {
            requests,
            input_tokens: 1_000,
            output_tokens,
            thinking_tokens: 200,
            micro_usd,
            ..Usage::default()
        }
    }

    #[tokio::test]
    async fn steps_add_up_per_exam_and_over_time() {
        let (_dir, store) = ledger().await;
        let exam = Some("exam-1");
        let text = "gemini-3.8-flash";
        let tts = "gemini-3.8-flash-tts";
        let rows = [
            (UsageStep::Script, exam, text, spent(1, 800, 4_000)),
            (UsageStep::Script, exam, text, spent(1, 700, 3_500)),
            (UsageStep::Questions, exam, text, spent(2, 900, 5_000)),
            (UsageStep::Recording, exam, tts, spent(9, 24_000, 216_000)),
            (UsageStep::Script, None, text, spent(1, 600, 3_000)),
            // A run with no request (validation failed first) leaves no row.
            (UsageStep::Topic, exam, text, Usage::default()),
        ];
        for (step, exam, model, usage) in rows {
            store.record(step, exam, model, &usage).await.unwrap();
        }

        let usage = store.for_exam("exam-1").await.unwrap();
        assert_eq!(usage.scripts.requests, 2);
        assert_eq!(usage.scripts.output_tokens, 1_500);
        assert_eq!(usage.scripts.thinking_tokens, 400);
        assert_eq!(usage.questions.micro_usd, 5_000);
        assert_eq!(usage.recordings.output_tokens, 24_000);
        assert!(usage.topics.is_empty());
        assert_eq!(usage.total().micro_usd, 4_000 + 3_500 + 5_000 + 216_000);
        assert_eq!(store.for_exam("other").await.unwrap(), ExamUsage::default());

        let everything = store.since(0).await.unwrap();
        assert_eq!(everything.requests, 14);
        assert_eq!(everything.micro_usd, 231_500);
        assert!(store.since(now_secs() + 60).await.unwrap().is_empty());
    }
}
