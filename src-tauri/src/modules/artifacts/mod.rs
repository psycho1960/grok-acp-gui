//! MOD-ARTIFACTS — managed image import and ACP-only image resolution.
//!
//! The renderer receives descriptors only. Image bytes stay in a task's
//! managed workspace directory until the backend serialises an ACP prompt.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::adapters::filesystem::{
    reveal_saved_artifact, save_managed_artifact, ArtifactFileExpectation, ArtifactSaveIoError,
};
use crate::bridge::types::TaskId;
use crate::domain::error::{codes, DomainError};
use crate::domain::types::{utc_now, AttachmentId, AttachmentRecord};
use crate::modules::persistence::Repository;

pub const MAX_IMAGES_PER_PROMPT: usize = 20;
pub const MAX_IMAGE_BYTES: u64 = 200 * 1024 * 1024;
pub const MAX_PREVIEW_PIXELS: u64 = 80_000_000;
/// Per-task ceiling for expendable cache files. Referenced original artifacts
/// are never evicted; only interrupted-import temporary files or other
/// unreferenced cache entries are eligible for least-recently-modified cleanup.
pub const CACHE_QUOTA_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDescriptor {
    pub artifact_id: String,
    pub display_name: String,
    pub mime_type: String,
    pub bytes: u64,
    pub state: String,
    pub preview_capability: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedImage {
    pub descriptor: ArtifactDescriptor,
    pub bytes: Vec<u8>,
}

/// A clipboard image blob crossing the bridge (base64, no filesystem path).
#[derive(Debug, Clone)]
pub struct BlobImage {
    pub display_name: String,
    pub base64_data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactSaveStatus {
    Saved,
    Cancelled,
    Conflict,
    Rejected,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSaveResult {
    pub status: ArtifactSaveStatus,
    pub artifact_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub trait ArtifactService: Send + Sync {
    fn import_images(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        paths: &[String],
    ) -> Result<Vec<ArtifactDescriptor>, DomainError>;

    /// Import clipboard image blobs (no filesystem path) through the same
    /// validation and managed-storage pipeline as path-based imports.
    fn import_blob_images(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        blobs: &[BlobImage],
    ) -> Result<Vec<ArtifactDescriptor>, DomainError>;

    fn resolve_images(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        artifact_ids: &[String],
    ) -> Result<Vec<ResolvedImage>, DomainError>;

    /// Accept an ACP-announced *relative* workspace path, then copy it through
    /// the same validation and managed-storage pipeline as a user import.
    fn register_agent_artifact(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        relative_path: &str,
    ) -> Result<ArtifactDescriptor, DomainError>;

    fn list(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
    ) -> Result<Vec<ArtifactDescriptor>, DomainError>;

    fn reveal(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        artifact_id: &str,
    ) -> Result<(), DomainError>;

    fn save(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        artifact_id: &str,
        target_path: &str,
        overwrite: bool,
    ) -> ArtifactSaveResult;

    fn reveal_saved(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        artifact_id: &str,
        target_path: &str,
    ) -> Result<(), DomainError>;

    fn enforce_cache_quota(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
    ) -> Result<(), DomainError>;
}

/// Artifacts are deliberately rooted in the task's current workspace, not in
/// a caller-supplied path. This lets the ACP process use the same workspace
/// while preventing it from receiving the original selected-file path.
#[derive(Default)]
pub struct ManagedArtifactService {
    protected_paths: Vec<PathBuf>,
}

impl ManagedArtifactService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_protected_database(database_path: PathBuf) -> Self {
        let database = database_path.to_string_lossy().into_owned();
        Self {
            protected_paths: vec![
                database_path,
                PathBuf::from(format!("{database}-wal")),
                PathBuf::from(format!("{database}-shm")),
                PathBuf::from(format!("{database}-journal")),
            ],
        }
    }

    fn workspace_for_task(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
    ) -> Result<PathBuf, DomainError> {
        let task = repo.get_task(&task_id.0)?;
        let project = repo.get_project(&task.project_id.0)?;
        let base = repo
            .get_binding_by_task(&task_id.0)?
            .and_then(|binding| binding.cwd)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(project.path));
        base.canonicalize().map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Task workspace is unavailable",
            )
        })
    }

    fn root_for_task(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
    ) -> Result<PathBuf, DomainError> {
        let base = self.workspace_for_task(repo, task_id)?;
        let root = base.join(".grok-acp-gui").join("artifacts");
        std::fs::create_dir_all(&root).map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Unable to create managed artifact storage",
            )
        })?;
        let root = root.canonicalize().map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Managed artifact storage is unavailable",
            )
        })?;
        if !root.starts_with(&base) {
            return Err(DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Managed artifact storage escaped the task workspace",
            ));
        }
        Ok(root)
    }

    fn agent_workspace_file(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        relative_path: &str,
    ) -> Result<PathBuf, DomainError> {
        let relative = Path::new(relative_path);
        if relative.is_absolute()
            || relative_path.is_empty()
            || relative.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(DomainError::new(
                codes::ARTIFACT_NOT_FOUND,
                "Agent artifact path is outside the managed workspace",
            ));
        }
        let workspace = self.workspace_for_task(repo, task_id)?;
        let candidate = workspace.join(relative).canonicalize().map_err(|_| {
            DomainError::new(codes::ARTIFACT_NOT_FOUND, "Agent artifact is unavailable")
        })?;
        if !candidate.starts_with(&workspace) {
            return Err(DomainError::new(
                codes::ARTIFACT_NOT_FOUND,
                "Agent artifact path escaped the managed workspace",
            ));
        }
        Ok(candidate)
    }

    fn import_one(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        raw_path: &str,
    ) -> Result<ArtifactDescriptor, DomainError> {
        let raw_source = PathBuf::from(raw_path);
        if std::fs::symlink_metadata(&raw_source)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(true)
        {
            return Err(DomainError::new(
                codes::ARTIFACT_INVALID_FORMAT,
                "Symbolic-link images cannot be imported",
            ));
        }
        let source = raw_source.canonicalize().map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_NOT_FOUND,
                "The selected image is no longer available",
            )
        })?;
        let metadata = std::fs::metadata(&source).map_err(|_| {
            DomainError::new(codes::ARTIFACT_NOT_FOUND, "Unable to read image metadata")
        })?;
        if !metadata.is_file() {
            return Err(DomainError::new(
                codes::ARTIFACT_NOT_FOUND,
                "The selected image is not a file",
            ));
        }
        if metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
            return Err(DomainError::new(
                codes::ARTIFACT_TOO_LARGE,
                "Each image must be between 1 byte and 200 MiB",
            ));
        }
        let bytes = std::fs::read(&source).map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_NOT_FOUND,
                "Unable to read the selected image",
            )
        })?;
        if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(DomainError::new(
                codes::ARTIFACT_TOO_LARGE,
                "The image changed while importing",
            ));
        }
        let mime = detect_image_mime(&bytes).ok_or_else(|| {
            DomainError::new(
                codes::ARTIFACT_INVALID_FORMAT,
                "Unsupported or invalid image format",
            )
        })?;
        validate_extension(&source, mime)?;
        let source_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("image")
            .to_owned();
        self.import_bytes(repo, task_id, &bytes, &source_name)
    }

    /// Shared pipeline: validate bytes, dedupe by hash, write to the managed
    /// cache, and register the attachment. Used by path and blob imports.
    fn import_bytes(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        bytes: &[u8],
        source_name: &str,
    ) -> Result<ArtifactDescriptor, DomainError> {
        if bytes.is_empty() || bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(DomainError::new(
                codes::ARTIFACT_TOO_LARGE,
                "Each image must be between 1 byte and 200 MiB",
            ));
        }
        let mime = detect_image_mime(bytes).ok_or_else(|| {
            DomainError::new(
                codes::ARTIFACT_INVALID_FORMAT,
                "Unsupported or invalid image format",
            )
        })?;
        validate_dimensions(bytes, mime)?;

        let sha256 = format!("{:x}", Sha256::digest(bytes));
        if let Some(existing) = repo
            .list_attachments_by_task(&task_id.0)?
            .into_iter()
            .find(|item| item.sha256 == sha256)
        {
            return Ok(descriptor(&existing));
        }

        let root = self.root_for_task(repo, task_id)?;
        let shard = root.join(&sha256[..2]);
        std::fs::create_dir_all(&shard).map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Unable to create managed artifact directory",
            )
        })?;
        let cache_path = shard.join(format!("{}.{}", sha256, canonical_extension(mime)));
        if !cache_path.exists() {
            let temporary = shard.join(format!(".{}.{}.tmp", sha256, uuid::Uuid::new_v4()));
            std::fs::write(&temporary, bytes).map_err(|_| {
                DomainError::new(
                    codes::ARTIFACT_CACHE_MISSING,
                    "Unable to write managed image",
                )
            })?;
            std::fs::rename(&temporary, &cache_path).map_err(|_| {
                let _ = std::fs::remove_file(&temporary);
                DomainError::new(
                    codes::ARTIFACT_CACHE_MISSING,
                    "Unable to finalise managed image",
                )
            })?;
        }
        let cache_path = cache_path.canonicalize().map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Managed image is unavailable",
            )
        })?;
        if !cache_path.starts_with(&root) {
            return Err(DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Managed image escaped storage",
            ));
        }
        let source_name = sanitize_source_name(source_name, mime);
        let record = AttachmentRecord {
            id: AttachmentId::new(format!("artifact-{}", &sha256[..24])),
            task_id: task_id.clone(),
            sha256,
            mime: mime.into(),
            bytes: bytes.len() as u64,
            cache_path: cache_path.to_string_lossy().into_owned(),
            source_name,
            created_at: utc_now(),
        };
        repo.create_attachment(&record)?;
        Ok(descriptor(&record))
    }
}

