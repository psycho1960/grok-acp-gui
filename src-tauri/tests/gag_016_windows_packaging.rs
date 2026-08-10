use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const V6_MIGRATIONS: [&str; 6] = [
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_session_events.sql"),
    include_str!("../migrations/0003_permissions_and_plans.sql"),
    include_str!("../migrations/0004_worktree_lifecycle.sql"),
    include_str!("../migrations/0005_checkpoints.sql"),
    include_str!("../migrations/0006_squash_integrations.sql"),
];

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("gag-016 Unicode 空格-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create GAG-016 fixture directory");
        Self(path)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sha256(path: &Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(fs::read(path).expect("read fixture"))
    )
}

fn create_v6_database(path: &Path) {
    let connection = Connection::open(path).expect("create v6 fixture");
    for (index, sql) in V6_MIGRATIONS.iter().enumerate() {
        connection
            .execute_batch(sql)
            .expect("apply historical migration");
        let checksum = format!("{:x}", Sha256::digest(sql.as_bytes()));
        connection
            .execute(
                "INSERT INTO _schema_version (version, applied_at, checksum) VALUES (?1, 'fixture', ?2)",
                params![(index + 1) as i64, checksum],
            )
            .expect("record historical migration");
    }
}

fn schema_version(path: &Path) -> i64 {
    Connection::open(path)
        .expect("open fixture for version query")
        .query_row("SELECT MAX(version) FROM _schema_version", [], |row| {
            row.get(0)
        })
        .expect("read schema version")
}

#[test]
fn v6_upgrade_uses_a_copy_and_preserves_the_original_database() {
    let fixture = FixtureDir::new();
    let original = fixture.0.join("grok_acp_gui-v6.db");
    let upgrade_copy = fixture.0.join("grok_acp_gui-upgrade-copy.db");
    create_v6_database(&original);
    let original_before = sha256(&original);
    fs::copy(&original, &upgrade_copy).expect("copy database before upgrade");

    let repository = SqliteRepository::open(&upgrade_copy).expect("upgrade copied database");
    drop(repository);

    assert_eq!(schema_version(&original), 6);
    assert_eq!(sha256(&original), original_before);
    assert_eq!(schema_version(&upgrade_copy), 8);
    assert!(
        upgrade_copy.exists(),
        "migration evidence copy must remain available"
    );
}

#[test]
fn newer_schema_is_rejected_without_replacing_or_mutating_the_database() {
    let fixture = FixtureDir::new();
    let future = fixture.0.join("grok_acp_gui-future.db");
    create_v6_database(&future);
    let connection = Connection::open(&future).expect("open future fixture");
    connection
        .execute(
            "INSERT INTO _schema_version (version, applied_at, checksum) VALUES (99, 'future', 'future')",
            [],
        )
        .expect("record future schema");
    drop(connection);
    let before = sha256(&future);

    let error = match SqliteRepository::open(&future) {
        Ok(_) => panic!("newer schema must fail closed"),
        Err(error) => error,
    };

    assert_eq!(error.code, "DB_MIGRATION_FAILED");
    assert!(error.message.contains("newer than known migrations"));
    assert_eq!(sha256(&future), before);
    assert_eq!(schema_version(&future), 99);
}
