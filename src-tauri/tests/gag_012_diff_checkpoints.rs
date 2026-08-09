use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::domain::types::{
    Project, ProjectId, Task, TaskId, TaskStatus, WorkspaceKind,
};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::workspace::{
    CheckpointSelection, CreateManagedWorktree, ManagedWorkspaceService, WorkspaceService,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git fixture command should start");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

struct Fixture {
    source: PathBuf,
    managed: PathBuf,
    worktree: PathBuf,
    branch: String,
    repo: Arc<dyn Repository>,
    service: ManagedWorkspaceService,
}

impl Fixture {
    fn new() -> Self {
        let suffix = uuid::Uuid::new_v4();
        let source = std::env::temp_dir().join(format!("gag-012-source-{suffix}"));
        let managed = std::env::temp_dir().join(format!("gag-012-managed-{suffix}"));
        let recovery = std::env::temp_dir().join(format!("gag-012-recovery-{suffix}"));
        std::fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "-b", "main"]);
        git(&source, &["config", "user.name", "GAG 012"]);
        git(&source, &["config", "user.email", "gag012@example.invalid"]);
        std::fs::write(source.join("alpha.txt"), "alpha\n").unwrap();
        std::fs::write(source.join("rename-me.txt"), "rename\n").unwrap();
        git(&source, &["add", "--", "alpha.txt", "rename-me.txt"]);
        git(&source, &["commit", "-m", "fixture"]);

        let repo: Arc<dyn Repository> = Arc::new(SqliteRepository::open_in_memory().unwrap());
        let now = grok_acp_gui_lib::domain::types::utc_now();
        repo.create_project(&Project {
            id: ProjectId::new("project-gag-012"),
            path: source.to_string_lossy().into_owned(),
            display_path: "fixture".into(),
            repo_root: Some(source.to_string_lossy().into_owned()),
            trusted_at: Some(now.clone()),
            last_opened_at: now.clone(),
        })
        .unwrap();
        repo.create_task(&Task {
            id: TaskId::new("GAG-012-task"),
            project_id: ProjectId::new("project-gag-012"),
            title: "diff checkpoints".into(),
            status: TaskStatus::Preparing,
            workspace_kind: WorkspaceKind::Worktree,
            mode: Some("agent".into()),
            model: None,
            reasoning: None,
            created_at: now.clone(),
            updated_at: now,
            interrupt_reason: None,
            interrupted_at: None,
            attempt_count: 0,
        })
        .unwrap();
        let service = ManagedWorkspaceService::new(repo.clone(), managed.clone(), recovery);
        let record = service
            .create_managed_worktree(CreateManagedWorktree {
                repo_root: source.clone(),
                task_id: TaskId::new("GAG-012-task"),
                task_slug: "ignored".into(),
                base_ref: "main".into(),
            })
            .unwrap();
        Self {
            source,
            managed,
            worktree: PathBuf::from(record.path),
            branch: record.branch,
            repo,
            service,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let path = self.worktree.to_string_lossy().into_owned();
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force", &path])
            .current_dir(&self.source)
            .output();
        let _ = Command::new("git")
            .args(["branch", "-D", &self.branch])
            .current_dir(&self.source)
            .output();
        let _ = std::fs::remove_dir_all(&self.source);
        let _ = std::fs::remove_dir_all(&self.managed);
    }
}

