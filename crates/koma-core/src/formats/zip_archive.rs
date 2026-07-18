use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    error::{KomaError, Result},
    formats::{
        MAX_PAGE_BYTES, MAX_PAGES, PublicationReader, ensure_safe_archive_path, is_image_path,
        manifest_id, mime_for_path, modified_at, validate_page_bytes,
    },
    metadata::ComicInfo,
    model::{
        PageData, PageDescriptor, PublicationFormat, PublicationManifest, PublicationMetadata,
    },
    natural_sort,
};

pub struct ZipPublication {
    manifest: PublicationManifest,
    entries: Vec<String>,
}

impl ZipPublication {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_format(path, PublicationFormat::Cbz, None)
    }

    pub fn open_epub(path: &Path) -> Result<Self> {
        let entries = epub_spine(path)?;
        Self::open_with_format(path, PublicationFormat::FixedLayoutEpub, Some(entries))
    }

    fn open_with_format(
        path: &Path,
        format: PublicationFormat,
        preferred_entries: Option<Vec<String>>,
    ) -> Result<Self> {
        let file = File::open(path)?;
        let mut archive = ZipArchive::new(file)?;
        let mut entries = Vec::<(String, u64)>::new();
        let mut comic_info = None;

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let name = entry.name().replace('\\', "/");
            ensure_safe_archive_path(&name)?;
            if entry.is_dir() {
                continue;
            }
            if name.eq_ignore_ascii_case("ComicInfo.xml") && entry.size() <= 2 * 1024 * 1024 {
                let mut source = String::new();
                entry.read_to_string(&mut source)?;
                comic_info = ComicInfo::from_xml(&source).ok();
            }
            if is_image_path(&name) {
                if entry.size() > MAX_PAGE_BYTES {
                    return Err(KomaError::PageTooLarge {
                        name,
                        limit_mb: MAX_PAGE_BYTES / 1024 / 1024,
                    });
                }
                entries.push((name, entry.size()));
            }
        }

        if let Some(preferred) = preferred_entries {
            let sizes = entries
                .iter()
                .map(|(name, size)| (name.clone(), *size))
                .collect::<std::collections::HashMap<_, _>>();
            entries = preferred
                .into_iter()
                .filter_map(|name| sizes.get(&name).copied().map(|size| (name, size)))
                .collect();
        } else {
            entries.sort_by(|left, right| natural_sort::compare(&left.0, &right.0));
        }
        if entries.is_empty() {
            return Err(KomaError::EmptyPublication);
        }
        if entries.len() > MAX_PAGES {
            return Err(KomaError::Other(format!(
                "archive contains more than {MAX_PAGES} pages"
            )));
        }

        let mut metadata = PublicationMetadata::inferred_from_path(path);
        if let Some(comic_info) = comic_info {
            comic_info.merge_into(&mut metadata);
        }
        let descriptors = entries
            .iter()
            .enumerate()
            .map(|(index, (name, byte_size))| PageDescriptor {
                index,
                label: (index + 1).to_string(),
                source_name: name.clone(),
                mime_type: mime_for_path(name).to_owned(),
                byte_size: *byte_size,
                width: None,
                height: None,
                is_cover: index == 0,
            })
            .collect::<Vec<_>>();
        let fingerprint = archive_fingerprint(path, &descriptors);
        let manifest = PublicationManifest {
            id: manifest_id(&fingerprint),
            path: path.to_path_buf(),
            format,
            metadata,
            pages: descriptors,
            fingerprint,
            modified_at: modified_at(path),
        };

        Ok(Self {
            manifest,
            entries: entries.into_iter().map(|(name, _)| name).collect(),
        })
    }

    pub fn write_cbz(
        output: &Path,
        pages: impl IntoIterator<Item = (String, Vec<u8>)>,
        comic_info: &ComicInfo,
    ) -> Result<()> {
        let temporary = TemporarySibling::new(output);
        let file = File::create(temporary.path())?;
        let mut writer = ZipWriter::new(file);
        writer.start_file("ComicInfo.xml", metadata_options())?;
        writer.write_all(comic_info.to_xml()?.as_bytes())?;
        for (name, bytes) in pages {
            ensure_safe_archive_path(&name)?;
            validate_page_bytes(&name, &bytes)?;
            writer.start_file(name, page_options())?;
            writer.write_all(&bytes)?;
        }
        let file = writer.finish()?;
        file.sync_all()?;

        // Opening the completed archive validates its central directory and page list.
        let _verified = Self::open(temporary.path())?;
        temporary.commit(output)
    }

    pub fn write_cbz_from_files(
        output: &Path,
        pages: impl IntoIterator<Item = (String, PathBuf)>,
        comic_info: &ComicInfo,
    ) -> Result<()> {
        let temporary = TemporarySibling::new(output);
        let file = File::create(temporary.path())?;
        let mut writer = ZipWriter::new(file);
        writer.start_file("ComicInfo.xml", metadata_options())?;
        writer.write_all(comic_info.to_xml()?.as_bytes())?;
        for (name, path) in pages {
            ensure_safe_archive_path(&name)?;
            let metadata = std::fs::metadata(&path)?;
            if metadata.len() > MAX_PAGE_BYTES {
                return Err(KomaError::PageTooLarge {
                    name,
                    limit_mb: MAX_PAGE_BYTES / 1024 / 1024,
                });
            }
            writer.start_file(name, page_options())?;
            let mut page = File::open(path)?;
            std::io::copy(&mut page, &mut writer)?;
        }
        let file = writer.finish()?;
        file.sync_all()?;

        let _verified = Self::open(temporary.path())?;
        temporary.commit(output)
    }

    pub fn rewrite_comic_info(path: &Path, comic_info: &ComicInfo) -> Result<()> {
        let temporary = TemporarySibling::new(path);
        let source = File::open(path)?;
        let mut archive = ZipArchive::new(source)?;
        let output = File::create(temporary.path())?;
        let mut writer = ZipWriter::new(output);
        writer.set_raw_comment(archive.comment().into())?;
        writer.start_file("ComicInfo.xml", metadata_options())?;
        writer.write_all(comic_info.to_xml()?.as_bytes())?;
        for index in 0..archive.len() {
            let entry = archive.by_index(index)?;
            let name = entry.name().to_owned();
            ensure_safe_archive_path(&name)?;
            if name.eq_ignore_ascii_case("ComicInfo.xml") {
                continue;
            }
            writer.raw_copy_file(entry)?;
        }
        let file = writer.finish()?;
        file.sync_all()?;
        let _verified = Self::open(temporary.path())?;
        temporary.commit(path)
    }
}

