mod folder;
mod pdf;
mod rar;
mod seven_zip;
mod tarball;
mod zip_archive;

use std::{
    fs,
    io::Cursor,
    path::{Component, Path},
};

use chrono::{DateTime, Utc};
use image::ImageReader;
use uuid::Uuid;

use crate::{
    error::{KomaError, Result},
    model::{PageData, PublicationFormat, PublicationManifest, PublicationMetadata},
};

pub use folder::FolderPublication;
pub use pdf::PdfPublication;
pub use rar::RarPublication;
pub use seven_zip::SevenZipPublication;
pub use tarball::TarPublication;
pub use zip_archive::ZipPublication;

pub const MAX_PAGE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_PAGES: usize = 100_000;

pub trait PublicationReader: Send + Sync {
    fn manifest(&self) -> &PublicationManifest;
    fn read_page(&self, index: usize) -> Result<PageData>;
}

struct MetadataPublication {
    inner: Box<dyn PublicationReader>,
    manifest: PublicationManifest,
}

impl PublicationReader for MetadataPublication {
    fn manifest(&self) -> &PublicationManifest {
        &self.manifest
    }

    fn read_page(&self, index: usize) -> Result<PageData> {
        self.inner.read_page(index)
    }
}

pub fn with_metadata(
    inner: Box<dyn PublicationReader>,
    metadata: PublicationMetadata,
) -> Box<dyn PublicationReader> {
    let mut manifest = inner.manifest().clone();
    manifest.metadata = metadata;
    Box::new(MetadataPublication { inner, manifest })
}

pub fn open_publication(
    path: impl AsRef<Path>,
    password: Option<&str>,
) -> Result<Box<dyn PublicationReader>> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(KomaError::MissingSource(path.to_path_buf()));
    }
    match PublicationFormat::from_path(path) {
        Some(PublicationFormat::Cbz) => Ok(Box::new(ZipPublication::open(path)?)),
        Some(PublicationFormat::Cbr) => Ok(Box::new(RarPublication::open(path, password)?)),
        Some(PublicationFormat::Cb7) => Ok(Box::new(SevenZipPublication::open(path, password)?)),
        Some(PublicationFormat::Cbt) => Ok(Box::new(TarPublication::open(path)?)),
        Some(PublicationFormat::Folder) => Ok(Box::new(FolderPublication::open(path)?)),
        Some(PublicationFormat::FixedLayoutEpub) => Ok(Box::new(ZipPublication::open_epub(path)?)),
        Some(PublicationFormat::Pdf) => Ok(Box::new(PdfPublication::open(path, password)?)),
        None => Err(KomaError::UnsupportedFormat(
            path.extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("unknown")
                .to_owned(),
        )),
    }
}

pub(crate) fn is_image_path(path: &str) -> bool {
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    matches!(
        extension.as_deref(),
        Some(
            "avif"
                | "bmp"
                | "dds"
                | "exr"
                | "ff"
                | "gif"
                | "heic"
                | "heif"
                | "hdr"
                | "ico"
                | "jfif"
                | "jpe"
                | "jpeg"
                | "jpg"
                | "jxl"
                | "pbm"
                | "pgm"
                | "png"
                | "pnm"
                | "ppm"
                | "qoi"
                | "svg"
                | "tga"
                | "tif"
                | "tiff"
                | "webp"
        )
    )
}

pub(crate) fn mime_for_path(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("gif") => "image/gif",
        Some("heic" | "heif") => "image/heif",
        Some("jxl") => "image/jxl",
        Some("svg") => "image/svg+xml",
        Some("tif" | "tiff") => "image/tiff",
        Some("webp") => "image/webp",
        Some("jpg" | "jpeg" | "jpe" | "jfif") => "image/jpeg",
        _ => "image/png",
    }
}

pub(crate) fn ensure_safe_archive_path(path: &str) -> Result<()> {
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return Err(KomaError::UnsafeArchiveEntry(
            path.to_string_lossy().into_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_page_bytes(name: &str, bytes: &[u8]) -> Result<(Option<u32>, Option<u32>)> {
    if bytes.len() as u64 > MAX_PAGE_BYTES {
        return Err(KomaError::PageTooLarge {
            name: name.to_owned(),
            limit_mb: MAX_PAGE_BYTES / 1024 / 1024,
        });
    }
    if mime_for_path(name) == "image/svg+xml" {
        let source =
            std::str::from_utf8(bytes).map_err(|_| KomaError::InvalidImage(name.to_owned()))?;
        if source.contains("<script")
            || source.contains("javascript:")
            || source.contains("<foreignObject")
        {
            return Err(KomaError::InvalidImage(format!(
                "{name} contains active SVG content"
            )));
        }
        return Ok((None, None));
    }

    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| KomaError::InvalidImage(name.to_owned()))?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| KomaError::InvalidImage(name.to_owned()))?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels > 268_435_456 {
        return Err(KomaError::InvalidImage(format!(
            "{name} exceeds the 268 megapixel safety limit"
        )));
    }
    Ok((Some(width), Some(height)))
}

pub(crate) fn manifest_id(fingerprint: &str) -> Uuid {
    let digest = blake3::hash(fingerprint.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

pub(crate) fn modified_at(path: &Path) -> Option<DateTime<Utc>> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .map(DateTime::<Utc>::from)
}
