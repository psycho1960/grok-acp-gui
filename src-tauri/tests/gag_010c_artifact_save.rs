use base64::Engine as _;
use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::bridge::types::TaskId;
use grok_acp_gui_lib::domain::types::{
    utc_now, Project, ProjectId, Task, TaskId as DomainTaskId, TaskStatus, WorkspaceKind,
};
use grok_acp_gui_lib::modules::artifacts::{
    ArtifactSaveStatus, ArtifactService, BlobImage, ManagedArtifactService,
};
use grok_acp_gui_lib::modules::persistence::Repository;
use std::path::{Path, PathBuf};

struct TestEnvironment {
    repo: SqliteRepository,
    workspace: PathBuf,
    export_dir: PathBuf,
    task_id: TaskId,
}

impl TestEnvironment {
    fn new(label: &str) -> Self {
        let nonce = uuid::Uuid::new_v4();
        let workspace = std::env::temp_dir().join(format!("gag-010c-{label}-{nonce}"));
        let export_dir = workspace.join("导出 结果");
        std::fs::create_dir_all(&export_dir).expect("test export directory");
        let repo = SqliteRepository::open_in_memory().expect("in-memory repository");
        let project_id = ProjectId::new(format!("project-{label}-{nonce}"));
        repo.create_project(&Project {
            id: project_id.clone(),
            path: workspace.to_string_lossy().into_owned(),
            display_path: label.into(),
            repo_root: None,
            trusted_at: Some(utc_now()),
            last_opened_at: utc_now(),
        })
        .expect("project");
        let task_id = DomainTaskId::new(format!("task-{label}-{nonce}"));
        create_task(&repo, &project_id, &task_id);
        Self {
            repo,
            workspace,
            export_dir,
            task_id: TaskId(task_id.0),
        }
    }

    fn import(&self, display_name: &str, bytes: &[u8]) -> String {
        ManagedArtifactService::new()
            .import_blob_images(
                &self.repo,
                &self.task_id,
                &[BlobImage {
                    display_name: display_name.into(),
                    base64_data: base64::engine::general_purpose::STANDARD.encode(bytes),
                }],
            )
            .expect("import fixture")[0]
            .artifact_id
            .clone()
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.workspace);
    }
}

fn create_task(repo: &SqliteRepository, project_id: &ProjectId, task_id: &DomainTaskId) {
    repo.create_task(&Task {
        id: task_id.clone(),
        project_id: project_id.clone(),
        title: "Artifact save".into(),
        status: TaskStatus::Idle,
        workspace_kind: WorkspaceKind::Direct,
        mode: None,
        model: None,
        reasoning: None,
        created_at: utc_now(),
        updated_at: utc_now(),
        interrupt_reason: None,
        interrupted_at: None,
        attempt_count: 1,
    })
    .expect("task");
}

fn png() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==")
        .expect("png fixture")
}

fn image_fixture(base64: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(base64)
        .expect("valid image fixture")
}

#[test]
fn saves_png_jpeg_webp_and_gif_with_unicode_and_long_names() {
    let environment = TestEnvironment::new("formats");
    let fixtures: [(&str, &str, Vec<u8>); 4] = [
        ("输入.png", "导出的图片.png", png()),
        ("photo.jpg", "照片 副本.jpg", image_fixture("/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAABAAEDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDyiiiiu04z/9k=")),
        ("image.webp", "网页图片.webp", image_fixture("UklGRjAAAABXRUJQVlA4ICQAAABQAQCdASoBAAEAAUAmJQBOgC6gAP76IZfF3YjjJ4dVU9ffoAA=")),
        ("motion.gif", "动图.gif", image_fixture("R0lGODdhAQABAIEAABctQwAAAAAAAAAAACwAAAAAAQABAAAIBAABBAQAOw==")),
    ];
    let service = ManagedArtifactService::new();
    for (source_name, target_name, bytes) in fixtures {
        let artifact_id = environment.import(source_name, &bytes);
        let target = environment.export_dir.join(target_name);
        let result = service.save(
            &environment.repo,
            &environment.task_id,
            &artifact_id,
            &target.to_string_lossy(),
            false,
        );
        assert_eq!(result.status, ArtifactSaveStatus::Saved);
        assert_eq!(std::fs::read(target).expect("saved bytes"), bytes);
    }

    let artifact_id = environment.import("long.png", &png());
    let long_name = format!("中文-{}-结果.png", "很长的文件名".repeat(12));
    let target = environment.export_dir.join(long_name);
    let result = service.save(
        &environment.repo,
        &environment.task_id,
        &artifact_id,
        &target.to_string_lossy(),
        false,
    );
    assert_eq!(result.status, ArtifactSaveStatus::Saved);
}

