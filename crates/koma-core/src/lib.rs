//! Koma's platform-independent reading, library, and import engine.

pub mod error;
pub mod formats;
pub mod importer;
pub mod library;
pub mod metadata;
pub mod model;
pub mod natural_sort;
pub mod operations;

pub use error::{KomaError, Result};
pub use formats::{PublicationReader, open_publication};
pub use importer::{
    ConnectorCapability, ConnectorKind, ConnectorManifest, ConnectorSummary, DeclarativeImporter,
    ImportChapter, ImportEvent, ImportOptions, ImportPreview, ImportScope, ImportVolume,
    LinkImporter, MangaFireImporter, bundled_mangafire_summary,
};
pub use library::{
    BackupRestoreReport, Library, LibraryBackup, LibraryFolder, LibraryScanFailure,
    LibraryScanReport, StoredMetadata,
};
pub use metadata::ComicInfo;
pub use model::*;
pub use operations::{
    ConversionOptions, ConversionReport, InspectionIssue, InspectionIssueCode, InspectionSeverity,
    OutputImageFormat, PublicationInspection, SkippedPage, convert_to_cbz, inspect_publication,
    repair_to_cbz, write_publication_metadata,
};
