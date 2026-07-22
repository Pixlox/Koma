use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PublicationFormat {
    Cbz,
    Cbr,
    Cb7,
    Cbt,
    Folder,
    Pdf,
    FixedLayoutEpub,
    Online,
}

impl PublicationFormat {
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        if path.is_dir() {
            return Some(Self::Folder);
        }
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "cbz" | "zip" => Some(Self::Cbz),
            "cbr" | "rar" => Some(Self::Cbr),
            "cb7" | "7z" => Some(Self::Cb7),
            "cbt" | "tar" | "tgz" | "tbz" | "tbz2" | "txz" => Some(Self::Cbt),
            "pdf" => Some(Self::Pdf),
            "epub" => Some(Self::FixedLayoutEpub),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ReadingDirection {
    #[default]
    Automatic,
    LeftToRight,
    RightToLeft,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ReaderMode {
    #[default]
    SinglePage,
    Spreads,
    Continuous,
    Webtoon,
    Guided,
    Presentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FitMode {
    #[default]
    Smart,
    Page,
    Width,
    Height,
    Original,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum WidePagePolicy {
    #[default]
    Keep,
    Split,
    Rotate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReaderSettings {
    pub mode: ReaderMode,
    pub direction: ReadingDirection,
    pub fit: FitMode,
    pub wide_page_policy: WidePagePolicy,
    pub crop_margins: bool,
    pub gap_px: u16,
    pub spread_gap_enabled: bool,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub gamma: f32,
    pub grayscale: bool,
    pub invert: bool,
    pub sharpen: bool,
    pub keep_awake: bool,
    pub show_page_number: bool,
}

impl Default for ReaderSettings {
    fn default() -> Self {
        Self {
            mode: ReaderMode::SinglePage,
            direction: ReadingDirection::Automatic,
            fit: FitMode::Smart,
            wide_page_policy: WidePagePolicy::Keep,
            crop_margins: false,
            gap_px: 12,
            spread_gap_enabled: true,
            brightness: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
            grayscale: false,
            invert: false,
            sharpen: false,
            keep_awake: true,
            show_page_number: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationMetadata {
    pub title: String,
    pub series: Option<String>,
    pub number: Option<String>,
    pub volume: Option<i32>,
    pub summary: Option<String>,
    pub writer: Option<String>,
    pub penciller: Option<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub web: Option<String>,
    pub direction: ReadingDirection,
}

impl PublicationMetadata {
    pub fn inferred_from_path(path: &std::path::Path) -> Self {
        let title = path
            .file_stem()
            .or_else(|| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Untitled")
            .to_owned();
        Self {
            title,
            series: None,
            number: None,
            volume: None,
            summary: None,
            writer: None,
            penciller: None,
            publisher: None,
            language: None,
            genres: Vec::new(),
            tags: Vec::new(),
            web: None,
            direction: ReadingDirection::Automatic,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDescriptor {
    pub index: usize,
    pub label: String,
    pub source_name: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub is_cover: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationManifest {
    pub id: Uuid,
    pub path: PathBuf,
    pub format: PublicationFormat,
    pub metadata: PublicationMetadata,
    pub pages: Vec<PageDescriptor>,
    #[serde(default)]
    pub chapters: Vec<ChapterRange>,
    pub fingerprint: String,
    pub modified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterRange {
    pub id: Option<String>,
    pub number: f64,
    pub title: Option<String>,
    pub start_page_index: usize,
    pub end_page_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KomaArchiveMetadata {
    pub schema_version: u32,
    #[serde(default)]
    pub chapters: Vec<ChapterRange>,
}

impl KomaArchiveMetadata {
    pub fn new(chapters: Vec<ChapterRange>) -> Self {
        Self {
            schema_version: 1,
            chapters,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageData {
    pub index: usize,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: Uuid,
    pub path: PathBuf,
    pub format: PublicationFormat,
    pub title: String,
    pub series: Option<String>,
    pub number: Option<String>,
    pub volume: Option<i32>,
    pub page_count: usize,
    pub current_page: usize,
    #[serde(default)]
    pub current_chapter: Option<f64>,
    pub progress: f64,
    #[serde(default)]
    pub total_reading_seconds: u64,
    pub is_completed: bool,
    pub is_hidden: bool,
    pub is_missing: bool,
    pub is_favorite: bool,
    pub cover_data_url: Option<String>,
    pub added_at: DateTime<Utc>,
    pub last_opened_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingState {
    pub publication_id: Uuid,
    pub current_page: usize,
    #[serde(default)]
    pub current_chapter: Option<f64>,
    pub progress: f64,
    pub completed: bool,
    #[serde(default)]
    pub total_reading_seconds: u64,
    pub settings: ReaderSettings,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: Uuid,
    pub publication_id: Uuid,
    pub page_index: usize,
    pub label: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReceipt {
    pub id: Uuid,
    pub provider: String,
    pub source_url: String,
    pub eligibility_url: String,
    pub eligibility_status: u16,
    pub checked_at: DateTime<Utc>,
    pub page_count: usize,
    pub output_path: PathBuf,
    pub output_hash: String,
    pub adapter_version: String,
}
