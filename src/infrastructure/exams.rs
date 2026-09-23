//! SQLite table of saved exams, in the same database as the jobs.
//!
//! One row per `Exam::id`: the whole `SavedExam` as JSON in `body`, plus the
//! columns the list needs so listing never parses JSON. `recording_job` names
//! the `jobs` row the exam keeps alive: the purge skips it, and deleting the
//! exam deletes it (and its WAV) when no other exam refers to it.
//!
//! Schema changes stay inline: `CREATE TABLE IF NOT EXISTS` for new tables and,
//! for a new column, an `ALTER TABLE` guarded by
//! `SELECT count(*) FROM pragma_table_info('exams') WHERE name = ?`.

use sqlx::sqlite::SqlitePool;

use crate::domain::Exam;

use super::jobs::{JobState, JobStore, now_secs};

/// Creates the `exams` table; run at startup right after the `jobs` table.
pub(crate) async fn create_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS exams (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            format TEXT NOT NULL,
            parts_total INTEGER NOT NULL,
            parts_with_script INTEGER NOT NULL,
            parts_complete INTEGER NOT NULL,
            recording_job TEXT,
            body TEXT NOT NULL,
            created_at_secs INTEGER NOT NULL,
            updated_at_secs INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS exams_recording_job ON exams (recording_job)")
        .execute(pool)
        .await?;
    Ok(())
}

/// One saved exam as the list sees it: no body, the recording's job state joined in.
#[derive(Debug, Clone, PartialEq)]
pub struct ExamRow {
    pub id: String,
    pub title: String,
    /// `FormatId::key()`.
    pub format: String,
    pub parts_total: i64,
    pub parts_with_script: i64,
    pub parts_complete: i64,
    pub recording_job: Option<String>,
    /// `None` when there is no recording job, or its row is gone.
    pub recording_state: Option<JobState>,
    pub created_at_secs: i64,
    pub updated_at_secs: i64,
}

type RowTuple = (
    String,
    String,
    String,
    i64,
    i64,
    i64,
    Option<String>,
    Option<String>,
    i64,
    i64,
);

fn row_from(t: RowTuple) -> ExamRow {
    let (
        id,
        title,
        format,
        parts_total,
        parts_with_script,
        parts_complete,
        recording_job,
        recording_state,
        created_at_secs,
        updated_at_secs,
    ) = t;
    ExamRow {
        id,
        title,
        format,
        parts_total,
        parts_with_script,
        parts_complete,
        recording_job,
        recording_state: recording_state.as_deref().map(JobState::parse),
        created_at_secs,
        updated_at_secs,
    }
}

const ROW_COLUMNS: &str =
    "e.id, e.title, e.format, e.parts_total, e.parts_with_script, e.parts_complete,
    e.recording_job, j.state, e.created_at_secs, e.updated_at_secs";

pub struct ExamStore {
    pool: SqlitePool,
}

impl ExamStore {
    /// The exam table of the process-wide database.
    pub async fn global() -> ExamStore {
        Self::with_pool(JobStore::global().await.pool().clone())
    }

    pub(crate) fn with_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Inserts or replaces the exam. `body` is the JSON the caller will get
    /// back from `get`; the summary columns are derived from `exam` here.
    pub async fn save(
        &self,
        exam: &Exam,
        recording_job: Option<&str>,
        body: &str,
    ) -> Result<ExamRow, sqlx::Error> {
        let id = exam.id.to_string();
        let title = match exam.title.trim() {
            "" => "Untitled",
            title => title,
        };
        let parts_with_script = exam.parts.iter().filter(|p| p.passage.is_some()).count() as i64;
        let parts_complete = exam.parts.iter().filter(|p| p.is_complete()).count() as i64;
        sqlx::query(
            "INSERT INTO exams (id, title, format, parts_total, parts_with_script, parts_complete,
                                recording_job, body, created_at_secs, updated_at_secs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title, format = excluded.format,
                parts_total = excluded.parts_total,
                parts_with_script = excluded.parts_with_script,
                parts_complete = excluded.parts_complete,
                recording_job = excluded.recording_job,
                body = excluded.body,
                updated_at_secs = excluded.updated_at_secs",
        )
        .bind(&id)
        .bind(title)
        .bind(exam.format.id.key())
        .bind(exam.parts.len() as i64)
        .bind(parts_with_script)
        .bind(parts_complete)
        .bind(recording_job)
        .bind(body)
        .bind(now_secs())
        .execute(&self.pool)
        .await?;
        let (row, _) = self
            .get(&id)
            .await?
            .ok_or_else(|| sqlx::Error::RowNotFound)?;
        Ok(row)
    }

