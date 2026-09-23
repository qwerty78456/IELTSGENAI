//! SQLite table of saved exams, in the same database as the jobs.
//!
//! Schema changes stay inline: `CREATE TABLE IF NOT EXISTS` for new tables and,
//! for a new column, an `ALTER TABLE` guarded by
//! `SELECT count(*) FROM pragma_table_info('exams') WHERE name = ?`.

use sqlx::sqlite::SqlitePool;

/// Creates the `exams` table; run at startup right after the `jobs` table.
/// `recording_job` names the `jobs` row a saved exam keeps alive.
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