impl ArtifactService for ManagedArtifactService {
    fn import_images(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        paths: &[String],
    ) -> Result<Vec<ArtifactDescriptor>, DomainError> {
        if paths.is_empty() || paths.len() > MAX_IMAGES_PER_PROMPT {
            return Err(DomainError::new(
                codes::ARTIFACT_TOO_LARGE,
                "A message may contain from 1 to 20 images",
            ));
        }
        let mut total = 0_u64;
        for path in paths {
            total = total.saturating_add(
                std::fs::metadata(path)
                    .map(|item| item.len())
                    .unwrap_or(MAX_IMAGE_BYTES + 1),
            );
        }
        if total > MAX_IMAGE_BYTES {
            return Err(DomainError::new(
                codes::ARTIFACT_TOO_LARGE,
                "Images total more than 200 MiB",
            ));
        }
        // A referenced artifact is an original for this task and must not be
        // evicted. Reclaim only orphan/temporary entries first, then reject
        // before writing when the task cache cannot hold another prompt.
        self.enforce_cache_quota(repo, task_id)?;
        let root = self.root_for_task(repo, task_id)?;
        let mut cache_files = Vec::new();
        collect_files(&root, &mut cache_files)?;
        let used = cache_files.iter().map(|(_, bytes, _)| *bytes).sum::<u64>();
        if !cache_has_capacity(used, total) {
            return Err(DomainError::new(
                codes::ARTIFACT_TOO_LARGE,
                "Task artifact cache is full; remove unused task artifacts before importing more images",
            ));
        }
        let imported = paths
            .iter()
            .map(|path| self.import_one(repo, task_id, path))
            .collect::<Result<Vec<_>, _>>()?;
        self.enforce_cache_quota(repo, task_id)?;
        Ok(imported)
    }