#[test]
fn conflict_never_overwrites_silently_and_explicit_overwrite_is_atomic() {
    let environment = TestEnvironment::new("conflict");
    let bytes = png();
    let artifact_id = environment.import("source.png", &bytes);
    let target = environment.export_dir.join("existing.png");
    std::fs::write(&target, b"old target content").expect("old target");
    let service = ManagedArtifactService::new();

    let conflict = service.save(
        &environment.repo,
        &environment.task_id,
        &artifact_id,
        &target.to_string_lossy(),
        false,
    );
    assert_eq!(conflict.status, ArtifactSaveStatus::Conflict);
    assert_eq!(std::fs::read(&target).unwrap(), b"old target content");

    let overwritten = service.save(
        &environment.repo,
        &environment.task_id,
        &artifact_id,
        &target.to_string_lossy(),
        true,
    );
    assert_eq!(overwritten.status, ArtifactSaveStatus::Saved);
    assert_eq!(std::fs::read(&target).unwrap(), bytes);
}

#[test]
fn rejects_missing_invalid_cross_task_and_unsafe_targets() {
    let environment = TestEnvironment::new("rejected");
    let service = ManagedArtifactService::new();
    let artifact_id = environment.import("source.png", &png());
    let safe_target = environment.export_dir.join("safe.png");

    let invalid = service.save(
        &environment.repo,
        &environment.task_id,
        "artifact-does-not-exist",
        &safe_target.to_string_lossy(),
        false,
    );
    assert_eq!(invalid.status, ArtifactSaveStatus::Rejected);

    let project = environment
        .repo
        .get_task(&environment.task_id.0)
        .and_then(|task| environment.repo.get_project(&task.project_id.0))
        .expect("project");
    let other_task = DomainTaskId::new("task-other");
    create_task(&environment.repo, &project.id, &other_task);
    let cross_task = service.save(
        &environment.repo,
        &TaskId(other_task.0),
        &artifact_id,
        &safe_target.to_string_lossy(),
        false,
    );
    assert_eq!(cross_task.status, ArtifactSaveStatus::Rejected);

    let relative = service.save(
        &environment.repo,
        &environment.task_id,
        &artifact_id,
        "..\\escape.png",
        false,
    );
    assert_eq!(relative.status, ArtifactSaveStatus::Rejected);

    let managed = environment
        .workspace
        .join(".grok-acp-gui")
        .join("unsafe.png");
    let managed_result = service.save(
        &environment.repo,
        &environment.task_id,
        &artifact_id,
        &managed.to_string_lossy(),
        true,
    );
    assert_eq!(managed_result.status, ArtifactSaveStatus::Rejected);

    let database = environment.export_dir.join("grok_acp_gui.db");
    std::fs::write(&database, b"database content").unwrap();
    let database_result = ManagedArtifactService::with_protected_database(database.clone()).save(
        &environment.repo,
        &environment.task_id,
        &artifact_id,
        &database.to_string_lossy(),
        true,
    );
    assert_eq!(database_result.status, ArtifactSaveStatus::Rejected);
    assert_eq!(std::fs::read(&database).unwrap(), b"database content");

    let directory_result = service.save(
        &environment.repo,
        &environment.task_id,
        &artifact_id,
        &environment.export_dir.to_string_lossy(),
        true,
    );
    assert_eq!(directory_result.status, ArtifactSaveStatus::Rejected);

    let record = environment
        .repo
        .get_attachment(&artifact_id)
        .expect("attachment");
    std::fs::remove_file(record.cache_path).expect("remove managed source");
    let missing = service.save(
        &environment.repo,
        &environment.task_id,
        &artifact_id,
        &safe_target.to_string_lossy(),
        false,
    );
    assert_eq!(missing.status, ArtifactSaveStatus::Rejected);
}

