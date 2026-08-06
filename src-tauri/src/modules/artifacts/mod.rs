//! MOD-ARTIFACTS — managed image import and safe attachment resolution.
//!
//! Renderer-facing DTOs never include source/cache paths or image bytes. The
//! service validates magic bytes, size and extension before copying a file to
//! the application-managed cache and registering only metadata in SQLite.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::bridge::types::TaskId;
use crate::domain::error::{codes, DomainError};
use crate::domain::types::{utc_now, AttachmentId, AttachmentRecord};
use crate::modules::persistence::Repository;

pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDescriptor {
    pub artifact_id: String,
    pub display_name: String,
    pub mime_type: String,
    pub bytes: u64,
    pub state: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedImage {
    pub descriptor: ArtifactDescriptor,
    pub bytes: Vec<u8>,
}

pub trait ArtifactService: Send + Sync {
    fn import_images(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        paths: &[String],
    ) -> Result<Vec<ArtifactDescriptor>, DomainError>;

    fn resolve_images(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        artifact_ids: &[String],
    ) -> Result<Vec<ResolvedImage>, DomainError>;
}

pub struct ManagedArtifactService {
    root: PathBuf,
}

impl ManagedArtifactService {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn ensure_root(&self) -> Result<PathBuf, DomainError> {
        std::fs::create_dir_all(&self.root).map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Unable to create the managed artifact cache",
            )
        })?;
        self.root.canonicalize().map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Managed artifact cache is unavailable",
            )
        })
    }

    fn import_one(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        raw_path: &str,
    ) -> Result<ArtifactDescriptor, DomainError> {
        let source = PathBuf::from(raw_path).canonicalize().map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_NOT_FOUND,
                "The selected image is no longer available",
            )
        })?;
        if !source.is_file() {
            return Err(DomainError::new(
                codes::ARTIFACT_NOT_FOUND,
                "The selected image is not a file",
            ));
        }
        let metadata = std::fs::metadata(&source).map_err(|_| {
            DomainError::new(codes::ARTIFACT_NOT_FOUND, "Unable to read image metadata")
        })?;
        if metadata.len() > MAX_IMAGE_BYTES {
            return Err(DomainError::new(
                codes::ARTIFACT_TOO_LARGE,
                "Each image must be 20 MiB or smaller",
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
                "The image changed while it was being imported",
            ));
        }
        let mime = detect_image_mime(&bytes).ok_or_else(|| {
            DomainError::new(
                codes::ARTIFACT_INVALID_FORMAT,
                "Only valid PNG, JPEG, GIF, and WebP images are supported",
            )
        })?;
        validate_extension(&source, mime)?;

        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        if let Some(existing) = repo.find_attachment_by_sha256(&sha256)? {
            if existing.task_id != *task_id {
                return Err(DomainError::new(
                    codes::ARTIFACT_INVALID_FORMAT,
                    "This cached image is already attached to another task",
                ));
            }
            verify_cached_record(&existing, &bytes)?;
            return Ok(descriptor(&existing));
        }

        let root = self.ensure_root()?;
        let shard = root.join(&sha256[..2]);
        std::fs::create_dir_all(&shard).map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Unable to create cache directory",
            )
        })?;
        let extension = canonical_extension(mime);
        let cache_path = shard.join(format!("{sha256}.{extension}"));
        if !cache_path.exists() {
            let temporary = shard.join(format!(".{}.{}.tmp", sha256, uuid::Uuid::new_v4()));
            std::fs::write(&temporary, &bytes).map_err(|_| {
                DomainError::new(codes::ARTIFACT_CACHE_MISSING, "Unable to cache the image")
            })?;
            std::fs::rename(&temporary, &cache_path).map_err(|_| {
                let _ = std::fs::remove_file(&temporary);
                DomainError::new(
                    codes::ARTIFACT_CACHE_MISSING,
                    "Unable to finalize image cache",
                )
            })?;
        }
        let canonical_cache = cache_path.canonicalize().map_err(|_| {
            DomainError::new(codes::ARTIFACT_CACHE_MISSING, "Cached image is unavailable")
        })?;
        if !canonical_cache.starts_with(&root) {
            return Err(DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Cached image escaped the managed directory",
            ));
        }
        let source_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("image")
            .to_string();
        let record = AttachmentRecord {
            id: AttachmentId::new(format!("artifact-{}", &sha256[..24])),
            task_id: task_id.clone(),
            sha256,
            mime: mime.to_string(),
            bytes: bytes.len() as u64,
            cache_path: canonical_cache.to_string_lossy().into_owned(),
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
        repo.get_task(&task_id.0)?;
        paths
            .iter()
            .map(|path| self.import_one(repo, task_id, path))
            .collect()
    }

    fn resolve_images(
        &self,
        repo: &dyn Repository,
        task_id: &TaskId,
        artifact_ids: &[String],
    ) -> Result<Vec<ResolvedImage>, DomainError> {
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
                let bytes = std::fs::read(&record.cache_path).map_err(|_| {
                    DomainError::new(
                        codes::ARTIFACT_CACHE_MISSING,
                        "The managed attachment cache is missing",
                    )
                })?;
                verify_cached_record(&record, &bytes)?;
                Ok(ResolvedImage {
                    descriptor: descriptor(&record),
                    bytes,
                })
            })
            .collect()
    }
}

