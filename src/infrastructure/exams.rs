//! Revision-checked saved exams. All writers share SQLite's immediate write lock.
use super::jobs::{JobState, JobStore, now_secs};
use crate::domain::Exam;
use sqlx::{
    Row,
    sqlite::{SqlitePool, SqliteRow},
};

pub(crate) async fn create_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS exams (
        id TEXT PRIMARY KEY, title TEXT NOT NULL, format TEXT NOT NULL,
        parts_total INTEGER NOT NULL, parts_with_script INTEGER NOT NULL,
        parts_complete INTEGER NOT NULL, recording_job TEXT, body TEXT NOT NULL,
        created_at_secs INTEGER NOT NULL, updated_at_secs INTEGER NOT NULL
    )",
    )
    .execute(&mut *tx)
    .await?;
    for (name, definition) in [
        ("revision", "INTEGER NOT NULL DEFAULT 1"),
        ("last_save_id", "TEXT"),
        ("last_save_hash", "TEXT"),
    ] {
        let (count,): (i64,) =
            sqlx::query_as("SELECT count(*) FROM pragma_table_info('exams') WHERE name = ?")
                .bind(name)
                .fetch_one(&mut *tx)
                .await?;
        if count == 0 {
            sqlx::query(&format!("ALTER TABLE exams ADD COLUMN {name} {definition}"))
                .execute(&mut *tx)
                .await?;
        }
    }
    sqlx::query("CREATE INDEX IF NOT EXISTS exams_recording_job ON exams (recording_job)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS deleted_exams (id TEXT PRIMARY KEY, deleted_at_secs INTEGER NOT NULL)").execute(&mut *tx).await?;
    tx.commit().await
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExamRow {
    pub id: String,
    pub title: String,
    pub format: String,
    pub parts_total: i64,
    pub parts_with_script: i64,
    pub parts_complete: i64,
    pub recording_job: Option<String>,
    pub recording_state: Option<JobState>,
    pub created_at_secs: i64,
    pub updated_at_secs: i64,
    pub revision: i64,
}
fn row_from(row: SqliteRow) -> ExamRow {
    ExamRow {
        id: row.get("id"),
        title: row.get("title"),
        format: row.get("format"),
        parts_total: row.get("parts_total"),
        parts_with_script: row.get("parts_with_script"),
        parts_complete: row.get("parts_complete"),
        recording_job: row.get("recording_job"),
        recording_state: row
            .get::<Option<String>, _>("recording_state")
            .as_deref()
            .map(JobState::parse),
        created_at_secs: row.get("created_at_secs"),
        updated_at_secs: row.get("updated_at_secs"),
        revision: row.get("revision"),
    }
}
const ROW_COLUMNS: &str = "e.*, j.state AS recording_state";