    fn import_blob_images(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        blobs: &[BlobImage],
    ) -> Result<Vec<ArtifactDescriptor>, DomainError> {
        if blobs.is_empty() || blobs.len() > MAX_IMAGES_PER_PROMPT {
            return Err(DomainError::new(
                codes::ARTIFACT_TOO_LARGE,
                "A message may contain from 1 to 20 images",
            ));
        }
        // Decode first so declared size can be checked before any writes.
        let mut decoded: Vec<(String, Vec<u8>)> = Vec::with_capacity(blobs.len());
        let mut total = 0_u64;
        for blob in blobs {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(blob.base64_data.trim())
                .map_err(|_| {
                    DomainError::new(
                        codes::ARTIFACT_INVALID_FORMAT,
                        "剪贴板图片数据已损坏，无法导入",
                    )
                })?;
            if bytes.is_empty() || bytes.len() as u64 > MAX_IMAGE_BYTES {
                return Err(DomainError::new(
                    codes::ARTIFACT_TOO_LARGE,
                    "Each image must be between 1 byte and 200 MiB",
                ));
            }
            total = total.saturating_add(bytes.len() as u64);
            if total > MAX_IMAGE_BYTES {
                return Err(DomainError::new(
                    codes::ARTIFACT_TOO_LARGE,
                    "Images total more than 200 MiB",
                ));
            }
            decoded.push((blob.display_name.clone(), bytes));
        }
        self.enforce_cache_quota(repo, task_id)?;
        let root = self.root_for_task(repo, task_id)?;
        let mut cache_files = Vec::new();
        collect_files(&root, &mut cache_files)?;
        let used = cache_files.iter().map(|(_, bytes, _)| *bytes).sum::<u64>();
        if !cache_has_capacity(used, total) {
            return Err(DomainError::new(
                codes::ARTIFACT_TOO_LARGE,
                "Task artifact cache is full; remove unused task artifacts before importing more images",
            ));
        }
        let imported = decoded
            .iter()
            .map(|(name, bytes)| self.import_bytes(repo, task_id, bytes, name))
            .collect::<Result<Vec<_>, _>>()?;
        self.enforce_cache_quota(repo, task_id)?;
        Ok(imported)
    }

