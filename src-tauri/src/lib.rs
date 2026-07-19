use std::{
    collections::{BTreeSet, HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
#[cfg(desktop)]
use discord_rich_presence::{DiscordIpc, DiscordIpcClient, activity};
use koma_core::{
    BackupRestoreReport, Bookmark, ConnectorManifest, ConnectorSummary, ConversionOptions,
    ConversionReport, DeclarativeImporter, ImportOptions, ImportPreview, ImportReceipt, KomaError,
    Library, LibraryBackup, LibraryFolder, LibraryItem, LibraryScanReport, LinkImporter,
    MangaFireImporter, PageData, PublicationFormat, PublicationInspection, PublicationManifest,
    PublicationMetadata, PublicationReader, ReaderSettings, ReadingState,
    bundled_mangafire_summary, convert_to_cbz, formats::with_metadata,
    inspect_publication as inspect_path, open_publication, repair_to_cbz,
    write_publication_metadata,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_deep_link::DeepLinkExt;
use uuid::Uuid;

mod tracking;

use tracking::{
    TrackingAccount, TrackingMapping, TrackingProvider, TrackingService, TrackingSuggestion,
};

struct AppState {
    library: Arc<Library>,
    importer: Arc<MangaFireImporter>,
    connectors: Mutex<Vec<Arc<DeclarativeImporter>>>,
    pending_connector_packages: Mutex<HashMap<Uuid, Vec<u8>>>,
    pending_file_uploads: Mutex<HashMap<Uuid, PendingFileUpload>>,
    connectors_directory: PathBuf,
    readers: Mutex<HashMap<Uuid, Arc<dyn PublicationReader>>>,
    pending_open_paths: Mutex<Vec<PathBuf>>,
    default_import_directory: PathBuf,
    managed_library_directory: PathBuf,
    tracking: Arc<TrackingService>,
    last_tracking_auth: Mutex<Option<TrackingAuthEvent>>,
    handled_oauth_callbacks: Mutex<BTreeSet<u64>>,
    #[cfg(desktop)]
    discord: Mutex<Option<DiscordIpcClient>>,
}

struct PendingFileUpload {
    temporary_path: PathBuf,
    file_name: String,
    expected_size: u64,
    received_size: u64,
}

impl AppState {
    fn new(
        database_path: &Path,
        default_import_directory: PathBuf,
        managed_library_directory: PathBuf,
        pending_open_paths: Vec<PathBuf>,
    ) -> Result<Self, KomaError> {
        let connectors_directory = database_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("connectors");
        let connectors = load_connectors(&connectors_directory)?;
        let tracking = Arc::new(
            TrackingService::new(
                database_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("tracking.json"),
            )
            .map_err(KomaError::Other)?,
        );
        Ok(Self {
            library: Arc::new(Library::open(database_path)?),
            importer: Arc::new(MangaFireImporter::new()?),
            connectors: Mutex::new(connectors),
            pending_connector_packages: Mutex::new(HashMap::new()),
            pending_file_uploads: Mutex::new(HashMap::new()),
            connectors_directory,
            readers: Mutex::new(HashMap::new()),
            pending_open_paths: Mutex::new(pending_open_paths),
            default_import_directory,
            managed_library_directory,
            tracking,
            last_tracking_auth: Mutex::new(None),
            handled_oauth_callbacks: Mutex::new(BTreeSet::new()),
            #[cfg(desktop)]
            discord: Mutex::new(None),
        })
    }

    fn open_reader(
        &self,
        publication_id: Uuid,
        password: Option<&str>,
    ) -> Result<Arc<dyn PublicationReader>, KomaError> {
        let item = self
            .library
            .get(publication_id)?
            .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
        let mut reader = open_publication(&item.path, password)?;
        if let Some(metadata) = self.library.metadata_override(publication_id)? {
            reader = with_metadata(reader, metadata);
        }
        let reader: Arc<dyn PublicationReader> = Arc::from(reader);
        self.readers
            .lock()
            .map_err(|_| KomaError::Other("the reader cache lock was poisoned".to_owned()))?
            .insert(publication_id, Arc::clone(&reader));
        Ok(reader)
    }

    fn cached_reader(&self, publication_id: Uuid) -> Result<Arc<dyn PublicationReader>, KomaError> {
        self.readers
            .lock()
            .map_err(|_| KomaError::Other("the reader cache lock was poisoned".to_owned()))?
            .get(&publication_id)
            .cloned()
            .ok_or_else(|| {
                KomaError::Other("open this publication before requesting a page".to_owned())
            })
    }

    fn importer_for(&self, source: &str) -> Result<Arc<dyn LinkImporter>, KomaError> {
        if self.importer.recognizes(source) {
            let importer: Arc<dyn LinkImporter> = self.importer.clone();
            return Ok(importer);
        }
        let connectors = self
            .connectors
            .lock()
            .map_err(|_| KomaError::Other("the connector registry lock was poisoned".to_owned()))?;
        connectors
            .iter()
            .find(|connector| connector.recognizes(source))
            .cloned()
            .map(|connector| connector as Arc<dyn LinkImporter>)
            .ok_or_else(|| {
                KomaError::UnsupportedFormat(
                    "no installed connector recognizes this link".to_owned(),
                )
            })
    }
}

fn safe_import_file_name(file_name: &str) -> Result<String, KomaError> {
    let file_name = Path::new(file_name)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| KomaError::Other("the selected file has no usable name".to_owned()))?;
    if file_name.starts_with('.') {
        return Err(KomaError::Other(
            "hidden files cannot be imported".to_owned(),
        ));
    }
    Ok(file_name.to_owned())
}

fn unique_managed_destination(directory: &Path, file_name: &str) -> PathBuf {
    let source_stem = Path::new(file_name)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Imported comic");
    let extension = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("cbz");
    let mut destination = directory.join(file_name);
    let mut suffix = 2_u32;
    while destination.exists() {
        destination = directory.join(format!("{source_stem} {suffix}.{extension}"));
        suffix += 1;
    }
    destination
}

fn load_connector(path: &Path) -> Result<Arc<DeclarativeImporter>, KomaError> {
    #[cfg(target_os = "ios")]
    {
        use objc2_foundation::{NSString, NSURL};
        let path_string = NSString::from_str(&path.to_string_lossy());
        let url = NSURL::fileURLWithPath(&path_string);
        let active = unsafe { url.startAccessingSecurityScopedResource() };
        let result = load_connector_file(path);
        if active {
            unsafe { url.stopAccessingSecurityScopedResource() };
        }
        return result;
    }
    #[cfg(not(target_os = "ios"))]
    load_connector_file(path)
}

fn load_connector_file(path: &Path) -> Result<Arc<DeclarativeImporter>, KomaError> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err(KomaError::ImportDenied(
            "connector package must be a file smaller than 1 MiB".to_owned(),
        ));
    }
    connector_from_bytes(&std::fs::read(path)?)
}