fn descriptor(record: &AttachmentRecord) -> ArtifactDescriptor {
    ArtifactDescriptor {
        artifact_id: record.id.0.clone(),
        display_name: record.source_name.clone(),
        mime_type: record.mime.clone(),
        bytes: record.bytes,
        state: "ready".into(),
    }
}

fn verify_cached_record(record: &AttachmentRecord, bytes: &[u8]) -> Result<(), DomainError> {
    if bytes.len() as u64 != record.bytes
        || format!("{:x}", Sha256::digest(bytes)) != record.sha256
        || detect_image_mime(bytes) != Some(record.mime.as_str())
    {
        return Err(DomainError::new(
            codes::ARTIFACT_CACHE_MISSING,
            "Cached attachment failed integrity validation",
        ));
    }
    Ok(())
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
        "image/jpeg" => matches!(extension.as_str(), "jpg" | "jpeg"),
        "image/gif" => extension == "gif",
        "image/webp" => extension == "webp",
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(DomainError::new(
            codes::ARTIFACT_INVALID_FORMAT,
            "The image extension does not match its actual format",
        ))
    }
}

fn canonical_extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite::SqliteRepository;
    use crate::domain::types::{Project, ProjectId, Task, TaskStatus, WorkspaceKind};
    use base64::Engine as _;

    #[test]
    fn detects_supported_magic_bytes() {
        assert_eq!(
            detect_image_mime(b"\x89PNG\r\n\x1a\nrest"),
            Some("image/png")
        );
        assert_eq!(detect_image_mime(b"\xff\xd8\xffrest"), Some("image/jpeg"));
        assert_eq!(detect_image_mime(b"GIF89arest"), Some("image/gif"));
        assert_eq!(detect_image_mime(b"RIFF0000WEBPrest"), Some("image/webp"));
        assert_eq!(detect_image_mime(b"not an image"), None);
    }

    #[test]
    fn imports_and_resolves_a_managed_png_without_exposing_paths() {
        let temp = std::env::temp_dir().join(format!("gag010-{}", uuid::Uuid::new_v4()));
        let source_dir = temp.join("source");
        let cache_dir = temp.join("cache");
        std::fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("screen.png");
        let png = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .unwrap();
        std::fs::write(&source, &png).unwrap();

        let repo = SqliteRepository::open_in_memory().unwrap();
        let project = Project {
            id: ProjectId::new("project-artifact"),
            path: source_dir.to_string_lossy().into_owned(),
            display_path: "fixture".into(),
            repo_root: None,
            trusted_at: Some(utc_now()),
            last_opened_at: utc_now(),
        };
        repo.create_project(&project).unwrap();
        let task = Task {
            id: TaskId::new("task-artifact"),
            project_id: project.id,
            title: "artifact".into(),
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
        };
        repo.create_task(&task).unwrap();

        let service = ManagedArtifactService::new(cache_dir);
        let imported = service
            .import_images(&repo, &task.id, &[source.to_string_lossy().into_owned()])
            .unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].mime_type, "image/png");
        let dto = serde_json::to_string(&imported[0]).unwrap();
        assert!(!dto.contains("cache"));
        assert!(!dto.contains(source_dir.to_string_lossy().as_ref()));

        let resolved = service
            .resolve_images(&repo, &task.id, &[imported[0].artifact_id.clone()])
            .unwrap();
        assert_eq!(resolved[0].bytes, png);

        std::fs::remove_dir_all(temp).unwrap();
    }
}