    fn resolve_images(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        artifact_ids: &[String],
    ) -> Result<Vec<ResolvedImage>, DomainError> {
        if artifact_ids.len() > MAX_IMAGES_PER_PROMPT {
            return Err(DomainError::new(
                codes::ARTIFACT_TOO_LARGE,
                "A message may contain at most 20 images",
            ));
        }
        let root = self.root_for_task(repo, task_id)?;
        let mut total = 0_u64;
        artifact_ids
            .iter()
            .map(|artifact_id| {
                let record = repo.get_attachment(artifact_id).map_err(|_| {
                    DomainError::new(codes::ARTIFACT_NOT_FOUND, "Attachment was not found")
                })?;
                if record.task_id != *task_id {
                    return Err(DomainError::new(
                        codes::ARTIFACT_NOT_FOUND,
                        "Attachment does not belong to this task",
                    ));
                }
                let path = PathBuf::from(&record.cache_path)
                    .canonicalize()
                    .map_err(|_| {
                        DomainError::new(
                            codes::ARTIFACT_CACHE_MISSING,
                            "Managed attachment is missing",
                        )
                    })?;
                if !path.starts_with(&root) {
                    return Err(DomainError::new(
                        codes::ARTIFACT_CACHE_MISSING,
                        "Attachment escaped managed storage",
                    ));
                }
                let bytes = std::fs::read(path).map_err(|_| {
                    DomainError::new(
                        codes::ARTIFACT_CACHE_MISSING,
                        "Unable to read managed attachment",
                    )
                })?;
                total = total.saturating_add(bytes.len() as u64);
                if total > MAX_IMAGE_BYTES
                    || bytes.len() as u64 != record.bytes
                    || format!("{:x}", Sha256::digest(&bytes)) != record.sha256
                {
                    return Err(DomainError::new(
                        codes::ARTIFACT_CACHE_MISSING,
                        "Managed attachment failed integrity validation",
                    ));
                }
                Ok(ResolvedImage {
                    descriptor: descriptor(&record),
                    bytes,
                })
            })
            .collect()
    }

    fn register_agent_artifact(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        relative_path: &str,
    ) -> Result<ArtifactDescriptor, DomainError> {
        let source = self.agent_workspace_file(repo, task_id, relative_path)?;
        self.import_one(repo, task_id, &source.to_string_lossy())
    }