fn connector_from_bytes(bytes: &[u8]) -> Result<Arc<DeclarativeImporter>, KomaError> {
    if bytes.len() > 1024 * 1024 {
        return Err(KomaError::ImportDenied(
            "connector package must be smaller than 1 MiB".to_owned(),
        ));
    }
    let manifest = ConnectorManifest::from_json(bytes)?;
    if manifest.id == "mangafire" {
        return Err(KomaError::ImportDenied(
            "the bundled MangaFire connector cannot be replaced".to_owned(),
        ));
    }
    Ok(Arc::new(DeclarativeImporter::new(manifest)?))
}

fn read_connector_file(path: &Path) -> Result<Vec<u8>, KomaError> {
    #[cfg(target_os = "ios")]
    {
        use objc2_foundation::{NSString, NSURL};
        let path_string = NSString::from_str(&path.to_string_lossy());
        let url = NSURL::fileURLWithPath(&path_string);
        let active = unsafe { url.startAccessingSecurityScopedResource() };
        let result = read_connector_file_unscoped(path);
        if active {
            unsafe { url.stopAccessingSecurityScopedResource() };
        }
        return result;
    }
    #[cfg(not(target_os = "ios"))]
    read_connector_file_unscoped(path)
}

fn read_connector_file_unscoped(path: &Path) -> Result<Vec<u8>, KomaError> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err(KomaError::ImportDenied(
            "connector package must be a file smaller than 1 MiB".to_owned(),
        ));
    }
    Ok(std::fs::read(path)?)
}

fn load_connectors(directory: &Path) -> Result<Vec<Arc<DeclarativeImporter>>, KomaError> {
    std::fs::create_dir_all(directory)?;
    let mut connectors = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".koma-connector.json"))
        {
            continue;
        }
        match load_connector(&path) {
            Ok(connector) => connectors.push(connector),
            Err(error) => eprintln!(
                "Koma skipped invalid connector package {}: {error}",
                path.display()
            ),
        }
    }
    connectors.sort_by(|left, right| {
        left.manifest()
            .name
            .to_lowercase()
            .cmp(&right.manifest().name.to_lowercase())
    });
    Ok(connectors)
}

