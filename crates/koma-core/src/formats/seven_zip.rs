use std::path::Path;

use sevenz_rust2::{ArchiveReader, Password};

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

pub struct SevenZipPublication {
    manifest: PublicationManifest,
    entries: Vec<String>,
    password: Option<String>,
}

impl SevenZipPublication {
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self> {
        let password_value = password.map(Password::new).unwrap_or_else(Password::empty);
        let reader = ArchiveReader::open(path, password_value)?;
        let mut entries = reader
            .archive()
            .files
            .iter()
            .filter(|entry| !entry.is_directory && entry.has_stream && is_image_path(&entry.name))
            .map(|entry| (entry.name.replace('\\', "/"), entry.size))
            .collect::<Vec<_>>();
        for (name, size) in &entries {
            ensure_safe_archive_path(name)?;
            if *size > MAX_PAGE_BYTES {
                return Err(KomaError::PageTooLarge {
                    name: name.clone(),
                    limit_mb: MAX_PAGE_BYTES / 1024 / 1024,
                });
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
        let fingerprint = seven_zip_fingerprint(path, &descriptors);
        let manifest = PublicationManifest {
            id: manifest_id(&fingerprint),
            path: path.to_path_buf(),
            format: PublicationFormat::Cb7,
            metadata: PublicationMetadata::inferred_from_path(path),
            pages: descriptors,
            chapters: Vec::new(),
            fingerprint,
            modified_at: modified_at(path),
        };
        Ok(Self {
            manifest,
            entries: entries.into_iter().map(|(name, _)| name).collect(),
            password: password.map(str::to_owned),
        })
    }
}

impl PublicationReader for SevenZipPublication {
    fn manifest(&self) -> &PublicationManifest {
        &self.manifest
    }

    fn read_page(&self, index: usize) -> Result<PageData> {
        let name = self
            .entries
            .get(index)
            .ok_or(KomaError::PageOutOfRange { index })?;
        let password = self
            .password
            .as_deref()
            .map(Password::new)
            .unwrap_or_else(Password::empty);
        let mut reader = ArchiveReader::open(&self.manifest.path, password)?;
        let bytes = reader.read_file(name)?;
        validate_page_bytes(name, &bytes)?;
        Ok(PageData {
            index,
            mime_type: mime_for_path(name).to_owned(),
            bytes,
        })
    }
}

fn seven_zip_fingerprint(path: &Path, pages: &[PageDescriptor]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.to_string_lossy().as_bytes());
    for page in pages {
        hasher.update(page.source_name.as_bytes());
        hasher.update(&page.byte_size.to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}