#[test]
fn status_diff_selection_checkpoint_and_history_are_consistent() {
    let fixture = Fixture::new();
    std::fs::write(fixture.worktree.join("alpha.txt"), "alpha changed\n").unwrap();
    std::fs::write(fixture.worktree.join("unselected.txt"), "keep me\n").unwrap();
    std::fs::write(fixture.worktree.join("binary.bin"), [0, 1, 2, 3]).unwrap();

    let snapshot = fixture.service.get_worktree_status("GAG-012-task").unwrap();
    assert_eq!(snapshot.files.len(), 3);
    let alpha = snapshot
        .files
        .iter()
        .find(|file| file.path == "alpha.txt")
        .unwrap();
    let binary = snapshot
        .files
        .iter()
        .find(|file| file.path == "binary.bin")
        .unwrap();
    assert_eq!(alpha.kind, "modified");
    assert!(binary.binary);
    let diff = fixture
        .service
        .get_diff("GAG-012-task", &alpha.path, &alpha.fingerprint)
        .unwrap();
    assert!(diff.text.unwrap().contains("+alpha changed"));

    let selection = vec![
        CheckpointSelection {
            path: alpha.path.clone(),
            fingerprint: alpha.fingerprint.clone(),
        },
        CheckpointSelection {
            path: binary.path.clone(),
            fingerprint: binary.fingerprint.clone(),
        },
    ];
    let receipt = fixture
        .service
        .create_checkpoint(
            "GAG-012-task",
            "chore(GAG-012): save selected files [GAG-012]",
            &selection,
        )
        .unwrap();
    assert_eq!(receipt.selection_manifest.len(), 2);
    assert_eq!(receipt.remaining_files.len(), 1);
    assert_eq!(receipt.remaining_files[0].path, "unselected.txt");
    let committed = git(
        &fixture.worktree,
        &["show", "--pretty=format:", "--name-only", "HEAD"],
    );
    assert!(committed.contains("alpha.txt"));
    assert!(committed.contains("binary.bin"));
    assert!(!committed.contains("unselected.txt"));
    let history = fixture.service.list_checkpoints("GAG-012-task").unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].commit_sha, receipt.commit_sha);
    assert_eq!(
        fixture
            .repo
            .list_checkpoints_by_task("GAG-012-task")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn stale_selection_and_preexisting_index_fail_closed() {
    let fixture = Fixture::new();
    std::fs::write(fixture.worktree.join("alpha.txt"), "first\n").unwrap();
    let first = fixture.service.get_worktree_status("GAG-012-task").unwrap();
    let alpha = first
        .files
        .iter()
        .find(|file| file.path == "alpha.txt")
        .unwrap();
    let stale = CheckpointSelection {
        path: alpha.path.clone(),
        fingerprint: alpha.fingerprint.clone(),
    };
    std::fs::write(fixture.worktree.join("alpha.txt"), "changed externally\n").unwrap();
    let validation = fixture
        .service
        .validate_selection("GAG-012-task", std::slice::from_ref(&stale))
        .unwrap();
    assert!(!validation.valid);
    assert_eq!(validation.stale_paths, vec!["alpha.txt"]);

    let current = fixture.service.get_worktree_status("GAG-012-task").unwrap();
    let alpha = current
        .files
        .iter()
        .find(|file| file.path == "alpha.txt")
        .unwrap();
    git(&fixture.worktree, &["add", "--", "alpha.txt"]);
    let error = fixture
        .service
        .create_checkpoint(
            "GAG-012-task",
            "chore(GAG-012): must refuse staged index [GAG-012]",
            &[CheckpointSelection {
                path: alpha.path.clone(),
                fingerprint: alpha.fingerprint.clone(),
            }],
        )
        .unwrap_err();
    assert_eq!(error.code, "GIT_INDEX_NOT_EMPTY");
    assert!(git(&fixture.worktree, &["diff", "--cached", "--name-only"]).contains("alpha.txt"));
}

#[test]
fn rename_and_conflict_metadata_are_explicit() {
    let fixture = Fixture::new();
    git(
        &fixture.worktree,
        &["mv", "rename-me.txt", "renamed 文件.txt"],
    );
    let snapshot = fixture.service.get_worktree_status("GAG-012-task").unwrap();
    let rename = snapshot
        .files
        .iter()
        .find(|file| file.kind == "renamed")
        .unwrap();
    assert_eq!(rename.path, "renamed 文件.txt");
    assert_eq!(rename.old_path.as_deref(), Some("rename-me.txt"));
    assert!(rename.staged);
}