fn open_paths_from_arguments(arguments: &[String], working_directory: &Path) -> Vec<PathBuf> {
    arguments
        .iter()
        .filter_map(|argument| {
            if argument.starts_with('-') {
                return None;
            }
            let path = if argument.starts_with("file:") {
                url::Url::parse(argument).ok()?.to_file_path().ok()?
            } else {
                PathBuf::from(argument)
            };
            let path = if path.is_absolute() {
                path
            } else {
                working_directory.join(path)
            };
            if !path.exists() || (!path.is_dir() && PublicationFormat::from_path(&path).is_none()) {
                return None;
            }
            Some(path.canonicalize().unwrap_or(path))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn queue_open_paths(app: &AppHandle, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    if let Ok(mut pending) = state.pending_open_paths.lock() {
        for path in paths {
            if !pending.contains(&path) {
                pending.push(path);
            }
        }
    }
    let _ = app.emit("koma://open-paths", ());
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandError {
    code: &'static str,
    message: String,
    recoverable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackingAuthEvent {
    success: bool,
    message: String,
}

impl From<KomaError> for CommandError {
    fn from(error: KomaError) -> Self {
        let (code, recoverable) = match &error {
            KomaError::ImportDenied(_) => ("import_denied", true),
            KomaError::ProviderUnavailable(_) => ("provider_unavailable", true),
            KomaError::ProviderChanged(_) => ("provider_changed", false),
            KomaError::PasswordRequired => ("password_required", true),
            KomaError::MissingSource(_) => ("missing_source", true),
            KomaError::UnsupportedFormat(_) => ("unsupported_format", true),
            KomaError::EmptyPublication => ("empty_publication", true),
            KomaError::PageOutOfRange { .. } => ("page_out_of_range", true),
            KomaError::UnsafeArchiveEntry(_)
            | KomaError::PageTooLarge { .. }
            | KomaError::InvalidImage(_) => ("unsafe_publication", false),
            KomaError::Cancelled => ("cancelled", true),
            KomaError::Database(_)
            | KomaError::Io(_)
            | KomaError::Zip(_)
            | KomaError::Rar(_)
            | KomaError::SevenZip(_)
            | KomaError::Pdf(_)
            | KomaError::Metadata(_)
            | KomaError::MetadataWrite(_)
            | KomaError::Network(_)
            | KomaError::Url(_)
            | KomaError::Other(_) => ("operation_failed", true),
        };
        Self {
            code,
            message: error.to_string(),
            recoverable,
        }
    }
}

impl From<std::io::Error> for CommandError {
    fn from(error: std::io::Error) -> Self {
        KomaError::Io(error).into()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapPayload {
    items: Vec<LibraryItem>,
    default_import_directory: PathBuf,
    default_reader_settings: ReaderSettings,
    import_warning: &'static str,
    app_version: &'static str,
    platform: &'static str,
    supported_formats: [&'static str; 7],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReaderOpenPayload {
    library_id: Uuid,
    manifest: PublicationManifest,
    reading_state: Option<ReadingState>,
    bookmarks: Vec<Bookmark>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PagePayload {
    index: usize,
    mime_type: String,
    data_url: String,
}

impl From<PageData> for PagePayload {
    fn from(page: PageData) -> Self {
        Self {
            index: page.index,
            data_url: format!(
                "data:{};base64,{}",
                page.mime_type,
                STANDARD.encode(page.bytes)
            ),
            mime_type: page.mime_type,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LinkImportResult {
    receipt: ImportReceipt,
    item: LibraryItem,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicationOperationResult {
    report: ConversionReport,
    item: LibraryItem,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetadataSaveResult {
    item: LibraryItem,
    backup_path: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryFolderScanResult {
    folder: LibraryFolder,
    report: LibraryScanReport,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorPackagePreview {
    install_token: Uuid,
    connector: ConnectorSummary,
    allowed_request_hosts: Vec<String>,
    allowed_page_hosts: Vec<String>,
    allow_local_network: bool,
}

#[tauri::command]
fn bootstrap(state: State<'_, AppState>) -> Result<BootstrapPayload, CommandError> {
    state.library.refresh_missing_flags()?;
    Ok(BootstrapPayload {
        items: state.library.list(true, None)?,
        default_import_directory: state.default_import_directory.clone(),
        default_reader_settings: ReaderSettings::default(),
        import_warning: koma_core::importer::IMPORT_WARNING,
        app_version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        supported_formats: [
            "CBZ/ZIP", "CBR/RAR", "CB7/7z", "CBT/TAR", "Folder", "EPUB", "PDF",
        ],
    })
}

#[tauri::command]
fn list_library(
    state: State<'_, AppState>,
    include_hidden: bool,
    search: Option<String>,
) -> Result<Vec<LibraryItem>, CommandError> {
    Ok(state.library.list(include_hidden, search.as_deref())?)
}

#[tauri::command]
fn take_open_paths(state: State<'_, AppState>) -> Result<Vec<PathBuf>, CommandError> {
    let mut pending = state
        .pending_open_paths
        .lock()
        .map_err(|_| KomaError::Other("the open-file queue lock was poisoned".to_owned()))?;
    Ok(std::mem::take(&mut *pending))
}

#[tauri::command]
async fn add_publication(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<String>,
) -> Result<LibraryItem, CommandError> {
    let library = Arc::clone(&state.library);
    let managed_directory = state.managed_library_directory.clone();
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(mobile)]
        let path = copy_into_managed_library(&path, &managed_directory)?;
        #[cfg(not(mobile))]
        let _ = managed_directory;
        library.import_path(path, password.as_deref())
    })
    .await
    .map_err(|error| CommandError {
        code: "operation_failed",
        message: format!("file import task failed: {error}"),
        recoverable: true,
    })?
    .map_err(Into::into)
}

#[tauri::command]
fn begin_file_upload(
    state: State<'_, AppState>,
    file_name: String,
    expected_size: u64,
) -> Result<Uuid, CommandError> {
    const MAX_UPLOAD_BYTES: u64 = 4 * 1024 * 1024 * 1024;
    if expected_size == 0 || expected_size > MAX_UPLOAD_BYTES {
        return Err(KomaError::Other(
            "the selected file is empty or exceeds Koma's 4 GiB limit".to_owned(),
        )
        .into());
    }
    let file_name = safe_import_file_name(&file_name)?;
    let incoming_directory = state.managed_library_directory.join(".incoming");
    std::fs::create_dir_all(&incoming_directory)?;
    let upload_id = Uuid::now_v7();
    let temporary_path = incoming_directory.join(format!("{upload_id}.part"));
    std::fs::File::create(&temporary_path)?;
    state
        .pending_file_uploads
        .lock()
        .map_err(|_| KomaError::Other("the upload registry lock was poisoned".to_owned()))?
        .insert(
            upload_id,
            PendingFileUpload {
                temporary_path,
                file_name,
                expected_size,
                received_size: 0,
            },
        );
    Ok(upload_id)
}

#[tauri::command]
fn append_file_upload(
    state: State<'_, AppState>,
    upload_id: Uuid,
    chunk: String,
) -> Result<u64, CommandError> {
    const MAX_CHUNK_BYTES: usize = 1024 * 1024;
    let bytes = STANDARD
        .decode(chunk)
        .map_err(|error| KomaError::Other(format!("invalid upload chunk: {error}")))?;
    if bytes.is_empty() || bytes.len() > MAX_CHUNK_BYTES {
        return Err(KomaError::Other("invalid upload chunk size".to_owned()).into());
    }
    let mut uploads = state
        .pending_file_uploads
        .lock()
        .map_err(|_| KomaError::Other("the upload registry lock was poisoned".to_owned()))?;
    let upload = uploads.get_mut(&upload_id).ok_or_else(|| {
        KomaError::Other("the mobile file upload expired; choose the file again".to_owned())
    })?;
    let next_size = upload.received_size.saturating_add(bytes.len() as u64);
    if next_size > upload.expected_size {
        return Err(
            KomaError::Other("the uploaded file was larger than expected".to_owned()).into(),
        );
    }
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&upload.temporary_path)?;
    file.write_all(&bytes)?;
    upload.received_size = next_size;
    Ok(next_size)
}

#[tauri::command]
async fn finish_publication_upload(
    state: State<'_, AppState>,
    upload_id: Uuid,
    password: Option<String>,
) -> Result<LibraryItem, CommandError> {
    let upload = state
        .pending_file_uploads
        .lock()
        .map_err(|_| KomaError::Other("the upload registry lock was poisoned".to_owned()))?
        .remove(&upload_id)
        .ok_or_else(|| {
            KomaError::Other("the mobile file upload expired; choose the file again".to_owned())
        })?;
    if upload.received_size != upload.expected_size {
        let _ = std::fs::remove_file(&upload.temporary_path);
        return Err(KomaError::Other(format!(
            "the file upload stopped at {} of {} bytes",
            upload.received_size, upload.expected_size
        ))
        .into());
    }
    let library = Arc::clone(&state.library);
    let managed_directory = state.managed_library_directory.clone();
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::OpenOptions::new()
            .write(true)
            .open(&upload.temporary_path)?
            .sync_all()?;
        let destination = unique_managed_destination(&managed_directory, &upload.file_name);
        std::fs::rename(&upload.temporary_path, &destination)?;
        match library.import_path(&destination, password.as_deref()) {
            Ok(item) => Ok(item),
            Err(error) => {
                let _ = std::fs::remove_file(destination);
                Err(error)
            }
        }
    })
    .await
    .map_err(|error| CommandError {
        code: "operation_failed",
        message: format!("file import task failed: {error}"),
        recoverable: true,
    })?
    .map_err(Into::into)
}

#[tauri::command]
fn cancel_file_upload(state: State<'_, AppState>, upload_id: Uuid) -> Result<bool, CommandError> {
    let upload = state
        .pending_file_uploads
        .lock()
        .map_err(|_| KomaError::Other("the upload registry lock was poisoned".to_owned()))?
        .remove(&upload_id);
    if let Some(upload) = upload {
        let _ = std::fs::remove_file(upload.temporary_path);
        return Ok(true);
    }
    Ok(false)
}

#[cfg(any(mobile, test))]
fn copy_into_managed_library(source: &Path, directory: &Path) -> Result<PathBuf, KomaError> {
    if source.starts_with(directory) {
        return Ok(source.to_owned());
    }
    std::fs::create_dir_all(directory)?;
    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Imported comic.cbz");
    let destination = unique_managed_destination(directory, file_name);

    #[cfg(target_os = "ios")]
    let (security_scoped_url, security_scope_active) = {
        use objc2_foundation::{NSString, NSURL};
        let path = NSString::from_str(&source.to_string_lossy());
        let url = NSURL::fileURLWithPath(&path);
        let active = unsafe { url.startAccessingSecurityScopedResource() };
        (url, active)
    };

    let copy_result = std::fs::copy(source, &destination);

    #[cfg(target_os = "ios")]
    if security_scope_active {
        unsafe { security_scoped_url.stopAccessingSecurityScopedResource() };
    }

    if let Err(error) = copy_result {
        let _ = std::fs::remove_file(&destination);
        return Err(error.into());
    }
    Ok(destination)
}

#[tauri::command]
async fn relink_publication(
    state: State<'_, AppState>,
    publication_id: Uuid,
    path: PathBuf,
    password: Option<String>,
) -> Result<LibraryItem, CommandError> {
    let library = Arc::clone(&state.library);
    let managed_directory = state.managed_library_directory.clone();
    let item = tauri::async_runtime::spawn_blocking(move || {
        #[cfg(mobile)]
        let path = copy_into_managed_library(&path, &managed_directory)?;
        #[cfg(not(mobile))]
        let _ = managed_directory;
        library.relink(publication_id, path, password.as_deref())
    })
    .await
    .map_err(|error| CommandError {
        code: "operation_failed",
        message: format!("relink task failed: {error}"),
        recoverable: true,
    })??;
    state
        .readers
        .lock()
        .map_err(|_| KomaError::Other("the reader cache lock was poisoned".to_owned()))?
        .remove(&publication_id);
    Ok(item)
}

#[tauri::command]
async fn scan_folder(
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<koma_core::library::LibraryScanReport, CommandError> {
    let library = Arc::clone(&state.library);
    tauri::async_runtime::spawn_blocking(move || library.scan_folder(path))
        .await
        .map_err(|error| CommandError {
            code: "operation_failed",
            message: format!("library scan task failed: {error}"),
            recoverable: true,
        })?
        .map_err(Into::into)
}

#[tauri::command]
fn list_library_folders(state: State<'_, AppState>) -> Result<Vec<LibraryFolder>, CommandError> {
    Ok(state.library.library_folders()?)
}

#[tauri::command]
async fn add_library_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
    scan_interval_minutes: u32,
) -> Result<LibraryFolderScanResult, CommandError> {
    let library = Arc::clone(&state.library);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let folder = library.add_library_folder(path, scan_interval_minutes)?;
        scan_registered_folder(&library, &folder)
    })
    .await
    .map_err(|error| CommandError {
        code: "operation_failed",
        message: format!("library folder task failed: {error}"),
        recoverable: true,
    })??;
    let _ = app.emit("koma://library-changed", ());
    Ok(result)
}

#[tauri::command]
fn update_library_folder(
    state: State<'_, AppState>,
    folder_id: Uuid,
    enabled: bool,
    scan_interval_minutes: u32,
) -> Result<LibraryFolder, CommandError> {
    Ok(state
        .library
        .update_library_folder(folder_id, enabled, scan_interval_minutes)?)
}

#[tauri::command]
fn remove_library_folder(
    state: State<'_, AppState>,
    folder_id: Uuid,
) -> Result<bool, CommandError> {
    Ok(state.library.remove_library_folder(folder_id)?)
}

#[tauri::command]
async fn scan_library_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_id: Uuid,
) -> Result<LibraryFolderScanResult, CommandError> {
    let library = Arc::clone(&state.library);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let folder = library
            .library_folders()?
            .into_iter()
            .find(|folder| folder.id == folder_id)
            .ok_or_else(|| KomaError::Other("library folder was not found".to_owned()))?;
        scan_registered_folder(&library, &folder)
    })
    .await
    .map_err(|error| CommandError {
        code: "operation_failed",
        message: format!("library folder scan task failed: {error}"),
        recoverable: true,
    })??;
    let _ = app.emit("koma://library-changed", ());
    Ok(result)
}

#[tauri::command]
fn remove_from_library(
    state: State<'_, AppState>,
    publication_id: Uuid,
) -> Result<bool, CommandError> {
    state
        .readers
        .lock()
        .map_err(|_| CommandError {
            code: "operation_failed",
            message: "the reader cache lock was poisoned".to_owned(),
            recoverable: true,
        })?
        .remove(&publication_id);
    Ok(state.library.remove(publication_id)?)
}

#[tauri::command]
fn set_hidden(
    state: State<'_, AppState>,
    publication_id: Uuid,
    hidden: bool,
) -> Result<bool, CommandError> {
    Ok(state.library.set_hidden(publication_id, hidden)?)
}

#[tauri::command]
fn set_favorite(
    state: State<'_, AppState>,
    publication_id: Uuid,
    favorite: bool,
) -> Result<bool, CommandError> {
    Ok(state.library.set_favorite(publication_id, favorite)?)
}

#[tauri::command]
fn set_reading_status(
    state: State<'_, AppState>,
    publication_id: Uuid,
    completed: bool,
) -> Result<ReadingState, CommandError> {
    Ok(state
        .library
        .set_reading_status(publication_id, completed)?)
}

#[tauri::command]
async fn open_reader(
    app: AppHandle,
    state: State<'_, AppState>,
    publication_id: Uuid,
    password: Option<String>,
) -> Result<ReaderOpenPayload, CommandError> {
    let item = state
        .library
        .get(publication_id)?
        .ok_or_else(|| CommandError {
            code: "missing_source",
            message: "publication is not in the library".to_owned(),
            recoverable: true,
        })?;
    if item.is_missing {
        return Err(CommandError {
            code: "missing_source",
            message: format!("the source file is missing: {}", item.path.display()),
            recoverable: true,
        });
    }
    let reader = state.open_reader(publication_id, password.as_deref())?;
    if reader.manifest().format == koma_core::PublicationFormat::Pdf {
        app.asset_protocol_scope()
            .allow_file(&reader.manifest().path)
            .map_err(|error| CommandError {
                code: "operation_failed",
                message: format!("could not grant PDF read access: {error}"),
                recoverable: true,
            })?;
    }
    Ok(ReaderOpenPayload {
        library_id: publication_id,
        manifest: reader.manifest().clone(),
        reading_state: state.library.reading_state(publication_id)?,
        bookmarks: state.library.bookmarks(publication_id)?,
    })
}

#[tauri::command]
async fn read_page(
    state: State<'_, AppState>,
    publication_id: Uuid,
    page_index: usize,
) -> Result<PagePayload, CommandError> {
    let reader = state.cached_reader(publication_id)?;
    tauri::async_runtime::spawn_blocking(move || reader.read_page(page_index))
        .await
        .map_err(|error| CommandError {
            code: "operation_failed",
            message: format!("page read task failed: {error}"),
            recoverable: true,
        })?
        .map(PagePayload::from)
        .map_err(Into::into)
}

#[tauri::command]
fn save_progress(
    state: State<'_, AppState>,
    publication_id: Uuid,
    current_page: usize,
    settings: Option<ReaderSettings>,
) -> Result<ReadingState, CommandError> {
    let reading_state =
        state
            .library
            .save_progress(publication_id, current_page, settings.as_ref())?;
    if let Some(chapter) = state
        .library
        .completed_chapter(publication_id, reading_state.current_page)?
    {
        let tracking = Arc::clone(&state.tracking);
        tauri::async_runtime::spawn(async move {
            tracking.sync_progress(publication_id, chapter).await;
        });
    }
    Ok(reading_state)
}

#[tauri::command]
fn record_reading_time(
    state: State<'_, AppState>,
    publication_id: Uuid,
    elapsed_seconds: u64,
) -> Result<u64, CommandError> {
    Ok(state
        .library
        .record_reading_time(publication_id, elapsed_seconds)?)
}

fn tracking_error(message: String) -> CommandError {
    CommandError {
        code: "tracking_failed",
        message,
        recoverable: true,
    }
}

#[tauri::command]
fn tracking_accounts(state: State<'_, AppState>) -> Result<Vec<TrackingAccount>, CommandError> {
    state.tracking.accounts().map_err(tracking_error)
}

#[tauri::command]
fn begin_tracking_oauth(
    state: State<'_, AppState>,
    provider: TrackingProvider,
) -> Result<String, CommandError> {
    state.tracking.begin_oauth(provider).map_err(tracking_error)
}

#[tauri::command]
fn take_tracking_auth(
    state: State<'_, AppState>,
) -> Result<Option<TrackingAuthEvent>, CommandError> {
    state
        .last_tracking_auth
        .lock()
        .map_err(|_| tracking_error("tracking authorization lock was poisoned".to_owned()))
        .map(|mut event| event.take())
}

#[tauri::command]
fn disconnect_tracking(
    state: State<'_, AppState>,
    provider: TrackingProvider,
) -> Result<(), CommandError> {
    state.tracking.disconnect(provider).map_err(tracking_error)
}

#[tauri::command]
async fn suggest_tracking(
    state: State<'_, AppState>,
    provider: TrackingProvider,
    query: String,
) -> Result<TrackingSuggestion, CommandError> {
    state
        .tracking
        .suggest(provider, &query)
        .await
        .map_err(tracking_error)
}

#[tauri::command]
fn tracking_mappings(
    state: State<'_, AppState>,
    publication_id: Uuid,
) -> Result<Vec<TrackingMapping>, CommandError> {
    state
        .tracking
        .mappings(publication_id)
        .map_err(tracking_error)
}

#[tauri::command]
fn set_tracking_mapping(
    state: State<'_, AppState>,
    mapping: TrackingMapping,
) -> Result<(), CommandError> {
    state.tracking.set_mapping(mapping).map_err(tracking_error)
}

#[tauri::command]
fn remove_tracking_mapping(
    state: State<'_, AppState>,
    publication_id: Uuid,
    provider: TrackingProvider,
) -> Result<(), CommandError> {
    state
        .tracking
        .remove_mapping(publication_id, provider)
        .map_err(tracking_error)
}

#[tauri::command]
async fn tracking_remote_progress(
    state: State<'_, AppState>,
    publication_id: Uuid,
) -> Result<Vec<tracking::TrackingRemoteProgress>, CommandError> {
    state
        .tracking
        .remote_progress(publication_id)
        .await
        .map_err(tracking_error)
}

fn handle_tracking_oauth_url(app: AppHandle, callback: String) {
    if !callback.starts_with("koma://oauth/") {
        return;
    }
    let mut hasher = DefaultHasher::new();
    callback.hash(&mut hasher);
    let callback_id = hasher.finish();
    {
        let state = app.state::<AppState>();
        let Ok(mut handled) = state.handled_oauth_callbacks.lock() else {
            return;
        };
        if !handled.insert(callback_id) {
            return;
        }
        if handled.len() > 32
            && let Some(oldest) = handled.first().copied()
        {
            handled.remove(&oldest);
        }
    }
    let tracking = Arc::clone(&app.state::<AppState>().tracking);
    tauri::async_runtime::spawn(async move {
        let event = match tracking.finish_oauth(&callback).await {
            Ok(account) => TrackingAuthEvent {
                success: true,
                message: format!(
                    "{} connected",
                    account.username.unwrap_or_else(|| "Account".to_owned())
                ),
            },
            Err(message) => TrackingAuthEvent {
                success: false,
                message,
            },
        };
        if !event.success {
            eprintln!("tracking authorization failed: {}", event.message);
        }
        if let Ok(mut last_event) = app.state::<AppState>().last_tracking_auth.lock() {
            *last_event = Some(event.clone());
        }
        let _ = app.emit("koma://tracking-auth", event);
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    });
}

#[tauri::command]
fn add_bookmark(
    state: State<'_, AppState>,
    publication_id: Uuid,
    page_index: usize,
    label: Option<String>,
    note: Option<String>,
) -> Result<Bookmark, CommandError> {
    Ok(state.library.add_bookmark(
        publication_id,
        page_index,
        label.as_deref(),
        note.as_deref(),
    )?)
}

#[tauri::command]
fn update_bookmark(
    state: State<'_, AppState>,
    bookmark_id: Uuid,
    label: Option<String>,
    note: Option<String>,
) -> Result<Bookmark, CommandError> {
    Ok(state
        .library
        .update_bookmark(bookmark_id, label.as_deref(), note.as_deref())?)
}

#[tauri::command]
fn remove_bookmark(state: State<'_, AppState>, bookmark_id: Uuid) -> Result<bool, CommandError> {
    Ok(state.library.remove_bookmark(bookmark_id)?)
}

#[tauri::command]
fn list_connectors(state: State<'_, AppState>) -> Result<Vec<ConnectorSummary>, CommandError> {
    let connectors = state
        .connectors
        .lock()
        .map_err(|_| KomaError::Other("the connector registry lock was poisoned".to_owned()))?;
    let mut summaries = Vec::with_capacity(connectors.len() + 1);
    summaries.push(bundled_mangafire_summary());
    summaries.extend(
        connectors
            .iter()
            .map(|connector| connector.manifest().summary()),
    );
    Ok(summaries)
}

#[tauri::command]
fn inspect_connector_package(
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<ConnectorPackagePreview, CommandError> {
    let bytes = read_connector_file(&path)?;
    let connector = connector_from_bytes(&bytes)?;
    let manifest = connector.manifest();
    let install_token = Uuid::now_v7();
    let mut pending = state.pending_connector_packages.lock().map_err(|_| {
        KomaError::Other("the pending connector package lock was poisoned".to_owned())
    })?;
    pending.clear();
    pending.insert(install_token, bytes);
    Ok(ConnectorPackagePreview {
        install_token,
        connector: manifest.summary(),
        allowed_request_hosts: manifest.allowed_request_hosts.clone(),
        allowed_page_hosts: manifest.allowed_page_hosts.clone(),
        allow_local_network: manifest.allow_local_network,
    })
}

#[tauri::command]
fn inspect_connector_contents(
    state: State<'_, AppState>,
    contents: String,
) -> Result<ConnectorPackagePreview, CommandError> {
    let bytes = contents.into_bytes();
    let connector = connector_from_bytes(&bytes)?;
    let manifest = connector.manifest();
    let install_token = Uuid::now_v7();
    let mut pending = state.pending_connector_packages.lock().map_err(|_| {
        KomaError::Other("the pending connector package lock was poisoned".to_owned())
    })?;
    pending.clear();
    pending.insert(install_token, bytes);
    Ok(ConnectorPackagePreview {
        install_token,
        connector: manifest.summary(),
        allowed_request_hosts: manifest.allowed_request_hosts.clone(),
        allowed_page_hosts: manifest.allowed_page_hosts.clone(),
        allow_local_network: manifest.allow_local_network,
    })
}

#[tauri::command]
fn install_connector_package(
    state: State<'_, AppState>,
    install_token: Uuid,
) -> Result<ConnectorSummary, CommandError> {
    let bytes = state
        .pending_connector_packages
        .lock()
        .map_err(|_| {
            KomaError::Other("the pending connector package lock was poisoned".to_owned())
        })?
        .remove(&install_token)
        .ok_or_else(|| {
            KomaError::ImportDenied(
                "the connector approval expired; choose the connector again".to_owned(),
            )
        })?;
    let connector = connector_from_bytes(&bytes)?;
    let manifest = connector.manifest();
    let summary = manifest.summary();
    std::fs::create_dir_all(&state.connectors_directory)?;
    let destination = state
        .connectors_directory
        .join(format!("{}.koma-connector.json", manifest.id));
    let temporary =
        state
            .connectors_directory
            .join(format!(".{}.{}.tmp", manifest.id, Uuid::now_v7()));
    std::fs::write(&temporary, bytes)?;

    let previous = state
        .connectors_directory
        .join(format!(".{}.previous", manifest.id));
    if previous.exists() {
        std::fs::remove_file(&previous)?;
    }
    if destination.exists() {
        std::fs::rename(&destination, &previous)?;
    }
    if let Err(error) = std::fs::rename(&temporary, &destination) {
        if previous.exists() {
            let _ = std::fs::rename(&previous, &destination);
        }
        return Err(error.into());
    }
    if previous.exists() {
        std::fs::remove_file(previous)?;
    }

    let mut connectors = state
        .connectors
        .lock()
        .map_err(|_| KomaError::Other("the connector registry lock was poisoned".to_owned()))?;
    connectors.retain(|existing| existing.manifest().id != manifest.id);
    connectors.push(connector);
    connectors.sort_by(|left, right| {
        left.manifest()
            .name
            .to_lowercase()
            .cmp(&right.manifest().name.to_lowercase())
    });
    Ok(summary)
}

#[tauri::command]
fn remove_connector(
    state: State<'_, AppState>,
    connector_id: String,
) -> Result<bool, CommandError> {
    if connector_id == "mangafire" {
        return Err(KomaError::ImportDenied(
            "the bundled MangaFire connector cannot be removed".to_owned(),
        )
        .into());
    }
    let mut connectors = state
        .connectors
        .lock()
        .map_err(|_| KomaError::Other("the connector registry lock was poisoned".to_owned()))?;
    let installed = connectors
        .iter()
        .any(|connector| connector.manifest().id == connector_id);
    if !installed {
        return Ok(false);
    }
    let destination = state
        .connectors_directory
        .join(format!("{connector_id}.koma-connector.json"));
    if destination.exists() {
        std::fs::remove_file(destination)?;
    }
    connectors.retain(|connector| connector.manifest().id != connector_id);
    Ok(true)
}

#[tauri::command]
async fn preview_link(
    state: State<'_, AppState>,
    source: String,
) -> Result<ImportPreview, CommandError> {
    let importer = state.importer_for(&source)?;
    importer.preview(&source).await.map_err(Into::into)
}

#[tauri::command]
async fn import_link(
    app: AppHandle,
    state: State<'_, AppState>,
    source: String,
    options: ImportOptions,
) -> Result<LinkImportResult, CommandError> {
    let (event_sender, mut event_receiver) = tokio::sync::mpsc::unbounded_channel();
    let event_app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = event_receiver.recv().await {
            let _ = event_app.emit("koma://import-event", event);
        }
    });

    let importer = state.importer_for(&source)?;
    let receipt = importer
        .import(&source, &options, Some(&event_sender))
        .await?;
    drop(event_sender);
    state.library.save_import_receipt(&receipt)?;
    let item = state.library.import_path(&receipt.output_path, None)?;
    Ok(LinkImportResult { receipt, item })
}

#[cfg(desktop)]
#[tauri::command]
fn set_discord_presence(
    state: State<'_, AppState>,
    enabled: bool,
    details: String,
    activity_state: String,
) -> Result<bool, CommandError> {
    let mut client = state
        .discord
        .lock()
        .map_err(|_| KomaError::Other("the Discord presence lock was poisoned".to_owned()))?;
    if !enabled {
        if let Some(mut active) = client.take() {
            let _ = active.clear_activity();
            let _ = active.close();
        }
        return Ok(false);
    }
    if client.is_none() {
        let mut connected = DiscordIpcClient::new("1528082669257101543");
        connected
            .connect()
            .map_err(|error| KomaError::Other(format!("Discord is not available: {error}")))?;
        *client = Some(connected);
    }
    client
        .as_mut()
        .expect("Discord client was initialized")
        .set_activity(
            activity::Activity::new()
                .details(&details)
                .state(&activity_state)
                .assets(
                    activity::Assets::new()
                        .large_image("icon")
                        .large_text("Koma"),
                )
                .buttons(vec![activity::Button::new(
                    "Try out Koma",
                    "https://github.com/Pixlox/Koma",
                )]),
        )
        .map_err(|error| {
            KomaError::Other(format!("Discord presence could not be updated: {error}"))
        })?;
    Ok(true)
}

#[tauri::command]
async fn export_library_backup(
    state: State<'_, AppState>,
    destination: PathBuf,
) -> Result<PathBuf, CommandError> {
    let library = Arc::clone(&state.library);
    let destination_for_task = destination.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), KomaError> {
        let backup = library.export_backup()?;
        let bytes = serde_json::to_vec_pretty(&backup)
            .map_err(|error| KomaError::Other(format!("could not serialize backup: {error}")))?;
        let temporary = destination_for_task.with_extension("koma-backup.tmp");
        std::fs::write(&temporary, bytes)?;
        if destination_for_task.exists() {
            let previous = destination_for_task.with_extension("koma-backup.previous");
            if previous.exists() {
                std::fs::remove_file(&previous)?;
            }
            std::fs::rename(&destination_for_task, &previous)?;
        }
        if let Err(error) = std::fs::rename(&temporary, &destination_for_task) {
            let previous = destination_for_task.with_extension("koma-backup.previous");
            if previous.exists() {
                let _ = std::fs::rename(previous, &destination_for_task);
            }
            return Err(error.into());
        }
        Ok(())
    })
    .await
    .map_err(|error| CommandError {
        code: "operation_failed",
        message: format!("backup task failed: {error}"),
        recoverable: true,
    })??;
    Ok(destination)
}

#[tauri::command]
async fn restore_library_backup(
    state: State<'_, AppState>,
    source: PathBuf,
) -> Result<BackupRestoreReport, CommandError> {
    let library = Arc::clone(&state.library);
    tauri::async_runtime::spawn_blocking(move || {
        let metadata = std::fs::metadata(&source)?;
        if metadata.len() > 64 * 1024 * 1024 {
            return Err(KomaError::Other(
                "the backup exceeds Koma's 64 MiB safety limit".to_owned(),
            ));
        }
        let bytes = std::fs::read(source)?;
        let backup: LibraryBackup = serde_json::from_slice(&bytes)
            .map_err(|error| KomaError::Other(format!("invalid Koma backup: {error}")))?;
        library.restore_backup(&backup)
    })
    .await
    .map_err(|error| CommandError {
        code: "operation_failed",
        message: format!("backup restore task failed: {error}"),
        recoverable: true,
    })?
    .map_err(Into::into)
}

#[tauri::command]
async fn inspect_library_publication(
    state: State<'_, AppState>,
    publication_id: Uuid,
    password: Option<String>,
) -> Result<PublicationInspection, CommandError> {
    let item = state
        .library
        .get(publication_id)?
        .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
    let override_metadata = state.library.metadata_override(publication_id)?;
    let mut inspection =
        tauri::async_runtime::spawn_blocking(move || inspect_path(&item.path, password.as_deref()))
            .await
            .map_err(|error| CommandError {
                code: "operation_failed",
                message: format!("inspection task failed: {error}"),
                recoverable: true,
            })??;
    if let Some(metadata) = override_metadata {
        inspection.metadata = metadata;
    }
    Ok(inspection)
}

#[tauri::command]
async fn convert_library_publication(
    state: State<'_, AppState>,
    publication_id: Uuid,
    destination: PathBuf,
    password: Option<String>,
    options: ConversionOptions,
) -> Result<PublicationOperationResult, CommandError> {
    let item = state
        .library
        .get(publication_id)?
        .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
    let report = tauri::async_runtime::spawn_blocking(move || {
        convert_to_cbz(&item.path, &destination, password.as_deref(), &options)
    })
    .await
    .map_err(|error| CommandError {
        code: "operation_failed",
        message: format!("conversion task failed: {error}"),
        recoverable: true,
    })??;
    let item = state.library.import_path(&report.output_path, None)?;
    Ok(PublicationOperationResult { report, item })
}

#[tauri::command]
async fn repair_library_publication(
    state: State<'_, AppState>,
    publication_id: Uuid,
    destination: PathBuf,
    password: Option<String>,
) -> Result<PublicationOperationResult, CommandError> {
    let item = state
        .library
        .get(publication_id)?
        .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
    let report = tauri::async_runtime::spawn_blocking(move || {
        repair_to_cbz(&item.path, &destination, password.as_deref())
    })
    .await
    .map_err(|error| CommandError {
        code: "operation_failed",
        message: format!("repair task failed: {error}"),
        recoverable: true,
    })??;
    let item = state.library.import_path(&report.output_path, None)?;
    Ok(PublicationOperationResult { report, item })
}

#[tauri::command]
async fn save_publication_metadata(
    state: State<'_, AppState>,
    publication_id: Uuid,
    metadata: PublicationMetadata,
    write_to_source: bool,
) -> Result<MetadataSaveResult, CommandError> {
    let item = state
        .library
        .get(publication_id)?
        .ok_or_else(|| KomaError::Other("publication is not in the library".to_owned()))?;
    let backup_path = if write_to_source {
        let path = item.path;
        let source_metadata = metadata.clone();
        tauri::async_runtime::spawn_blocking(move || {
            write_publication_metadata(&path, &source_metadata)
        })
        .await
        .map_err(|error| CommandError {
            code: "operation_failed",
            message: format!("metadata write task failed: {error}"),
            recoverable: true,
        })??
    } else {
        None
    };
    let item = state
        .library
        .save_metadata_override(publication_id, &metadata)?;
    state
        .readers
        .lock()
        .map_err(|_| KomaError::Other("the reader cache lock was poisoned".to_owned()))?
        .remove(&publication_id);
    Ok(MetadataSaveResult { item, backup_path })
}

fn setup_state(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_directory = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_directory)?;
    #[cfg(not(mobile))]
    let managed_library_directory = data_directory.join("Library");
    #[cfg(mobile)]
    let managed_library_directory = app.path().document_dir()?.join("Koma");
    std::fs::create_dir_all(&managed_library_directory)?;
    let incoming_directory = managed_library_directory.join(".incoming");
    if incoming_directory.exists() {
        std::fs::remove_dir_all(&incoming_directory)?;
    }
    #[cfg(not(mobile))]
    let default_import_directory = app
        .path()
        .download_dir()
        .map(|directory| directory.join("Koma"))
        .unwrap_or_else(|_| data_directory.join("Imports"));
    #[cfg(mobile)]
    let default_import_directory = managed_library_directory.clone();
    let working_directory = std::env::current_dir().unwrap_or_else(|_| data_directory.clone());
    let arguments = std::env::args().collect::<Vec<_>>();
    let pending_open_paths = open_paths_from_arguments(&arguments, &working_directory);
    let state = AppState::new(
        &data_directory.join("library.sqlite3"),
        default_import_directory,
        managed_library_directory,
        pending_open_paths,
    )?;
    let library = Arc::clone(&state.library);
    let app_handle = app.handle().clone();
    app.manage(state);
    #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
    app.deep_link().register_all()?;
    let oauth_app = app.handle().clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            handle_tracking_oauth_url(oauth_app.clone(), url.to_string());
        }
    });
    if let Some(urls) = app.deep_link().get_current()? {
        for url in urls {
            handle_tracking_oauth_url(app.handle().clone(), url.to_string());
        }
    }
    tauri::async_runtime::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(60));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            let library_for_scan = Arc::clone(&library);
            let due = tauri::async_runtime::spawn_blocking(move || {
                library_for_scan.due_library_folders(chrono::Utc::now())
            })
            .await;
            let Ok(Ok(folders)) = due else {
                continue;
            };
            for folder in folders {
                let library_for_scan = Arc::clone(&library);
                let result = tauri::async_runtime::spawn_blocking(move || {
                    scan_registered_folder(&library_for_scan, &folder)
                })
                .await;
                if matches!(result, Ok(Ok(_))) {
                    let _ = app_handle.emit("koma://library-changed", ());
                }
            }
        }
    });
    Ok(())
}

