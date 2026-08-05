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

#[derive(Debug, Clone)]
pub struct WorkspaceFilesystem {
    root: PathBuf,
}

impl WorkspaceFilesystem {
    pub fn new(root: &Path) -> Result<Self, FilesystemError> {
        let root = std::fs::canonicalize(root)
            .map_err(|_| FilesystemError::new("Workspace is not accessible"))?;
        if !root.is_dir() {
            return Err(FilesystemError::new("Workspace is not a directory"));
        }
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

        let path = std::fs::canonicalize(requested_path)
            .map_err(|_| FilesystemError::new("File is not accessible"))?;
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