    fn list(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
    ) -> Result<Vec<ArtifactDescriptor>, DomainError> {
        let root = self.root_for_task(repo, task_id)?;
        repo.list_attachments_by_task(&task_id.0)?
            .into_iter()
            .map(|record| {
                let mut dto = descriptor(&record);
                let valid = PathBuf::from(&record.cache_path)
                    .canonicalize()
                    .ok()
                    .is_some_and(|path| path.starts_with(&root))
                    && std::fs::read(&record.cache_path).ok().is_some_and(|bytes| {
                        bytes.len() as u64 == record.bytes
                            && format!("{:x}", Sha256::digest(bytes)) == record.sha256
                    });
                if !valid {
                    dto.state = "missing".into();
                    dto.preview_capability = "none".into();
                }
                Ok(dto)
            })
            .collect()
    }

    fn reveal(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        artifact_id: &str,
    ) -> Result<(), DomainError> {
        self.resolve_images(repo, task_id, &[artifact_id.to_owned()])?;
        let record = repo.get_attachment(artifact_id)?;
        std::process::Command::new("explorer.exe")
            .arg("/select,")
            .arg(&record.cache_path)
            .spawn()
            .map_err(|_| {
                DomainError::new(
                    codes::ARTIFACT_NOT_FOUND,
                    "Unable to reveal managed artifact",
                )
            })?;
        Ok(())
    }

    fn save(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        artifact_id: &str,
        target_path: &str,
        overwrite: bool,
    ) -> ArtifactSaveResult {
        // Only fully validated imports create AttachmentRecord rows. Missing,
        // quarantined, rejected, invalid, and cross-task IDs therefore fail
        // this backend lookup/ownership gate before any destination I/O.
        let record = match repo.get_attachment(artifact_id) {
            Ok(record) if record.task_id == *task_id => record,
            Ok(_) => {
                return artifact_save_failure(
                    artifact_id,
                    None,
                    None,
                    ArtifactSaveIoError::Rejected("制品不属于当前任务"),
                )
            }
            Err(_) => {
                return artifact_save_failure(
                    artifact_id,
                    None,
                    None,
                    ArtifactSaveIoError::Rejected("制品不存在或不可保存"),
                )
            }
        };
        let root = match self.root_for_task(repo, task_id) {
            Ok(root) => root,
            Err(_) => {
                return artifact_save_failure(
                    artifact_id,
                    None,
                    None,
                    ArtifactSaveIoError::Rejected("受管制品存储不可用"),
                )
            }
        };
        let raw_target = Path::new(target_path);
        let target_name = raw_target
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned);
        let warning = artifact_extension_warning(raw_target, &record.mime);
        let expectation = ArtifactFileExpectation {
            bytes: record.bytes,
            sha256: &record.sha256,
        };
        match save_managed_artifact(
            Path::new(&record.cache_path),
            &root,
            target_path,
            &expectation,
            overwrite,
            &self.protected_paths,
        ) {
            Ok(saved) => ArtifactSaveResult {
                status: ArtifactSaveStatus::Saved,
                artifact_id: artifact_id.to_owned(),
                target_name: saved.target_name,
                extension_warning: warning,
                message: Some("制品已安全保存".into()),
            },
            Err(error) => artifact_save_failure(artifact_id, target_name, warning, error),
        }
    }

    fn reveal_saved(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        artifact_id: &str,
        target_path: &str,
    ) -> Result<(), DomainError> {
        let record = repo.get_attachment(artifact_id).map_err(|_| {
            DomainError::new(codes::ARTIFACT_NOT_FOUND, "Saved artifact is unavailable")
        })?;
        if record.task_id != *task_id {
            return Err(DomainError::new(
                codes::ARTIFACT_NOT_FOUND,
                "Saved artifact does not belong to this task",
            ));
        }
        let root = self.root_for_task(repo, task_id)?;
        reveal_saved_artifact(
            Path::new(&record.cache_path),
            &root,
            target_path,
            &ArtifactFileExpectation {
                bytes: record.bytes,
                sha256: &record.sha256,
            },
            &self.protected_paths,
        )
        .map_err(artifact_save_domain_error)
    }

    fn enforce_cache_quota(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
    ) -> Result<(), DomainError> {
        let root = self.root_for_task(repo, task_id)?;
        let protected: HashSet<PathBuf> = repo
            .list_attachments_by_task(&task_id.0)?
            .into_iter()
            .filter_map(|record| PathBuf::from(record.cache_path).canonicalize().ok())
            .collect();
        let mut files = Vec::new();
        collect_files(&root, &mut files)?;
        let mut total = files.iter().map(|(_, bytes, _)| *bytes).sum::<u64>();
        if total <= CACHE_QUOTA_BYTES {
            return Ok(());
        }
        files.sort_by_key(|(_, _, modified)| *modified);
        for (path, bytes, _) in files {
            if total <= CACHE_QUOTA_BYTES {
                break;
            }
            if protected.contains(&path) {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(bytes);
            }
        }
        Ok(())
    }
}