    /// Every saved exam, most recently updated first.
    pub async fn list(&self) -> Result<Vec<ExamRow>, sqlx::Error> {
        let rows: Vec<RowTuple> = sqlx::query_as(&format!(
            "SELECT {ROW_COLUMNS} FROM exams e LEFT JOIN jobs j ON j.id = e.recording_job
             ORDER BY e.updated_at_secs DESC, e.title"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_from).collect())
    }

    /// The row and its JSON body.
    pub async fn get(&self, id: &str) -> Result<Option<(ExamRow, String)>, sqlx::Error> {
        let row: Option<(
            String,
            String,
            String,
            i64,
            i64,
            i64,
            Option<String>,
            Option<String>,
            i64,
            i64,
            String,
        )> = sqlx::query_as(&format!(
            "SELECT {ROW_COLUMNS}, e.body FROM exams e LEFT JOIN jobs j ON j.id = e.recording_job
             WHERE e.id = ?1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(a, b, c, d, e, f, g, h, i, j, body)| {
            (row_from((a, b, c, d, e, f, g, h, i, j)), body)
        }))
    }

    /// Deletes the exam. Its recording job goes with it when no other exam
    /// refers to it and it has finished; the WAV's stored path is returned so
    /// the caller can remove the file. A job still running is left alone and
    /// falls to the retention purge later.
    pub async fn delete(&self, id: &str) -> Result<Option<String>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let found: Option<(Option<String>,)> =
            sqlx::query_as("SELECT recording_job FROM exams WHERE id = ?1")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some((recording_job,)) = found else {
            tx.rollback().await?;
            return Ok(None);
        };
        sqlx::query("DELETE FROM exams WHERE id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let mut removed = None;
        if let Some(job_id) = recording_job {
            let (others,): (i64,) =
                sqlx::query_as("SELECT count(*) FROM exams WHERE recording_job = ?1")
                    .bind(&job_id)
                    .fetch_one(&mut *tx)
                    .await?;
            let job: Option<(String, Option<String>)> =
                sqlx::query_as("SELECT state, output_path FROM jobs WHERE id = ?1")
                    .bind(&job_id)
                    .fetch_optional(&mut *tx)
                    .await?;
            if let Some((state, output_path)) = job
                && others == 0
                && matches!(
                    JobState::parse(&state),
                    JobState::Completed | JobState::Failed
                )
            {
                sqlx::query("DELETE FROM jobs WHERE id = ?1")
                    .bind(&job_id)
                    .execute(&mut *tx)
                    .await?;
                removed = output_path;
            }
        }
        tx.commit().await?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ExamFormat;
    use crate::infrastructure::jobs::JobKind;
    use sqlx::sqlite::SqliteConnectOptions;

    async fn stores() -> (tempfile::TempDir, JobStore, ExamStore) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("jobs.db"))
            .create_if_missing(true);
        let jobs = JobStore::open_options(options).await.unwrap();
        let exams = ExamStore::with_pool(jobs.pool().clone());
        (dir, jobs, exams)
    }

    async fn backdate(jobs: &JobStore, id: &str) {
        sqlx::query("UPDATE jobs SET created_at_secs = 0 WHERE id = ?1")
            .bind(id)
            .execute(jobs.pool())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn save_then_list_then_get_roundtrip() {
        let (_dir, _jobs, exams) = stores().await;
        let mut exam = Exam::new(ExamFormat::hsg_national(), "  ", "news");
        let first = exams.save(&exam, None, "{\"v\":1}").await.unwrap();
        assert_eq!(first.title, "Untitled");
        assert_eq!(first.format, "hsg");
        assert_eq!((first.parts_total, first.parts_with_script), (4, 0));
        assert_eq!(first.recording_state, None);

        exam.title = "Mock 1".into();
        exam.parts[0].passage = Some(
            crate::domain::Passage::parse(
                1,
                "topic",
                "Speaker A: Hello there.",
                &["Speaker A".into()],
            )
            .unwrap(),
        );
        sqlx::query("UPDATE exams SET created_at_secs = 5, updated_at_secs = 5")
            .execute(exams.pool.clone().acquire().await.unwrap().as_mut())
            .await
            .unwrap();
        let second = exams.save(&exam, None, "{\"v\":2}").await.unwrap();
        assert_eq!(second.title, "Mock 1");
        assert_eq!(second.parts_with_script, 1);
        assert_eq!(second.created_at_secs, 5, "upsert keeps the creation time");
        assert!(second.updated_at_secs > 5);

        let listed = exams.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, exam.id.to_string());
        let (row, body) = exams.get(&exam.id.to_string()).await.unwrap().unwrap();
        assert_eq!(row, second);
        assert_eq!(body, "{\"v\":2}");
        assert!(exams.get("missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn pinned_job_survives_purge_and_dies_with_its_exam() {
        let (_dir, jobs, exams) = stores().await;
        let pinned = jobs.create(JobKind::ExamAudio).await.unwrap();
        let loose = jobs.create(JobKind::PartAudio).await.unwrap();
        jobs.complete(&pinned, &format!("{pinned}.wav"))
            .await
            .unwrap();
        jobs.complete(&loose, &format!("{loose}.wav"))
            .await
            .unwrap();
        backdate(&jobs, &pinned).await;
        backdate(&jobs, &loose).await;
        let exam = Exam::new(ExamFormat::ielts_listening(), "Pinned", "");
        let row = exams.save(&exam, Some(&pinned), "{}").await.unwrap();
        assert_eq!(row.recording_state, Some(JobState::Completed));

        let purged = jobs.purge_before(now_secs()).await.unwrap();
        assert_eq!(purged, vec![format!("{loose}.wav")]);
        assert!(jobs.get(&pinned).await.unwrap().is_some());

        let removed = exams.delete(&exam.id.to_string()).await.unwrap();
        assert_eq!(removed, Some(format!("{pinned}.wav")));
        assert!(jobs.get(&pinned).await.unwrap().is_none());
        assert!(exams.list().await.unwrap().is_empty());
        assert_eq!(exams.delete(&exam.id.to_string()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn running_or_shared_job_is_not_deleted_with_the_exam() {
        let (_dir, jobs, exams) = stores().await;
        let running = jobs.create(JobKind::ExamAudio).await.unwrap();
        let first = Exam::new(ExamFormat::hsg_national(), "One", "");
        exams.save(&first, Some(&running), "{}").await.unwrap();
        assert_eq!(exams.delete(&first.id.to_string()).await.unwrap(), None);
        assert!(jobs.get(&running).await.unwrap().is_some());

        let shared = jobs.create(JobKind::ExamAudio).await.unwrap();
        jobs.complete(&shared, &format!("{shared}.wav"))
            .await
            .unwrap();
        let a = Exam::new(ExamFormat::hsg_national(), "A", "");
        let b = Exam::new(ExamFormat::hsg_national(), "B", "");
        exams.save(&a, Some(&shared), "{}").await.unwrap();
        exams.save(&b, Some(&shared), "{}").await.unwrap();
        assert_eq!(exams.delete(&a.id.to_string()).await.unwrap(), None);
        assert!(jobs.get(&shared).await.unwrap().is_some());
        assert_eq!(
            exams.delete(&b.id.to_string()).await.unwrap(),
            Some(format!("{shared}.wav"))
        );
    }
}
