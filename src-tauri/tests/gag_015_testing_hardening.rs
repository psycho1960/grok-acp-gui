//! GAG-015 isolated cross-layer harness acceptance tests.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use grok_acp_gui_lib::adapters::grok_acp::{FakeAcpTransport, FakeScenario};
use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::bridge::dispatch::{execute_impl, DesktopResult};
use grok_acp_gui_lib::modules::agent_runtime::{AgentRuntime, AgentRuntimeImpl};
use grok_acp_gui_lib::modules::task_runtime::TaskRuntimeImpl;

fn isolated_path(label: &str, extension: Option<&str>) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let suffix = extension
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "gag-015-{label}-{}-{nonce}{suffix}",
        std::process::id()
    ))
}

fn assert_isolated(path: &Path) {
    assert!(path.is_absolute(), "fixture path must be absolute");
    assert!(
        path.starts_with(std::env::temp_dir()),
        "fixture must remain under the operating-system temp directory"
    );
}

struct TempRepositoryFixture {
    root: PathBuf,
}

impl TempRepositoryFixture {
    fn new() -> Self {
        let root = isolated_path("repository-含 空格", None);
        assert_isolated(&root);
        std::fs::create_dir_all(&root).expect("create repository fixture");
        let output = Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(&root)
            .output()
            .expect("run git init with argv");
        assert!(output.status.success(), "git init failed");
        Self { root }
    }

    fn git(&self, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .expect("run fixture git command")
    }
}

impl Drop for TempRepositoryFixture {
    fn drop(&mut self) {
        assert_isolated(&self.root);
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root).expect("remove isolated repository fixture");
        }
    }
}

struct TempSqliteFixture {
    path: PathBuf,
}

impl TempSqliteFixture {
    fn new() -> Self {
        let path = isolated_path("sqlite", Some("sqlite"));
        assert_isolated(&path);
        Self { path }
    }

    fn open(&self) -> SqliteRepository {
        SqliteRepository::open(&self.path).expect("open isolated SQLite fixture")
    }
}

impl Drop for TempSqliteFixture {
    fn drop(&mut self) {
        assert_isolated(&self.path);
        for candidate in [
            self.path.clone(),
            PathBuf::from(format!("{}-wal", self.path.display())),
            PathBuf::from(format!("{}-shm", self.path.display())),
        ] {
            match std::fs::remove_file(candidate) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("remove isolated SQLite fixture: {error}"),
            }
        }
    }
}

#[derive(Clone, Copy)]
struct FakeAcpScript {
    scenario: FakeScenario,
}

impl FakeAcpScript {
    fn path(self) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("tests")
            .join("fake-acp-agent")
            .join("agent.mjs")
    }
}

#[derive(Default)]
struct FaultInjector {
    visited: Vec<&'static str>,
}

impl FaultInjector {
    fn inject(&mut self, point: &'static str, action: impl FnOnce()) {
        self.visited.push(point);
        action();
    }
}

#[derive(Default)]
struct EvidenceRecorder {
    entries: Vec<(&'static str, String)>,
}

impl EvidenceRecorder {
    fn record(&mut self, name: &'static str, value: impl Into<String>) {
        self.entries.push((name, value.into()));
    }
}

struct FakeDesktopBridgeScenario {
    repo: Arc<SqliteRepository>,
    runtime: Arc<AgentRuntimeImpl<FakeAcpTransport>>,
    task_runtime: Arc<TaskRuntimeImpl<AgentRuntimeImpl<FakeAcpTransport>>>,
}

impl FakeDesktopBridgeScenario {
    fn new(script: FakeAcpScript) -> Self {
        let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repository"));
        let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(script.scenario, script.path()));
        let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
        Self {
            repo,
            runtime,
            task_runtime,
        }
    }

    async fn execute(&self, command: serde_json::Value) -> DesktopResult {
        execute_impl(
            self.repo.as_ref(),
            self.runtime.as_ref(),
            self.task_runtime.as_ref(),
            command,
        )
        .await
    }
}

#[test]
fn temp_repository_fixture_uses_real_git_only_inside_temp() {
    let fixture = TempRepositoryFixture::new();
    let mut evidence = EvidenceRecorder::default();
    let output = fixture.git(&["status", "--porcelain=v2", "--branch"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("git UTF-8");
    assert!(stdout.contains("# branch.head main"));
    evidence.record("fixture-root", fixture.root.display().to_string());
    evidence.record(
        "git-status-exit",
        output.status.code().unwrap_or(-1).to_string(),
    );
    assert_eq!(evidence.entries.len(), 2);
}

#[test]
fn corrupt_sqlite_copy_fails_without_touching_any_user_database() {
    let fixture = TempSqliteFixture::new();
    drop(fixture.open());
    let mut faults = FaultInjector::default();
    faults.inject("sqlite-corrupt-copy", || {
        std::fs::write(&fixture.path, b"not a sqlite database").expect("corrupt temp copy")
    });
    let error = match SqliteRepository::open(&fixture.path) {
        Ok(_) => panic!("corrupt database must not open"),
        Err(error) => error,
    };
    assert!(!error.code.is_empty());
    assert_eq!(faults.visited, vec!["sqlite-corrupt-copy"]);
}

#[tokio::test]
async fn fake_desktop_bridge_scenario_uses_the_real_dispatcher_and_fake_acp_adapter() {
    let scenario = FakeDesktopBridgeScenario::new(FakeAcpScript {
        scenario: FakeScenario::Normal,
    });
    let result = scenario
        .execute(serde_json::json!({ "type": "runtime.refresh", "payload": {} }))
        .await;
    match result {
        DesktopResult::Ok { data } => assert_eq!(data["ready"], true),
        DesktopResult::Err { error } => panic!("runtime refresh failed: {error:?}"),
    }
    scenario
        .runtime
        .shutdown_all("gag-015 harness complete")
        .await;
}