fn artifact_save_failure(
    artifact_id: &str,
    target_name: Option<String>,
    extension_warning: Option<String>,
    failure: ArtifactSaveIoError,
) -> ArtifactSaveResult {
    let (status, message) = match failure {
        ArtifactSaveIoError::Conflict => (
            ArtifactSaveStatus::Conflict,
            "目标文件已存在或已变化；请选择取消、另存为新名称或再次明确覆盖",
        ),
        ArtifactSaveIoError::Rejected(message) => (ArtifactSaveStatus::Rejected, message),
        ArtifactSaveIoError::Failed(message) => (ArtifactSaveStatus::Failed, message),
    };
    ArtifactSaveResult {
        status,
        artifact_id: artifact_id.to_owned(),
        target_name,
        extension_warning,
        message: Some(message.into()),
    }
}

fn artifact_save_domain_error(failure: ArtifactSaveIoError) -> DomainError {
    match failure {
        ArtifactSaveIoError::Conflict => DomainError::new(
            codes::ARTIFACT_INVALID_FORMAT,
            "Saved artifact target conflicts with an existing file",
        ),
        ArtifactSaveIoError::Rejected(message) => {
            DomainError::new(codes::ARTIFACT_INVALID_FORMAT, message)
        }
        ArtifactSaveIoError::Failed(message) => {
            DomainError::new(codes::ARTIFACT_CACHE_MISSING, message)
        }
    }
}

fn artifact_extension_warning(path: &Path, mime: &str) -> Option<String> {
    validate_extension(path, mime)
        .err()
        .map(|_| format!("所选扩展名与 {mime} 内容不一致；文件只会被复制，不会执行"))
}

fn collect_files(
    directory: &Path,
    output: &mut Vec<(PathBuf, u64, std::time::SystemTime)>,
) -> Result<(), DomainError> {
    for entry in std::fs::read_dir(directory).map_err(|_| {
        DomainError::new(
            codes::ARTIFACT_CACHE_MISSING,
            "Unable to scan managed artifact storage",
        )
    })? {
        let entry = entry.map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Unable to inspect managed artifact storage",
            )
        })?;
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Unable to inspect managed artifact storage",
            )
        })?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_files(&entry.path(), output)?;
        } else if metadata.is_file() {
            let path = entry.path().canonicalize().map_err(|_| {
                DomainError::new(
                    codes::ARTIFACT_CACHE_MISSING,
                    "Managed artifact cache is unavailable",
                )
            })?;
            output.push((
                path,
                metadata.len(),
                metadata
                    .modified()
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            ));
        }
    }
    Ok(())
}

fn cache_has_capacity(used: u64, incoming: u64) -> bool {
    used.saturating_add(incoming) <= CACHE_QUOTA_BYTES
}

fn descriptor(record: &AttachmentRecord) -> ArtifactDescriptor {
    ArtifactDescriptor {
        artifact_id: record.id.0.clone(),
        display_name: record.source_name.clone(),
        mime_type: record.mime.clone(),
        bytes: record.bytes,
        state: "ready".into(),
        preview_capability: if record.bytes <= 10 * 1024 * 1024 {
            "inline"
        } else {
            "onDemand"
        }
        .into(),
    }
}