fn metadata_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644)
}

fn page_options() -> SimpleFileOptions {
    // Comic pages are already compressed image streams. Deflating them again
    // costs substantial CPU and rarely reduces the archive.
    SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o644)
}

impl PublicationReader for ZipPublication {
    fn manifest(&self) -> &PublicationManifest {
        &self.manifest
    }

    fn read_page(&self, index: usize) -> Result<PageData> {
        let name = self
            .entries
            .get(index)
            .ok_or(KomaError::PageOutOfRange { index })?;
        let file = File::open(&self.manifest.path)?;
        let mut archive = ZipArchive::new(file)?;
        let mut entry = archive.by_name(name)?;
        if entry.size() > MAX_PAGE_BYTES {
            return Err(KomaError::PageTooLarge {
                name: name.clone(),
                limit_mb: MAX_PAGE_BYTES / 1024 / 1024,
            });
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .by_ref()
            .take(MAX_PAGE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        validate_page_bytes(name, &bytes)?;
        Ok(PageData {
            index,
            mime_type: mime_for_path(name).to_owned(),
            bytes,
        })
    }
}

fn archive_fingerprint(path: &Path, pages: &[PageDescriptor]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.to_string_lossy().as_bytes());
    for page in pages {
        hasher.update(page.source_name.as_bytes());
        hasher.update(&page.byte_size.to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

struct TemporarySibling {
    path: PathBuf,
    committed: bool,
}

impl TemporarySibling {
    fn new(output: &Path) -> Self {
        let file_name = output
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("publication.cbz");
        Self {
            path: output.with_file_name(format!(".{file_name}.{}.koma-tmp", Uuid::new_v4())),
            committed: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn commit(mut self, output: &Path) -> Result<()> {
        fs_replace(&self.path, output)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for TemporarySibling {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn fs_replace(source: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        let backup = destination.with_extension("cbz.koma-backup");
        if backup.exists() {
            std::fs::remove_file(&backup)?;
        }
        std::fs::rename(destination, &backup)?;
        if let Err(error) = std::fs::rename(source, destination) {
            let _ = std::fs::rename(&backup, destination);
            return Err(error.into());
        }
    } else {
        std::fs::rename(source, destination)?;
    }
    Ok(())
}

fn epub_spine(path: &Path) -> Result<Vec<String>> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let container = read_small_text(&mut archive, "META-INF/container.xml")?;
    let rootfile = capture_attribute(
        &container,
        "rootfile",
        "full-path",
        "EPUB container has no rootfile",
    )?;
    ensure_safe_archive_path(&rootfile)?;
    let package = read_small_text(&mut archive, &rootfile)?;
    let package_dir = Path::new(&rootfile).parent().unwrap_or(Path::new(""));

    let item_regex = regex::Regex::new(r#"<item\b[^>]*\bid=["']([^"']+)["'][^>]*\bhref=["']([^"']+)["'][^>]*\bmedia-type=["'](image/[^"']+)["'][^>]*/?>"#)
        .map_err(|error| KomaError::Other(error.to_string()))?;
    let mut manifest = std::collections::HashMap::new();
    for captures in item_regex.captures_iter(&package) {
        let id = captures
            .get(1)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let href = captures
            .get(2)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let joined = package_dir.join(href);
        manifest.insert(id.to_owned(), joined.to_string_lossy().replace('\\', "/"));
    }
    let itemref_regex = regex::Regex::new(r#"<itemref\b[^>]*\bidref=["']([^"']+)["'][^>]*/?>"#)
        .map_err(|error| KomaError::Other(error.to_string()))?;
    let mut spine = itemref_regex
        .captures_iter(&package)
        .filter_map(|captures| captures.get(1))
        .filter_map(|id| manifest.get(id.as_str()).cloned())
        .collect::<Vec<_>>();
    if spine.is_empty() {
        spine.extend(manifest.into_values());
        spine.sort_by(|left, right| natural_sort::compare(left, right));
    }
    Ok(spine)
}

fn capture_attribute(source: &str, tag: &str, attribute: &str, error: &str) -> Result<String> {
    let expression = format!(
        r#"<{tag}\b[^>]*\b{attribute}=["']([^"']+)["'][^>]*/?>"#,
        tag = regex::escape(tag),
        attribute = regex::escape(attribute)
    );
    let regex =
        regex::Regex::new(&expression).map_err(|error| KomaError::Other(error.to_string()))?;
    regex
        .captures(source)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_owned())
        .ok_or_else(|| KomaError::Other(error.to_owned()))
}

fn read_small_text(archive: &mut ZipArchive<File>, name: &str) -> Result<String> {
    let mut entry = archive.by_name(name)?;
    if entry.size() > 4 * 1024 * 1024 {
        return Err(KomaError::Other(format!("{name} is unexpectedly large")));
    }
    let mut source = String::new();
    entry.read_to_string(&mut source)?;
    Ok(source)
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use tempfile::tempdir;
    use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

    use super::ZipPublication;
    use crate::{formats::PublicationReader, metadata::ComicInfo};

    #[test]
    fn reads_cbz_in_natural_order_and_uses_comic_info() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("test.cbz");
        let file = File::create(&path).expect("create archive");
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        writer.start_file("10.png", options).expect("start page");
        writer.write_all(tiny_png()).expect("write page");
        writer.start_file("2.png", options).expect("start page");
        writer.write_all(tiny_png()).expect("write page");
        writer
            .start_file("ComicInfo.xml", options)
            .expect("start metadata");
        writer
            .write_all(b"<ComicInfo><Title>Proof Book</Title></ComicInfo>")
            .expect("write metadata");
        writer.finish().expect("finish archive");

        let publication = ZipPublication::open(&path).expect("open publication");
        assert_eq!(publication.manifest().metadata.title, "Proof Book");
        assert_eq!(publication.manifest().pages[0].source_name, "2.png");
        assert_eq!(
            publication.read_page(1).expect("read page").bytes,
            tiny_png()
        );
    }

    #[test]
    fn failed_write_leaves_no_partial_archive_or_temporary_file() {
        let directory = tempdir().expect("temp directory");
        let output = directory.path().join("proof.cbz");
        ZipPublication::write_cbz(
            &output,
            [("001.jpg".to_owned(), b"not an image".to_vec())],
            &ComicInfo::default(),
        )
        .expect_err("invalid image must abort");

        assert!(!output.exists());
        assert_eq!(
            std::fs::read_dir(directory.path())
                .expect("read directory")
                .count(),
            0
        );
    }

    #[test]
    fn stores_precompressed_pages_and_deflates_metadata() {
        let directory = tempdir().expect("temp directory");
        let output = directory.path().join("proof.cbz");
        ZipPublication::write_cbz(
            &output,
            [("001.png".to_owned(), tiny_png().to_vec())],
            &ComicInfo::default(),
        )
        .expect("write archive");

        let file = File::open(output).expect("open archive");
        let mut archive = ZipArchive::new(file).expect("read archive");
        assert_eq!(
            archive
                .by_name("001.png")
                .expect("page entry")
                .compression(),
            CompressionMethod::Stored
        );
        assert_eq!(
            archive
                .by_name("ComicInfo.xml")
                .expect("metadata entry")
                .compression(),
            CompressionMethod::Deflated
        );
    }

    fn tiny_png() -> &'static [u8] {
        &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 8, 215, 99, 248, 207,
            192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
            96, 130,
        ]
    }
}
