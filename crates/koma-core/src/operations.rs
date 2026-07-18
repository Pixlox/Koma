use std::{
    collections::BTreeMap,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use image::{
    DynamicImage, ExtendedColorType, ImageEncoder, ImageFormat, codecs::jpeg::JpegEncoder,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::{KomaError, Result},
    formats::{ZipPublication, open_publication},
    metadata::ComicInfo,
    model::{PublicationFormat, PublicationMetadata},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OutputImageFormat {
    #[default]
    Original,
    Jpeg,
    Png,
    Webp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionOptions {
    pub image_format: OutputImageFormat,
    pub jpeg_quality: u8,
    pub max_dimension: Option<u32>,
    pub skip_unreadable_pages: bool,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            image_format: OutputImageFormat::Original,
            jpeg_quality: 90,
            max_dimension: None,
            skip_unreadable_pages: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedPage {
    pub index: usize,
    pub source_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionReport {
    pub output_path: PathBuf,
    pub source_format: PublicationFormat,
    pub page_count: usize,
    pub skipped_pages: Vec<SkippedPage>,
    pub source_bytes: u64,
    pub output_bytes: u64,
    pub output_hash: String,
    pub backup_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InspectionSeverity {
    Information,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InspectionIssueCode {
    MetadataIncomplete,
    DuplicateContent,
    ExtensionMismatch,
    UnreadablePage,
    VeryLargePage,
    WidePages,
    PdfManifestOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionIssue {
    pub severity: InspectionSeverity,
    pub code: InspectionIssueCode,
    pub page_index: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationInspection {
    pub path: PathBuf,
    pub format: PublicationFormat,
    pub page_count: usize,
    pub validated_pages: usize,
    pub source_bytes: u64,
    pub duplicate_groups: Vec<Vec<usize>>,
    pub issues: Vec<InspectionIssue>,
    pub metadata: PublicationMetadata,
}

pub fn inspect_publication(path: &Path, password: Option<&str>) -> Result<PublicationInspection> {
    let reader = open_publication(path, password)?;
    let manifest = reader.manifest();
    let source_bytes = source_size(path, manifest.pages.iter().map(|page| page.byte_size));
    let mut issues = Vec::new();

    if manifest.metadata.series.is_none()
        && manifest.metadata.writer.is_none()
        && manifest.metadata.tags.is_empty()
    {
        issues.push(InspectionIssue {
            severity: InspectionSeverity::Information,
            code: InspectionIssueCode::MetadataIncomplete,
            page_index: None,
            message: "Only basic title metadata is available.".to_owned(),
        });
    }

    if manifest.format == PublicationFormat::Pdf {
        issues.push(InspectionIssue {
            severity: InspectionSeverity::Information,
            code: InspectionIssueCode::PdfManifestOnly,
            page_index: None,
            message: "The PDF structure and page tree are valid; individual pages render on demand in the PDF engine."
                .to_owned(),
        });
        return Ok(PublicationInspection {
            path: path.to_path_buf(),
            format: manifest.format,
            page_count: manifest.pages.len(),
            validated_pages: 0,
            source_bytes,
            duplicate_groups: Vec::new(),
            issues,
            metadata: manifest.metadata.clone(),
        });
    }

    let mut hashes: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut validated_pages = 0;
    let mut wide_pages = 0;
    for descriptor in &manifest.pages {
        match reader.read_page(descriptor.index) {
            Ok(page) => {
                validated_pages += 1;
                let hash = blake3::hash(&page.bytes).to_hex().to_string();
                hashes.entry(hash).or_default().push(descriptor.index);
                if page.bytes.len() > 64 * 1024 * 1024 {
                    issues.push(InspectionIssue {
                        severity: InspectionSeverity::Warning,
                        code: InspectionIssueCode::VeryLargePage,
                        page_index: Some(descriptor.index),
                        message: format!(
                            "Page {} is larger than 64 MiB and may open slowly on mobile devices.",
                            descriptor.index + 1
                        ),
                    });
                }
                if let Ok(format) = image::guess_format(&page.bytes)
                    && !extension_matches_format(&descriptor.source_name, format)
                {
                    issues.push(InspectionIssue {
                        severity: InspectionSeverity::Warning,
                        code: InspectionIssueCode::ExtensionMismatch,
                        page_index: Some(descriptor.index),
                        message: format!(
                            "Page {} has a filename extension that does not match its image data.",
                            descriptor.index + 1
                        ),
                    });
                }
                if descriptor
                    .width
                    .zip(descriptor.height)
                    .is_some_and(|(width, height)| width > height)
                {
                    wide_pages += 1;
                }
            }
            Err(error) => issues.push(InspectionIssue {
                severity: InspectionSeverity::Error,
                code: InspectionIssueCode::UnreadablePage,
                page_index: Some(descriptor.index),
                message: format!("Page {} could not be read: {error}", descriptor.index + 1),
            }),
        }
    }

    let duplicate_groups = hashes
        .into_values()
        .filter(|indexes| indexes.len() > 1)
        .collect::<Vec<_>>();
    if !duplicate_groups.is_empty() {
        let count = duplicate_groups
            .iter()
            .map(|group| group.len().saturating_sub(1))
            .sum::<usize>();
        issues.push(InspectionIssue {
            severity: InspectionSeverity::Warning,
            code: InspectionIssueCode::DuplicateContent,
            page_index: None,
            message: format!(
                "{count} duplicate page{} detected.",
                if count == 1 { "" } else { "s" }
            ),
        });
    }
    if wide_pages > 0 {
        issues.push(InspectionIssue {
            severity: InspectionSeverity::Information,
            code: InspectionIssueCode::WidePages,
            page_index: None,
            message: format!(
                "{wide_pages} wide page{} can use Koma's split or rotate policy.",
                if wide_pages == 1 { "" } else { "s" }
            ),
        });
    }

    Ok(PublicationInspection {
        path: path.to_path_buf(),
        format: manifest.format,
        page_count: manifest.pages.len(),
        validated_pages,
        source_bytes,
        duplicate_groups,
        issues,
        metadata: manifest.metadata.clone(),
    })
}

pub fn convert_to_cbz(
    input: &Path,
    output: &Path,
    password: Option<&str>,
    options: &ConversionOptions,
) -> Result<ConversionReport> {
    let reader = open_publication(input, password)?;
    if reader.manifest().format == PublicationFormat::Pdf {
        return Err(KomaError::UnsupportedFormat(
            "PDF pages must be rendered before they can be exported to CBZ".to_owned(),
        ));
    }
    if input == output {
        return Err(KomaError::Other(
            "choose a different output path when converting a publication".to_owned(),
        ));
    }
    if output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("cbz"))
    {
        return Err(KomaError::UnsupportedFormat(
            "converted publications must use the .cbz extension".to_owned(),
        ));
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let staging_parent = output.parent().unwrap_or_else(|| Path::new("."));
    let staging = tempfile::tempdir_in(staging_parent)?;
    let page_count = reader.manifest().pages.len();
    let number_width = page_count.to_string().len().max(3);
    let mut staged_pages = Vec::with_capacity(page_count);
    let mut skipped_pages = Vec::new();

    for descriptor in &reader.manifest().pages {
        let page = match reader.read_page(descriptor.index) {
            Ok(page) => page,
            Err(error) if options.skip_unreadable_pages => {
                skipped_pages.push(SkippedPage {
                    index: descriptor.index,
                    source_name: descriptor.source_name.clone(),
                    reason: error.to_string(),
                });
                continue;
            }
            Err(error) => return Err(error),
        };
        let (bytes, extension) = convert_page(
            page.bytes,
            &descriptor.source_name,
            page.mime_type.as_str(),
            options,
        )?;
        let name = format!(
            "{:0width$}.{extension}",
            staged_pages.len() + 1,
            width = number_width
        );
        let path = staging.path().join(&name);
        fs::write(&path, bytes)?;
        staged_pages.push((name, path));
    }
    if staged_pages.is_empty() {
        return Err(KomaError::EmptyPublication);
    }

    let metadata = ComicInfo::from_metadata(&reader.manifest().metadata, staged_pages.len());
    let backup_path = output
        .exists()
        .then(|| output.with_extension("cbz.koma-backup"));
    ZipPublication::write_cbz_from_files(output, staged_pages, &metadata)?;
    let output_bytes = fs::metadata(output)?.len();
    Ok(ConversionReport {
        output_path: output.to_path_buf(),
        source_format: reader.manifest().format,
        page_count: reader.manifest().pages.len() - skipped_pages.len(),
        skipped_pages,
        source_bytes: source_size(
            input,
            reader.manifest().pages.iter().map(|page| page.byte_size),
        ),
        output_bytes,
        output_hash: hash_file(output)?,
        backup_path,
    })
}

pub fn repair_to_cbz(
    input: &Path,
    output: &Path,
    password: Option<&str>,
) -> Result<ConversionReport> {
    convert_to_cbz(
        input,
        output,
        password,
        &ConversionOptions {
            skip_unreadable_pages: true,
            ..ConversionOptions::default()
        },
    )
}

pub fn write_publication_metadata(
    path: &Path,
    metadata: &PublicationMetadata,
) -> Result<Option<PathBuf>> {
    let comic_info = ComicInfo::from_metadata(
        metadata,
        open_publication(path, None)?.manifest().pages.len(),
    );
    match PublicationFormat::from_path(path) {
        Some(PublicationFormat::Cbz) => {
            let backup = path.with_extension("cbz.koma-backup");
            ZipPublication::rewrite_comic_info(path, &comic_info)?;
            Ok(Some(backup))
        }
        Some(PublicationFormat::Folder) => {
            let destination = path.join("ComicInfo.xml");
            let temporary = path.join(".ComicInfo.xml.koma-tmp");
            fs::write(&temporary, comic_info.to_xml()?.as_bytes())?;
            let backup = path.join("ComicInfo.xml.koma-backup");
            let had_existing_metadata = destination.exists();
            if had_existing_metadata {
                if backup.exists() {
                    fs::remove_file(&backup)?;
                }
                fs::rename(&destination, &backup)?;
            }
            if let Err(error) = fs::rename(&temporary, &destination) {
                if backup.exists() {
                    let _ = fs::rename(&backup, &destination);
                }
                return Err(error.into());
            }
            Ok(had_existing_metadata.then_some(backup))
        }
        Some(format) => Err(KomaError::UnsupportedFormat(format!(
            "writing embedded metadata is not supported for {format:?}; convert it to CBZ first"
        ))),
        None => Err(KomaError::UnsupportedFormat(
            "unknown publication type".to_owned(),
        )),
    }
}

fn convert_page(
    bytes: Vec<u8>,
    source_name: &str,
    mime_type: &str,
    options: &ConversionOptions,
) -> Result<(Vec<u8>, &'static str)> {
    if options.image_format == OutputImageFormat::Original && options.max_dimension.is_none() {
        return Ok((bytes, extension_for_mime(mime_type, source_name)));
    }

    let mut image = image::load_from_memory(&bytes)
        .map_err(|error| KomaError::InvalidImage(format!("{source_name}: {error}")))?;
    if let Some(max_dimension) = options.max_dimension.filter(|value| *value > 0)
        && (image.width() > max_dimension || image.height() > max_dimension)
    {
        image = image.thumbnail(max_dimension, max_dimension);
    }

    let format = match options.image_format {
        OutputImageFormat::Original => guessed_output_format(&bytes),
        format => format,
    };
    encode_image(image, format, options.jpeg_quality)
}

fn encode_image(
    image: DynamicImage,
    format: OutputImageFormat,
    jpeg_quality: u8,
) -> Result<(Vec<u8>, &'static str)> {
    let mut encoded = Cursor::new(Vec::new());
    match format {
        OutputImageFormat::Original | OutputImageFormat::Png => {
            image
                .write_to(&mut encoded, ImageFormat::Png)
                .map_err(|error| KomaError::InvalidImage(error.to_string()))?;
            Ok((encoded.into_inner(), "png"))
        }
        OutputImageFormat::Webp => {
            image
                .write_to(&mut encoded, ImageFormat::WebP)
                .map_err(|error| KomaError::InvalidImage(error.to_string()))?;
            Ok((encoded.into_inner(), "webp"))
        }
        OutputImageFormat::Jpeg => {
            let rgb = image.to_rgb8();
            let encoder = JpegEncoder::new_with_quality(&mut encoded, jpeg_quality.clamp(1, 100));
            encoder
                .write_image(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    ExtendedColorType::Rgb8,
                )
                .map_err(|error| KomaError::InvalidImage(error.to_string()))?;
            Ok((encoded.into_inner(), "jpg"))
        }
    }
}

fn guessed_output_format(bytes: &[u8]) -> OutputImageFormat {
    match image::guess_format(bytes) {
        Ok(ImageFormat::Jpeg) => OutputImageFormat::Jpeg,
        Ok(ImageFormat::WebP) => OutputImageFormat::Webp,
        _ => OutputImageFormat::Png,
    }
}

fn extension_for_mime(mime_type: &str, source_name: &str) -> &'static str {
    match mime_type {
        "image/avif" => "avif",
        "image/bmp" => "bmp",
        "image/gif" => "gif",
        "image/heif" => "heif",
        "image/jpeg" => "jpg",
        "image/svg+xml" => "svg",
        "image/tiff" => "tiff",
        "image/webp" => "webp",
        _ => match Path::new(source_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("jpg" | "jpeg" | "jpe" | "jfif") => "jpg",
            Some("webp") => "webp",
            Some("gif") => "gif",
            Some("avif") => "avif",
            Some("bmp") => "bmp",
            Some("svg") => "svg",
            Some("tif" | "tiff") => "tiff",
            _ => "png",
        },
    }
}

fn extension_matches_format(source_name: &str, format: ImageFormat) -> bool {
    let extension = Path::new(source_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match format {
        ImageFormat::Jpeg => matches!(extension.as_str(), "jpg" | "jpeg" | "jpe" | "jfif"),
        ImageFormat::Png => extension == "png",
        ImageFormat::Gif => extension == "gif",
        ImageFormat::WebP => extension == "webp",
        ImageFormat::Tiff => matches!(extension.as_str(), "tif" | "tiff"),
        ImageFormat::Bmp => extension == "bmp",
        ImageFormat::Avif => extension == "avif",
        _ => true,
    }
}

fn source_size(path: &Path, page_sizes: impl Iterator<Item = u64>) -> u64 {
    fs::metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .unwrap_or_else(|| page_sizes.sum())
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        ConversionOptions, InspectionIssueCode, OutputImageFormat, convert_to_cbz,
        inspect_publication, write_publication_metadata,
    };
    use crate::{
        formats::{PublicationReader, ZipPublication},
        metadata::ComicInfo,
    };

    fn tiny_png() -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("encode test image");
        bytes.into_inner()
    }

    #[test]
    fn inspection_detects_duplicate_pages() {
        let directory = tempdir().expect("temporary directory");
        let source = directory.path().join("duplicates.cbz");
        ZipPublication::write_cbz(
            &source,
            [
                ("001.png".to_owned(), tiny_png()),
                ("002.png".to_owned(), tiny_png()),
            ],
            &ComicInfo::default(),
        )
        .expect("write fixture");
        let inspection = inspect_publication(&source, None).expect("inspect");
        assert_eq!(inspection.validated_pages, 2);
        assert_eq!(inspection.duplicate_groups, vec![vec![0, 1]]);
        assert!(
            inspection
                .issues
                .iter()
                .any(|issue| issue.code == InspectionIssueCode::DuplicateContent)
        );
    }

    #[test]
    fn converts_image_folder_to_verified_jpeg_cbz() {
        let directory = tempdir().expect("temporary directory");
        let source = directory.path().join("pages");
        fs::create_dir(&source).expect("create folder");
        fs::write(source.join("page 1.png"), tiny_png()).expect("write page");
        let output = directory.path().join("converted.cbz");
        let report = convert_to_cbz(
            &source,
            &output,
            None,
            &ConversionOptions {
                image_format: OutputImageFormat::Jpeg,
                ..ConversionOptions::default()
            },
        )
        .expect("convert");
        assert_eq!(report.page_count, 1);
        assert!(output.exists());
        let converted = ZipPublication::open(&output).expect("verified output");
        assert_eq!(converted.manifest().pages.len(), 1);
        assert_eq!(converted.manifest().pages[0].mime_type, "image/jpeg");
    }

    #[test]
    fn metadata_write_is_atomic_and_keeps_the_previous_cbz() {
        let directory = tempdir().expect("temporary directory");
        let source = directory.path().join("metadata.cbz");
        ZipPublication::write_cbz(
            &source,
            [("001.png".to_owned(), tiny_png())],
            &ComicInfo {
                title: Some("Before".to_owned()),
                ..ComicInfo::default()
            },
        )
        .expect("write fixture");
        let mut metadata = crate::model::PublicationMetadata::inferred_from_path(&source);
        metadata.title = "After".to_owned();
        let backup = write_publication_metadata(&source, &metadata)
            .expect("write metadata")
            .expect("backup path");
        assert!(backup.exists());
        let updated = ZipPublication::open(&source).expect("updated source");
        let original = ZipPublication::open(&backup).expect("backup source");
        assert_eq!(updated.manifest().metadata.title, "After");
        assert_eq!(original.manifest().metadata.title, "Before");
        assert_eq!(updated.manifest().pages.len(), 1);
        assert_eq!(original.manifest().pages.len(), 1);
    }
}
