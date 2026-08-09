//! SQLite Migration runner.
//!
//! Migrations are embedded at compile time so they work in installed
//! packages (MSI/NSIS) where the source directory does not exist.
//! Each migration carries its SQL content and a SHA-256 checksum;
//! the runner applies them in order inside transactions.
//!
//! # Invariants
//! - Any migration failure rolls back the entire batch.
//! - Already-applied migrations are skipped (checksum verified).
//! - A schema version newer than any known migration returns `DB_MIGRATION_FAILED`.
//! - Merged migration files MUST NOT be modified.

use rusqlite::Connection;
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Embedded migrations
// ---------------------------------------------------------------------------

/// A single migration, bundled at compile time.
struct EmbeddedMigration {
    version: i64,
    sql: &'static str,
    checksum: String,
}

/// All known migrations, in version order. Called once per migration run.
fn embedded_migrations() -> Vec<EmbeddedMigration> {
    let sql_0001 = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations/0001_initial.sql"
    ));
    let sql_0002 = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations/0002_session_events.sql"
    ));
    let sql_0003 = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations/0003_permissions_and_plans.sql"
    ));
    let sql_0004 = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations/0004_worktree_lifecycle.sql"
    ));
    let sql_0005 = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations/0005_checkpoints.sql"
    ));
    let sql_0006 = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations/0006_squash_integrations.sql"
    ));
    vec![
        EmbeddedMigration {
            version: 1,
            sql: sql_0001,
            checksum: compute_checksum(sql_0001),
        },
        EmbeddedMigration {
            version: 2,
            sql: sql_0002,
            checksum: compute_checksum(sql_0002),
        },
        EmbeddedMigration {
            version: 3,
            sql: sql_0003,
            checksum: compute_checksum(sql_0003),
        },
        EmbeddedMigration {
            version: 4,
            sql: sql_0004,
            checksum: compute_checksum(sql_0004),
        },
        EmbeddedMigration {
            version: 5,
            sql: sql_0005,
            checksum: compute_checksum(sql_0005),
        },
        EmbeddedMigration {
            version: 6,
            sql: sql_0006,
            checksum: compute_checksum(sql_0006),
        },
    ]
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Apply all pending (embedded) migrations to `conn`. Runs inside a
/// single transaction so any failure rolls back the entire batch.
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

    let migrations = embedded_migrations();
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
        conn.execute_batch(mig.sql)
            .map_err(|e| MigrationError::ApplyFailed {
                version: mig.version,
                message: e.to_string(),
            })?;

        // Record the migration.
        let now = crate::domain::types::utc_now();
        conn.execute(
            "INSERT OR REPLACE INTO _schema_version (version, applied_at, checksum) VALUES (?1, ?2, ?3)",
            rusqlite::params![mig.version, now, &mig.checksum],
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
// Helpers
// ---------------------------------------------------------------------------

fn compute_checksum(sql: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(sql.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn verify_checksum(conn: &Connection, mig: &EmbeddedMigration) -> Result<(), MigrationError> {
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

    #[test]
    fn embedded_migrations_bundle_v1() {
        let migs = embedded_migrations();
        assert!(!migs.is_empty());
        assert_eq!(migs[0].version, 1);
        assert!(!migs[0].sql.is_empty());
        assert!(!migs[0].checksum.is_empty());
    }
}
