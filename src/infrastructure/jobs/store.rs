//! SQLite-backed job table. Outputs live on disk under `DATA_DIR/audio`.

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
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
    /// The process-wide store, opened on first use.
    pub async fn global() -> &'static JobStore {
        STORE
            .get_or_init(|| async {
                let path = config().db_path();
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                JobStore::open(&format!("sqlite://{}?mode=rwc", path.to_string_lossy()))
                    .await
                    .expect("cannot open the job database")
            })
            .await
    }

    pub async fn open(url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(url)
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

    /// Deletes jobs created before `cutoff_secs` and returns their output paths for removal.
    pub async fn purge_before(&self, cutoff_secs: i64) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT id, output_path FROM jobs WHERE created_at_secs <= ?1")
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
