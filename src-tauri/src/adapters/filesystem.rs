//! Workspace-scoped filesystem adapter used by ACP client callbacks.
//!
//! The agent receives file contents only after the requested path is
//! canonicalized beneath the session workspace. File contents and absolute
//! paths never enter diagnostics or Renderer events.

use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactSaveIoError {
    Conflict,
    Rejected(&'static str),
    Failed(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactFileExpectation<'a> {
    pub bytes: u64,
    pub sha256: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSaveIoSuccess {
    pub target_name: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct FileDigest {
    bytes: u64,
    sha256: String,
}

#[derive(Debug, PartialEq, Eq)]
struct TargetSnapshot {
    bytes: u64,
    modified: Option<std::time::SystemTime>,
    readonly: bool,
}

impl From<&std::fs::Metadata> for TargetSnapshot {
    fn from(metadata: &std::fs::Metadata) -> Self {
        Self {
            bytes: metadata.len(),
            modified: metadata.modified().ok(),
            readonly: metadata.permissions().readonly(),
        }
    }
}

struct SaveTemporary {
    path: PathBuf,
    file: File,
}

pub fn save_managed_artifact(
    source_path: &Path,
    managed_root: &Path,
    target_path: &str,
    expectation: &ArtifactFileExpectation<'_>,
    overwrite: bool,
    protected_paths: &[PathBuf],
) -> Result<ArtifactSaveIoSuccess, ArtifactSaveIoError> {
    save_managed_artifact_with_copy(
        source_path,
        managed_root,
        target_path,
        expectation,
        overwrite,
        protected_paths,
        copy_verified_source,
    )
}

fn save_managed_artifact_with_copy<C>(
    source_path: &Path,
    managed_root: &Path,
    target_path: &str,
    expectation: &ArtifactFileExpectation<'_>,
    overwrite: bool,
    protected_paths: &[PathBuf],
    copy: C,
) -> Result<ArtifactSaveIoSuccess, ArtifactSaveIoError>
where
    C: FnOnce(&Path, &ArtifactFileExpectation<'_>, &mut File) -> std::io::Result<FileDigest>,
{
    let source = validate_managed_source(source_path, managed_root, expectation)?;
    let target = validate_artifact_save_target(target_path, managed_root, protected_paths)?;
    let target_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    let mut target_snapshot = None;
    if let Ok(metadata) = std::fs::symlink_metadata(&target) {
        if metadata.is_dir() || !metadata.is_file() || is_link_or_reparse(&metadata) {
            return Err(ArtifactSaveIoError::Rejected(
                "目标不是可安全替换的普通文件",
            ));
        }
        if !overwrite {
            return Err(ArtifactSaveIoError::Conflict);
        }
        target_snapshot = Some(TargetSnapshot::from(&metadata));
    } else if target.exists() {
        return Err(ArtifactSaveIoError::Rejected("无法安全检查目标位置"));
    }

    let temporary = create_save_temporary(&target)
        .map_err(|_| ArtifactSaveIoError::Failed("无法在目标目录创建临时文件"))?;
    let temporary_path = temporary.path.clone();
    let mut temporary_file = temporary.file;
    let copy_result = copy(&source, expectation, &mut temporary_file).and_then(|digest| {
        temporary_file.flush()?;
        temporary_file.sync_all()?;
        Ok(digest)
    });
    drop(temporary_file);
    let digest = match copy_result {
        Ok(value) => value,
        Err(_) => {
            let _ = std::fs::remove_file(&temporary_path);
            return Err(ArtifactSaveIoError::Failed(
                "复制制品失败；已有目标未被修改",
            ));
        }
    };
    if digest.bytes != expectation.bytes || digest.sha256 != expectation.sha256 {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(ArtifactSaveIoError::Rejected("源制品完整性校验失败"));
    }

    if overwrite {
        match std::fs::symlink_metadata(&target) {
            Ok(metadata)
                if metadata.is_file()
                    && !is_link_or_reparse(&metadata)
                    && target_snapshot.as_ref() == Some(&TargetSnapshot::from(&metadata)) => {}
            Ok(_) | Err(_) => {
                let _ = std::fs::remove_file(&temporary_path);
                return Err(ArtifactSaveIoError::Conflict);
            }
        }
    }

    let commit = if overwrite {
        replace_file_atomically(&temporary_path, &target)
    } else {
        commit_new_file_atomically(&temporary_path, &target)
    };
    if commit.is_err() {
        let conflict = !overwrite && target.exists();
        let _ = std::fs::remove_file(&temporary_path);
        return Err(if conflict {
            ArtifactSaveIoError::Conflict
        } else {
            ArtifactSaveIoError::Failed("无法完成原子保存；已有目标未被修改")
        });
    }
    Ok(ArtifactSaveIoSuccess { target_name })
}

pub fn reveal_saved_artifact(
    source_path: &Path,
    managed_root: &Path,
    target_path: &str,
    expectation: &ArtifactFileExpectation<'_>,
    protected_paths: &[PathBuf],
) -> Result<(), ArtifactSaveIoError> {
    validate_managed_source(source_path, managed_root, expectation)?;
    let target = validate_artifact_save_target(target_path, managed_root, protected_paths)?;
    let metadata = std::fs::symlink_metadata(&target)
        .map_err(|_| ArtifactSaveIoError::Rejected("保存结果已不可用"))?;
    if !metadata.is_file() || is_link_or_reparse(&metadata) {
        return Err(ArtifactSaveIoError::Rejected("保存结果不是安全的普通文件"));
    }
    let mut file = open_file_no_follow(&target)
        .map_err(|_| ArtifactSaveIoError::Rejected("保存结果已不可用"))?;
    let digest =
        digest_reader(&mut file).map_err(|_| ArtifactSaveIoError::Rejected("保存结果无法验证"))?;
    if digest.bytes != expectation.bytes || digest.sha256 != expectation.sha256 {
        return Err(ArtifactSaveIoError::Rejected("保存结果与受管制品不再一致"));
    }
    std::process::Command::new("explorer.exe")
        .arg("/select,")
        .arg(&target)
        .spawn()
        .map_err(|_| ArtifactSaveIoError::Failed("无法在资源管理器中显示保存结果"))?;
    Ok(())
}

fn validate_managed_source(
    source_path: &Path,
    managed_root: &Path,
    expectation: &ArtifactFileExpectation<'_>,
) -> Result<PathBuf, ArtifactSaveIoError> {
    let root = managed_root
        .canonicalize()
        .map_err(|_| ArtifactSaveIoError::Rejected("受管制品存储不可用"))?;
    let metadata = std::fs::symlink_metadata(source_path)
        .map_err(|_| ArtifactSaveIoError::Rejected("源制品已缺失"))?;
    if !metadata.is_file() || is_link_or_reparse(&metadata) {
        return Err(ArtifactSaveIoError::Rejected("源制品不是安全的受管文件"));
    }
    let source = source_path
        .canonicalize()
        .map_err(|_| ArtifactSaveIoError::Rejected("源制品已缺失"))?;
    if !source.starts_with(&root) {
        return Err(ArtifactSaveIoError::Rejected("源制品已逃逸受管存储"));
    }
    let mut source_file = open_file_no_follow(&source)
        .map_err(|_| ArtifactSaveIoError::Rejected("源制品不是安全的受管文件"))?;
    let digest = digest_reader(&mut source_file)
        .map_err(|_| ArtifactSaveIoError::Rejected("源制品完整性校验失败"))?;
    if digest.bytes != expectation.bytes || digest.sha256 != expectation.sha256 {
        return Err(ArtifactSaveIoError::Rejected("源制品完整性校验失败"));
    }
    Ok(source)
}

fn validate_artifact_save_target(
    raw: &str,
    managed_root: &Path,
    protected_paths: &[PathBuf],
) -> Result<PathBuf, ArtifactSaveIoError> {
    let raw_path = Path::new(raw);
    if raw.trim().is_empty()
        || !raw_path.is_absolute()
        || raw_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(ArtifactSaveIoError::Rejected(
            "目标路径必须是系统对话框返回的绝对路径",
        ));
    }
    let raw_lower = raw.to_ascii_lowercase();
    if raw_lower.starts_with(r"\\.\")
        || raw_lower.starts_with(r"\\?\")
        || raw_lower.contains("::$data")
    {
        return Err(ArtifactSaveIoError::Rejected(
            "不允许保存到设备路径或备用数据流",
        ));
    }
    reject_managed_components(raw_path)?;
    let file_name = raw_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or(ArtifactSaveIoError::Rejected("目标文件名无效"))?;
    let device_stem = file_name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    let reserved = matches!(
        device_stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$"
    ) || (device_stem.len() == 4
        && (device_stem.starts_with("COM") || device_stem.starts_with("LPT"))
        && matches!(device_stem.as_bytes()[3], b'1'..=b'9'));
    if reserved || file_name.contains(':') || file_name.ends_with([' ', '.']) {
        return Err(ArtifactSaveIoError::Rejected(
            "目标文件名是 Windows 保留名称",
        ));
    }
    let parent = raw_path
        .parent()
        .ok_or(ArtifactSaveIoError::Rejected("目标目录无效"))?;
    let parent_metadata = std::fs::symlink_metadata(parent)
        .map_err(|_| ArtifactSaveIoError::Rejected("目标目录不存在或不可访问"))?;
    if !parent_metadata.is_dir() {
        return Err(ArtifactSaveIoError::Rejected("目标父路径不是目录"));
    }
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| ArtifactSaveIoError::Rejected("目标目录不存在或不可访问"))?;
    reject_managed_components(&canonical_parent)?;
    let target = canonical_parent.join(file_name);
    let canonical_root = managed_root
        .canonicalize()
        .map_err(|_| ArtifactSaveIoError::Rejected("受管制品存储不可用"))?;
    if target.starts_with(&canonical_root) {
        return Err(ArtifactSaveIoError::Rejected("不允许覆盖应用受管数据"));
    }
    if protected_paths.iter().any(|protected| {
        normalize_intended_path(protected)
            .is_some_and(|normalized| paths_equivalent(&target, &normalized))
    }) {
        return Err(ArtifactSaveIoError::Rejected("不允许覆盖应用受管数据"));
    }
    if let Ok(canonical_target) = target.canonicalize() {
        if canonical_target.starts_with(&canonical_root)
            || protected_paths.iter().any(|protected| {
                protected
                    .canonicalize()
                    .ok()
                    .is_some_and(|path| path == canonical_target)
            })
        {
            return Err(ArtifactSaveIoError::Rejected("不允许覆盖应用受管数据"));
        }
    }
    Ok(target)
}

fn normalize_intended_path(path: &Path) -> Option<PathBuf> {
    let file_name = path.file_name()?;
    let parent = path.parent()?.canonicalize().ok()?;
    Some(parent.join(file_name))
}

#[cfg(windows)]
fn paths_equivalent(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(not(windows))]
fn paths_equivalent(left: &Path, right: &Path) -> bool {
    left == right
}

fn reject_managed_components(path: &Path) -> Result<(), ArtifactSaveIoError> {
    if path.components().any(|component| {
        component
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(".grok-acp-gui")
    }) {
        Err(ArtifactSaveIoError::Rejected("不允许覆盖应用受管数据"))
    } else {
        Ok(())
    }
}

fn copy_verified_source(
    source: &Path,
    expectation: &ArtifactFileExpectation<'_>,
    destination: &mut File,
) -> std::io::Result<FileDigest> {
    let mut input = open_file_no_follow(source)?;
    let mut hasher = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        copied = copied.saturating_add(read as u64);
        if copied > expectation.bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "managed source changed while saving",
            ));
        }
        hasher.update(&buffer[..read]);
        destination.write_all(&buffer[..read])?;
    }
    Ok(FileDigest {
        bytes: copied,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn digest_reader(reader: &mut File) -> std::io::Result<FileDigest> {
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes = bytes.saturating_add(read as u64);
        hasher.update(&buffer[..read]);
    }
    Ok(FileDigest {
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn create_save_temporary(target: &Path) -> std::io::Result<SaveTemporary> {
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "target has no parent")
    })?;
    for _ in 0..8 {
        let path = parent.join(format!(".gag-save-{}.tmp", uuid::Uuid::new_v4()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok(SaveTemporary { path, file }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "unable to allocate unique temporary file",
    ))
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn open_file_no_follow(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(0x0020_0000)
        .open(path)
}

#[cfg(not(windows))]
fn open_file_no_follow(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

#[cfg(windows)]
fn commit_new_file_atomically(temporary: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, target)
}

#[cfg(not(windows))]
fn commit_new_file_atomically(temporary: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::hard_link(temporary, target)?;
    std::fs::remove_file(temporary)
}

#[cfg(windows)]
fn replace_file_atomically(temporary: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};

    let target_wide: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let temporary_wide: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let replaced = unsafe {
        ReplaceFileW(
            target_wide.as_ptr(),
            temporary_wide.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if replaced == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file_atomically(temporary: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, target)
}

#[cfg(test)]
mod artifact_save_tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        managed: PathBuf,
        export: PathBuf,
        source: PathBuf,
        sha256: String,
        bytes: u64,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "gag-010c-filesystem-{label}-{}",
                uuid::Uuid::new_v4()
            ));
            let managed = root.join("managed");
            let export = root.join("export");
            std::fs::create_dir_all(&managed).unwrap();
            std::fs::create_dir_all(&export).unwrap();
            let source = managed.join("source.png");
            let content = b"managed artifact content";
            std::fs::write(&source, content).unwrap();
            Self {
                root,
                managed,
                export,
                source,
                sha256: format!("{:x}", Sha256::digest(content)),
                bytes: content.len() as u64,
            }
        }

        fn expectation(&self) -> ArtifactFileExpectation<'_> {
            ArtifactFileExpectation {
                bytes: self.bytes,
                sha256: &self.sha256,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn simulated_disk_full_cleans_temporary_and_creates_no_target() {
        let fixture = Fixture::new("disk-full");
        let target = fixture.export.join("disk-full.png");
        let result = save_managed_artifact_with_copy(
            &fixture.source,
            &fixture.managed,
            &target.to_string_lossy(),
            &fixture.expectation(),
            false,
            &[],
            |_, _, _| Err(std::io::Error::from_raw_os_error(112)),
        );
        assert!(matches!(result, Err(ArtifactSaveIoError::Failed(_))));
        assert!(!target.exists());
        assert_eq!(temporary_count(&fixture.export), 0);
    }

    #[test]
    fn interrupted_copy_cleans_temporary_and_preserves_old_target() {
        let fixture = Fixture::new("interrupted");
        let target = fixture.export.join("existing.png");
        std::fs::write(&target, b"old content").unwrap();
        let result = save_managed_artifact_with_copy(
            &fixture.source,
            &fixture.managed,
            &target.to_string_lossy(),
            &fixture.expectation(),
            true,
            &[],
            |_, _, output| {
                output.write_all(b"partial copy")?;
                Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "simulated interrupted copy",
                ))
            },
        );
        assert!(matches!(result, Err(ArtifactSaveIoError::Failed(_))));
        assert_eq!(std::fs::read(&target).unwrap(), b"old content");
        assert_eq!(temporary_count(&fixture.export), 0);
    }

    #[test]
    fn target_changed_during_copy_is_not_overwritten() {
        let fixture = Fixture::new("target-race");
        let target = fixture.export.join("existing.png");
        std::fs::write(&target, b"confirmed content").unwrap();
        let target_during_copy = target.clone();
        let result = save_managed_artifact_with_copy(
            &fixture.source,
            &fixture.managed,
            &target.to_string_lossy(),
            &fixture.expectation(),
            true,
            &[],
            move |source, expectation, output| {
                let digest = copy_verified_source(source, expectation, output)?;
                std::fs::write(&target_during_copy, b"new external content").unwrap();
                Ok(digest)
            },
        );
        assert_eq!(result, Err(ArtifactSaveIoError::Conflict));
        assert_eq!(std::fs::read(&target).unwrap(), b"new external content");
    }

    #[test]
    fn device_ads_reserved_and_real_protected_paths_are_rejected() {
        let fixture = Fixture::new("protected");
        for path in [
            r"\\.\NUL",
            r"\\?\C:\unsafe.png",
            &fixture.export.join("image.png:stream").to_string_lossy(),
            &fixture.export.join("NUL.png").to_string_lossy(),
        ] {
            let result = save_managed_artifact(
                &fixture.source,
                &fixture.managed,
                path,
                &fixture.expectation(),
                true,
                &[],
            );
            assert!(matches!(result, Err(ArtifactSaveIoError::Rejected(_))));
        }

        let database = fixture.export.join("renamed-application-data.bin");
        std::fs::write(&database, b"database content").unwrap();
        let result = save_managed_artifact(
            &fixture.source,
            &fixture.managed,
            &database.to_string_lossy(),
            &fixture.expectation(),
            true,
            std::slice::from_ref(&database),
        );
        assert!(matches!(result, Err(ArtifactSaveIoError::Rejected(_))));
        assert_eq!(std::fs::read(database).unwrap(), b"database content");

        let absent_sidecar = fixture.export.join("renamed-application-data.bin-wal");
        let result = save_managed_artifact(
            &fixture.source,
            &fixture.managed,
            &absent_sidecar.to_string_lossy(),
            &fixture.expectation(),
            false,
            std::slice::from_ref(&absent_sidecar),
        );
        assert!(matches!(result, Err(ArtifactSaveIoError::Rejected(_))));
        assert!(!absent_sidecar.exists());
    }

    #[cfg(windows)]
    #[test]
    fn escaping_source_and_target_junctions_are_rejected() {
        let fixture = Fixture::new("junction");
        let target_alias = fixture.root.join("target-alias");
        create_junction(&target_alias, &fixture.managed);
        let target = target_alias.join("overwrite.png");
        let result = save_managed_artifact(
            &fixture.source,
            &fixture.managed,
            &target.to_string_lossy(),
            &fixture.expectation(),
            false,
            &[],
        );
        assert!(matches!(result, Err(ArtifactSaveIoError::Rejected(_))));

        let external = fixture.root.join("external");
        std::fs::create_dir_all(&external).unwrap();
        let external_source = external.join("source.png");
        std::fs::write(&external_source, b"managed artifact content").unwrap();
        let source_alias = fixture.managed.join("source-alias");
        create_junction(&source_alias, &external);
        let escaped_source = source_alias.join("source.png");
        let result = save_managed_artifact(
            &escaped_source,
            &fixture.managed,
            &fixture.export.join("escaped.png").to_string_lossy(),
            &fixture.expectation(),
            false,
            &[],
        );
        assert!(matches!(result, Err(ArtifactSaveIoError::Rejected(_))));
    }

    fn temporary_count(directory: &Path) -> usize {
        std::fs::read_dir(directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".gag-save-")
            })
            .count()
    }

    #[cfg(windows)]
    fn create_junction(link: &Path, target: &Path) {
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()
            .expect("start mklink");
        assert!(status.success(), "junction fixture must be available");
    }
}
