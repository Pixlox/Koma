use std::path::Path;

use lopdf::{Document, Object, decode_text_string};

use crate::{
    error::{KomaError, Result},
    formats::{MAX_PAGES, PublicationReader, manifest_id, modified_at},
    model::{
        PageData, PageDescriptor, PublicationFormat, PublicationManifest, PublicationMetadata,
    },
};

pub struct PdfPublication {
    manifest: PublicationManifest,
}

impl PdfPublication {
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self> {
        let mut document =
            Document::load(path).map_err(|error| KomaError::Pdf(error.to_string()))?;
        if document.is_encrypted() {
            let password = password.ok_or(KomaError::PasswordRequired)?;
            document
                .decrypt(password)
                .map_err(|_| KomaError::PasswordRequired)?;
        }

        let pages = document.get_pages();
        if pages.is_empty() {
            return Err(KomaError::EmptyPublication);
        }
        if pages.len() > MAX_PAGES {
            return Err(KomaError::Pdf(format!(
                "the document contains more than {MAX_PAGES} pages"
            )));
        }

        let source_bytes = std::fs::metadata(path)?.len();
        let average_page_bytes = source_bytes / pages.len() as u64;
        let descriptors = pages
            .keys()
            .enumerate()
            .map(|(index, page_number)| PageDescriptor {
                index,
                label: page_number.to_string(),
                source_name: format!("page-{page_number:05}.pdf"),
                mime_type: "application/pdf".to_owned(),
                byte_size: average_page_bytes,
                width: None,
                height: None,
                is_cover: index == 0,
            })
            .collect::<Vec<_>>();

        let mut metadata = PublicationMetadata::inferred_from_path(path);
        if let Some(title) = pdf_info_string(&document, b"Title") {
            metadata.title = title;
        }
        if let Some(author) = pdf_info_string(&document, b"Author") {
            metadata.writer = Some(author);
        }
        if let Some(subject) = pdf_info_string(&document, b"Subject") {
            metadata.summary = Some(subject);
        }
        if let Some(keywords) = pdf_info_string(&document, b"Keywords") {
            metadata.tags = keywords
                .split([',', ';'])
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect();
        }

        let fingerprint = pdf_fingerprint(path, source_bytes, pages.len());
        Ok(Self {
            manifest: PublicationManifest {
                id: manifest_id(&fingerprint),
                path: path.to_path_buf(),
                format: PublicationFormat::Pdf,
                metadata,
                pages: descriptors,
                fingerprint,
                modified_at: modified_at(path),
            },
        })
    }
}

impl PublicationReader for PdfPublication {
    fn manifest(&self) -> &PublicationManifest {
        &self.manifest
    }

    fn read_page(&self, index: usize) -> Result<PageData> {
        if index >= self.manifest.pages.len() {
            return Err(KomaError::PageOutOfRange { index });
        }
        Err(KomaError::Pdf(
            "PDF pages are rendered from the original document by Koma's PDF engine".to_owned(),
        ))
    }
}

fn pdf_info_string(document: &Document, key: &[u8]) -> Option<String> {
    let info = document.trailer.get(b"Info").ok()?;
    let dictionary = match info {
        Object::Reference(id) => document.get_dictionary(*id).ok()?,
        Object::Dictionary(dictionary) => dictionary,
        _ => return None,
    };
    let value = dictionary.get(key).ok()?;
    decode_text_string(value)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn pdf_fingerprint(path: &Path, source_bytes: u64, page_count: usize) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(&source_bytes.to_le_bytes());
    hasher.update(&page_count.to_le_bytes());
    if let Ok(metadata) = std::fs::metadata(path)
        && let Ok(modified) = metadata.modified()
        && let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH)
    {
        hasher.update(&duration.as_nanos().to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use lopdf::{Document, Object, dictionary};
    use tempfile::tempdir;

    use super::PdfPublication;
    use crate::{formats::PublicationReader, model::PublicationFormat};

    #[test]
    fn opens_pdf_pages_and_reads_info_metadata() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("sample.pdf");
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 600.into(), 900.into()],
            "Resources" => dictionary! {},
        });
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        let info_id = document.add_object(dictionary! {
            "Title" => Object::string_literal("Koma PDF Proof"),
            "Author" => Object::string_literal("Koma"),
        });
        document.trailer.set("Root", catalog_id);
        document.trailer.set("Info", info_id);
        document.save(&path).expect("save fixture");

        let publication = PdfPublication::open(&path, None).expect("open PDF");
        assert_eq!(publication.manifest().format, PublicationFormat::Pdf);
        assert_eq!(publication.manifest().pages.len(), 1);
        assert_eq!(publication.manifest().metadata.title, "Koma PDF Proof");
        assert_eq!(
            publication.manifest().metadata.writer.as_deref(),
            Some("Koma")
        );
    }
}