pub struct SaveInput<'a> {
    pub exam: &'a Exam,
    pub recording_job: Option<&'a str>,
    pub body: &'a str,
    pub expected_revision: i64,
    pub mutation_id: &'a str,
    pub hash: &'a str,
}
#[derive(Debug)]
pub enum StoreSave {
    Saved {
        row: ExamRow,
        recording_missing: bool,
    },
    Conflict(i64),
    Deleted,
    Invalid,
}
#[derive(Debug, PartialEq)]
pub enum StoreDelete {
    Deleted(Option<String>),
    AlreadyDeleted,
    Conflict(i64),
}
pub struct ExamStore {
    pool: SqlitePool,
}
impl ExamStore {
    pub async fn global() -> Self {
        Self::with_pool(JobStore::global().await.pool().clone())
    }
    pub(crate) fn with_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn save(&self, input: SaveInput<'_>) -> Result<StoreSave, sqlx::Error> {
        let SaveInput {
            exam,
            recording_job,
            body,
            expected_revision,
            mutation_id,
            hash,
        } = input;
        let id = exam.id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let deleted: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM deleted_exams WHERE id = ?)")
                .bind(&id)
                .fetch_one(&mut *tx)
                .await?;
        if deleted {
            return Ok(StoreSave::Deleted);
        }
        let existing: Option<(i64, Option<String>, Option<String>)> =
            sqlx::query_as("SELECT revision, last_save_id, last_save_hash FROM exams WHERE id = ?")
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some((_, Some(last_id), last_hash)) = &existing
            && last_id == mutation_id
        {
            if last_hash.as_deref() != Some(hash) {
                return Ok(StoreSave::Invalid);
            }
            let row = sqlx::query(&format!("SELECT {ROW_COLUMNS} FROM exams e LEFT JOIN jobs j ON j.id=e.recording_job WHERE e.id=?"))
                .bind(&id).fetch_one(&mut *tx).await?;
            let row = row_from(row);
            let recording_missing = recording_job.is_some() && row.recording_job.is_none();
            tx.commit().await?;
            tracing::info!(exam = %id, revision = row.revision, "exam save retry acknowledged");
            return Ok(StoreSave::Saved {
                row,
                recording_missing,
            });
        }
        let actual = existing.as_ref().map_or(0, |r| r.0);
        if actual != expected_revision || expected_revision < 0 || expected_revision == i64::MAX {
            tracing::info!(exam = %id, revision = actual, "exam save conflict");
            return Ok(StoreSave::Conflict(actual));
        }
        let valid_job = if let Some(job) = recording_job {
            sqlx::query_scalar::<_, String>("SELECT id FROM jobs WHERE id=?")
                .bind(job)
                .fetch_optional(&mut *tx)
                .await?
        } else {
            None
        };
        let recording_missing = recording_job.is_some() && valid_job.is_none();
        // The database reference is authoritative on reads; normalize new bodies too.
        let mut value: serde_json::Value =
            serde_json::from_str(body).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        if let Some(object) = value.as_object_mut() {
            object.insert("recording_job".into(), serde_json::json!(valid_job));
            object.insert("recording".into(), serde_json::Value::Null);
        }
        let body = value.to_string();
        let title = if exam.title.trim().is_empty() {
            "Untitled"
        } else {
            exam.title.trim()
        };
        let statement = if actual == 0 {
            "INSERT INTO exams (id,title,format,parts_total,parts_with_script,parts_complete,recording_job,body,created_at_secs,updated_at_secs,revision,last_save_id,last_save_hash)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9,?12+1,?10,?11)"
        } else {
            "UPDATE exams SET title=?2,format=?3,parts_total=?4,parts_with_script=?5,parts_complete=?6,
            recording_job=?7,body=?8,updated_at_secs=?9,last_save_id=?10,last_save_hash=?11,revision=?12+1
            WHERE id=?1 AND revision=?12"
        };
        sqlx::query(statement)
            .bind(&id)
            .bind(title)
            .bind(exam.format.id.key())
            .bind(exam.parts.len() as i64)
            .bind(exam.parts.iter().filter(|p| p.passage.is_some()).count() as i64)
            .bind(exam.parts.iter().filter(|p| p.is_complete()).count() as i64)
            .bind(&valid_job)
            .bind(body)
            .bind(now_secs())
            .bind(mutation_id)
            .bind(hash)
            .bind(expected_revision)
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query(&format!("SELECT {ROW_COLUMNS} FROM exams e LEFT JOIN jobs j ON j.id=e.recording_job WHERE e.id=?"))
            .bind(&id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(StoreSave::Saved {
            row: row_from(row),
            recording_missing,
        })
    }
    pub async fn list(&self) -> Result<Vec<ExamRow>, sqlx::Error> {
        let rows = sqlx::query(&format!("SELECT {ROW_COLUMNS} FROM exams e LEFT JOIN jobs j ON j.id=e.recording_job ORDER BY e.updated_at_secs DESC,e.title"))
            .fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(row_from).collect())
    }
    pub async fn get(&self, id: &str) -> Result<Option<(ExamRow, String)>, sqlx::Error> {
        let row = sqlx::query(&format!("SELECT {ROW_COLUMNS} FROM exams e LEFT JOIN jobs j ON j.id=e.recording_job WHERE e.id=?"))
            .bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(|r| {
            let body = r.get("body");
            (row_from(r), body)
        }))
    }
    pub async fn delete(
        &self,
        id: &str,
        expected_revision: i64,
    ) -> Result<StoreDelete, sqlx::Error> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let found: Option<(i64, Option<String>)> =
            sqlx::query_as("SELECT revision,recording_job FROM exams WHERE id=?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some((revision, job)) = found else {
            // Also fences off a delayed initial create for a never-saved local draft.
            sqlx::query("INSERT OR IGNORE INTO deleted_exams VALUES (?,?)")
                .bind(id)
                .bind(now_secs())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(StoreDelete::AlreadyDeleted);
        };
        if revision != expected_revision {
            tracing::info!(exam = %id, revision, "exam delete conflict");
            return Ok(StoreDelete::Conflict(revision));
        }
        sqlx::query("INSERT OR IGNORE INTO deleted_exams VALUES (?,?)")
            .bind(id)
            .bind(now_secs())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM exams WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let removed: Option<(Option<String>,)> = if let Some(job) = job {
            sqlx::query_as("DELETE FROM jobs WHERE id=? AND state IN ('completed','failed') AND NOT EXISTS (SELECT 1 FROM exams WHERE recording_job=jobs.id) RETURNING output_path")
                .bind(job).fetch_optional(&mut *tx).await?
        } else {
            None
        };
        tx.commit().await?;
        Ok(StoreDelete::Deleted(removed.and_then(|r| r.0)))
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

    async fn save(exams: &ExamStore, exam: &Exam, job: Option<&str>, body: &str) -> ExamRow {
        let revision = exams
            .get(&exam.id.to_string())
            .await
            .unwrap()
            .map_or(0, |r| r.0.revision);
        match exams
            .save(SaveInput {
                exam,
                recording_job: job,
                body,
                expected_revision: revision,
                mutation_id: &uuid::Uuid::new_v4().to_string(),
                hash: body,
            })
            .await
            .unwrap()
        {
            StoreSave::Saved { row, .. } => row,
            other => panic!("{other:?}"),
        }
    }
    async fn delete(exams: &ExamStore, id: &str) -> Option<String> {
        let revision = exams.get(id).await.unwrap().map_or(0, |r| r.0.revision);
        match exams.delete(id, revision).await.unwrap() {
            StoreDelete::Deleted(path) => path,
            StoreDelete::AlreadyDeleted => None,
            other => panic!("{other:?}"),
        }
    }
    async fn backdate(jobs: &JobStore, id: &str) {
        sqlx::query("UPDATE jobs SET created_at_secs = 0, finished_at_secs = 0 WHERE id = ?1")
            .bind(id)
            .execute(jobs.pool())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn save_then_list_then_get_roundtrip() {
        let (_dir, _jobs, exams) = stores().await;
        let mut exam = Exam::new(ExamFormat::hsg_national(), "  ", "news");
        let first = save(&exams, &exam, None, "{\"v\":1}").await;
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
        let second = save(&exams, &exam, None, "{\"v\":2}").await;
        assert_eq!(second.title, "Mock 1");
        assert_eq!(second.parts_with_script, 1);
        assert_eq!(second.created_at_secs, 5, "upsert keeps the creation time");
        assert!(second.updated_at_secs > 5);

        let listed = exams.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, exam.id.to_string());
        let (row, body) = exams.get(&exam.id.to_string()).await.unwrap().unwrap();
        assert_eq!(row, second);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["v"],
            2
        );
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
        let row = save(&exams, &exam, Some(&pinned), "{}").await;
        assert_eq!(row.recording_state, Some(JobState::Completed));

        let purged = jobs.purge_before(now_secs()).await.unwrap();
        assert_eq!(purged, vec![format!("{loose}.wav")]);
        assert!(jobs.get(&pinned).await.unwrap().is_some());

        let removed = delete(&exams, &exam.id.to_string()).await;
        assert_eq!(removed, Some(format!("{pinned}.wav")));
        assert!(jobs.get(&pinned).await.unwrap().is_none());
        assert!(exams.list().await.unwrap().is_empty());
        assert_eq!(delete(&exams, &exam.id.to_string()).await, None);
    }

    #[tokio::test]
    async fn running_or_shared_job_is_not_deleted_with_the_exam() {
        let (_dir, jobs, exams) = stores().await;
        let running = jobs.create(JobKind::ExamAudio).await.unwrap();
        let first = Exam::new(ExamFormat::hsg_national(), "One", "");
        save(&exams, &first, Some(&running), "{}").await;
        assert_eq!(delete(&exams, &first.id.to_string()).await, None);
        assert!(jobs.get(&running).await.unwrap().is_some());

        let shared = jobs.create(JobKind::ExamAudio).await.unwrap();
        jobs.complete(&shared, &format!("{shared}.wav"))
            .await
            .unwrap();
        let a = Exam::new(ExamFormat::hsg_national(), "A", "");
        let b = Exam::new(ExamFormat::hsg_national(), "B", "");
        save(&exams, &a, Some(&shared), "{}").await;
        save(&exams, &b, Some(&shared), "{}").await;
        assert_eq!(delete(&exams, &a.id.to_string()).await, None);
        assert!(jobs.get(&shared).await.unwrap().is_some());
        assert_eq!(
            delete(&exams, &b.id.to_string()).await,
            Some(format!("{shared}.wav"))
        );
    }
    fn input<'a>(
        exam: &'a Exam,
        revision: i64,
        mutation: &'a str,
        hash: &'a str,
        job: Option<&'a str>,
    ) -> SaveInput<'a> {
        SaveInput {
            exam,
            expected_revision: revision,
            mutation_id: mutation,
            hash,
            recording_job: job,
            body: "{}",
        }
    }
    #[tokio::test]
    async fn concurrent_writers_and_confirmed_overwrite_use_compare_and_swap() {
        let (_dir, _jobs, exams) = stores().await;
        let exam = Exam::new(ExamFormat::hsg_national(), "A", "");
        save(&exams, &exam, None, "{}").await;
        let barrier = tokio::sync::Barrier::new(2);
        let writer = |id| {
            let (barrier, exams, exam) = (&barrier, &exams, &exam);
            async move {
                barrier.wait().await;
                exams.save(input(exam, 1, id, id, None)).await.unwrap()
            }
        };
        let (a, b) = tokio::join!(writer("tab-a"), writer("tab-b"));
        assert!(matches!(
            (&a, &b),
            (StoreSave::Saved { .. }, StoreSave::Conflict(2))
                | (StoreSave::Conflict(2), StoreSave::Saved { .. })
        ));
        assert!(matches!(
            exams
                .save(input(&exam, 2, "third", "third", None))
                .await
                .unwrap(),
            StoreSave::Saved { .. }
        ));
        assert!(matches!(
            exams
                .save(input(&exam, 2, "confirmed-overwrite", "replace", None))
                .await
                .unwrap(),
            StoreSave::Conflict(3)
        ));
    }
    #[tokio::test]
    async fn lost_ack_is_idempotent_but_old_retry_never_replays_over_newer_work() {
        let (_dir, _jobs, exams) = stores().await;
        let exam = Exam::new(ExamFormat::hsg_national(), "A", "");
        for _ in 0..2 {
            let StoreSave::Saved { row, .. } = exams
                .save(input(&exam, 0, "one", "same", None))
                .await
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(row.revision, 1);
        }
        assert!(matches!(
            exams
                .save(input(&exam, 0, "one", "different", None))
                .await
                .unwrap(),
            StoreSave::Invalid
        ));
        exams
            .save(input(&exam, 1, "two", "next", None))
            .await
            .unwrap();
        assert!(matches!(
            exams
                .save(input(&exam, 0, "one", "same", None))
                .await
                .unwrap(),
            StoreSave::Conflict(2)
        ));
        assert_eq!(
            exams.delete(&exam.id.to_string(), 1).await.unwrap(),
            StoreDelete::Conflict(2)
        );
        exams.delete(&exam.id.to_string(), 2).await.unwrap();
        assert!(matches!(
            exams
                .save(input(&exam, 0, "one", "same", None))
                .await
                .unwrap(),
            StoreSave::Deleted
        ));
        assert!(matches!(
            exams
                .save(input(&exam, 0, "new-id", "same", None))
                .await
                .unwrap(),
            StoreSave::Deleted
        ));
    }
    #[tokio::test]
    async fn concurrent_delete_and_save_never_resurrect_an_exam() {
        let (_dir, _jobs, exams) = stores().await;
        let exam = Exam::new(ExamFormat::hsg_national(), "A", "");
        save(&exams, &exam, None, "{}").await;
        let barrier = tokio::sync::Barrier::new(2);
        let writer = async {
            barrier.wait().await;
            exams
                .save(input(&exam, 1, "two", "two", None))
                .await
                .unwrap()
        };
        let deleter = async {
            barrier.wait().await;
            exams.delete(&exam.id.to_string(), 1).await.unwrap()
        };
        let (saved, deleted) = tokio::join!(writer, deleter);
        assert!(matches!(
            (saved, deleted),
            (StoreSave::Saved { .. }, StoreDelete::Conflict(2))
                | (StoreSave::Deleted, StoreDelete::Deleted(_))
        ));
    }
    #[tokio::test]
    async fn cleanup_and_pin_are_serialized_in_either_order() {
        let (_dir, jobs, exams) = stores().await;
        let job = jobs.create(JobKind::ExamAudio).await.unwrap();
        jobs.complete(&job, "a.wav").await.unwrap();
        backdate(&jobs, &job).await;
        let exam = Exam::new(ExamFormat::hsg_national(), "A", "");
        let barrier = tokio::sync::Barrier::new(2);
        let pin = async {
            barrier.wait().await;
            exams
                .save(input(&exam, 0, "one", "one", Some(&job)))
                .await
                .unwrap()
        };
        let cleanup = async {
            barrier.wait().await;
            jobs.purge_before(1).await.unwrap()
        };
        let (saved, removed) = tokio::join!(pin, cleanup);
        let StoreSave::Saved {
            row,
            recording_missing,
        } = saved
        else {
            panic!()
        };
        if recording_missing {
            assert!(row.recording_job.is_none());
            assert_eq!(removed, ["a.wav"]);
        } else {
            assert_eq!(row.recording_job.as_deref(), Some(job.as_str()));
            assert!(removed.is_empty());
            assert!(jobs.get(&job).await.unwrap().is_some());
        }
        // Explicit cleanup-first order: text survives with a normalized reference.
        let late = Exam::new(ExamFormat::hsg_national(), "Late", "");
        let StoreSave::Saved {
            row,
            recording_missing,
        } = exams
            .save(input(&late, 0, "late", "late", Some("missing")))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert!(recording_missing);
        assert!(row.recording_job.is_none());
        let (_, body) = exams.get(&late.id.to_string()).await.unwrap().unwrap();
        assert!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["recording_job"].is_null()
        );
    }
    #[tokio::test]
    async fn legacy_migration_preserves_json_and_runs_once() {
        let directory = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(directory.path().join("legacy.db"))
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options.clone()).await.unwrap();
        sqlx::query("CREATE TABLE jobs (id TEXT PRIMARY KEY,kind TEXT NOT NULL,state TEXT NOT NULL,progress REAL NOT NULL,error TEXT,output_path TEXT,created_at_secs INTEGER NOT NULL)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO jobs VALUES ('done','exam_audio','completed',1,NULL,'done.wav',0),('running','exam_audio','processing',0,NULL,NULL,0)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE exams (id TEXT PRIMARY KEY,title TEXT NOT NULL,format TEXT NOT NULL,parts_total INTEGER NOT NULL,parts_with_script INTEGER NOT NULL,parts_complete INTEGER NOT NULL,recording_job TEXT,body TEXT NOT NULL,created_at_secs INTEGER NOT NULL,updated_at_secs INTEGER NOT NULL)").execute(&pool).await.unwrap();
        let body = include_str!("../application/fixtures/saved_exam_0_7_1.json");
        sqlx::query("INSERT INTO exams VALUES ('legacy','Legacy','hsg',4,1,0,NULL,?,5,6)")
            .bind(body)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let before = now_secs();
        let jobs = JobStore::open_options(options.clone()).await.unwrap();
        let exams = ExamStore::with_pool(jobs.pool().clone());
        let (row, actual) = exams.get("legacy").await.unwrap().unwrap();
        assert_eq!(actual, body);
        assert_eq!(row.revision, 1);
        assert_eq!(row.created_at_secs, 5);
        let first: Option<i64> =
            sqlx::query_scalar("SELECT finished_at_secs FROM jobs WHERE id='done'")
                .fetch_one(jobs.pool())
                .await
                .unwrap();
        assert!(first.unwrap() >= before);
        assert!(jobs.purge_before(before - 1).await.unwrap().is_empty());
        jobs.pool().close().await;
        let jobs = JobStore::open_options(options).await.unwrap();
        let second: Option<i64> =
            sqlx::query_scalar("SELECT finished_at_secs FROM jobs WHERE id='done'")
                .fetch_one(jobs.pool())
                .await
                .unwrap();
        assert_eq!(first, second);
        let running: Option<i64> =
            sqlx::query_scalar("SELECT finished_at_secs FROM jobs WHERE id='running'")
                .fetch_one(jobs.pool())
                .await
                .unwrap();
        assert_eq!(running, None);
    }
}
