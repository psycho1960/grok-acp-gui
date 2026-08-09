//! Workspace-scoped filesystem adapter used by ACP client callbacks.
//!
//! The agent receives file contents only after the requested path is
//! canonicalized beneath the session workspace. File contents and absolute
//! paths never enter diagnostics or Renderer events.

use std::path::{Path, PathBuf};

const MAX_TEXT_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemError {
    message: &'static str,
}

impl FilesystemError {
    fn new(message: &'static str) -> Self {
        Self { message }
    }

    pub fn safe_message(&self) -> &'static str {
        self.message
    }
}

impl std::fmt::Display for FilesystemError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for FilesystemError {}

fn normalize_canonical_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

/// Resolve an existing directory to its canonical filesystem identity.
/// Callers use the returned path for relationship checks so junctions and
/// symlinks cannot disguise the project checkout as another workspace.
pub fn canonicalize_existing_directory(path: &Path) -> Result<PathBuf, FilesystemError> {
    if !path.is_absolute() {
        return Err(FilesystemError::new("Workspace path must be absolute"));
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| FilesystemError::new("Workspace is not accessible"))?;
    if !canonical.is_dir() {
        return Err(FilesystemError::new("Workspace is not a directory"));
    }
    Ok(normalize_canonical_path(canonical))
}

fn canonicalize_existing_file(path: &Path) -> Result<PathBuf, FilesystemError> {
    if !path.is_absolute() {
        return Err(FilesystemError::new(
            "Workspace metadata path must be absolute",
        ));
    }
    let canonical = normalize_canonical_path(
        std::fs::canonicalize(path)
            .map_err(|_| FilesystemError::new("Workspace metadata is not accessible"))?,
    );
    if !canonical.is_file() {
        return Err(FilesystemError::new("Workspace metadata is not a file"));
    }
    Ok(canonical)
}

fn read_small_path_file(path: &Path) -> Result<String, FilesystemError> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| FilesystemError::new("Workspace metadata is not accessible"))?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 {
        return Err(FilesystemError::new("Workspace metadata is invalid"));
    }
    std::fs::read_to_string(path)
        .map(|value| value.trim().to_string())
        .map_err(|_| FilesystemError::new("Workspace metadata is not readable text"))
}

fn resolve_metadata_path(base: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

/// Verify the on-disk linked-worktree registration without invoking Git.
/// The worktree marker must point into `<repo>/.git/worktrees/*`, its
/// `commondir` must resolve back to the same repository, and its back-reference
/// must resolve to this worktree's `.git` marker.
pub fn validate_linked_worktree_metadata(
    repo_root: &Path,
    worktree_root: &Path,
) -> Result<(), FilesystemError> {
    let repo_root = canonicalize_existing_directory(repo_root)?;
    let worktree_root = canonicalize_existing_directory(worktree_root)?;
    let common_git_dir = canonicalize_existing_directory(&repo_root.join(".git"))?;
    let registrations = canonicalize_existing_directory(&common_git_dir.join("worktrees"))?;

    let marker = canonicalize_existing_file(&worktree_root.join(".git"))?;
    let marker_text = read_small_path_file(&marker)?;
    let git_dir_text = marker_text
        .strip_prefix("gitdir:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| FilesystemError::new("Worktree gitdir marker is invalid"))?;
    let registered_git_dir =
        canonicalize_existing_directory(&resolve_metadata_path(&worktree_root, git_dir_text))?;
    if registered_git_dir.parent() != Some(registrations.as_path()) {
        return Err(FilesystemError::new(
            "Worktree is not registered beneath the repository metadata",
        ));
    }

    let commondir_text = read_small_path_file(&registered_git_dir.join("commondir"))?;
    let registered_common_dir = canonicalize_existing_directory(&resolve_metadata_path(
        &registered_git_dir,
        &commondir_text,
    ))?;
    if registered_common_dir != common_git_dir {
        return Err(FilesystemError::new(
            "Worktree repository identity does not match",
        ));
    }

    let backref_text = read_small_path_file(&registered_git_dir.join("gitdir"))?;
    let registered_marker =
        canonicalize_existing_file(&resolve_metadata_path(&registered_git_dir, &backref_text))?;
    if registered_marker != marker {
        return Err(FilesystemError::new(
            "Worktree registration does not point back to the workspace",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct WorkspaceFilesystem {
    root: PathBuf,
}

impl WorkspaceFilesystem {
    pub fn new(root: &Path) -> Result<Self, FilesystemError> {
        let root = canonicalize_existing_directory(root)?;
        Ok(Self { root })
    }

    pub fn read_text_file(
        &self,
        requested_path: &Path,
        line: Option<u32>,
        limit: Option<u32>,
    ) -> Result<String, FilesystemError> {
        if !requested_path.is_absolute() {
            return Err(FilesystemError::new("File path must be absolute"));
        }
        if line == Some(0) {
            return Err(FilesystemError::new("Line must be one-based"));
        }

        let path = normalize_canonical_path(
            std::fs::canonicalize(requested_path)
                .map_err(|_| FilesystemError::new("File is not accessible"))?,
        );
        if !path.starts_with(&self.root) {
            return Err(FilesystemError::new("File is outside the workspace"));
        }

        let metadata = std::fs::metadata(&path)
            .map_err(|_| FilesystemError::new("File metadata is not accessible"))?;
        if !metadata.is_file() {
            return Err(FilesystemError::new("Requested path is not a file"));
        }
        if metadata.len() > MAX_TEXT_FILE_BYTES {
            return Err(FilesystemError::new("Text file exceeds the read limit"));
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|_| FilesystemError::new("File is not valid readable text"))?;
        if line.is_none() && limit.is_none() {
            return Ok(content);
        }

        let start = line.unwrap_or(1).saturating_sub(1) as usize;
        let count = limit.unwrap_or(u32::MAX) as usize;
        Ok(content
            .lines()
            .skip(start)
            .take(count)
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
