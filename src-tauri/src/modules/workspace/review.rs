//! GAG-012 review/checkpoint implementation behind the WorkspaceService interface.

use super::*;
use crate::domain::types::CheckpointRecord;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

const DIFF_LIMIT: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub kind: String,
    pub binary: bool,
    pub size: u64,
    pub mode: String,
    pub fingerprint: String,
    pub staged: bool,
    pub conflicted: bool,
    pub submodule: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSnapshot {
    pub head: String,
    pub version: String,
    pub files: Vec<FileChange>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffDocument {
    pub path: String,
    pub old_path: Option<String>,
    pub binary: bool,
    pub oversized: bool,
    pub truncated: bool,
    pub text: Option<String>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointSelection {
    pub path: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionValidation {
    pub valid: bool,
    pub stale_paths: Vec<String>,
    pub missing_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointReceipt {
    pub id: String,
    pub task_id: String,
    pub attempt_number: u32,
    pub commit_sha: String,
    pub tree_sha: String,
    pub head_before: String,
    pub selection_manifest: Vec<CheckpointSelection>,
    pub selection_hash: String,
    pub message: String,
    pub created_at: String,
    pub remaining_files: Vec<FileChange>,
}

impl ManagedWorkspaceService {
    pub(super) fn review_status(&self, task_id: &str) -> Result<ReviewSnapshot, WorkspaceError> {
        let record = active_managed_record(self.repo.as_ref(), task_id)?;
        let worktree = self.prove_registered_target(&record)?;
        let repository = self
            .git
            .inspect_repository(&worktree)
            .map_err(map_git_error)?;
        let raw = self
            .git
            .capture(
                &worktree,
                &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
            )
            .map_err(map_git_error)?;
        let files = parse_status(&worktree, &raw)?;
        let mut hasher = Sha256::new();
        hasher.update(repository.head.as_bytes());
        for file in &files {
            hasher.update(file.path.as_bytes());
            hasher.update(file.fingerprint.as_bytes());
        }
        Ok(ReviewSnapshot {
            head: repository.head,
            version: format!("{:x}", hasher.finalize()),
            files,
        })
    }

    pub(super) fn review_diff(
        &self,
        task_id: &str,
        path: &str,
        fingerprint: &str,
    ) -> Result<DiffDocument, WorkspaceError> {
        let snapshot = self.review_status(task_id)?;
        let change = snapshot
            .files
            .iter()
            .find(|file| file.path == path)
            .ok_or_else(|| {
                workspace_error("GIT_SELECTION_STALE", "Selected file is no longer changed")
            })?;
        if change.fingerprint != fingerprint {
            return Err(workspace_error(
                "GIT_SELECTION_STALE",
                "Selected file changed after review",
            ));
        }
        let record = active_managed_record(self.repo.as_ref(), task_id)?;
        let worktree = self.prove_registered_target(&record)?;
        if change.kind == "untracked" {
            let bytes = std::fs::read(worktree.join(&change.path)).map_err(|_| {
                workspace_error("GIT_DIFF_FAILED", "Untracked file could not be read")
            })?;
            return Ok(document_from_bytes(
                change,
                render_untracked(&change.path, &bytes),
            ));
        }
        let bytes = match self.git.capture(
            &worktree,
            &[
                "diff",
                "--no-ext-diff",
                "--find-renames",
                "--",
                &change.path,
            ],
        ) {
            Ok(bytes) => bytes,
            Err(error) if error.code == "GIT_OUTPUT_LIMIT" => {
                return Ok(oversized_document(change));
            }
            Err(error) => return Err(map_git_error(error)),
        };
        if bytes.is_empty() && change.staged {
            let staged = match self.git.capture(
                &worktree,
                &[
                    "diff",
                    "--cached",
                    "--no-ext-diff",
                    "--find-renames",
                    "--",
                    &change.path,
                ],
            ) {
                Ok(bytes) => bytes,
                Err(error) if error.code == "GIT_OUTPUT_LIMIT" => {
                    return Ok(oversized_document(change));
                }
                Err(error) => return Err(map_git_error(error)),
            };
            return Ok(document_from_bytes(change, staged));
        }
        Ok(document_from_bytes(change, bytes))
    }

    pub(super) fn validate_review_selection(
        &self,
        task_id: &str,
        selection: &[CheckpointSelection],
    ) -> Result<SelectionValidation, WorkspaceError> {
        if selection.is_empty() {
            return Err(workspace_error(
                "GIT_EMPTY_SELECTION",
                "Select at least one file",
            ));
        }
        let snapshot = self.review_status(task_id)?;
        let current: BTreeMap<&str, &FileChange> = snapshot
            .files
            .iter()
            .map(|file| (file.path.as_str(), file))
            .collect();
        let mut seen = BTreeSet::new();
        let mut stale_paths = Vec::new();
        let mut missing_paths = Vec::new();
        for item in selection {
            validate_relative_path(&item.path)?;
            if !seen.insert(item.path.as_str()) {
                return Err(workspace_error(
                    "GIT_INVALID_SELECTION",
                    "Selection contains duplicate paths",
                ));
            }
            match current.get(item.path.as_str()) {
                None => missing_paths.push(item.path.clone()),
                Some(file) if file.fingerprint != item.fingerprint => {
                    stale_paths.push(item.path.clone())
                }
                Some(file) if file.conflicted || file.submodule => {
                    stale_paths.push(item.path.clone())
                }
                Some(_) => {}
            }
        }
        Ok(SelectionValidation {
            valid: stale_paths.is_empty() && missing_paths.is_empty(),
            stale_paths,
            missing_paths,
        })
    }

    pub(super) fn create_review_checkpoint(
        &self,
        task_id: &str,
        message: &str,
        selection: &[CheckpointSelection],
    ) -> Result<CheckpointReceipt, WorkspaceError> {
        let _lock = self.lifecycle_lock.lock().map_err(|_| {
            workspace_error("GIT_CHECKPOINT_LOCKED", "Checkpoint lock is unavailable")
        })?;
        validate_checkpoint_message(message)?;
        let validation = self.validate_review_selection(task_id, selection)?;
        if !validation.valid {
            return Err(workspace_error(
                "GIT_SELECTION_STALE",
                "Checkpoint selection is stale",
            ));
        }
        let record = active_managed_record(self.repo.as_ref(), task_id)?;
        let worktree = self.prove_registered_target(&record)?;
        let before = self
            .git
            .inspect_repository(&worktree)
            .map_err(map_git_error)?;
        let snapshot = self.review_status(task_id)?;
        if snapshot.files.iter().any(|file| file.staged) {
            return Err(workspace_error(
                "GIT_INDEX_NOT_EMPTY",
                "Checkpoint refused because the index already contains changes",
            ));
        }
        let path_args = checkpoint_path_args(&snapshot.files, selection)?;
        let mut args = vec!["add", "--"];
        args.extend(path_args.iter().map(String::as_str));
        authorize_managed_git(&worktree, &args, &[&before.common_git_dir])?;
        self.git
            .run_checked(&worktree, &args)
            .map_err(map_git_error)?;

        let staged_bytes = self
            .git
            .capture(&worktree, &["diff", "--cached", "--name-only", "-z"])
            .map_err(map_git_error)?;
        let expected: BTreeSet<&str> = selection.iter().map(|item| item.path.as_str()).collect();
        let actual: BTreeSet<&str> = split_nul(&staged_bytes).collect();
        if actual != expected {
            return Err(workspace_error(
                "GIT_INDEX_MISMATCH",
                "Staged index does not exactly match the selected manifest; index was preserved for diagnosis",
            ));
        }
        let after_stage = self
            .git
            .inspect_repository(&worktree)
            .map_err(map_git_error)?;
        if after_stage.head != before.head {
            return Err(workspace_error(
                "GIT_HEAD_CHANGED",
                "HEAD changed during checkpoint creation",
            ));
        }
        let revalidated = self.validate_review_selection(task_id, selection)?;
        if !revalidated.valid {
            return Err(workspace_error(
                "GIT_SELECTION_STALE",
                "A selected file changed while staging; index was preserved for diagnosis",
            ));
        }
        let commit_args = ["commit", "-m", message];
        authorize_managed_git(&worktree, &commit_args, &[&before.common_git_dir])?;
        self.git.run_checked(&worktree, &commit_args).map_err(|_| {
            workspace_error(
                "GIT_COMMIT_FAILED_INDEX_PRESERVED",
                "Commit failed; the selected index is preserved for diagnosis",
            )
        })?;
        let commit_sha = first_line(
            &self
                .git
                .capture(&worktree, &["rev-parse", "HEAD"])
                .map_err(map_git_error)?,
        )?;
        let tree_sha = first_line(
            &self
                .git
                .capture(&worktree, &["rev-parse", "HEAD^{tree}"])
                .map_err(map_git_error)?,
        )?;
        let manifest_json = serde_json::to_string(selection).map_err(|_| {
            workspace_error(
                "GIT_CHECKPOINT_FAILED",
                "Selection manifest could not be encoded",
            )
        })?;
        let selection_hash = format!("{:x}", Sha256::digest(manifest_json.as_bytes()));
        let attempt_number = self
            .repo
            .get_binding_by_task(task_id)
            .map_err(map_repo_error)?
            .map(|binding| binding.attempt_number)
            .unwrap_or(0);
        let checkpoint = CheckpointRecord {
            id: format!("checkpoint-{}", uuid::Uuid::new_v4()),
            task_id: TaskId::new(task_id.to_owned()),
            attempt_number,
            commit_sha: commit_sha.clone(),
            tree_sha: tree_sha.clone(),
            head_before: before.head.clone(),
            selection_manifest: manifest_json,
            selection_hash: selection_hash.clone(),
            message: message.to_owned(),
            created_at: crate::domain::types::utc_now(),
        };
        self.repo.create_checkpoint(&checkpoint).map_err(|_| {
            workspace_error(
                "GIT_CHECKPOINT_AUDIT_FAILED",
                "Commit succeeded but checkpoint audit persistence failed",
            )
        })?;
        let remaining_files = self.review_status(task_id)?.files;
        Ok(CheckpointReceipt {
            id: checkpoint.id,
            task_id: task_id.to_owned(),
            attempt_number,
            commit_sha,
            tree_sha,
            head_before: before.head,
            selection_manifest: selection.to_vec(),
            selection_hash,
            message: message.to_owned(),
            created_at: checkpoint.created_at,
            remaining_files,
        })
    }

    pub(super) fn review_checkpoints(
        &self,
        task_id: &str,
    ) -> Result<Vec<CheckpointRecord>, WorkspaceError> {
        self.repo
            .list_checkpoints_by_task(task_id)
            .map_err(map_repo_error)
    }
}

fn parse_status(root: &Path, raw: &[u8]) -> Result<Vec<FileChange>, WorkspaceError> {
    let fields: Vec<&[u8]> = raw
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .collect();
    let mut files = Vec::new();
    let mut index = 0;
    while index < fields.len() {
        let text = std::str::from_utf8(fields[index]).map_err(|_| {
            workspace_error("GIT_INVALID_OUTPUT", "Git status contains a non-UTF-8 path")
        })?;
        let prefix = text.as_bytes()[0];
        let (xy, submodule, path, old_path, kind) = match prefix {
            b'1' => {
                let parts: Vec<&str> = text.splitn(9, ' ').collect();
                if parts.len() != 9 {
                    return Err(invalid_status());
                }
                (
                    parts[1],
                    parts[2] != "N...",
                    parts[8].to_owned(),
                    None,
                    status_kind(parts[1]),
                )
            }
            b'2' => {
                let parts: Vec<&str> = text.splitn(10, ' ').collect();
                if parts.len() != 10 || index + 1 >= fields.len() {
                    return Err(invalid_status());
                }
                index += 1;
                let old = std::str::from_utf8(fields[index])
                    .map_err(|_| invalid_status())?
                    .to_owned();
                (
                    parts[1],
                    parts[2] != "N...",
                    parts[9].to_owned(),
                    Some(old),
                    "renamed".to_owned(),
                )
            }
            b'u' => {
                let parts: Vec<&str> = text.splitn(11, ' ').collect();
                if parts.len() != 11 {
                    return Err(invalid_status());
                }
                (
                    parts[1],
                    parts[2] != "N...",
                    parts[10].to_owned(),
                    None,
                    "conflicted".to_owned(),
                )
            }
            b'?' => (
                "??",
                false,
                text[2..].to_owned(),
                None,
                "untracked".to_owned(),
            ),
            _ => return Err(invalid_status()),
        };
        validate_relative_path(&path)?;
        if let Some(old) = &old_path {
            validate_relative_path(old)?;
        }
        let metadata = file_metadata(root, &path)?;
        let mut hasher = Sha256::new();
        hasher.update(path.as_bytes());
        if let Some(old) = &old_path {
            hasher.update(old.as_bytes());
        }
        hasher.update(&metadata.content);
        hasher.update(metadata.mode.as_bytes());
        files.push(FileChange {
            path,
            old_path,
            kind,
            binary: metadata.binary,
            size: metadata.size,
            mode: metadata.mode,
            fingerprint: format!("{:x}", hasher.finalize()),
            staged: xy
                .as_bytes()
                .first()
                .is_some_and(|value| *value != b'.' && *value != b'?'),
            conflicted: prefix == b'u' || xy.contains('U') || xy == "AA" || xy == "DD",
            submodule,
        });
        index += 1;
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

struct FileMetadata {
    content: Vec<u8>,
    size: u64,
    mode: String,
    binary: bool,
}

fn file_metadata(root: &Path, relative: &str) -> Result<FileMetadata, WorkspaceError> {
    let path = root.join(relative);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Ok(FileMetadata {
                content: b"<deleted>".to_vec(),
                size: 0,
                mode: "deleted".into(),
                binary: false,
            })
        }
    };
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(&path).map_err(|_| invalid_status())?;
        let content = target.to_string_lossy().as_bytes().to_vec();
        return Ok(FileMetadata {
            size: content.len() as u64,
            binary: false,
            content,
            mode: "symlink".into(),
        });
    }
    if metadata.is_dir() {
        return Ok(FileMetadata {
            content: b"<submodule>".to_vec(),
            size: 0,
            mode: "submodule".into(),
            binary: false,
        });
    }
    let content = std::fs::read(&path).map_err(|_| invalid_status())?;
    let binary = content.iter().take(8192).any(|byte| *byte == 0);
    Ok(FileMetadata {
        size: metadata.len(),
        content,
        mode: "file".into(),
        binary,
    })
}

fn document_from_bytes(change: &FileChange, bytes: Vec<u8>) -> DiffDocument {
    let binary = change.binary || bytes.windows(12).any(|window| window == b"Binary files");
    let oversized =
        bytes.len() > DIFF_LIMIT || (bytes.is_empty() && change.size > DIFF_LIMIT as u64);
    let visible = &bytes[..bytes.len().min(DIFF_LIMIT)];
    DiffDocument {
        path: change.path.clone(),
        old_path: change.old_path.clone(),
        binary,
        oversized,
        truncated: bytes.len() > DIFF_LIMIT,
        text: (!binary).then(|| String::from_utf8_lossy(visible).into_owned()),
        bytes: change.size,
    }
}

fn oversized_document(change: &FileChange) -> DiffDocument {
    DiffDocument {
        path: change.path.clone(),
        old_path: change.old_path.clone(),
        binary: change.binary,
        oversized: true,
        truncated: true,
        text: None,
        bytes: change.size,
    }
}

fn render_untracked(path: &str, bytes: &[u8]) -> Vec<u8> {
    if bytes.iter().take(8192).any(|byte| *byte == 0) {
        return Vec::new();
    }
    let mut rendered = format!("--- /dev/null\n+++ b/{path}\n").into_bytes();
    for line in String::from_utf8_lossy(bytes).lines() {
        rendered.extend_from_slice(b"+");
        rendered.extend_from_slice(line.as_bytes());
        rendered.push(b'\n');
    }
    rendered
}

fn checkpoint_path_args(
    files: &[FileChange],
    selection: &[CheckpointSelection],
) -> Result<Vec<String>, WorkspaceError> {
    let by_path: BTreeMap<&str, &FileChange> = files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let mut path_args = Vec::new();
    for selected in selection {
        let file = by_path
            .get(selected.path.as_str())
            .copied()
            .ok_or_else(|| {
                workspace_error(
                    "GIT_SELECTION_STALE",
                    "Selected path vanished during checkpoint validation",
                )
            })?;
        if let Some(old_path) = &file.old_path {
            path_args.push(old_path.clone());
        }
        path_args.push(file.path.clone());
    }
    path_args.sort();
    path_args.dedup();
    Ok(path_args)
}

fn status_kind(xy: &str) -> String {
    let value = xy.chars().find(|value| *value != '.').unwrap_or('M');
    match value {
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        'T' => "mode_changed",
        _ => "modified",
    }
    .into()
}

fn validate_relative_path(value: &str) -> Result<(), WorkspaceError> {
    let path = Path::new(value);
    if value.is_empty() || path.is_absolute() || value.contains('\0') || path.components().any(|part| !matches!(part, Component::Normal(_))) || path.components().any(|part| matches!(part, Component::Normal(name) if name.to_string_lossy().eq_ignore_ascii_case(".git"))) {
        return Err(workspace_error("GIT_INVALID_SELECTION", "Selected path is outside the task worktree"));
    }
    Ok(())
}

fn validate_checkpoint_message(value: &str) -> Result<(), WorkspaceError> {
    let message = value.trim();
    let allowed = [
        "feat(",
        "fix(",
        "docs(",
        "test(",
        "chore(",
        "refactor(",
        "build(",
        "ci(",
    ];
    if message.len() > 240
        || !allowed.iter().any(|prefix| message.starts_with(prefix))
        || !message.contains("): ")
        || !message.contains("[GAG-")
        || message.contains('\n')
        || message.contains('\r')
    {
        return Err(workspace_error(
            "GIT_INVALID_MESSAGE",
            "Checkpoint message must be Conventional Commit format and include [GAG-###]",
        ));
    }
    Ok(())
}

fn first_line(bytes: &[u8]) -> Result<String, WorkspaceError> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|value| value.lines().next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            workspace_error(
                "GIT_INVALID_OUTPUT",
                "Git returned incomplete checkpoint metadata",
            )
        })
}