fn detect_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else if bytes.len() >= 4
        && (&bytes[..4] == b"\x00\x00\x01\x00" || &bytes[..4] == b"\x00\x00\x02\x00")
    {
        Some("image/x-icon")
    } else if bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*") {
        Some("image/tiff")
    } else if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"avif" | b"avis")
    {
        Some("image/avif")
    } else {
        None
    }
}

fn validate_extension(path: &Path, mime: &str) -> Result<(), DomainError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let valid = match mime {
        "image/png" => extension == "png",
        "image/jpeg" => matches!(extension.as_str(), "jpg" | "jpeg" | "jpe"),
        "image/gif" => extension == "gif",
        "image/webp" => extension == "webp",
        "image/bmp" => matches!(extension.as_str(), "bmp" | "dib"),
        "image/x-icon" => matches!(extension.as_str(), "ico" | "cur"),
        "image/tiff" => matches!(extension.as_str(), "tif" | "tiff"),
        "image/avif" => extension == "avif",
        _ => false,
    };
    valid.then_some(()).ok_or_else(|| {
        DomainError::new(
            codes::ARTIFACT_INVALID_FORMAT,
            "Image extension does not match its contents",
        )
    })
}

fn validate_dimensions(bytes: &[u8], mime: &str) -> Result<(), DomainError> {
    let dimensions = match mime {
        "image/png" if bytes.len() >= 24 => Some((
            u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
            u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
        )),
        "image/gif" if bytes.len() >= 10 => Some((
            u16::from_le_bytes(bytes[6..8].try_into().unwrap()) as u32,
            u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as u32,
        )),
        "image/bmp" if bytes.len() >= 26 => Some((
            u32::from_le_bytes(bytes[18..22].try_into().unwrap()),
            i32::from_le_bytes(bytes[22..26].try_into().unwrap()).unsigned_abs(),
        )),
        _ => None,
    };
    if dimensions.is_some_and(|(width, height)| {
        width == 0
            || height == 0
            || u64::from(width).saturating_mul(u64::from(height)) > MAX_PREVIEW_PIXELS
    }) {
        return Err(DomainError::new(
            codes::ARTIFACT_INVALID_FORMAT,
            "Image dimensions exceed the safe preview limit",
        ));
    }
    Ok(())
}

fn canonical_extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/x-icon" => "ico",
        "image/tiff" => "tiff",
        "image/avif" => "avif",
        _ => "bin",
    }
}