fn scan_registered_folder(
    library: &Library,
    folder: &LibraryFolder,
) -> Result<LibraryFolderScanResult, KomaError> {
    match library.scan_folder(&folder.path) {
        Ok(report) => {
            library.record_library_folder_scan(folder.id, Some(&report), None)?;
            let updated = library
                .library_folders()?
                .into_iter()
                .find(|candidate| candidate.id == folder.id)
                .ok_or_else(|| KomaError::Other("library folder was not found".to_owned()))?;
            Ok(LibraryFolderScanResult {
                folder: updated,
                report,
            })
        }
        Err(error) => {
            library.record_library_folder_scan(folder.id, None, Some(&error.to_string()))?;
            Err(error)
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(
            |app, arguments, working_directory| {
                for argument in &arguments {
                    handle_tracking_oauth_url(app.clone(), argument.clone());
                }
                let paths = open_paths_from_arguments(&arguments, Path::new(&working_directory));
                queue_open_paths(app, paths);
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            },
        ))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());
    #[cfg(not(desktop))]
    let builder = builder.plugin(tauri_plugin_deep_link::init());

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(setup_state)
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            list_library,
            take_open_paths,
            add_publication,
            begin_file_upload,
            append_file_upload,
            finish_publication_upload,
            cancel_file_upload,
            relink_publication,
            scan_folder,
            list_library_folders,
            add_library_folder,
            update_library_folder,
            remove_library_folder,
            scan_library_folder,
            remove_from_library,
            set_hidden,
            set_favorite,
            set_reading_status,
            open_reader,
            read_page,
            save_progress,
            record_reading_time,
            tracking_accounts,
            begin_tracking_oauth,
            take_tracking_auth,
            disconnect_tracking,
            suggest_tracking,
            tracking_mappings,
            set_tracking_mapping,
            remove_tracking_mapping,
            tracking_remote_progress,
            add_bookmark,
            update_bookmark,
            remove_bookmark,
            list_connectors,
            inspect_connector_package,
            inspect_connector_contents,
            install_connector_package,
            remove_connector,
            preview_link,
            import_link,
            #[cfg(desktop)]
            set_discord_presence,
            export_library_backup,
            restore_library_backup,
            inspect_library_publication,
            convert_library_publication,
            repair_library_publication,
            save_publication_metadata,
        ])
        .build(tauri::generate_context!())
        .expect("Koma could not start")
        .run(|_app, _event| {
            #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
            if let tauri::RunEvent::Opened { urls } = _event {
                let paths: Vec<PathBuf> = urls
                    .into_iter()
                    .filter_map(|url| url.to_file_path().ok())
                    .filter(|path| path.is_dir() || PublicationFormat::from_path(path).is_some())
                    .collect();
                #[cfg(target_os = "ios")]
                let paths = {
                    let state = _app.state::<AppState>();
                    paths
                        .into_iter()
                        .filter_map(|path| {
                            match copy_into_managed_library(&path, &state.managed_library_directory)
                            {
                                Ok(managed) => Some(managed),
                                Err(error) => {
                                    eprintln!(
                                        "Koma could not copy opened publication {}: {error}",
                                        path.display()
                                    );
                                    None
                                }
                            }
                        })
                        .collect()
                };
                queue_open_paths(_app, paths);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::copy_into_managed_library;

    #[test]
    fn mobile_imports_are_copied_to_unique_managed_paths() {
        let root = tempfile::tempdir().expect("temporary directory");
        let incoming = root.path().join("Incoming");
        let managed = root.path().join("Documents").join("Koma");
        std::fs::create_dir_all(&incoming).expect("incoming directory");
        let source = incoming.join("Volume.cbz");
        std::fs::write(&source, b"comic").expect("source file");

        let first = copy_into_managed_library(&source, &managed).expect("first copy");
        let second = copy_into_managed_library(&source, &managed).expect("second copy");

        assert_eq!(first, managed.join("Volume.cbz"));
        assert_eq!(second, managed.join("Volume 2.cbz"));
        assert_eq!(std::fs::read(first).expect("copied bytes"), b"comic");
        assert_eq!(
            copy_into_managed_library(&second, &managed).expect("managed source"),
            second
        );
    }
}
