use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KomaError {
    #[error("the file type is not supported: {0}")]
    UnsupportedFormat(String),
    #[error("the publication contains no readable pages")]
    EmptyPublication,
    #[error("page {index} does not exist")]
    PageOutOfRange { index: usize },
    #[error("archive entry is unsafe: {0}")]
    UnsafeArchiveEntry(String),
    #[error("archive entry exceeds the {limit_mb} MiB safety limit: {name}")]
    PageTooLarge { name: String, limit_mb: u64 },
    #[error("the image could not be decoded: {0}")]
    InvalidImage(String),
    #[error("the archive needs a password")]
    PasswordRequired,
    #[error("the source is unavailable: {0}")]
    MissingSource(PathBuf),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("RAR error: {0}")]
    Rar(#[from] unrar::error::UnrarError),
    #[error("7z error: {0}")]
    SevenZip(#[from] sevenz_rust2::Error),
    #[error("PDF error: {0}")]
    Pdf(String),
    #[error("metadata error: {0}")]
    Metadata(#[from] quick_xml::DeError),
    #[error("metadata serialization error: {0}")]
    MetadataWrite(#[from] quick_xml::SeError),
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("import was denied: {0}")]
    ImportDenied(String),
    #[error("import provider is unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("import response was not recognized: {0}")]
    ProviderChanged(String),
    #[error("operation cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, KomaError>;
