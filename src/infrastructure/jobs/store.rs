//! SQLite-backed job table. Outputs live on disk under `DATA_DIR/audio`.
//!
//! `output_path` holds the WAV's file name. Rows written before 0.6.0 hold an
//! absolute path; `output_file_in` reads both, so a `data/` folder that moves
//! with a portable install keeps its recordings.

use std::path::{Path, PathBuf};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use tokio::sync::OnceCell;

use super::super::config::config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    /// One part's passage rendered to WAV.
    PartAudio,
    /// The whole exam programme rendered to WAV.
    ExamAudio,
}

impl JobKind {
    fn as_str(self) -> &'static str {
        match self {
            JobKind::PartAudio => "part_audio",
            JobKind::ExamAudio => "exam_audio",
        }
    }

    fn parse(s: &str) -> JobKind {
        match s {
            "exam_audio" => JobKind::ExamAudio,
            _ => JobKind::PartAudio,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Pending,
    Processing,
    Completed,
    Failed,
}

impl JobState {
    fn as_str(self) -> &'static str {
        match self {
            JobState::Pending => "pending",
            JobState::Processing => "processing",
            JobState::Completed => "completed",
            JobState::Failed => "failed",
        }
    }

    fn parse(s: &str) -> JobState {
        match s {
            "pending" => JobState::Pending,
            "processing" => JobState::Processing,
            "completed" => JobState::Completed,
            _ => JobState::Failed,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // `kind` and `created_at_secs` feed the roadmap's job list view
pub struct JobRecord {
    pub id: String,
    pub kind: JobKind,
    pub state: JobState,
    pub progress: f32,
    pub error: Option<String>,
    pub output_path: Option<String>,
    pub created_at_secs: i64,
}

impl JobRecord {
    /// Where the finished WAV is under the current `DATA_DIR`, if the job wrote one.
    pub fn output_file(&self) -> Option<PathBuf> {
        self.output_path
            .as_deref()
            .map(|p| output_file_in(&config().audio_dir(), p))
    }
}

/// Resolves a stored `output_path` against `audio_dir`, keeping only the file
/// name so old absolute rows from either OS and new bare names both work.
pub fn output_file_in(audio_dir: &Path, output_path: &str) -> PathBuf {
    let name = output_path
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(output_path);
    audio_dir.join(name)
}

pub struct JobStore {
    pool: SqlitePool,
}

static STORE: OnceCell<JobStore> = OnceCell::const_new();

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl JobStore {
    /// Initialized before accepting requests. This accessor cannot perform fallible I/O.
    pub async fn global() -> &'static JobStore {
        STORE
            .get()
            .expect("job store initialized before server starts")
    }

    pub async fn initialize() -> Result<(), String> {
        let path = config().db_path();
        STORE
            .get_or_try_init(|| async {
                Self::open_options(
                    SqliteConnectOptions::new()
                        .filename(&path)
                        .create_if_missing(true),
                )
                .await
                .map_err(|e| format!("Cannot open job database {}: {e}", path.display()))
            })
            .await?;
        Ok(())
    }

    pub(crate) async fn open_options(options: SqliteConnectOptions) -> Result<Self, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                state TEXT NOT NULL,
                progress REAL NOT NULL,
                error TEXT,
                output_path TEXT,
                created_at_secs INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        super::super::exams::create_schema(&pool).await?;
        // SQLite can open an existing database read-only despite requesting writes.
        // Exercise a real write and roll it back before accepting any requests.
        let mut transaction = pool.begin().await?;
        sqlx::query("INSERT INTO jobs (id, kind, state, progress, created_at_secs) VALUES (?1, 'startup_check', 'pending', 0, 0)")
            .bind(uuid::Uuid::new_v4().to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.rollback().await?;
        Ok(Self { pool })
    }

    pub async fn create(&self, kind: JobKind) -> Result<String, sqlx::Error> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO jobs (id, kind, state, progress, created_at_secs) VALUES (?1, ?2, ?3, 0.0, ?4)")
            .bind(&id)
            .bind(kind.as_str())
            .bind(JobState::Pending.as_str())
            .bind(now_secs())
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    pub async fn active_count(&self) -> Result<i64, sqlx::Error> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT count(*) FROM jobs WHERE state IN ('pending', 'processing')")
                .fetch_one(&self.pool)
                .await?;
        Ok(count)
    }

    pub async fn mark_processing(&self, id: &str, progress: f32) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE jobs SET state = ?1, progress = ?2 WHERE id = ?3")
            .bind(JobState::Processing.as_str())
            .bind(progress)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn complete(&self, id: &str, output_path: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE jobs SET state = ?1, progress = 1.0, output_path = ?2 WHERE id = ?3")
            .bind(JobState::Completed.as_str())
            .bind(output_path)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn fail(&self, id: &str, error: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE jobs SET state = ?1, error = ?2 WHERE id = ?3")
            .bind(JobState::Failed.as_str())
            .bind(error)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get(&self, id: &str) -> Result<Option<JobRecord>, sqlx::Error> {
        let row: Option<(String, String, String, f32, Option<String>, Option<String>, i64)> = sqlx::query_as(
            "SELECT id, kind, state, progress, error, output_path, created_at_secs FROM jobs WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(id, kind, state, progress, error, output_path, created_at_secs)| JobRecord {
                id,
                kind: JobKind::parse(&kind),
                state: JobState::parse(&state),
                progress,
                error,
                output_path,
                created_at_secs,
            },
        ))
    }

    /// Deletes jobs created before `cutoff_secs` and returns their output paths
    /// for removal. A job a saved exam refers to is never purged here.
    pub async fn purge_before(&self, cutoff_secs: i64) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, output_path FROM jobs
             WHERE created_at_secs <= ?1
               AND id NOT IN (SELECT recording_job FROM exams WHERE recording_job IS NOT NULL)",
        )
        .bind(cutoff_secs)
        .fetch_all(&self.pool)
        .await?;
        let mut paths = Vec::new();
        for (id, path) in rows {
            sqlx::query("DELETE FROM jobs WHERE id = ?1")
                .bind(&id)
                .execute(&self.pool)
                .await?;
            paths.extend(path);
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_file_resolves_old_absolute_and_new_relative_names() {
        let dir = Path::new("current-data").join("audio");
        for stored in [
            "/srv/data/audio/a.wav",
            "D:\\app\\data\\audio\\a.wav",
            "C:/mixed\\separators/a.wav",
            "a.wav",
        ] {
            assert_eq!(output_file_in(&dir, stored), dir.join("a.wav"), "{stored}");
        }
        assert_eq!(output_file_in(&dir, "trailing/"), dir.join("trailing/"));
    }

    #[tokio::test]
    async fn startup_write_probe_leaves_no_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let store = JobStore::open_options(options.clone()).await.unwrap();
        assert_eq!(store.active_count().await.unwrap(), 0);
        let id = store.create(JobKind::PartAudio).await.unwrap();
        store.pool.close().await;
        let restarted = JobStore::open_options(options).await.unwrap();
        assert_eq!(restarted.active_count().await.unwrap(), 1);
        assert!(restarted.get(&id).await.unwrap().is_some());
        restarted.pool.close().await;
    }

    #[tokio::test]
    async fn existing_readonly_database_is_rejected_at_startup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.db");
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let store = JobStore::open_options(options.clone()).await.unwrap();
        store.pool.close().await;
        assert!(
            JobStore::open_options(options.read_only(true))
                .await
                .is_err()
        );
    }
}