#[test]
fn extension_mismatch_warns_but_preserves_bytes() {
    let environment = TestEnvironment::new("extension");
    let bytes = png();
    let artifact_id = environment.import("source.png", &bytes);
    let target = environment.export_dir.join("renamed.jpg");
    let result = ManagedArtifactService::new().save(
        &environment.repo,
        &environment.task_id,
        &artifact_id,
        &target.to_string_lossy(),
        false,
    );
    assert_eq!(result.status, ArtifactSaveStatus::Saved);
    assert!(result.extension_warning.is_some());
    assert_eq!(std::fs::read(target).unwrap(), bytes);
}

#[cfg(windows)]
#[test]
#[allow(clippy::permissions_set_readonly_false)] // Windows read-only attribute cleanup.
fn read_only_and_reparse_targets_do_not_damage_existing_content() {
    use std::os::windows::fs::symlink_file;

    let environment = TestEnvironment::new("windows-targets");
    let artifact_id = environment.import("source.png", &png());
    let service = ManagedArtifactService::new();
    let read_only = environment.export_dir.join("read-only.png");
    std::fs::write(&read_only, b"protected").unwrap();
    let mut permissions = std::fs::metadata(&read_only).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&read_only, permissions).unwrap();
    let result = service.save(
        &environment.repo,
        &environment.task_id,
        &artifact_id,
        &read_only.to_string_lossy(),
        true,
    );
    assert_eq!(result.status, ArtifactSaveStatus::Failed);
    assert_eq!(std::fs::read(&read_only).unwrap(), b"protected");
    let mut permissions = std::fs::metadata(&read_only).unwrap().permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(&read_only, permissions).unwrap();

    let linked_source = environment.export_dir.join("linked-source.png");
    let link = environment.export_dir.join("linked-target.png");
    std::fs::write(&linked_source, b"linked content").unwrap();
    if symlink_file(&linked_source, &link).is_ok() {
        let linked = service.save(
            &environment.repo,
            &environment.task_id,
            &artifact_id,
            &link.to_string_lossy(),
            true,
        );
        assert_eq!(linked.status, ArtifactSaveStatus::Rejected);
        assert_eq!(std::fs::read(&linked_source).unwrap(), b"linked content");
    }

    let source_artifact = environment.import("source-reparse.png", &png());
    let record = environment.repo.get_attachment(&source_artifact).unwrap();
    let managed_path = PathBuf::from(&record.cache_path);
    let external_source = environment.export_dir.join("external-source.png");
    std::fs::write(&external_source, png()).unwrap();
    std::fs::remove_file(&managed_path).unwrap();
    if symlink_file(&external_source, &managed_path).is_ok() {
        let source_link_result = service.save(
            &environment.repo,
            &environment.task_id,
            &source_artifact,
            &environment.export_dir.join("escaped.png").to_string_lossy(),
            false,
        );
        assert_eq!(source_link_result.status, ArtifactSaveStatus::Rejected);
    }
}

#[test]
fn historical_artifact_can_be_saved_after_repository_restart() {
    let nonce = uuid::Uuid::new_v4();
    let root = std::env::temp_dir().join(format!("gag-010c-restart-{nonce}"));
    let export_dir = root.join("export");
    std::fs::create_dir_all(&export_dir).unwrap();
    let db_path = root.join("history.sqlite");
    let task_id = TaskId(format!("task-{nonce}"));
    let artifact_id;
    {
        let repo = SqliteRepository::open(&db_path).unwrap();
        let project_id = ProjectId::new(format!("project-{nonce}"));
        repo.create_project(&Project {
            id: project_id.clone(),
            path: root.to_string_lossy().into_owned(),
            display_path: "restart".into(),
            repo_root: None,
            trusted_at: Some(utc_now()),
            last_opened_at: utc_now(),
        })
        .unwrap();
        create_task(&repo, &project_id, &DomainTaskId::new(task_id.0.clone()));
        artifact_id = ManagedArtifactService::new()
            .import_blob_images(
                &repo,
                &task_id,
                &[BlobImage {
                    display_name: "history.png".into(),
                    base64_data: base64::engine::general_purpose::STANDARD.encode(png()),
                }],
            )
            .unwrap()[0]
            .artifact_id
            .clone();
    }
    {
        let reopened = SqliteRepository::open(&db_path).unwrap();
        let target = export_dir.join("restored.png");
        let result = ManagedArtifactService::new().save(
            &reopened,
            &task_id,
            &artifact_id,
            &target.to_string_lossy(),
            false,
        );
        assert_eq!(result.status, ArtifactSaveStatus::Saved);
        assert!(Path::new(&target).is_file());
    }
    let _ = std::fs::remove_dir_all(root);
}
