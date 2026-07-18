use std::{
    fs,
    path::{Path, PathBuf},
};

use walkdir::WalkDir;

use crate::{
    error::{KomaError, Result},
    formats::{
        MAX_PAGE_BYTES, MAX_PAGES, PublicationReader, is_image_path, manifest_id, mime_for_path,
        modified_at, validate_page_bytes,
    },
    model::{
        PageData, PageDescriptor, PublicationFormat, PublicationManifest, PublicationMetadata,
    },
    natural_sort,
};

pub struct FolderPublication {
    manifest: PublicationManifest,
    pages: Vec<PathBuf>,
}

impl FolderPublication {
    pub fn open(path: &Path) -> Result<Self> {
        let mut pages = WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.into_path())
            .filter(|entry| is_image_path(&entry.to_string_lossy()))
            .collect::<Vec<_>>();
        pages.sort_by(|left, right| {
            let left = left.strip_prefix(path).unwrap_or(left).to_string_lossy();
            let right = right.strip_prefix(path).unwrap_or(right).to_string_lossy();
            natural_sort::compare(&left, &right)
        });
        if pages.is_empty() {
            return Err(KomaError::EmptyPublication);
        }
        if pages.len() > MAX_PAGES {
            return Err(KomaError::Other(format!(
                "folder contains more than {MAX_PAGES} images"
            )));
        }

        let descriptors = pages
            .iter()
            .enumerate()
            .map(|(index, page)| {
                let relative = page.strip_prefix(path).unwrap_or(page);
                let byte_size = page.metadata().map(|metadata| metadata.len()).unwrap_or(0);
                PageDescriptor {
                    index,
                    label: (index + 1).to_string(),
                    source_name: relative.to_string_lossy().into_owned(),
                    mime_type: mime_for_path(&page.to_string_lossy()).to_owned(),
                    byte_size,
                    width: None,
                    height: None,
                    is_cover: index == 0,
                }
            })
            .collect::<Vec<_>>();

        let fingerprint = folder_fingerprint(path, &descriptors);
        let mut metadata = PublicationMetadata::inferred_from_path(path);
        let comic_info_path = path.join("ComicInfo.xml");
        if let Ok(xml) = std::fs::read_to_string(comic_info_path)
            && let Ok(comic_info) = crate::metadata::ComicInfo::from_xml(&xml)
        {
            comic_info.merge_into(&mut metadata);
        }
        let manifest = PublicationManifest {
            id: manifest_id(&fingerprint),
            path: path.to_path_buf(),
            format: PublicationFormat::Folder,
            metadata,
            pages: descriptors,
            fingerprint,
            modified_at: modified_at(path),
        };
        Ok(Self { manifest, pages })
    }
}

impl PublicationReader for FolderPublication {
    fn manifest(&self) -> &PublicationManifest {
        &self.manifest
    }

    fn read_page(&self, index: usize) -> Result<PageData> {
        let path = self
            .pages
            .get(index)
            .ok_or(KomaError::PageOutOfRange { index })?;
        let metadata = path.metadata()?;
        if metadata.len() > MAX_PAGE_BYTES {
            return Err(KomaError::PageTooLarge {
                name: path.to_string_lossy().into_owned(),
                limit_mb: MAX_PAGE_BYTES / 1024 / 1024,
            });
        }
        let bytes = fs::read(path)?;
        validate_page_bytes(&path.to_string_lossy(), &bytes)?;
        Ok(PageData {
            index,
            mime_type: mime_for_path(&path.to_string_lossy()).to_owned(),
            bytes,
        })
    }
}

fn folder_fingerprint(path: &Path, pages: &[PageDescriptor]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.to_string_lossy().as_bytes());
    for page in pages {
        hasher.update(page.source_name.as_bytes());
        hasher.update(&page.byte_size.to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::FolderPublication;
    use crate::formats::PublicationReader;

    #[test]
    fn finds_and_naturally_sorts_nested_images() {
        let directory = tempdir().expect("temp directory");
        fs::write(directory.path().join("10.png"), tiny_png()).expect("write page");
        fs::write(directory.path().join("2.png"), tiny_png()).expect("write page");
        let publication = FolderPublication::open(directory.path()).expect("open folder");
        assert_eq!(publication.manifest().pages[0].source_name, "2.png");
        assert_eq!(publication.manifest().pages[1].source_name, "10.png");
        assert_eq!(
            publication.read_page(0).expect("read page").bytes,
            tiny_png()
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
