use std::{fs::File, io::Read, path::Path};

use crate::{
    error::{KomaError, Result},
    formats::{
        MAX_PAGE_BYTES, MAX_PAGES, PublicationReader, ensure_safe_archive_path, is_image_path,
        manifest_id, mime_for_path, modified_at, validate_page_bytes,
    },
    model::{
        PageData, PageDescriptor, PublicationFormat, PublicationManifest, PublicationMetadata,
    },
    natural_sort,
};

pub struct TarPublication {
    manifest: PublicationManifest,
    entries: Vec<String>,
}

impl TarPublication {
    pub fn open(path: &Path) -> Result<Self> {
        let reader = tar_reader(path)?;
        let mut archive = tar::Archive::new(reader);
        let mut entries = Vec::<(String, u64)>::new();
        for entry in archive.entries()? {
            let entry = entry?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let name = entry.path()?.to_string_lossy().replace('\\', "/");
            ensure_safe_archive_path(&name)?;
            if is_image_path(&name) {
                let size = entry.size();
                if size > MAX_PAGE_BYTES {
                    return Err(KomaError::PageTooLarge {
                        name,
                        limit_mb: MAX_PAGE_BYTES / 1024 / 1024,
                    });
                }
                entries.push((name, size));
            }
        }
        entries.sort_by(|left, right| natural_sort::compare(&left.0, &right.0));
        if entries.is_empty() {
            return Err(KomaError::EmptyPublication);
        }
        if entries.len() > MAX_PAGES {
            return Err(KomaError::Other(format!(
                "archive contains more than {MAX_PAGES} pages"
            )));
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
        let fingerprint = tar_fingerprint(path, &descriptors);
        let manifest = PublicationManifest {
            id: manifest_id(&fingerprint),
            path: path.to_path_buf(),
            format: PublicationFormat::Cbt,
            metadata: PublicationMetadata::inferred_from_path(path),
            pages: descriptors,
            fingerprint,
            modified_at: modified_at(path),
        };
        Ok(Self {
            manifest,
            entries: entries.into_iter().map(|(name, _)| name).collect(),
        })
    }
}

impl PublicationReader for TarPublication {
    fn manifest(&self) -> &PublicationManifest {
        &self.manifest
    }

    fn read_page(&self, index: usize) -> Result<PageData> {
        let wanted = self
            .entries
            .get(index)
            .ok_or(KomaError::PageOutOfRange { index })?;
        let reader = tar_reader(&self.manifest.path)?;
        let mut archive = tar::Archive::new(reader);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let name = entry.path()?.to_string_lossy().replace('\\', "/");
            if &name == wanted {
                let mut bytes = Vec::with_capacity(entry.size() as usize);
                entry
                    .by_ref()
                    .take(MAX_PAGE_BYTES + 1)
                    .read_to_end(&mut bytes)?;
                validate_page_bytes(wanted, &bytes)?;
                return Ok(PageData {
                    index,
                    mime_type: mime_for_path(wanted).to_owned(),
                    bytes,
                });
            }
        }
        Err(KomaError::PageOutOfRange { index })
    }
}

fn tar_reader(path: &Path) -> Result<Box<dyn Read + Send>> {
    let file = File::open(path)?;
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let reader: Box<dyn Read + Send> = match extension.as_deref() {
        Some("tgz" | "gz") => Box::new(flate2::read::GzDecoder::new(file)),
        Some("tbz" | "tbz2" | "bz2") => Box::new(bzip2::read::BzDecoder::new(file)),
        Some("txz" | "xz") => Box::new(xz2::read::XzDecoder::new(file)),
        _ => Box::new(file),
    };
    Ok(reader)
}

fn tar_fingerprint(path: &Path, pages: &[PageDescriptor]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.to_string_lossy().as_bytes());
    for page in pages {
        hasher.update(page.source_name.as_bytes());
        hasher.update(&page.byte_size.to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}