fn invalid_status() -> WorkspaceError {
    workspace_error("GIT_INVALID_OUTPUT", "Git returned invalid status metadata")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_validation_rejects_escape_and_git_metadata() {
        assert!(validate_relative_path("src/main.rs").is_ok());
        assert!(validate_relative_path("../secret").is_err());
        assert!(validate_relative_path(".git/index").is_err());
    }

    #[test]
    fn message_requires_conventional_gag_reference() {
        assert!(validate_checkpoint_message("chore(GAG-012): save review [GAG-012]").is_ok());
        assert!(validate_checkpoint_message("save stuff").is_err());
    }

    #[test]
    fn porcelain_parser_preserves_spaces_in_paths() {
        let root = std::env::temp_dir().join(format!("gag-012-space-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("my file.txt"), "changed\n").unwrap();
        let raw = b"1 .M N... 100644 100644 100644 abc123 def456 my file.txt\0";
        let files = parse_status(&root, raw).unwrap();
        assert_eq!(files[0].path, "my file.txt");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn vanished_selection_returns_stale_instead_of_panicking() {
        let selection = [CheckpointSelection {
            path: "vanished.txt".into(),
            fingerprint: "fingerprint".into(),
        }];
        let error = checkpoint_path_args(&[], &selection).unwrap_err();
        assert_eq!(error.code, "GIT_SELECTION_STALE");
    }
}
