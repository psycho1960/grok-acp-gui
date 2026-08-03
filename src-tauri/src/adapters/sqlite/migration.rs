//! SQLite Migration runner.
//!
//! Reads numbered `.sql` files from `src-tauri/migrations/`, applies them
//! in order inside transactions, and records the schema version + checksum
//! in the `_schema_version` table.
//!
//! # Invariants
//! - Any migration failure rolls back the entire batch.
//! - Already-applied migrations are skipped.
//! - A schema version newer than any known migration returns `DB_MIGRATION_FAILED`.
//! - Merged migration files MUST NOT be modified.

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::path::Path;

/// The embedded directory containing migration SQL files.
const MIGRATIONS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations");

/// Apply all pending migrations to `conn`. Runs inside a single transaction
/// so any failure rolls back the entire batch.
///
/// Returns the number of migrations applied (0 if already up-to-date).
pub fn run_migrations(conn: &Connection) -> Result<u32, MigrationError> {
    // Ensure the schema version table exists (idempotent).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _schema_version (
            version     INTEGER NOT NULL PRIMARY KEY,
            applied_at  TEXT    NOT NULL,
            checksum    TEXT    NOT NULL
        )",
    )
    .map_err(|e| MigrationError::QueryFailed(e.to_string()))?;

    // Read the current schema version (0 if no migrations applied).
    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
            [],
            |row| row.get(0),
        )
        .map_err(|e| MigrationError::QueryFailed(e.to_string()))?;

    // Discover migration files.
    let migrations = discover_migrations()?;
    if migrations.is_empty() {
        return Ok(0);
    }
    let max_known = migrations.last().unwrap().version;

    // If the DB has a version beyond our known migrations, error out.
    if current_version > max_known {
        return Err(MigrationError::SchemaTooNew {
            db_version: current_version,
            max_known,
        });
    }

    let mut applied = 0u32;

    for mig in &migrations {
        if mig.version <= current_version {
            // Already applied — verify checksum matches.
            verify_checksum(conn, mig)?;
            continue;
        }

        // Apply this migration.
        conn.execute_batch(&mig.sql)
            .map_err(|e| MigrationError::ApplyFailed {
                version: mig.version,
                message: e.to_string(),
            })?;

        // Record the migration.
        let now = crate::domain::types::utc_now();
        conn.execute(
            "INSERT OR REPLACE INTO _schema_version (version, applied_at, checksum) VALUES (?1, ?2, ?3)",
            rusqlite::params![mig.version, now, mig.checksum],
        )
        .map_err(|e| MigrationError::QueryFailed(e.to_string()))?;

        applied += 1;
    }

    Ok(applied)
}

/// Run migrations in a single transaction — the standard entry point.
pub fn run_migrations_transactional(conn: &mut Connection) -> Result<u32, MigrationError> {
    let tx = conn
        .transaction()
        .map_err(|e| MigrationError::QueryFailed(e.to_string()))?;
    let count = run_migrations(&tx)?;
    tx.commit()
        .map_err(|e| MigrationError::QueryFailed(e.to_string()))?;
    Ok(count)
}

// ---------------------------------------------------------------------------
// Migration discovery
// ---------------------------------------------------------------------------

struct MigrationFile {
    version: i64,
    sql: String,
    checksum: String,
}

fn discover_migrations() -> Result<Vec<MigrationFile>, MigrationError> {
    let dir = Path::new(MIGRATIONS_DIR);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| MigrationError::IoFailed(e.to_string()))?
        .filter_map(|entry| entry.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".sql"))
        .collect();

    // Sort by filename for deterministic ordering.
    files.sort_by_key(|e| e.file_name());

    let mut migrations = Vec::new();

    for file in files {
        let name = file.file_name().to_string_lossy().to_string();
        // Parse version number: "0001_initial.sql" → 1
        let version: i64 = name
            .split('_')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| MigrationError::BadFilename(name.clone()))?;

        let sql = std::fs::read_to_string(file.path())
            .map_err(|e| MigrationError::IoFailed(e.to_string()))?;

        let checksum = compute_checksum(&sql);
        migrations.push(MigrationFile {
            version,
            sql,
            checksum,
        });
    }

    Ok(migrations)
}

fn compute_checksum(sql: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(sql.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn verify_checksum(conn: &Connection, mig: &MigrationFile) -> Result<(), MigrationError> {
    let stored: String = conn
        .query_row(
            "SELECT checksum FROM _schema_version WHERE version = ?1",
            [mig.version],
            |row| row.get(0),
        )
        .map_err(|e| MigrationError::QueryFailed(e.to_string()))?;

    if stored != mig.checksum {
        return Err(MigrationError::ChecksumMismatch {
            version: mig.version,
            expected: stored,
            actual: mig.checksum.clone(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// MigrationError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    IoFailed(String),
    BadFilename(String),
    QueryFailed(String),
    ApplyFailed {
        version: i64,
        message: String,
    },
    SchemaTooNew {
        db_version: i64,
        max_known: i64,
    },
    ChecksumMismatch {
        version: i64,
        expected: String,
        actual: String,
    },
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationError::IoFailed(msg) => write!(f, "Migration I/O error: {}", msg),
            MigrationError::BadFilename(name) => write!(f, "Bad migration filename: {}", name),
            MigrationError::QueryFailed(msg) => write!(f, "Migration query failed: {}", msg),
            MigrationError::ApplyFailed { version, message } => {
                write!(f, "Migration {} failed: {}", version, message)
            }
            MigrationError::SchemaTooNew {
                db_version,
                max_known,
            } => write!(
                f,
                "Database schema version {} is newer than known migrations (max {})",
                db_version, max_known
            ),
            MigrationError::ChecksumMismatch {
                version,
                expected,
                actual,
            } => write!(
                f,
                "Checksum mismatch for migration {}: expected {}, got {}",
                version, expected, actual
            ),
        }
    }
}

impl std::error::Error for MigrationError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_conn() -> Connection {
        Connection::open_in_memory().expect("in-memory SQLite")
    }

    #[test]
    fn fresh_install_applies_migration() {
        let conn = in_memory_conn();
        let count = run_migrations(&conn).unwrap();
        assert!(count > 0, "should apply at least one migration");
        // Verify _schema_version was updated.
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM _schema_version", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(version > 0);
    }

    #[test]
    fn repeat_run_is_idempotent() {
        let conn = in_memory_conn();
        let first = run_migrations(&conn).unwrap();
        let second = run_migrations(&conn).unwrap();
        assert_eq!(second, 0, "repeat run should apply zero migrations");
        assert!(first > 0);
    }

    #[test]
    fn checksum_mismatch_detected() {
        let conn = in_memory_conn();
        // Apply migrations first.
        run_migrations(&conn).unwrap();
        // Corrupt the checksum.
        conn.execute(
            "UPDATE _schema_version SET checksum = 'bad' WHERE version = 1",
            [],
        )
        .unwrap();
        let result = run_migrations(&conn);
        assert!(result.is_err());
    }

    #[test]
    fn compute_checksum_is_stable() {
        let sql = "CREATE TABLE t(id INTEGER);";
        let cs1 = compute_checksum(sql);
        let cs2 = compute_checksum(sql);
        assert_eq!(cs1, cs2);
        assert!(!cs1.is_empty());
    }
}
