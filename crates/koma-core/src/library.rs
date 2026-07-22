use std::{
    collections::BTreeSet,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::{
    error::{KomaError, Result},
    formats::{is_image_path, modified_at, open_publication},
    importer::RemotePublication,
    model::{
        Bookmark, ChapterRange, ImportReceipt, LibraryItem, PageDescriptor, PublicationFormat,
        PublicationManifest, PublicationMetadata, ReaderSettings, ReadingDirection, ReadingState,
    },
};

const LIBRARY_SCHEMA_VERSION: i64 = 5;
const ITEM_COLUMNS: &str = "
    p.id,
    p.path,
    p.format_json,
    p.title,
    p.series,
    p.number,
    p.volume,
    p.page_count,
    COALESCE(r.current_page, 0),
    COALESCE(r.progress, 0.0),
    COALESCE(r.completed, 0),
    p.is_hidden,
    p.is_missing,
    p.is_favorite,
    p.cover_data_url,
    p.added_at,
    p.last_opened_at,
    r.current_chapter,
    COALESCE(r.total_reading_seconds, 0)
";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryScanFailure {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LibraryScanReport {
    pub imported: Vec<LibraryItem>,
    pub skipped: Vec<PathBuf>,
    pub failures: Vec<LibraryScanFailure>,
    pub unchanged: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFolder {
    pub id: Uuid,
    pub path: PathBuf,
    pub enabled: bool,
    pub scan_interval_minutes: u32,
    pub last_scanned_at: Option<DateTime<Utc>>,
    pub last_imported_count: usize,
    pub last_failure_count: usize,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBackup {
    pub schema_version: i64,
    pub exported_at: DateTime<Utc>,
    pub items: Vec<LibraryItem>,
    pub reading_states: Vec<ReadingState>,
    pub bookmarks: Vec<Bookmark>,
    pub import_receipts: Vec<ImportReceipt>,
    #[serde(default)]
    pub metadata_overrides: Vec<StoredMetadata>,
    #[serde(default)]
    pub online_sources: Vec<StoredOnlineSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMetadata {
    pub publication_id: Uuid,
    pub metadata: crate::model::PublicationMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredOnlineSource {
    pub publication_id: Uuid,
    pub publication: RemotePublication,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupRestoreReport {
    pub publications: usize,
    pub reading_states: usize,
    pub bookmarks: usize,
    pub import_receipts: usize,
    pub metadata_overrides: usize,
    pub online_sources: usize,
    pub missing_sources: usize,
}

/// Koma's local-first catalogue.
///
/// A mutex is deliberately kept at this boundary: SQLite operations are short,
/// while archive decoding and network I/O happen before the lock is acquired.
pub struct Library {
    connection: Mutex<Connection>,
}

impl Library {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn import_path(
        &self,
        path: impl AsRef<Path>,
        password: Option<&str>,
    ) -> Result<LibraryItem> {
        let reader = open_publication(path.as_ref(), password)?;
        let manifest = reader.manifest().clone();
        let cover_data_url = reader
            .read_page(0)
            .ok()
            .and_then(|page| thumbnail_data_url(&page.bytes).ok());
        self.upsert_manifest(&manifest, cover_data_url.as_deref())
    }

    pub fn add_online(
        &self,
        publication: &RemotePublication,
        cover_data_url: Option<&str>,
    ) -> Result<LibraryItem> {
        let id = Uuid::now_v7();
        let mut index = 0_usize;
        let pages = publication
            .chapters
            .iter()
            .flat_map(|chapter| chapter.pages.iter())
            .map(|page| {
                let current = index;
                index += 1;
                PageDescriptor {
                    index: current,
                    label: (current + 1).to_string(),
                    source_name: page.url.clone(),
                    mime_type: "image/*".to_owned(),
                    byte_size: 0,
                    width: page.width,
                    height: page.height,
                    is_cover: current == 0,
                }
            })
            .collect::<Vec<_>>();
        if pages.is_empty() {
            return Err(KomaError::ProviderChanged(
                "the online publication contains no pages".to_owned(),
            ));
        }
        let path = PathBuf::from(format!("koma-online://{id}"));
        let fingerprint = blake3::hash(
            format!(
                "{}\n{}\n{:?}\n{:?}\n{:?}",
                publication.provider,
                publication.source_url,
                publication.scope,
                publication.volume_id,
                publication.selected_chapter_ids
            )
            .as_bytes(),
        )
        .to_hex()
        .to_string();
        let manifest = PublicationManifest {
            id,
            path,
            format: PublicationFormat::Online,
            metadata: PublicationMetadata {
                title: publication.title.clone(),
                series: Some(publication.title.clone()),
                number: None,
                volume: None,
                summary: None,
                writer: None,
                penciller: None,
                publisher: Some(publication.provider.clone()),
                language: publication.language.clone(),
                genres: Vec::new(),
                tags: vec!["online".to_owned()],
                web: Some(publication.source_url.clone()),
                direction: ReadingDirection::Automatic,
            },
            pages,
            chapters: publication.chapter_ranges(),
            fingerprint,
            modified_at: None,
        };
        let item = self.upsert_manifest(&manifest, cover_data_url)?;
        let connection = self.lock()?;
        connection.execute(
            "INSERT OR REPLACE INTO online_sources (publication_id, source_json, updated_at) VALUES (?1, ?2, ?3)",
            params![id.to_string(), to_json(publication)?, Utc::now().to_rfc3339()],
        )?;
        Ok(item)
    }

    pub fn online_source(&self, publication_id: Uuid) -> Result<Option<RemotePublication>> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT source_json FROM online_sources WHERE publication_id = ?1",
                params![publication_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|json| from_json(&json))
            .transpose()
    }

    pub fn refresh_online(
        &self,
        publication_id: Uuid,
        publication: &RemotePublication,
    ) -> Result<()> {
        let page_count = publication.page_count();
        if page_count == 0 {
            return Err(KomaError::ProviderChanged(
                "the refreshed online publication contains no pages".to_owned(),
            ));
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE publications SET title=?1, page_count=?2, chapters_json=?3, is_missing=0 WHERE id=?4",
            params![
                publication.title,
                page_count as i64,
                to_json(&publication.chapter_ranges())?,
                publication_id.to_string(),
            ],
        )?;
        transaction.execute(
            "UPDATE reading_state SET current_page=MIN(current_page, ?1) WHERE publication_id=?2",
            params![
                page_count.saturating_sub(1) as i64,
                publication_id.to_string()
            ],
        )?;
        transaction.execute(
            "UPDATE online_sources SET source_json=?1, updated_at=?2 WHERE publication_id=?3",
            params![
                to_json(publication)?,
                Utc::now().to_rfc3339(),
                publication_id.to_string(),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn online_manifest(&self, publication_id: Uuid) -> Result<Option<PublicationManifest>> {
        let Some(publication) = self.online_source(publication_id)? else {
            return Ok(None);
        };
        let item = self
            .get(publication_id)?
            .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
        let mut index = 0_usize;
        let pages = publication
            .chapters
            .iter()
            .flat_map(|chapter| chapter.pages.iter())
            .map(|page| {
                let current = index;
                index += 1;
                PageDescriptor {
                    index: current,
                    label: (current + 1).to_string(),
                    source_name: page.url.clone(),
                    mime_type: "image/*".to_owned(),
                    byte_size: 0,
                    width: page.width,
                    height: page.height,
                    is_cover: current == 0,
                }
            })
            .collect();
        Ok(Some(PublicationManifest {
            id: publication_id,
            path: item.path,
            format: PublicationFormat::Online,
            metadata: PublicationMetadata {
                title: item.title,
                series: item.series,
                number: item.number,
                volume: item.volume,
                summary: None,
                writer: None,
                penciller: None,
                publisher: Some(publication.provider.clone()),
                language: publication.language.clone(),
                genres: Vec::new(),
                tags: vec!["online".to_owned()],
                web: Some(publication.source_url.clone()),
                direction: ReadingDirection::Automatic,
            },
            pages,
            chapters: publication.chapter_ranges(),
            fingerprint: format!("online:{publication_id}"),
            modified_at: None,
        }))
    }

    pub fn replace_online_with_path(
        &self,
        publication_id: Uuid,
        path: impl AsRef<Path>,
    ) -> Result<LibraryItem> {
        let old_source = self.online_source(publication_id)?;
        let old_state = self.reading_state(publication_id)?;
        let reader = open_publication(path.as_ref(), None)?;
        let manifest = reader.manifest().clone();
        let cover_data_url = reader
            .read_page(0)
            .ok()
            .and_then(|page| thumbnail_data_url(&page.bytes).ok());
        let mapped_page = old_source
            .as_ref()
            .zip(old_state.as_ref())
            .and_then(|(source, state)| {
                let old = source.chapter_ranges().into_iter().find(|chapter| {
                    state.current_page >= chapter.start_page_index
                        && state.current_page <= chapter.end_page_index
                })?;
                let offset = state.current_page.saturating_sub(old.start_page_index);
                let next = manifest.chapters.iter().find(|chapter| {
                    (old.id.is_some() && chapter.id == old.id)
                        || (chapter.number - old.number).abs() < f64::EPSILON
                })?;
                Some((next.start_page_index + offset).min(next.end_page_index))
            })
            .or_else(|| old_state.as_ref().map(|state| state.current_page))
            .unwrap_or(0)
            .min(manifest.pages.len().saturating_sub(1));
        let mapped_chapter = chapter_at_page(&manifest.chapters, mapped_page);
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let database_path = path_to_database(&manifest.path);
        let conflict = transaction
            .query_row(
                "SELECT id FROM publications WHERE path = ?1 AND id <> ?2",
                params![database_path, publication_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if conflict.is_some() {
            return Err(KomaError::Other(
                "the downloaded file already belongs to another library entry".to_owned(),
            ));
        }
        transaction.execute(
            "UPDATE publications SET path=?1, format_json=?2, fingerprint=?3, title=?4, series=?5, number=?6, volume=?7, page_count=?8, chapters_json=?9, cover_data_url=COALESCE(?10, cover_data_url), modified_at=?11, is_missing=0 WHERE id=?12",
            params![
                database_path,
                to_json(&manifest.format)?,
                manifest.fingerprint,
                manifest.metadata.title,
                manifest.metadata.series,
                manifest.metadata.number,
                manifest.metadata.volume,
                manifest.pages.len() as i64,
                to_json(&manifest.chapters)?,
                cover_data_url,
                manifest.modified_at.map(|date| date.to_rfc3339()),
                publication_id.to_string(),
            ],
        )?;
        transaction.execute(
            "DELETE FROM online_sources WHERE publication_id = ?1",
            params![publication_id.to_string()],
        )?;
        transaction.execute(
            "UPDATE reading_state SET current_page=?1, current_chapter=?2, progress=CASE WHEN ?3 <= 1 THEN 1.0 ELSE CAST(?1 AS REAL) / CAST(?3 - 1 AS REAL) END WHERE publication_id=?4",
            params![mapped_page as i64, mapped_chapter, manifest.pages.len() as i64, publication_id.to_string()],
        )?;
        transaction.commit()?;
        item_by_id(&connection, publication_id)?.ok_or_else(|| {
            KomaError::Other("the downloaded publication disappeared while saving".to_owned())
        })
    }

    pub fn thumbnail_data_url(bytes: &[u8]) -> Result<String> {
        thumbnail_data_url(bytes)
    }

    pub fn relink(
        &self,
        publication_id: Uuid,
        path: impl AsRef<Path>,
        password: Option<&str>,
    ) -> Result<LibraryItem> {
        let reader = open_publication(path.as_ref(), password)?;
        let manifest = reader.manifest().clone();
        let cover_data_url = reader
            .read_page(0)
            .ok()
            .and_then(|page| thumbnail_data_url(&page.bytes).ok());

        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let expected_pages: i64 = transaction
            .query_row(
                "SELECT page_count FROM publications WHERE id = ?1",
                params![publication_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
        if usize::try_from(expected_pages.max(0)).unwrap_or(0) != manifest.pages.len() {
            return Err(KomaError::Other(format!(
                "the selected source has {} pages, but this library entry expects {}; choose the moved copy of the same publication",
                manifest.pages.len(),
                expected_pages.max(0)
            )));
        }

        let database_path = path_to_database(&manifest.path);
        let conflicting_id = transaction
            .query_row(
                "SELECT id FROM publications WHERE path = ?1 AND id <> ?2",
                params![database_path, publication_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if conflicting_id.is_some() {
            return Err(KomaError::Other(
                "the selected source already belongs to another library entry".to_owned(),
            ));
        }

        transaction.execute(
            "
            UPDATE publications
            SET path = ?1,
                format_json = ?2,
                fingerprint = ?3,
                page_count = ?4,
                chapters_json = ?5,
                cover_data_url = COALESCE(?6, cover_data_url),
                modified_at = ?7,
                is_missing = 0
            WHERE id = ?8
            ",
            params![
                database_path,
                to_json(&manifest.format)?,
                manifest.fingerprint,
                manifest.pages.len() as i64,
                to_json(&manifest.chapters)?,
                cover_data_url,
                manifest.modified_at.map(|value| value.to_rfc3339()),
                publication_id.to_string(),
            ],
        )?;
        transaction.commit()?;
        item_by_id(&connection, publication_id)?.ok_or_else(|| {
            KomaError::Other("the publication disappeared while relinking".to_owned())
        })
    }

    pub fn scan_folder(&self, root: impl AsRef<Path>) -> Result<LibraryScanReport> {
        let root = root.as_ref();
        if !root.exists() {
            return Err(KomaError::MissingSource(root.to_path_buf()));
        }

        let mut candidates = BTreeSet::new();
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            let path = entry.path();
            if entry.file_type().is_file() {
                if PublicationFormat::from_path(path).is_some() {
                    candidates.insert(path.to_path_buf());
                } else if is_image_path(&path.to_string_lossy())
                    && let Some(parent) = path.parent()
                {
                    candidates.insert(parent.to_path_buf());
                }
            }
        }

        let mut report = LibraryScanReport::default();
        for candidate in candidates {
            if candidate.is_file() && self.source_is_unchanged(&candidate)? {
                report.unchanged += 1;
                continue;
            }
            match self.import_path(&candidate, None) {
                Ok(item) => report.imported.push(item),
                Err(KomaError::EmptyPublication | KomaError::UnsupportedFormat(_)) => {
                    report.skipped.push(candidate);
                }
                Err(error) => report.failures.push(LibraryScanFailure {
                    path: candidate,
                    reason: error.to_string(),
                }),
            }
        }
        self.refresh_missing_flags()?;
        Ok(report)
    }

    fn source_is_unchanged(&self, path: &Path) -> Result<bool> {
        let Some(current_modified_at) = modified_at(path) else {
            return Ok(false);
        };
        let connection = self.lock()?;
        let stored: Option<(Option<String>, bool)> = connection
            .query_row(
                "SELECT modified_at, is_missing FROM publications WHERE path = ?1",
                params![path_to_database(path)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((Some(stored_modified_at), false)) = stored else {
            return Ok(false);
        };
        let stored_modified_at = parse_datetime(&stored_modified_at)?;
        Ok(stored_modified_at.timestamp_millis() == current_modified_at.timestamp_millis())
    }

    pub fn add_library_folder(
        &self,
        path: impl AsRef<Path>,
        scan_interval_minutes: u32,
    ) -> Result<LibraryFolder> {
        let path = path.as_ref();
        if !path.is_dir() {
            return Err(KomaError::MissingSource(path.to_path_buf()));
        }
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let database_path = path_to_database(&canonical);
        let id = Uuid::now_v7();
        let interval = scan_interval_minutes.clamp(5, 10_080);
        let connection = self.lock()?;
        connection.execute(
            "
            INSERT INTO library_folders (
                id, path, enabled, scan_interval_minutes, last_imported_count,
                last_failure_count
            ) VALUES (?1, ?2, 1, ?3, 0, 0)
            ON CONFLICT(path) DO UPDATE SET
                enabled = 1,
                scan_interval_minutes = excluded.scan_interval_minutes
            ",
            params![id.to_string(), database_path, interval],
        )?;
        library_folder_by_path(&connection, &canonical)?.ok_or_else(|| {
            KomaError::Other("the library folder was not found after saving".to_owned())
        })
    }

    pub fn library_folders(&self) -> Result<Vec<LibraryFolder>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "
            SELECT id, path, enabled, scan_interval_minutes, last_scanned_at,
                   last_imported_count, last_failure_count, last_error
            FROM library_folders
            ORDER BY path COLLATE NOCASE
            ",
        )?;
        let mut rows = statement.query([])?;
        let mut folders = Vec::new();
        while let Some(row) = rows.next()? {
            folders.push(library_folder_from_row(row)?);
        }
        Ok(folders)
    }

    pub fn due_library_folders(&self, now: DateTime<Utc>) -> Result<Vec<LibraryFolder>> {
        Ok(self
            .library_folders()?
            .into_iter()
            .filter(|folder| {
                if !folder.enabled {
                    return false;
                }
                folder.last_scanned_at.is_none_or(|last| {
                    let elapsed = now.signed_duration_since(last).num_minutes();
                    elapsed >= i64::from(folder.scan_interval_minutes)
                })
            })
            .collect())
    }

    pub fn update_library_folder(
        &self,
        id: Uuid,
        enabled: bool,
        scan_interval_minutes: u32,
    ) -> Result<LibraryFolder> {
        let connection = self.lock()?;
        let interval = scan_interval_minutes.clamp(5, 10_080);
        if connection.execute(
            "
            UPDATE library_folders
            SET enabled = ?1, scan_interval_minutes = ?2
            WHERE id = ?3
            ",
            params![enabled, interval, id.to_string()],
        )? == 0
        {
            return Err(KomaError::Other("library folder was not found".to_owned()));
        }
        library_folder_by_id(&connection, id)?
            .ok_or_else(|| KomaError::Other("library folder disappeared after saving".to_owned()))
    }

    pub fn remove_library_folder(&self, id: Uuid) -> Result<bool> {
        let connection = self.lock()?;
        Ok(connection.execute(
            "DELETE FROM library_folders WHERE id = ?1",
            params![id.to_string()],
        )? > 0)
    }

    pub fn record_library_folder_scan(
        &self,
        id: Uuid,
        report: Option<&LibraryScanReport>,
        error: Option<&str>,
    ) -> Result<()> {
        let connection = self.lock()?;
        let imported = report.map_or(0, |value| value.imported.len());
        let failures = report.map_or(0, |value| value.failures.len());
        let updated = connection.execute(
            "
            UPDATE library_folders
            SET last_scanned_at = ?1,
                last_imported_count = ?2,
                last_failure_count = ?3,
                last_error = ?4
            WHERE id = ?5
            ",
            params![
                Utc::now().to_rfc3339(),
                imported as i64,
                failures as i64,
                error,
                id.to_string(),
            ],
        )?;
        if updated == 0 {
            return Err(KomaError::Other("library folder was not found".to_owned()));
        }
        Ok(())
    }

    pub fn upsert_manifest(
        &self,
        manifest: &PublicationManifest,
        cover_data_url: Option<&str>,
    ) -> Result<LibraryItem> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let now = Utc::now().to_rfc3339();
        let id = manifest.id.to_string();
        let path = path_to_database(&manifest.path);
        let format_json = to_json(&manifest.format)?;
        let modified_at = manifest.modified_at.map(|value| value.to_rfc3339());

        transaction.execute(
            "
            INSERT INTO publications (
                id, path, format_json, fingerprint, title, series, number, volume,
                page_count, chapters_json, cover_data_url, added_at, modified_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(path) DO UPDATE SET
                id = excluded.id,
                format_json = excluded.format_json,
                fingerprint = excluded.fingerprint,
                title = excluded.title,
                series = excluded.series,
                number = excluded.number,
                volume = excluded.volume,
                page_count = excluded.page_count,
                chapters_json = excluded.chapters_json,
                cover_data_url = COALESCE(excluded.cover_data_url, publications.cover_data_url),
                modified_at = excluded.modified_at,
                is_missing = 0
            ",
            params![
                id,
                path,
                format_json,
                manifest.fingerprint,
                manifest.metadata.title,
                manifest.metadata.series,
                manifest.metadata.number,
                manifest.metadata.volume,
                manifest.pages.len() as i64,
                to_json(&manifest.chapters)?,
                cover_data_url,
                now,
                modified_at,
            ],
        )?;
        transaction.execute(
            "
            INSERT INTO reading_state (
                publication_id, current_page, progress, completed, settings_json, updated_at
            ) VALUES (?1, 0, 0.0, 0, ?2, ?3)
            ON CONFLICT(publication_id) DO NOTHING
            ",
            params![id, to_json(&ReaderSettings::default())?, now],
        )?;
        transaction.commit()?;
        item_by_id(&connection, manifest.id)?.ok_or_else(|| {
            KomaError::Other("the imported publication was not found after saving".to_owned())
        })
    }

    pub fn list(&self, include_hidden: bool, search: Option<&str>) -> Result<Vec<LibraryItem>> {
        let connection = self.lock()?;
        let search = search.map(str::trim).filter(|value| !value.is_empty());
        let mut sql = format!(
            "SELECT {ITEM_COLUMNS}
             FROM publications p
             LEFT JOIN reading_state r ON r.publication_id = p.id
             WHERE (?1 OR p.is_hidden = 0)"
        );
        if search.is_some() {
            sql.push_str(
                " AND (
                    p.title LIKE ?2 ESCAPE '\\'
                    OR COALESCE(p.series, '') LIKE ?2 ESCAPE '\\'
                    OR COALESCE(p.number, '') LIKE ?2 ESCAPE '\\'
                )",
            );
        }
        sql.push_str(
            " ORDER BY
                CASE WHEN p.last_opened_at IS NULL THEN 1 ELSE 0 END,
                p.last_opened_at DESC,
                p.added_at DESC",
        );

        let mut statement = connection.prepare(&sql)?;
        let mut items = Vec::new();
        if let Some(search) = search {
            let pattern = format!("%{}%", escape_like(search));
            let mut rows = statement.query(params![include_hidden, pattern])?;
            while let Some(row) = rows.next()? {
                items.push(item_from_row(row)?);
            }
        } else {
            let mut rows = statement.query(params![include_hidden])?;
            while let Some(row) = rows.next()? {
                items.push(item_from_row(row)?);
            }
        }
        Ok(items)
    }

    pub fn get(&self, id: Uuid) -> Result<Option<LibraryItem>> {
        let connection = self.lock()?;
        item_by_id(&connection, id)
    }

    pub fn remove(&self, id: Uuid) -> Result<bool> {
        let connection = self.lock()?;
        Ok(connection.execute(
            "DELETE FROM publications WHERE id = ?1",
            params![id.to_string()],
        )? > 0)
    }

    pub fn set_hidden(&self, id: Uuid, hidden: bool) -> Result<bool> {
        self.set_boolean_flag(id, "is_hidden", hidden)
    }

    pub fn set_favorite(&self, id: Uuid, favorite: bool) -> Result<bool> {
        self.set_boolean_flag(id, "is_favorite", favorite)
    }

    pub fn set_reading_status(
        &self,
        publication_id: Uuid,
        completed: bool,
    ) -> Result<ReadingState> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let (page_count, chapters_json, settings_json, total_reading_seconds): (
            i64,
            String,
            String,
            i64,
        ) = transaction
            .query_row(
                "
                SELECT p.page_count, p.chapters_json, r.settings_json,
                       r.total_reading_seconds
                FROM publications p
                JOIN reading_state r ON r.publication_id = p.id
                WHERE p.id = ?1
                ",
                params![publication_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
            .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
        let page_count = usize::try_from(page_count.max(0)).unwrap_or(0);
        let current_page = if completed {
            page_count.saturating_sub(1)
        } else {
            0
        };
        let progress = if completed && page_count > 0 {
            1.0
        } else {
            0.0
        };
        let completed = completed && page_count > 0;
        let chapters: Vec<ChapterRange> = from_json(&chapters_json)?;
        let current_chapter = chapter_at_page(&chapters, current_page);
        let updated_at = Utc::now();
        transaction.execute(
            "
            UPDATE reading_state
            SET current_page = ?1, progress = ?2, completed = ?3,
                current_chapter = ?4, updated_at = ?5
            WHERE publication_id = ?6
            ",
            params![
                current_page as i64,
                progress,
                completed,
                current_chapter,
                updated_at.to_rfc3339(),
                publication_id.to_string(),
            ],
        )?;
        transaction.commit()?;
        Ok(ReadingState {
            publication_id,
            current_page,
            progress,
            completed,
            current_chapter,
            total_reading_seconds: u64::try_from(total_reading_seconds.max(0)).unwrap_or(0),
            settings: from_json(&settings_json)?,
            updated_at,
        })
    }

    pub fn metadata_override(
        &self,
        publication_id: Uuid,
    ) -> Result<Option<crate::model::PublicationMetadata>> {
        let connection = self.lock()?;
        let metadata = connection
            .query_row(
                "SELECT metadata_json FROM metadata_overrides WHERE publication_id = ?1",
                params![publication_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        metadata.map(|value| from_json(&value)).transpose()
    }

    pub fn save_metadata_override(
        &self,
        publication_id: Uuid,
        metadata: &crate::model::PublicationMetadata,
    ) -> Result<LibraryItem> {
        let title = metadata.title.trim();
        if title.is_empty() {
            return Err(KomaError::Other(
                "a publication title cannot be empty".to_owned(),
            ));
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        if transaction.execute(
            "
            UPDATE publications
            SET title = ?1, series = ?2, number = ?3, volume = ?4
            WHERE id = ?5
            ",
            params![
                title,
                clean_optional(metadata.series.as_deref()),
                clean_optional(metadata.number.as_deref()),
                metadata.volume,
                publication_id.to_string(),
            ],
        )? == 0
        {
            return Err(KomaError::Other(
                "publication is not in the library".to_owned(),
            ));
        }
        transaction.execute(
            "
            INSERT INTO metadata_overrides (publication_id, metadata_json, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(publication_id) DO UPDATE SET
                metadata_json = excluded.metadata_json,
                updated_at = excluded.updated_at
            ",
            params![
                publication_id.to_string(),
                to_json(metadata)?,
                Utc::now().to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        item_by_id(&connection, publication_id)?.ok_or_else(|| {
            KomaError::Other("publication disappeared after saving metadata".to_owned())
        })
    }

    fn set_boolean_flag(&self, id: Uuid, column: &str, value: bool) -> Result<bool> {
        let column = match column {
            "is_hidden" => "is_hidden",
            "is_favorite" => "is_favorite",
            _ => return Err(KomaError::Other("unknown library flag".to_owned())),
        };
        let connection = self.lock()?;
        Ok(connection.execute(
            &format!("UPDATE publications SET {column} = ?1 WHERE id = ?2"),
            params![value, id.to_string()],
        )? > 0)
    }

    pub fn save_progress(
        &self,
        publication_id: Uuid,
        current_page: usize,
        settings: Option<&ReaderSettings>,
    ) -> Result<ReadingState> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let (page_count, chapters_json): (i64, String) = transaction
            .query_row(
                "SELECT page_count, chapters_json FROM publications WHERE id = ?1",
                params![publication_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
        let page_count = usize::try_from(page_count.max(0)).unwrap_or(0);
        let bounded_page = current_page.min(page_count.saturating_sub(1));
        let progress = if page_count <= 1 {
            if page_count == 0 { 0.0 } else { 1.0 }
        } else {
            bounded_page as f64 / (page_count - 1) as f64
        };
        let completed = page_count > 0 && bounded_page + 1 >= page_count;
        let chapters: Vec<ChapterRange> = from_json(&chapters_json)?;
        let current_chapter = chapter_at_page(&chapters, bounded_page);
        let now = Utc::now();
        let stored_state: Option<(String, i64)> = transaction
            .query_row(
                "
                SELECT settings_json, total_reading_seconds
                FROM reading_state WHERE publication_id = ?1
                ",
                params![publication_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let total_reading_seconds = stored_state
            .as_ref()
            .map(|(_, seconds)| u64::try_from((*seconds).max(0)).unwrap_or(0))
            .unwrap_or(0);
        let settings_json = match settings {
            Some(settings) => to_json(settings)?,
            None => stored_state
                .map(|(settings, _)| settings)
                .unwrap_or(to_json(&ReaderSettings::default())?),
        };
        transaction.execute(
            "
            INSERT INTO reading_state (
                publication_id, current_page, progress, completed, settings_json,
                updated_at, current_chapter
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(publication_id) DO UPDATE SET
                current_page = excluded.current_page,
                progress = excluded.progress,
                completed = excluded.completed,
                settings_json = excluded.settings_json,
                current_chapter = excluded.current_chapter,
                updated_at = excluded.updated_at
            ",
            params![
                publication_id.to_string(),
                bounded_page as i64,
                progress,
                completed,
                settings_json,
                now.to_rfc3339(),
                current_chapter,
            ],
        )?;
        transaction.execute(
            "UPDATE publications SET last_opened_at = ?1 WHERE id = ?2",
            params![now.to_rfc3339(), publication_id.to_string()],
        )?;
        transaction.commit()?;
        Ok(ReadingState {
            publication_id,
            current_page: bounded_page,
            progress,
            completed,
            current_chapter,
            total_reading_seconds,
            settings: from_json(&settings_json)?,
            updated_at: now,
        })
    }

    pub fn reading_state(&self, publication_id: Uuid) -> Result<Option<ReadingState>> {
        let connection = self.lock()?;
        let row = connection
            .query_row(
                "
                SELECT current_page, progress, completed, settings_json, updated_at,
                       current_chapter, total_reading_seconds
                FROM reading_state WHERE publication_id = ?1
                ",
                params![publication_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, bool>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<f64>>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(
                current_page,
                progress,
                completed,
                settings,
                updated_at,
                current_chapter,
                total_reading_seconds,
            )| {
                Ok(ReadingState {
                    publication_id,
                    current_page: usize::try_from(current_page.max(0)).unwrap_or(0),
                    progress,
                    completed,
                    current_chapter,
                    total_reading_seconds: u64::try_from(total_reading_seconds.max(0)).unwrap_or(0),
                    settings: from_json(&settings)?,
                    updated_at: parse_datetime(&updated_at)?,
                })
            },
        )
        .transpose()
    }

    pub fn record_reading_time(&self, publication_id: Uuid, elapsed_seconds: u64) -> Result<u64> {
        let elapsed_seconds = elapsed_seconds.min(120);
        let connection = self.lock()?;
        if elapsed_seconds > 0 {
            let updated = connection.execute(
                "
                UPDATE reading_state
                SET total_reading_seconds = total_reading_seconds + ?1,
                    updated_at = ?2
                WHERE publication_id = ?3
                ",
                params![
                    elapsed_seconds as i64,
                    Utc::now().to_rfc3339(),
                    publication_id.to_string(),
                ],
            )?;
            if updated == 0 {
                return Err(KomaError::Other(
                    "publication is not in the library".to_owned(),
                ));
            }
        }
        let seconds: i64 = connection
            .query_row(
                "SELECT total_reading_seconds FROM reading_state WHERE publication_id = ?1",
                params![publication_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
        Ok(u64::try_from(seconds.max(0)).unwrap_or(0))
    }

    pub fn completed_chapter(
        &self,
        publication_id: Uuid,
        current_page: usize,
    ) -> Result<Option<u32>> {
        let connection = self.lock()?;
        let chapters_json: String = connection
            .query_row(
                "SELECT chapters_json FROM publications WHERE id = ?1",
                params![publication_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
        let chapters: Vec<ChapterRange> = from_json(&chapters_json)?;
        Ok(chapters
            .iter()
            .filter(|chapter| {
                chapter.number.is_finite()
                    && chapter.number >= 1.0
                    && chapter.end_page_index <= current_page
            })
            .map(|chapter| chapter.number.floor() as u32)
            .max())
    }

    pub fn add_bookmark(
        &self,
        publication_id: Uuid,
        page_index: usize,
        label: Option<&str>,
        note: Option<&str>,
    ) -> Result<Bookmark> {
        let connection = self.lock()?;
        let page_count: i64 = connection
            .query_row(
                "SELECT page_count FROM publications WHERE id = ?1",
                params![publication_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
        if page_index >= usize::try_from(page_count.max(0)).unwrap_or(0) {
            return Err(KomaError::PageOutOfRange { index: page_index });
        }
        validate_annotation(label, note)?;
        let bookmark = Bookmark {
            id: Uuid::now_v7(),
            publication_id,
            page_index,
            label: clean_optional(label),
            note: clean_optional(note),
            created_at: Utc::now(),
        };
        connection.execute(
            "
            INSERT INTO bookmarks (id, publication_id, page_index, label, note, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ",
            params![
                bookmark.id.to_string(),
                bookmark.publication_id.to_string(),
                bookmark.page_index as i64,
                bookmark.label,
                bookmark.note,
                bookmark.created_at.to_rfc3339(),
            ],
        )?;
        Ok(bookmark)
    }

    pub fn update_bookmark(
        &self,
        id: Uuid,
        label: Option<&str>,
        note: Option<&str>,
    ) -> Result<Bookmark> {
        validate_annotation(label, note)?;
        let connection = self.lock()?;
        if connection.execute(
            "UPDATE bookmarks SET label = ?1, note = ?2 WHERE id = ?3",
            params![clean_optional(label), clean_optional(note), id.to_string()],
        )? == 0
        {
            return Err(KomaError::Other(
                "the bookmark is not in the library".to_owned(),
            ));
        }
        connection
            .query_row(
                "
                SELECT publication_id, page_index, label, note, created_at
                FROM bookmarks WHERE id = ?1
                ",
                params![id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .map_err(Into::into)
            .and_then(|(publication_id, page_index, label, note, created_at)| {
                Ok(Bookmark {
                    id,
                    publication_id: parse_uuid(&publication_id)?,
                    page_index: usize::try_from(page_index.max(0)).unwrap_or(0),
                    label,
                    note,
                    created_at: parse_datetime(&created_at)?,
                })
            })
    }

    pub fn bookmarks(&self, publication_id: Uuid) -> Result<Vec<Bookmark>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "
            SELECT id, page_index, label, note, created_at
            FROM bookmarks
            WHERE publication_id = ?1
            ORDER BY page_index, created_at
            ",
        )?;
        let mut rows = statement.query(params![publication_id.to_string()])?;
        let mut bookmarks = Vec::new();
        while let Some(row) = rows.next()? {
            bookmarks.push(Bookmark {
                id: parse_uuid(&row.get::<_, String>(0)?)?,
                publication_id,
                page_index: usize::try_from(row.get::<_, i64>(1)?.max(0)).unwrap_or(0),
                label: row.get(2)?,
                note: row.get(3)?,
                created_at: parse_datetime(&row.get::<_, String>(4)?)?,
            });
        }
        Ok(bookmarks)
    }

    pub fn remove_bookmark(&self, id: Uuid) -> Result<bool> {
        let connection = self.lock()?;
        Ok(connection.execute(
            "DELETE FROM bookmarks WHERE id = ?1",
            params![id.to_string()],
        )? > 0)
    }

    pub fn save_import_receipt(&self, receipt: &ImportReceipt) -> Result<()> {
        let connection = self.lock()?;
        connection.execute(
            "
            INSERT INTO import_receipts (
                id, provider, source_url, eligibility_url, eligibility_status,
                checked_at, page_count, output_path, output_hash, adapter_version
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                provider = excluded.provider,
                source_url = excluded.source_url,
                eligibility_url = excluded.eligibility_url,
                eligibility_status = excluded.eligibility_status,
                checked_at = excluded.checked_at,
                page_count = excluded.page_count,
                output_path = excluded.output_path,
                output_hash = excluded.output_hash,
                adapter_version = excluded.adapter_version
            ",
            params![
                receipt.id.to_string(),
                receipt.provider,
                receipt.source_url,
                receipt.eligibility_url,
                receipt.eligibility_status as i64,
                receipt.checked_at.to_rfc3339(),
                receipt.page_count as i64,
                path_to_database(&receipt.output_path),
                receipt.output_hash,
                receipt.adapter_version,
            ],
        )?;
        Ok(())
    }

    pub fn export_backup(&self) -> Result<LibraryBackup> {
        let items = self.list(true, None)?;
        let mut reading_states = Vec::new();
        let mut bookmarks = Vec::new();
        for item in &items {
            if let Some(state) = self.reading_state(item.id)? {
                reading_states.push(state);
            }
            bookmarks.extend(self.bookmarks(item.id)?);
        }
        Ok(LibraryBackup {
            schema_version: LIBRARY_SCHEMA_VERSION,
            exported_at: Utc::now(),
            items,
            reading_states,
            bookmarks,
            import_receipts: self.import_receipts()?,
            metadata_overrides: self.metadata_overrides()?,
            online_sources: self.online_sources()?,
        })
    }

    pub fn restore_backup(&self, backup: &LibraryBackup) -> Result<BackupRestoreReport> {
        if backup.schema_version > LIBRARY_SCHEMA_VERSION {
            return Err(KomaError::Other(format!(
                "this backup uses library schema {}, but this Koma build supports up to {}",
                backup.schema_version, LIBRARY_SCHEMA_VERSION
            )));
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let mut report = BackupRestoreReport::default();

        for item in &backup.items {
            let is_missing = item.format != PublicationFormat::Online && !item.path.exists();
            transaction.execute(
                "
                INSERT INTO publications (
                    id, path, format_json, fingerprint, title, series, number, volume,
                    page_count, cover_data_url, added_at, last_opened_at,
                    is_hidden, is_missing, is_favorite
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                ON CONFLICT(path) DO UPDATE SET
                    id = excluded.id,
                    format_json = excluded.format_json,
                    title = excluded.title,
                    series = excluded.series,
                    number = excluded.number,
                    volume = excluded.volume,
                    page_count = excluded.page_count,
                    cover_data_url = COALESCE(excluded.cover_data_url, publications.cover_data_url),
                    last_opened_at = excluded.last_opened_at,
                    is_hidden = excluded.is_hidden,
                    is_missing = excluded.is_missing,
                    is_favorite = excluded.is_favorite
                ",
                params![
                    item.id.to_string(),
                    path_to_database(&item.path),
                    to_json(&item.format)?,
                    format!("restored:{}", item.id),
                    item.title,
                    item.series,
                    item.number,
                    item.volume,
                    item.page_count as i64,
                    item.cover_data_url,
                    item.added_at.to_rfc3339(),
                    item.last_opened_at.map(|value| value.to_rfc3339()),
                    item.is_hidden,
                    is_missing,
                    item.is_favorite,
                ],
            )?;
            report.publications += 1;
            report.missing_sources += usize::from(is_missing);
        }

        for state in &backup.reading_states {
            let page_count = backup
                .items
                .iter()
                .find(|item| item.id == state.publication_id)
                .map(|item| item.page_count)
                .unwrap_or(0);
            let current_page = state.current_page.min(page_count.saturating_sub(1));
            let progress = state.progress.clamp(0.0, 1.0);
            transaction.execute(
                "
                INSERT INTO reading_state (
                    publication_id, current_page, progress, completed, settings_json,
                    updated_at, current_chapter, total_reading_seconds
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(publication_id) DO UPDATE SET
                    current_page = excluded.current_page,
                    progress = excluded.progress,
                    completed = excluded.completed,
                    settings_json = excluded.settings_json,
                    current_chapter = excluded.current_chapter,
                    total_reading_seconds = excluded.total_reading_seconds,
                    updated_at = excluded.updated_at
                ",
                params![
                    state.publication_id.to_string(),
                    current_page as i64,
                    progress,
                    state.completed && page_count > 0,
                    to_json(&state.settings)?,
                    state.updated_at.to_rfc3339(),
                    state.current_chapter,
                    state.total_reading_seconds as i64,
                ],
            )?;
            report.reading_states += 1;
        }

        for bookmark in &backup.bookmarks {
            transaction.execute(
                "
                INSERT INTO bookmarks (id, publication_id, page_index, label, note, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(id) DO UPDATE SET
                    page_index = excluded.page_index,
                    label = excluded.label,
                    note = excluded.note,
                    created_at = excluded.created_at
                ",
                params![
                    bookmark.id.to_string(),
                    bookmark.publication_id.to_string(),
                    bookmark.page_index as i64,
                    bookmark.label,
                    bookmark.note,
                    bookmark.created_at.to_rfc3339(),
                ],
            )?;
            report.bookmarks += 1;
        }

        for receipt in &backup.import_receipts {
            transaction.execute(
                "
                INSERT INTO import_receipts (
                    id, provider, source_url, eligibility_url, eligibility_status,
                    checked_at, page_count, output_path, output_hash, adapter_version
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(id) DO UPDATE SET
                    provider = excluded.provider,
                    source_url = excluded.source_url,
                    eligibility_url = excluded.eligibility_url,
                    eligibility_status = excluded.eligibility_status,
                    checked_at = excluded.checked_at,
                    page_count = excluded.page_count,
                    output_path = excluded.output_path,
                    output_hash = excluded.output_hash,
                    adapter_version = excluded.adapter_version
                ",
                params![
                    receipt.id.to_string(),
                    receipt.provider,
                    receipt.source_url,
                    receipt.eligibility_url,
                    receipt.eligibility_status as i64,
                    receipt.checked_at.to_rfc3339(),
                    receipt.page_count as i64,
                    path_to_database(&receipt.output_path),
                    receipt.output_hash,
                    receipt.adapter_version,
                ],
            )?;
            report.import_receipts += 1;
        }

        for stored in &backup.metadata_overrides {
            transaction.execute(
                "
                INSERT INTO metadata_overrides (publication_id, metadata_json, updated_at)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(publication_id) DO UPDATE SET
                    metadata_json = excluded.metadata_json,
                    updated_at = excluded.updated_at
                ",
                params![
                    stored.publication_id.to_string(),
                    to_json(&stored.metadata)?,
                    Utc::now().to_rfc3339(),
                ],
            )?;
            report.metadata_overrides += 1;
        }
        for stored in &backup.online_sources {
            transaction.execute(
                "INSERT OR REPLACE INTO online_sources (publication_id, source_json, updated_at) VALUES (?1, ?2, ?3)",
                params![
                    stored.publication_id.to_string(),
                    to_json(&stored.publication)?,
                    Utc::now().to_rfc3339(),
                ],
            )?;
            report.online_sources += 1;
        }

        transaction.commit()?;
        Ok(report)
    }

    fn metadata_overrides(&self) -> Result<Vec<StoredMetadata>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT publication_id, metadata_json FROM metadata_overrides ORDER BY publication_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut values = Vec::new();
        for row in rows {
            let (publication_id, metadata) = row?;
            values.push(StoredMetadata {
                publication_id: parse_uuid(&publication_id)?,
                metadata: from_json(&metadata)?,
            });
        }
        Ok(values)
    }

    fn online_sources(&self) -> Result<Vec<StoredOnlineSource>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT publication_id, source_json FROM online_sources ORDER BY publication_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut sources = Vec::new();
        for row in rows {
            let (publication_id, source_json) = row?;
            sources.push(StoredOnlineSource {
                publication_id: parse_uuid(&publication_id)?,
                publication: from_json(&source_json)?,
            });
        }
        Ok(sources)
    }

    fn import_receipts(&self) -> Result<Vec<ImportReceipt>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "
            SELECT id, provider, source_url, eligibility_url, eligibility_status,
                   checked_at, page_count, output_path, output_hash, adapter_version
            FROM import_receipts
            ORDER BY checked_at DESC
            ",
        )?;
        let mut rows = statement.query([])?;
        let mut receipts = Vec::new();
        while let Some(row) = rows.next()? {
            receipts.push(ImportReceipt {
                id: parse_uuid(&row.get::<_, String>(0)?)?,
                provider: row.get(1)?,
                source_url: row.get(2)?,
                eligibility_url: row.get(3)?,
                eligibility_status: u16::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                checked_at: parse_datetime(&row.get::<_, String>(5)?)?,
                page_count: usize::try_from(row.get::<_, i64>(6)?.max(0)).unwrap_or(0),
                output_path: PathBuf::from(row.get::<_, String>(7)?),
                output_hash: row.get(8)?,
                adapter_version: row.get(9)?,
            });
        }
        Ok(receipts)
    }

    pub fn refresh_missing_flags(&self) -> Result<usize> {
        let connection = self.lock()?;
        let mut statement =
            connection.prepare("SELECT id, path, is_missing, format_json FROM publications")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut updates = Vec::new();
        for row in rows {
            let (id, path, was_missing, format_json) = row?;
            let format: PublicationFormat = from_json(&format_json)?;
            let is_missing = format != PublicationFormat::Online && !Path::new(&path).exists();
            if is_missing != was_missing {
                updates.push((id, is_missing));
            }
        }
        drop(statement);
        for (id, is_missing) in &updates {
            connection.execute(
                "UPDATE publications SET is_missing = ?1 WHERE id = ?2",
                params![is_missing, id],
            )?;
        }
        Ok(updates.len())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| KomaError::Other("the library database lock was poisoned".to_owned()))
    }
}

fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS publications (
            id TEXT PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            format_json TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            title TEXT NOT NULL,
            series TEXT,
            number TEXT,
            volume INTEGER,
            page_count INTEGER NOT NULL CHECK (page_count >= 0),
            chapters_json TEXT NOT NULL DEFAULT '[]',
            cover_data_url TEXT,
            added_at TEXT NOT NULL,
            modified_at TEXT,
            last_opened_at TEXT,
            is_hidden INTEGER NOT NULL DEFAULT 0 CHECK (is_hidden IN (0, 1)),
            is_missing INTEGER NOT NULL DEFAULT 0 CHECK (is_missing IN (0, 1)),
            is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0, 1))
        );

        CREATE TABLE IF NOT EXISTS reading_state (
            publication_id TEXT PRIMARY KEY,
            current_page INTEGER NOT NULL DEFAULT 0 CHECK (current_page >= 0),
            progress REAL NOT NULL DEFAULT 0.0 CHECK (progress >= 0.0 AND progress <= 1.0),
            completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1)),
            current_chapter REAL,
            total_reading_seconds INTEGER NOT NULL DEFAULT 0
                CHECK (total_reading_seconds >= 0),
            settings_json TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (publication_id) REFERENCES publications(id)
                ON DELETE CASCADE ON UPDATE CASCADE
        );

        CREATE TABLE IF NOT EXISTS bookmarks (
            id TEXT PRIMARY KEY,
            publication_id TEXT NOT NULL,
            page_index INTEGER NOT NULL CHECK (page_index >= 0),
            label TEXT,
            note TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (publication_id) REFERENCES publications(id)
                ON DELETE CASCADE ON UPDATE CASCADE
        );

        CREATE INDEX IF NOT EXISTS bookmarks_publication_page
            ON bookmarks(publication_id, page_index);

        CREATE TABLE IF NOT EXISTS import_receipts (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            source_url TEXT NOT NULL,
            eligibility_url TEXT NOT NULL,
            eligibility_status INTEGER NOT NULL,
            checked_at TEXT NOT NULL,
            page_count INTEGER NOT NULL,
            output_path TEXT NOT NULL,
            output_hash TEXT NOT NULL,
            adapter_version TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS metadata_overrides (
            publication_id TEXT PRIMARY KEY,
            metadata_json TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (publication_id) REFERENCES publications(id)
                ON DELETE CASCADE ON UPDATE CASCADE
        );

        CREATE TABLE IF NOT EXISTS online_sources (
            publication_id TEXT PRIMARY KEY,
            source_json TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            FOREIGN KEY (publication_id) REFERENCES publications(id)
                ON DELETE CASCADE ON UPDATE CASCADE
        );

        CREATE TABLE IF NOT EXISTS library_folders (
            id TEXT PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
            scan_interval_minutes INTEGER NOT NULL DEFAULT 60
                CHECK (scan_interval_minutes BETWEEN 5 AND 10080),
            last_scanned_at TEXT,
            last_imported_count INTEGER NOT NULL DEFAULT 0,
            last_failure_count INTEGER NOT NULL DEFAULT 0,
            last_error TEXT
        );
        ",
    )?;
    add_column_if_missing(
        connection,
        "publications",
        "chapters_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    add_column_if_missing(connection, "reading_state", "current_chapter", "REAL")?;
    add_column_if_missing(
        connection,
        "reading_state",
        "total_reading_seconds",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    connection.pragma_update(None, "user_version", LIBRARY_SCHEMA_VERSION)?;
    Ok(())
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for existing in columns {
        if existing? == column {
            return Ok(());
        }
    }
    connection.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )?;
    Ok(())
}

fn library_folder_by_id(connection: &Connection, id: Uuid) -> Result<Option<LibraryFolder>> {
    connection
        .query_row(
            "
            SELECT id, path, enabled, scan_interval_minutes, last_scanned_at,
                   last_imported_count, last_failure_count, last_error
            FROM library_folders
            WHERE id = ?1
            ",
            params![id.to_string()],
            library_folder_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn library_folder_by_path(connection: &Connection, path: &Path) -> Result<Option<LibraryFolder>> {
    connection
        .query_row(
            "
            SELECT id, path, enabled, scan_interval_minutes, last_scanned_at,
                   last_imported_count, last_failure_count, last_error
            FROM library_folders
            WHERE path = ?1
            ",
            params![path_to_database(path)],
            library_folder_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn library_folder_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryFolder> {
    let id: String = row.get(0)?;
    let last_scanned_at: Option<String> = row.get(4)?;
    Ok(LibraryFolder {
        id: Uuid::parse_str(&id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        path: PathBuf::from(row.get::<_, String>(1)?),
        enabled: row.get(2)?,
        scan_interval_minutes: u32::try_from(row.get::<_, i64>(3)?).unwrap_or(60),
        last_scanned_at: last_scanned_at
            .map(|value| {
                DateTime::parse_from_rfc3339(&value)
                    .map(|date| date.with_timezone(&Utc))
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })
            })
            .transpose()?,
        last_imported_count: usize::try_from(row.get::<_, i64>(5)?.max(0)).unwrap_or(0),
        last_failure_count: usize::try_from(row.get::<_, i64>(6)?.max(0)).unwrap_or(0),
        last_error: row.get(7)?,
    })
}

fn item_by_id(connection: &Connection, id: Uuid) -> Result<Option<LibraryItem>> {
    let sql = format!(
        "SELECT {ITEM_COLUMNS}
         FROM publications p
         LEFT JOIN reading_state r ON r.publication_id = p.id
         WHERE p.id = ?1"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query(params![id.to_string()])?;
    rows.next()?.map(item_from_row).transpose()
}

fn item_from_row(row: &rusqlite::Row<'_>) -> Result<LibraryItem> {
    let id = parse_uuid(&row.get::<_, String>(0)?)?;
    let format = from_json(&row.get::<_, String>(2)?)?;
    let current_page = usize::try_from(row.get::<_, i64>(8)?.max(0)).unwrap_or(0);
    let page_count = usize::try_from(row.get::<_, i64>(7)?.max(0)).unwrap_or(0);
    Ok(LibraryItem {
        id,
        path: PathBuf::from(row.get::<_, String>(1)?),
        format,
        title: row.get(3)?,
        series: row.get(4)?,
        number: row.get(5)?,
        volume: row.get(6)?,
        page_count,
        current_page: current_page.min(page_count.saturating_sub(1)),
        current_chapter: row.get(17)?,
        progress: row.get::<_, f64>(9)?.clamp(0.0, 1.0),
        total_reading_seconds: u64::try_from(row.get::<_, i64>(18)?.max(0)).unwrap_or(0),
        is_completed: row.get(10)?,
        is_hidden: row.get(11)?,
        is_missing: row.get(12)?,
        is_favorite: row.get(13)?,
        cover_data_url: row.get(14)?,
        added_at: parse_datetime(&row.get::<_, String>(15)?)?,
        last_opened_at: row
            .get::<_, Option<String>>(16)?
            .map(|value| parse_datetime(&value))
            .transpose()?,
    })
}

fn chapter_at_page(chapters: &[ChapterRange], page_index: usize) -> Option<f64> {
    chapters
        .iter()
        .find(|chapter| {
            page_index >= chapter.start_page_index && page_index <= chapter.end_page_index
        })
        .map(|chapter| chapter.number)
}

fn thumbnail_data_url(bytes: &[u8]) -> Result<String> {
    let image = image::load_from_memory(bytes)
        .map_err(|error| KomaError::InvalidImage(error.to_string()))?;
    let thumbnail = image.thumbnail(320, 480);
    let mut encoded = Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut encoded, image::ImageFormat::WebP)
        .map_err(|error| KomaError::InvalidImage(error.to_string()))?;
    Ok(format!(
        "data:image/webp;base64,{}",
        STANDARD.encode(encoded.into_inner())
    ))
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| KomaError::Other(format!("invalid stored date: {error}")))
}

fn parse_uuid(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|error| KomaError::Other(format!("invalid stored identifier: {error}")))
}

fn to_json(value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|error| KomaError::Other(format!("could not serialize library data: {error}")))
}

fn from_json<T: for<'de> Deserialize<'de>>(value: &str) -> Result<T> {
    serde_json::from_str(value)
        .map_err(|error| KomaError::Other(format!("could not read library data: {error}")))
}

fn path_to_database(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn validate_annotation(label: Option<&str>, note: Option<&str>) -> Result<()> {
    if label.is_some_and(|value| value.len() > 512) {
        return Err(KomaError::Other(
            "a bookmark label cannot exceed 512 bytes".to_owned(),
        ));
    }
    if note.is_some_and(|value| value.len() > 64 * 1024) {
        return Err(KomaError::Other(
            "a bookmark note cannot exceed 64 KiB".to_owned(),
        ));
    }
    Ok(())
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::Utc;
    use tempfile::tempdir;

    use super::Library;
    use crate::{
        formats::ZipPublication,
        importer::{ImportScope, RemoteChapter, RemotePage, RemotePublication},
        metadata::ComicInfo,
        model::{ChapterRange, KomaArchiveMetadata, PublicationFormat, ReadingDirection},
    };

    fn remote_publication() -> RemotePublication {
        RemotePublication {
            provider: "Fixture".to_owned(),
            source_url: "https://reader.example/title/test".to_owned(),
            eligibility_url: "https://reader.example/title/test".to_owned(),
            eligibility_status: 200,
            title: "Online proof".to_owned(),
            language: Some("en".to_owned()),
            scope: ImportScope::Series,
            volume_id: None,
            chapter_id: None,
            selected_chapter_ids: vec![],
            chapters: vec![
                RemoteChapter {
                    id: Some("10".to_owned()),
                    number: 1.0,
                    title: Some("One".to_owned()),
                    volume: Some(1.0),
                    pages: vec![RemotePage {
                        url: "https://images.reader.example/1.png".to_owned(),
                        width: Some(1),
                        height: Some(1),
                    }],
                },
                RemoteChapter {
                    id: Some("11".to_owned()),
                    number: 2.0,
                    title: Some("Two".to_owned()),
                    volume: Some(1.0),
                    pages: vec![RemotePage {
                        url: "https://images.reader.example/2.png".to_owned(),
                        width: Some(1),
                        height: Some(1),
                    }],
                },
            ],
            chapter_catalog: Vec::new(),
            volume_catalog: Vec::new(),
            allowed_page_hosts: vec!["images.reader.example".to_owned()],
            allow_local_network: false,
        }
    }

    fn tiny_png() -> Vec<u8> {
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 8, 215, 99, 248, 207,
            192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
            96, 130,
        ]
    }

    fn create_cbz(path: &Path) {
        let info = ComicInfo {
            title: Some("A Quiet Proof".to_owned()),
            manga: Some("YesAndRightToLeft".to_owned()),
            ..ComicInfo::default()
        };
        ZipPublication::write_cbz(
            path,
            [
                ("001.png".to_owned(), tiny_png()),
                ("002.png".to_owned(), tiny_png()),
            ],
            &info,
        )
        .expect("write fixture");
    }

    #[test]
    fn online_publications_persist_ranges_and_never_look_missing() {
        let library = Library::in_memory().expect("library");
        let item = library
            .add_online(&remote_publication(), None)
            .expect("online item");
        assert_eq!(item.format, PublicationFormat::Online);
        let manifest = library
            .online_manifest(item.id)
            .expect("manifest")
            .expect("online manifest");
        assert_eq!(manifest.pages.len(), 2);
        assert_eq!(manifest.chapters[1].start_page_index, 1);
        library.refresh_missing_flags().expect("missing refresh");
        assert!(!library.get(item.id).expect("get").expect("item").is_missing);
        let backup = library.export_backup().expect("backup");
        assert_eq!(backup.online_sources.len(), 1);
    }

    #[test]
    fn persists_progress_visibility_and_bookmarks() {
        let directory = tempdir().expect("temp directory");
        let archive = directory.path().join("proof.cbz");
        create_cbz(&archive);
        let database = directory.path().join("library.sqlite3");
        let id;
        {
            let library = Library::open(&database).expect("open library");
            let item = library.import_path(&archive, None).expect("import");
            id = item.id;
            library.set_hidden(id, true).expect("hide");
            let state = library.save_progress(id, 1, None).expect("progress");
            assert!(state.completed);
            library
                .add_bookmark(id, 1, Some("Ending"), None)
                .expect("bookmark");
        }
        {
            let library = Library::open(&database).expect("reopen library");
            assert!(library.list(false, None).expect("visible list").is_empty());
            let item = library.get(id).expect("get").expect("item exists");
            assert!(item.is_hidden);
            assert_eq!(item.format, PublicationFormat::Cbz);
            assert_eq!(item.current_page, 1);
            assert!(item.is_completed);
            assert_eq!(library.bookmarks(id).expect("bookmarks").len(), 1);
            let state = library
                .reading_state(id)
                .expect("state")
                .expect("state exists");
            assert_eq!(state.settings.direction, ReadingDirection::Automatic);
        }
    }

    #[test]
    fn persists_chapter_progress_and_active_reading_time() {
        let directory = tempdir().expect("temp directory");
        let first = directory.path().join("001.png");
        let second = directory.path().join("002.png");
        std::fs::write(&first, tiny_png()).expect("first page");
        std::fs::write(&second, tiny_png()).expect("second page");
        let archive = directory.path().join("series.cbz");
        ZipPublication::write_cbz_from_files_with_metadata(
            &archive,
            [
                ("001.png".to_owned(), first),
                ("002.png".to_owned(), second),
            ],
            &ComicInfo::default(),
            Some(&KomaArchiveMetadata::new(vec![
                ChapterRange {
                    id: Some("10".to_owned()),
                    number: 0.5,
                    title: Some("Prologue".to_owned()),
                    start_page_index: 0,
                    end_page_index: 0,
                },
                ChapterRange {
                    id: Some("11".to_owned()),
                    number: 1.0,
                    title: None,
                    start_page_index: 1,
                    end_page_index: 1,
                },
            ])),
        )
        .expect("write archive");

        let library = Library::in_memory().expect("library");
        let item = library.import_path(&archive, None).expect("import");
        let first_state = library.save_progress(item.id, 0, None).expect("progress");
        assert_eq!(first_state.current_chapter, Some(0.5));
        assert_eq!(
            library.completed_chapter(item.id, 0).expect("completed"),
            None
        );
        let second_state = library.save_progress(item.id, 1, None).expect("progress");
        assert_eq!(second_state.current_chapter, Some(1.0));
        assert_eq!(
            library.completed_chapter(item.id, 1).expect("completed"),
            Some(1)
        );
        assert_eq!(
            library
                .record_reading_time(item.id, 300)
                .expect("record time"),
            120
        );
        let stored = library.get(item.id).expect("get").expect("item");
        assert_eq!(stored.total_reading_seconds, 120);
    }

    #[test]
    fn scans_archives_and_image_directories() {
        let directory = tempdir().expect("temp directory");
        create_cbz(&directory.path().join("archive.cbz"));
        let pages = directory.path().join("folder-book");
        std::fs::create_dir(&pages).expect("create page folder");
        std::fs::write(pages.join("1.png"), tiny_png()).expect("write page");

        let library = Library::in_memory().expect("library");
        let report = library.scan_folder(directory.path()).expect("scan");
        assert_eq!(report.imported.len(), 2);
        assert!(report.failures.is_empty());
    }

    #[test]
    fn manages_periodic_folders_and_skips_unchanged_archives() {
        let directory = tempdir().expect("temp directory");
        create_cbz(&directory.path().join("archive.cbz"));
        let library = Library::in_memory().expect("library");

        let folder = library
            .add_library_folder(directory.path(), 1)
            .expect("add managed folder");
        assert_eq!(folder.scan_interval_minutes, 5);
        assert_eq!(
            library
                .due_library_folders(Utc::now())
                .expect("due folders")
                .len(),
            1
        );

        let first = library.scan_folder(directory.path()).expect("first scan");
        assert_eq!(first.imported.len(), 1);
        library
            .record_library_folder_scan(folder.id, Some(&first), None)
            .expect("record scan");
        let stored = library
            .library_folders()
            .expect("folders")
            .into_iter()
            .next()
            .expect("stored folder");
        assert!(stored.last_scanned_at.is_some());
        assert_eq!(stored.last_imported_count, 1);
        assert!(
            library
                .due_library_folders(Utc::now())
                .expect("due folders")
                .is_empty()
        );

        let second = library.scan_folder(directory.path()).expect("second scan");
        assert_eq!(second.unchanged, 1);
        assert!(second.imported.is_empty());

        let disabled = library
            .update_library_folder(folder.id, false, 60)
            .expect("disable folder");
        assert!(!disabled.enabled);
        assert!(
            library
                .due_library_folders(Utc::now())
                .expect("disabled folders")
                .is_empty()
        );
        assert!(
            library
                .remove_library_folder(folder.id)
                .expect("remove folder")
        );
        assert!(library.library_folders().expect("folders").is_empty());
    }

    #[test]
    fn relinks_a_moved_source_without_losing_library_state() {
        let directory = tempdir().expect("temp directory");
        let original = directory.path().join("original.cbz");
        let moved = directory.path().join("moved.cbz");
        create_cbz(&original);
        let library = Library::in_memory().expect("library");
        let item = library.import_path(&original, None).expect("import");
        library.set_favorite(item.id, true).expect("favorite");
        library.save_progress(item.id, 1, None).expect("progress");
        library
            .add_bookmark(item.id, 1, Some("Kept"), Some("Still here"))
            .expect("bookmark");
        std::fs::rename(&original, &moved).expect("move source");
        library.refresh_missing_flags().expect("refresh missing");
        assert!(library.get(item.id).expect("get").expect("item").is_missing);

        let relinked = library.relink(item.id, &moved, None).expect("relink");
        assert_eq!(relinked.id, item.id);
        assert_eq!(relinked.path, moved);
        assert!(!relinked.is_missing);
        assert!(relinked.is_favorite);
        assert!(relinked.is_completed);
        assert_eq!(library.bookmarks(item.id).expect("bookmarks").len(), 1);
    }

    #[test]
    fn explicit_reading_status_can_reset_a_single_page_publication() {
        let directory = tempdir().expect("temp directory");
        let archive = directory.path().join("single.cbz");
        ZipPublication::write_cbz(
            &archive,
            [("001.png".to_owned(), tiny_png())],
            &ComicInfo::default(),
        )
        .expect("write fixture");
        let library = Library::in_memory().expect("library");
        let item = library.import_path(&archive, None).expect("import");

        let completed = library
            .set_reading_status(item.id, true)
            .expect("mark read");
        assert!(completed.completed);
        assert_eq!(completed.progress, 1.0);

        let unread = library
            .set_reading_status(item.id, false)
            .expect("mark unread");
        assert!(!unread.completed);
        assert_eq!(unread.progress, 0.0);
        assert_eq!(unread.current_page, 0);
    }

    #[test]
    fn bookmark_annotations_are_editable_and_bounded() {
        let directory = tempdir().expect("temp directory");
        let archive = directory.path().join("annotation.cbz");
        create_cbz(&archive);
        let library = Library::in_memory().expect("library");
        let item = library.import_path(&archive, None).expect("import");
        let bookmark = library
            .add_bookmark(item.id, 0, Some("First thought"), Some("Draft"))
            .expect("bookmark");
        let updated = library
            .update_bookmark(
                bookmark.id,
                Some("Opening composition"),
                Some("Return to the use of negative space."),
            )
            .expect("update");
        assert_eq!(updated.label.as_deref(), Some("Opening composition"));
        assert_eq!(
            updated.note.as_deref(),
            Some("Return to the use of negative space.")
        );
        assert!(
            library
                .update_bookmark(bookmark.id, None, Some(&"x".repeat(64 * 1024 + 1)))
                .is_err()
        );
    }

    #[test]
    fn restores_progress_bookmarks_flags_and_metadata_without_deleting_sources() {
        let directory = tempdir().expect("temp directory");
        let archive = directory.path().join("restore-proof.cbz");
        create_cbz(&archive);
        let source = Library::in_memory().expect("source library");
        let item = source.import_path(&archive, None).expect("import source");
        source.set_favorite(item.id, true).expect("favorite");
        source.save_progress(item.id, 1, None).expect("progress");
        source
            .add_bookmark(item.id, 1, Some("Proof"), Some("Restored"))
            .expect("bookmark");
        let mut metadata = crate::model::PublicationMetadata::inferred_from_path(&archive);
        metadata.title = "Restored title".to_owned();
        metadata.tags = vec!["proof".to_owned(), "portable".to_owned()];
        source
            .save_metadata_override(item.id, &metadata)
            .expect("metadata");
        let backup = source.export_backup().expect("backup");

        let restored = Library::in_memory().expect("restored library");
        let report = restored.restore_backup(&backup).expect("restore");
        assert_eq!(report.publications, 1);
        assert_eq!(report.reading_states, 1);
        assert_eq!(report.bookmarks, 1);
        assert_eq!(report.metadata_overrides, 1);
        assert_eq!(report.missing_sources, 0);
        let item = restored.get(item.id).expect("get").expect("restored item");
        assert!(item.is_favorite);
        assert!(item.is_completed);
        assert_eq!(item.title, "Restored title");
        assert_eq!(restored.bookmarks(item.id).expect("bookmarks").len(), 1);
        assert_eq!(
            restored
                .metadata_override(item.id)
                .expect("metadata")
                .expect("metadata exists")
                .tags,
            vec!["proof", "portable"]
        );
        assert!(
            archive.exists(),
            "restore never moves or deletes source files"
        );
    }
}