/// Keep a blob's display name safe for the UI and ensure it carries an
/// extension matching the detected content so downstream tools can guess
/// the format without trusting client-supplied mime types.
fn sanitize_source_name(name: &str, mime: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter(|ch| {
            !matches!(
                ch,
                '/' | '\\' | '\0' | '<' | '>' | ':' | '"' | '|' | '?' | '*'
            )
        })
        .take(120)
        .collect();
    let trimmed = sanitized.trim().to_string();
    let sanitized = if trimmed.is_empty() {
        "剪贴板图片".to_string()
    } else {
        trimmed
    };
    let has_known_extension = sanitized.rsplit_once('.').is_some_and(|(_, extension)| {
        canonical_extension(mime) == extension.to_ascii_lowercase().as_str()
    });
    if has_known_extension {
        sanitized
    } else {
        format!("{}.{}", sanitized, canonical_extension(mime))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_bytes_and_extensions_are_conservative() {
        assert_eq!(
            detect_image_mime(b"\x89PNG\r\n\x1a\nrest"),
            Some("image/png")
        );
        assert_eq!(detect_image_mime(b"RIFF0000WEBPrest"), Some("image/webp"));
        assert_eq!(detect_image_mime(b"<svg onload=alert(1)>"), None);
        assert_eq!(detect_image_mime(b"not an image"), None);
    }

    #[test]
    fn rejects_png_pixel_bombs_before_preview() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&20_000_u32.to_be_bytes());
        png.extend_from_slice(&20_000_u32.to_be_bytes());
        assert!(validate_dimensions(&png, "image/png").is_err());
    }

    #[test]
    fn cache_quota_rejects_before_referenced_originals_are_evicted() {
        assert!(cache_has_capacity(CACHE_QUOTA_BYTES - 1, 1));
        assert!(!cache_has_capacity(CACHE_QUOTA_BYTES, 1));
    }

    #[test]
    fn sanitize_source_name_appends_matching_extension() {
        assert_eq!(sanitize_source_name("截图", "image/png"), "截图.png");
        assert_eq!(
            sanitize_source_name("a/b\\c:d.png", "image/png"),
            "abcd.png"
        );
        assert_eq!(
            sanitize_source_name("shot.jpg", "image/png"),
            "shot.jpg.png"
        );
        assert_eq!(sanitize_source_name("   ", "image/gif"), "剪贴板图片.gif");
    }

    #[test]
    fn sanitize_source_name_keeps_matching_extension() {
        assert_eq!(sanitize_source_name("clip.png", "image/png"), "clip.png");
        assert_eq!(sanitize_source_name("clip.JPG", "image/jpeg"), "clip.JPG");
    }

    #[test]
    fn blob_import_round_trips_through_managed_storage() {
        use crate::adapters::sqlite::SqliteRepository;
        use crate::domain::types::{
            utc_now, Project, ProjectId, Task, TaskId as DomainTaskId, TaskStatus, WorkspaceKind,
        };
        use std::path::PathBuf;

        let repo = SqliteRepository::open_in_memory().expect("in-memory repository");
        let workspace = std::env::temp_dir().join(format!(
            "grok-acp-gui-blob-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&workspace).expect("workspace dir");
        repo.create_project(&Project {
            id: ProjectId::new("proj-blob"),
            path: workspace.to_string_lossy().into_owned(),
            display_path: "blob-test".into(),
            repo_root: None,
            trusted_at: Some(utc_now()),
            last_opened_at: utc_now(),
        })
        .expect("project");
        let task_id = DomainTaskId::new("task-blob-1");
        repo.create_task(&Task {
            id: task_id.clone(),
            project_id: ProjectId::new("proj-blob"),
            title: "Blob import".into(),
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

        // 1x1 transparent PNG.
        let png = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==")
            .expect("valid png base64");

        let service = ManagedArtifactService::new();
        let imported = service
            .import_blob_images(
                &repo,
                &TaskId("task-blob-1".into()),
                &[BlobImage {
                    display_name: "剪贴板截图".into(),
                    base64_data: base64::engine::general_purpose::STANDARD.encode(&png),
                }],
            )
            .expect("blob import");
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].mime_type, "image/png");
        assert_eq!(imported[0].bytes, png.len() as u64);
        assert!(imported[0].display_name.contains("剪贴板截图"));

        // Identical bytes dedupe to the same artifact id.
        let again = service
            .import_blob_images(
                &repo,
                &TaskId("task-blob-1".into()),
                &[BlobImage {
                    display_name: "另一名字.png".into(),
                    base64_data: base64::engine::general_purpose::STANDARD.encode(&png),
                }],
            )
            .expect("blob re-import");
        assert_eq!(again[0].artifact_id, imported[0].artifact_id);

        // Garbage base64 fails closed.
        let bad = service.import_blob_images(
            &repo,
            &TaskId("task-blob-1".into()),
            &[BlobImage {
                display_name: "坏图".into(),
                base64_data: "not base64 !!!".into(),
            }],
        );
        assert!(bad.is_err());
        // Valid base64 of non-image bytes fails closed too.
        let not_image = service.import_blob_images(
            &repo,
            &TaskId("task-blob-1".into()),
            &[BlobImage {
                display_name: "文本".into(),
                base64_data: base64::engine::general_purpose::STANDARD
                    .encode(b"<svg onload=alert(1)>"),
            }],
        );
        assert!(not_image.is_err());

        let _ = std::fs::remove_dir_all(&workspace);
        let _ = PathBuf::new();
    }
}
