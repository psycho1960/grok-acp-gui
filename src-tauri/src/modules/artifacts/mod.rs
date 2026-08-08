//! MOD-ARTIFACTS — managed image import and ACP-only image resolution.
//!
//! The renderer receives descriptors only. Image bytes stay in a task's
//! managed workspace directory until the backend serialises an ACP prompt.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::bridge::types::TaskId;
use crate::domain::error::{codes, DomainError};
use crate::domain::types::{utc_now, AttachmentId, AttachmentRecord};
use crate::modules::persistence::Repository;

pub const MAX_IMAGES_PER_PROMPT: usize = 20;
pub const MAX_IMAGE_BYTES: u64 = 200 * 1024 * 1024;
pub const MAX_PREVIEW_PIXELS: u64 = 80_000_000;

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

/// Artifacts are deliberately rooted in the task's current workspace, not in
/// a caller-supplied path. This lets the ACP process use the same workspace
/// while preventing it from receiving the original selected-file path.
#[derive(Default)]
pub struct ManagedArtifactService;

impl ManagedArtifactService {
    pub fn new() -> Self {
        Self
    }

    fn root_for_task(
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
        let base = base.canonicalize().map_err(|_| {
            DomainError::new(
                codes::ARTIFACT_CACHE_MISSING,
                "Task workspace is unavailable",
            )
        })?;
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
        validate_dimensions(&bytes, mime)?;

        let sha256 = format!("{:x}", Sha256::digest(&bytes));
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
            std::fs::write(&temporary, &bytes).map_err(|_| {
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
        let record = AttachmentRecord {
            id: AttachmentId::new(format!("artifact-{}", &sha256[..24])),
            task_id: task_id.clone(),
            sha256,
            mime: mime.into(),
            bytes: bytes.len() as u64,
            cache_path: cache_path.to_string_lossy().into_owned(),
            source_name: source
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("image")
                .to_owned(),
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
}
