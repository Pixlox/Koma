use std::path::Path;

use unrar::Archive;

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

pub struct RarPublication {
    manifest: PublicationManifest,
    entries: Vec<String>,
    password: Option<String>,
}

impl RarPublication {
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self> {
        let archive = match password {
            Some(password) => Archive::with_password(path, password).open_for_listing()?,
            None => Archive::new(path).as_first_part().open_for_listing()?,
        };
        let mut entries = Vec::<(String, u64)>::new();
        for entry in archive {
            let entry = entry?;
            let name = entry.filename.to_string_lossy().replace('\\', "/");
            ensure_safe_archive_path(&name)?;
            if entry.is_file() && is_image_path(&name) {
                if entry.unpacked_size > MAX_PAGE_BYTES {
                    return Err(KomaError::PageTooLarge {
                        name,
                        limit_mb: MAX_PAGE_BYTES / 1024 / 1024,
                    });
                }
                entries.push((name, entry.unpacked_size));
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
        let fingerprint = rar_fingerprint(path, &descriptors);
        let manifest = PublicationManifest {
            id: manifest_id(&fingerprint),
            path: path.to_path_buf(),
            format: PublicationFormat::Cbr,
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

impl PublicationReader for RarPublication {
    fn manifest(&self) -> &PublicationManifest {
        &self.manifest
    }

    fn read_page(&self, index: usize) -> Result<PageData> {
        let wanted = self
            .entries
            .get(index)
            .ok_or(KomaError::PageOutOfRange { index })?
            .clone();
        let mut archive = match self.password.as_deref() {
            Some(password) => {
                Archive::with_password(&self.manifest.path, password).open_for_processing()?
            }
            None => Archive::new(&self.manifest.path)
                .as_first_part()
                .open_for_processing()?,
        };

        loop {
            let Some(file) = archive.read_header()? else {
                break;
            };
            let name = file.entry().filename.to_string_lossy().replace('\\', "/");
            if name == wanted {
                if file.entry().unpacked_size > MAX_PAGE_BYTES {
                    return Err(KomaError::PageTooLarge {
                        name,
                        limit_mb: MAX_PAGE_BYTES / 1024 / 1024,
                    });
                }
                let (bytes, _archive) = file.read()?;
                validate_page_bytes(&wanted, &bytes)?;
                return Ok(PageData {
                    index,
                    mime_type: mime_for_path(&wanted).to_owned(),
                    bytes,
                });
            }
            archive = file.skip()?;
        }
        Err(KomaError::PageOutOfRange { index })
    }
}

fn rar_fingerprint(path: &Path, pages: &[PageDescriptor]) -> String {
    let mut hasher = blake3::Hasher::new();
    let first_part = Archive::new(path).first_part();
    hasher.update(first_part.to_string_lossy().as_bytes());
    for page in pages {
        hasher.update(page.source_name.as_bytes());
        hasher.update(&page.byte_size.to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}
