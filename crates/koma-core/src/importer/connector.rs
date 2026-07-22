use std::{
    collections::{BTreeMap, HashMap},
    net::ToSocketAddrs,
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use futures_util::{StreamExt, TryStreamExt, stream};
use hmac::{Hmac, Mac};
use reqwest::{
    Client,
    header::{ACCEPT, CONTENT_TYPE, REFERER},
};
use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, Scope};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::UnboundedSender;
use url::Url;
use uuid::Uuid;

use super::{
    ADAPTER_VERSION, ImportChapter, ImportEvent, ImportOptions, ImportPreview, ImportScope,
    ImportVolume, LinkImporter, MAX_IMPORT_BYTES, MAX_IMPORT_PAGE_BYTES, MAX_JSON_BYTES,
    RemoteChapter, RemoteNavigationItem, RemotePage, RemotePublication, choose_output_path,
    client_builder, emit, hash_file, is_non_public_ip, page_extension, read_bounded,
    require_success, sanitize_file_component, volume_number_label,
};
use crate::{
    error::{KomaError, Result},
    formats::{MAX_PAGES, ZipPublication, validate_page_bytes},
    metadata::ComicInfo,
    model::{ChapterRange, ImportReceipt, KomaArchiveMetadata},
};

const CONNECTOR_SCHEMA_VERSION_V1: u32 = 1;
const CONNECTOR_SCHEMA_VERSION_V2: u32 = 2;
const MAX_CONNECTOR_CHAPTERS: usize = 2_000;
const MAX_SCRIPT_OPERATIONS: u64 = 500_000;
const MAX_SCRIPT_SECONDS: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectorCapability {
    Chapter,
    Volume,
    Series,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ConnectorManifest {
    #[serde(rename = "$schema", default)]
    pub json_schema: Option<String>,
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub source_pattern: String,
    pub request_url: String,
    #[serde(default)]
    pub page_request_url: Option<String>,
    pub allowed_request_hosts: Vec<String>,
    pub allowed_page_hosts: Vec<String>,
    #[serde(default)]
    pub allow_local_network: bool,
    #[serde(default)]
    pub response_type: ConnectorResponseType,
    #[serde(default = "default_capabilities")]
    pub capabilities: Vec<ConnectorCapability>,
    pub mapping: ConnectorMapping,
    #[serde(default)]
    pub transform_script: Option<String>,
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectorResponseType {
    #[default]
    Json,
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ConnectorMapping {
    pub title: String,
    #[serde(default)]
    pub language: Option<String>,
    pub chapters: String,
    pub chapter_number: String,
    #[serde(default)]
    pub chapter_volume: Option<String>,
    #[serde(default)]
    pub chapter_pages: Option<String>,
    #[serde(default)]
    pub page_response_pages: Option<String>,
    #[serde(default)]
    pub page_url: Option<String>,
    #[serde(default)]
    pub page_width: Option<String>,
    #[serde(default)]
    pub page_height: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub kind: ConnectorKind,
    pub enabled: bool,
    pub removable: bool,
    pub schema_version: u32,
    pub runs_code: bool,
    pub capabilities: Vec<ConnectorCapability>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectorKind {
    Bundled,
    Declarative,
}

fn default_capabilities() -> Vec<ConnectorCapability> {
    vec![ConnectorCapability::Series]
}

impl ConnectorManifest {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 1024 * 1024 {
            return Err(KomaError::ImportDenied(
                "connector package exceeds the 1 MiB limit".to_owned(),
            ));
        }
        let manifest: Self = serde_json::from_slice(bytes).map_err(|error| {
            KomaError::ImportDenied(format!("invalid connector package: {error}"))
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        if !matches!(
            self.schema_version,
            CONNECTOR_SCHEMA_VERSION_V1 | CONNECTOR_SCHEMA_VERSION_V2
        ) {
            return Err(KomaError::ImportDenied(format!(
                "connector schema {} is not supported",
                self.schema_version
            )));
        }
        if self.schema_version == CONNECTOR_SCHEMA_VERSION_V1 && self.transform_script.is_some() {
            return Err(KomaError::ImportDenied(
                "Rhai scripts require connector schema version 2".to_owned(),
            ));
        }
        if self.response_type == ConnectorResponseType::Text && self.transform_script.is_none() {
            return Err(KomaError::ImportDenied(
                "text connector responses require a Rhai transform".to_owned(),
            ));
        }
        if let Some(script) = &self.transform_script {
            if script.trim().is_empty() || script.len() > 256 * 1024 {
                return Err(KomaError::ImportDenied(
                    "connector Rhai transform must contain 1 to 262144 characters".to_owned(),
                ));
            }
            validate_script(script)?;
        }
        if self.settings.len() > 64
            || self
                .settings
                .iter()
                .any(|(key, value)| key.is_empty() || key.len() > 80 || value.len() > 4096)
        {
            return Err(KomaError::ImportDenied(
                "connector settings exceed the key or value limits".to_owned(),
            ));
        }
        if self.id.len() < 3
            || self.id.len() > 64
            || !self.id.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            })
        {
            return Err(KomaError::ImportDenied(
                "connector id must use lowercase letters, numbers, and hyphens".to_owned(),
            ));
        }
        if self.name.trim().is_empty() || self.name.len() > 80 {
            return Err(KomaError::ImportDenied(
                "connector name must contain 1 to 80 characters".to_owned(),
            ));
        }
        if self.version.trim().is_empty() || self.version.len() > 32 {
            return Err(KomaError::ImportDenied(
                "connector version is invalid".to_owned(),
            ));
        }
        if self
            .description
            .as_ref()
            .is_some_and(|description| description.len() > 280)
        {
            return Err(KomaError::ImportDenied(
                "connector description must not exceed 280 characters".to_owned(),
            ));
        }
        if self.source_pattern.len() > 1024 {
            return Err(KomaError::ImportDenied(
                "connector source pattern is too long".to_owned(),
            ));
        }
        regex::Regex::new(&self.source_pattern).map_err(|error| {
            KomaError::ImportDenied(format!("invalid connector source pattern: {error}"))
        })?;
        if self.request_url.trim().is_empty() || self.request_url.len() > 2048 {
            return Err(KomaError::ImportDenied(
                "connector request URL template is invalid".to_owned(),
            ));
        }
        if self
            .page_request_url
            .as_ref()
            .is_some_and(|template| template.trim().is_empty() || template.len() > 2048)
        {
            return Err(KomaError::ImportDenied(
                "connector page request URL template is invalid".to_owned(),
            ));
        }
        if self.allowed_request_hosts.is_empty() || self.allowed_page_hosts.is_empty() {
            return Err(KomaError::ImportDenied(
                "connector request and page hosts must be declared".to_owned(),
            ));
        }
        for host in self
            .allowed_request_hosts
            .iter()
            .chain(self.allowed_page_hosts.iter())
        {
            if !valid_host_permission(host) {
                return Err(KomaError::ImportDenied(format!(
                    "invalid connector host permission: {host}"
                )));
            }
        }
        if !self.capabilities.contains(&ConnectorCapability::Series) {
            return Err(KomaError::ImportDenied(
                "connector must support whole-series imports".to_owned(),
            ));
        }
        let inline_pages = self.mapping.chapter_pages.is_some();
        let staged_pages =
            self.page_request_url.is_some() && self.mapping.page_response_pages.is_some();
        if inline_pages == staged_pages {
            return Err(KomaError::ImportDenied(
                "connector must declare either inline pages or a staged page request".to_owned(),
            ));
        }
        for pointer in [
            Some(self.mapping.title.as_str()),
            Some(self.mapping.chapters.as_str()),
            Some(self.mapping.chapter_number.as_str()),
            self.mapping.chapter_pages.as_deref(),
            self.mapping.page_response_pages.as_deref(),
            self.mapping.language.as_deref(),
            self.mapping.chapter_volume.as_deref(),
            self.mapping.page_url.as_deref(),
            self.mapping.page_width.as_deref(),
            self.mapping.page_height.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !pointer.is_empty() && !pointer.starts_with('/') {
                return Err(KomaError::ImportDenied(format!(
                    "connector mapping {pointer} is not a JSON pointer"
                )));
            }
        }
        Ok(())
    }

    pub fn summary(&self) -> ConnectorSummary {
        ConnectorSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
            kind: ConnectorKind::Declarative,
            enabled: true,
            removable: true,
            schema_version: self.schema_version,
            runs_code: self.transform_script.is_some(),
            capabilities: self.capabilities.clone(),
        }
    }
}

pub struct DeclarativeImporter {
    manifest: ConnectorManifest,
    matcher: regex::Regex,
    pinned_clients: Mutex<HashMap<String, Client>>,
}

impl DeclarativeImporter {
    pub fn new(manifest: ConnectorManifest) -> Result<Self> {
        manifest.validate()?;
        Ok(Self {
            matcher: regex::Regex::new(&manifest.source_pattern).map_err(|error| {
                KomaError::ImportDenied(format!("invalid connector source pattern: {error}"))
            })?,
            manifest,
            pinned_clients: Mutex::new(HashMap::new()),
        })
    }

    pub fn manifest(&self) -> &ConnectorManifest {
        &self.manifest
    }

    fn request_url(&self, source: &str) -> Result<Url> {
        let captures = self.matcher.captures(source.trim()).ok_or_else(|| {
            KomaError::UnsupportedFormat("link does not match this connector".to_owned())
        })?;
        let mut expanded = String::new();
        captures.expand(&self.manifest.request_url, &mut expanded);
        let url = Url::parse(&expanded)?;
        self.validate_url(&url, &self.manifest.allowed_request_hosts)?;
        Ok(url)
    }

    fn source_captures(&self, source: &str) -> Result<BTreeMap<String, String>> {
        let captures = self.matcher.captures(source.trim()).ok_or_else(|| {
            KomaError::UnsupportedFormat("link does not match this connector".to_owned())
        })?;
        Ok(self
            .matcher
            .capture_names()
            .flatten()
            .filter_map(|name| {
                captures
                    .name(name)
                    .map(|value| (name.to_owned(), value.as_str().to_owned()))
            })
            .collect())
    }

    fn validate_url(&self, url: &Url, allowed_hosts: &[String]) -> Result<()> {
        if url.username() != "" || url.password().is_some() || url.fragment().is_some() {
            return Err(KomaError::ImportDenied(
                "connector returned a URL with credentials or a fragment".to_owned(),
            ));
        }
        if url.scheme() != "https" && !(self.manifest.allow_local_network && url.scheme() == "http")
        {
            return Err(KomaError::ImportDenied(
                "connector URLs must use HTTPS".to_owned(),
            ));
        }
        let host = url
            .host_str()
            .ok_or_else(|| KomaError::ImportDenied("connector URL has no host".to_owned()))?
            .to_ascii_lowercase();
        if !allowed_hosts
            .iter()
            .any(|allowed| host_matches(&host, allowed))
        {
            return Err(KomaError::ImportDenied(format!(
                "connector did not declare access to {host}"
            )));
        }
        Ok(())
    }

    fn clients(&self) -> Result<MutexGuard<'_, HashMap<String, Client>>> {
        self.pinned_clients
            .lock()
            .map_err(|_| KomaError::Other("connector client cache was poisoned".to_owned()))
    }

    async fn client_for(&self, url: &Url) -> Result<Client> {
        let host = url
            .host_str()
            .ok_or_else(|| KomaError::ImportDenied("connector URL has no host".to_owned()))?
            .to_ascii_lowercase();
        if let Some(client) = self.clients()?.get(&host).cloned() {
            return Ok(client);
        }
        let port = url.port_or_known_default().ok_or_else(|| {
            KomaError::ImportDenied("connector URL has no recognized port".to_owned())
        })?;
        let addresses = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|error| {
                KomaError::ProviderUnavailable(format!(
                    "could not resolve connector host {host}: {error}"
                ))
            })?
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            return Err(KomaError::ProviderUnavailable(format!(
                "connector host {host} resolved to no addresses"
            )));
        }
        let private = addresses
            .iter()
            .any(|address| is_non_public_ip(address.ip()));
        if private && !self.manifest.allow_local_network {
            return Err(KomaError::ImportDenied(format!(
                "connector host {host} resolved to a local network address"
            )));
        }
        let client = client_builder()
            .resolve_to_addrs(&host, &addresses)
            .build()?;
        let mut clients = self.clients()?;
        Ok(clients
            .entry(host)
            .or_insert_with(|| client.clone())
            .clone())
    }

    async fn fetch_feed(&self, source: &str) -> Result<(MappedFeed, Url, u16)> {
        let url = self.request_url(source)?;
        let response = self
            .client_for(&url)
            .await?
            .get(url.clone())
            .header(ACCEPT, "application/json")
            .send()
            .await?;
        let status = response.status();
        require_success(status, "connector request")?;
        let bytes = read_bounded(response, MAX_JSON_BYTES, "connector response").await?;
        let value = match self.manifest.response_type {
            ConnectorResponseType::Json => serde_json::from_slice(&bytes).map_err(|error| {
                KomaError::ProviderChanged(format!("connector returned invalid JSON: {error}"))
            })?,
            ConnectorResponseType::Text => {
                Value::String(String::from_utf8(bytes.to_vec()).map_err(|error| {
                    KomaError::ProviderChanged(format!(
                        "connector returned invalid UTF-8 text: {error}"
                    ))
                })?)
            }
        };
        let value = self.transform_response(value, source).await?;
        let mut feed = map_feed(&value, &self.manifest)?;
        self.hydrate_pages(&mut feed).await?;
        for chapter in &feed.chapters {
            for page in &chapter.pages {
                let page_url = Url::parse(&page.url)?;
                self.validate_url(&page_url, &self.manifest.allowed_page_hosts)?;
            }
        }
        Ok((feed, url, status.as_u16()))
    }

    async fn transform_response(&self, value: Value, source: &str) -> Result<Value> {
        let Some(script) = self.manifest.transform_script.clone() else {
            return Ok(value);
        };
        let captures = self.source_captures(source)?;
        let source = source.trim().to_owned();
        let policy = ScriptNetworkPolicy {
            allowed_hosts: self.manifest.allowed_request_hosts.clone(),
            allow_local_network: self.manifest.allow_local_network,
        };
        let settings = self.manifest.settings.clone();
        tokio::task::spawn_blocking(move || {
            run_transform_with_network(&script, value, &source, captures, settings, Some(policy))
        })
        .await
        .map_err(|error| KomaError::Other(format!("connector script task failed: {error}")))?
    }

    async fn hydrate_pages(&self, feed: &mut MappedFeed) -> Result<()> {
        if self.manifest.mapping.chapter_pages.is_some() {
            return Ok(());
        }
        let chapters = std::mem::take(&mut feed.chapters);
        feed.chapters = stream::iter(chapters)
            .map(|mut chapter| async move {
                let request_url = chapter.page_request_url.as_ref().ok_or_else(|| {
                    KomaError::ProviderChanged(
                        "connector chapter has no page request URL".to_owned(),
                    )
                })?;
                let url = Url::parse(request_url)?;
                self.validate_url(&url, &self.manifest.allowed_request_hosts)?;
                let response = self
                    .client_for(&url)
                    .await?
                    .get(url)
                    .header(ACCEPT, "application/json")
                    .send()
                    .await?;
                require_success(response.status(), "connector page-list request")?;
                let bytes =
                    read_bounded(response, MAX_JSON_BYTES, "connector page-list response").await?;
                let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
                    KomaError::ProviderChanged(format!(
                        "connector returned an invalid page-list response: {error}"
                    ))
                })?;
                let pointer = self
                    .manifest
                    .mapping
                    .page_response_pages
                    .as_deref()
                    .ok_or_else(|| {
                        KomaError::ProviderChanged(
                            "connector page-list mapping is missing".to_owned(),
                        )
                    })?;
                let pages = value
                    .pointer(pointer)
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        KomaError::ProviderChanged(
                            "connector page-list mapping is not an array".to_owned(),
                        )
                    })?;
                chapter.pages = map_pages(pages, &self.manifest.mapping)?;
                Ok::<MappedChapter, KomaError>(chapter)
            })
            .buffered(6)
            .try_collect()
            .await?;
        Ok(())
    }

    async fn download_page(
        &self,
        page: MappedPage,
        index: usize,
        stem: String,
        staging: PathBuf,
        referer: Url,
        downloaded_bytes: Arc<AtomicU64>,
    ) -> Result<(String, PathBuf)> {
        let url = Url::parse(&page.url)?;
        self.validate_url(&url, &self.manifest.allowed_page_hosts)?;
        let response = self
            .client_for(&url)
            .await?
            .get(url.clone())
            .header(ACCEPT, "image/avif,image/webp,image/*")
            .header(REFERER, referer.as_str())
            .send()
            .await?;
        require_success(response.status(), &format!("page {} download", index + 1))?;
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = read_bounded(response, MAX_IMPORT_PAGE_BYTES, "connector page").await?;
        let byte_size = bytes.len() as u64;
        downloaded_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(byte_size)
                    .filter(|next| *next <= MAX_IMPORT_BYTES)
            })
            .map_err(|_| {
                KomaError::ProviderChanged(
                    "connector import exceeded the 4 GiB safety limit".to_owned(),
                )
            })?;
        let extension = page_extension(&url, content_type.as_deref());
        let name = format!("{stem}.{extension}");
        let (width, height) = validate_page_bytes(&name, &bytes)?;
        if page.width.is_some_and(|expected| width != Some(expected))
            || page.height.is_some_and(|expected| height != Some(expected))
        {
            return Err(KomaError::InvalidImage(format!(
                "page {} dimensions did not match the connector feed",
                index + 1
            )));
        }
        let path = staging.join(&name);
        tokio::fs::write(&path, bytes).await?;
        Ok((name, path))
    }
}

#[async_trait]
impl LinkImporter for DeclarativeImporter {
    fn provider(&self) -> &str {
        &self.manifest.name
    }

    fn recognizes(&self, source: &str) -> bool {
        self.matcher.is_match(source.trim()) && self.request_url(source).is_ok()
    }

    async fn preview(&self, source: &str) -> Result<ImportPreview> {
        let (feed, request_url, status) = self.fetch_feed(source).await?;
        let volumes = feed.volumes();
        let chapters = feed.chapter_summaries();
        let selected_volume_id = volumes.first().map(|volume| volume.id);
        let selected_chapter_id = chapters.last().map(|chapter| chapter.id);
        let estimated_page_count = volumes.first().and_then(|volume| volume.page_count);
        let series_chapter_count = feed.chapters.len();
        let series_page_count = feed.page_count();
        Ok(ImportPreview {
            provider: self.provider().to_owned(),
            title: feed.title,
            source_url: source.trim().to_owned(),
            eligibility_url: request_url.to_string(),
            eligibility_status: status,
            eligible: true,
            warning: super::IMPORT_WARNING.to_owned(),
            volumes,
            chapters,
            selected_volume_id,
            selected_chapter_id,
            estimated_page_count,
            series_chapter_count: Some(series_chapter_count),
            series_page_count: Some(series_page_count),
            available_scopes: self
                .manifest
                .capabilities
                .iter()
                .map(|capability| match capability {
                    ConnectorCapability::Chapter => ImportScope::Chapter,
                    ConnectorCapability::Volume => ImportScope::Volume,
                    ConnectorCapability::Series => ImportScope::Series,
                })
                .collect(),
        })
    }

    async fn resolve_online(
        &self,
        source: &str,
        options: &ImportOptions,
    ) -> Result<RemotePublication> {
        let (feed, request_url, status) = self.fetch_feed(source).await?;
        let title = feed.title.clone();
        let language = Some(feed.language.clone());
        let chapter_summaries = feed.chapter_summaries();
        let volume_summaries = feed.volumes();
        let active_chapter_id = (options.scope == ImportScope::Chapter)
            .then(|| {
                options
                    .chapter_id
                    .or_else(|| chapter_summaries.last().map(|chapter| chapter.id))
            })
            .flatten();
        let active_volume_id = (options.scope == ImportScope::Volume)
            .then(|| {
                options
                    .volume_id
                    .or_else(|| volume_summaries.first().map(|volume| volume.id))
            })
            .flatten();
        let chapters = feed
            .selected_chapters(options)?
            .into_iter()
            .enumerate()
            .map(|(index, chapter)| RemoteChapter {
                id: Some((index + 1).to_string()),
                number: chapter.number,
                title: None,
                volume: chapter.volume,
                pages: chapter
                    .pages
                    .into_iter()
                    .map(|page| RemotePage {
                        url: page.url,
                        width: page.width,
                        height: page.height,
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        if chapters.is_empty() || chapters.iter().all(|chapter| chapter.pages.is_empty()) {
            return Err(KomaError::ProviderChanged(
                "connector returned no pages for the selected scope".to_owned(),
            ));
        }
        Ok(RemotePublication {
            provider: self.provider().to_owned(),
            source_url: source.trim().to_owned(),
            eligibility_url: request_url.to_string(),
            eligibility_status: status,
            title,
            language,
            scope: options.scope,
            volume_id: active_volume_id,
            chapter_id: active_chapter_id,
            selected_chapter_ids: options.selected_chapter_ids.clone(),
            chapters,
            chapter_catalog: if options.scope == ImportScope::Chapter {
                chapter_summaries
                    .into_iter()
                    .map(|chapter| RemoteNavigationItem {
                        id: chapter.id,
                        number: chapter.number,
                        title: chapter.name,
                        language: chapter.language,
                    })
                    .collect()
            } else {
                Vec::new()
            },
            volume_catalog: if options.scope == ImportScope::Volume {
                volume_summaries
                    .into_iter()
                    .map(|volume| RemoteNavigationItem {
                        id: volume.id,
                        number: volume.number,
                        title: volume.name,
                        language: volume.language,
                    })
                    .collect()
            } else {
                Vec::new()
            },
            allowed_page_hosts: self.manifest.allowed_page_hosts.clone(),
            allow_local_network: self.manifest.allow_local_network,
        })
    }

    async fn import(
        &self,
        source: &str,
        options: &ImportOptions,
        events: Option<&UnboundedSender<ImportEvent>>,
    ) -> Result<ImportReceipt> {
        emit(
            events,
            ImportEvent::Checking {
                url: source.trim().to_owned(),
            },
        );
        let (feed, request_url, status) = self.fetch_feed(source).await?;
        emit(events, ImportEvent::Eligible { status });
        let chapters = feed.selected_chapters(options)?;
        let total = chapters
            .iter()
            .map(|chapter| chapter.pages.len())
            .sum::<usize>();
        if total == 0 || total > MAX_PAGES {
            return Err(KomaError::ProviderChanged(format!(
                "connector import must contain between 1 and {MAX_PAGES} pages"
            )));
        }
        emit(
            events,
            ImportEvent::Discovered {
                title: feed.title.clone(),
                volume: match options.scope {
                    ImportScope::Chapter => "Chapter".to_owned(),
                    ImportScope::Volume => "Volume".to_owned(),
                    ImportScope::Series => "Series".to_owned(),
                },
                page_count: total,
            },
        );
        std::fs::create_dir_all(&options.destination_directory)?;
        let staging = tempfile::tempdir_in(&options.destination_directory)?;
        let staging_pages = staging.path().join("pages");
        std::fs::create_dir(&staging_pages)?;
        let width = total.to_string().len().max(4);
        let mut specifications = Vec::with_capacity(total);
        let mut chapter_ranges = Vec::with_capacity(chapters.len());
        for chapter in chapters {
            let start_page_index = specifications.len();
            let page_width = chapter.pages.len().to_string().len().max(3);
            for (page_index, page) in chapter.pages.into_iter().enumerate() {
                let index = specifications.len();
                specifications.push((
                    page,
                    format!(
                        "{:0width$}-ch{}-{:0page_width$}",
                        index + 1,
                        volume_number_label(chapter.number),
                        page_index + 1,
                    ),
                ));
            }
            chapter_ranges.push(ChapterRange {
                id: None,
                number: chapter.number,
                title: None,
                start_page_index,
                end_page_index: specifications.len() - 1,
            });
        }
        let concurrency = options.download_concurrency.clamp(1, 8);
        let completed = Arc::new(AtomicUsize::new(0));
        let bytes = Arc::new(AtomicU64::new(0));
        let sender = events.cloned();
        let results = stream::iter(specifications.into_iter().enumerate())
            .map(|(index, (page, stem))| {
                let staging_pages = staging_pages.clone();
                let request_url = request_url.clone();
                let bytes = Arc::clone(&bytes);
                let completed = Arc::clone(&completed);
                let sender = sender.clone();
                async move {
                    let result = self
                        .download_page(page, index, stem, staging_pages, request_url, bytes)
                        .await;
                    if result.is_ok() {
                        let completed = completed.fetch_add(1, Ordering::Relaxed) + 1;
                        emit(
                            sender.as_ref(),
                            ImportEvent::Downloading { completed, total },
                        );
                    }
                    (index, result)
                }
            })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;
        let mut ordered = vec![None; total];
        for (index, result) in results {
            ordered[index] = Some(result?);
        }
        let downloaded = ordered
            .into_iter()
            .enumerate()
            .map(|(index, page)| {
                page.ok_or_else(|| {
                    KomaError::ProviderUnavailable(format!(
                        "connector page {} was not downloaded",
                        index + 1
                    ))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let title = sanitize_file_component(&feed.title);
        let language = sanitize_file_component(&feed.language);
        let qualifier = match options.scope {
            ImportScope::Chapter => "Chapter",
            ImportScope::Volume => "Volume",
            ImportScope::Series => "Complete",
        };
        let output_path = choose_output_path(
            &options
                .destination_directory
                .join(format!("{title} — {qualifier} [{language}].cbz")),
            options.overwrite_existing,
        );
        emit(
            events,
            ImportEvent::Packaging {
                output_path: output_path.clone(),
            },
        );
        let comic_info = ComicInfo {
            title: Some(feed.title.clone()),
            series: Some(feed.title.clone()),
            web: Some(source.trim().to_owned()),
            language_iso: Some(feed.language.clone()),
            page_count: Some(total),
            ..ComicInfo::default()
        };
        let package_path = output_path.clone();
        tokio::task::spawn_blocking(move || {
            ZipPublication::write_cbz_from_files_with_metadata(
                &package_path,
                downloaded,
                &comic_info,
                Some(&KomaArchiveMetadata::new(chapter_ranges)),
            )
        })
        .await
        .map_err(|error| KomaError::Other(format!("CBZ packaging task failed: {error}")))??;
        let hash_path = output_path.clone();
        let output_hash = tokio::task::spawn_blocking(move || hash_file(&hash_path))
            .await
            .map_err(|error| KomaError::Other(format!("hash task failed: {error}")))??;
        let receipt = ImportReceipt {
            id: Uuid::now_v7(),
            provider: self.provider().to_owned(),
            source_url: source.trim().to_owned(),
            eligibility_url: request_url.to_string(),
            eligibility_status: status,
            checked_at: Utc::now(),
            page_count: total,
            output_path,
            output_hash,
            adapter_version: format!(
                "connector-v{}:{}:{}:{ADAPTER_VERSION}",
                self.manifest.schema_version, self.manifest.id, self.manifest.version
            ),
        };
        emit(
            events,
            ImportEvent::Completed {
                receipt: receipt.clone(),
            },
        );
        Ok(receipt)
    }
}

#[derive(Debug)]
struct MappedFeed {
    title: String,
    language: String,
    chapters: Vec<MappedChapter>,
}

#[derive(Debug, Clone)]
struct MappedChapter {
    number: f64,
    volume: Option<f64>,
    pages: Vec<MappedPage>,
    page_request_url: Option<String>,
}

#[derive(Debug, Clone)]
struct MappedPage {
    url: String,
    width: Option<u32>,
    height: Option<u32>,
}

impl MappedFeed {
    fn page_count(&self) -> usize {
        self.chapters
            .iter()
            .map(|chapter| chapter.pages.len())
            .sum()
    }

    fn volumes(&self) -> Vec<ImportVolume> {
        let mut grouped = BTreeMap::<String, (f64, usize, usize)>::new();
        for chapter in &self.chapters {
            let Some(volume) = chapter.volume else {
                continue;
            };
            let key = volume_number_label(volume);
            let entry = grouped.entry(key).or_insert((volume, 0, 0));
            entry.1 += 1;
            entry.2 += chapter.pages.len();
        }
        grouped
            .into_iter()
            .enumerate()
            .map(|(index, (_, (number, chapters, pages)))| ImportVolume {
                id: index as u64 + 1,
                number,
                name: None,
                language: self.language.clone(),
                chapter_count: Some(chapters),
                page_count: Some(pages),
                selected: index == 0,
            })
            .collect()
    }

    fn chapter_summaries(&self) -> Vec<ImportChapter> {
        let last = self.chapters.len();
        self.chapters
            .iter()
            .enumerate()
            .map(|(index, chapter)| ImportChapter {
                id: index as u64 + 1,
                number: chapter.number,
                name: None,
                language: self.language.clone(),
                page_count: Some(chapter.pages.len()),
                selected: index + 1 == last,
            })
            .collect()
    }

    fn selected_chapters(&self, options: &ImportOptions) -> Result<Vec<MappedChapter>> {
        match options.scope {
            ImportScope::Series => {
                if options.selected_chapter_ids.is_empty() {
                    return Ok(self.chapters.clone());
                }
                let selected = options
                    .selected_chapter_ids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>();
                let chapters = self
                    .chapters
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| selected.contains(&(*index as u64 + 1)))
                    .map(|(_, chapter)| chapter.clone())
                    .collect::<Vec<_>>();
                if chapters.len() != selected.len() {
                    return Err(KomaError::ImportDenied(
                        "one or more selected chapters are not exposed by this connector"
                            .to_owned(),
                    ));
                }
                if chapters.is_empty() {
                    return Err(KomaError::ImportDenied(
                        "select at least one chapter to import".to_owned(),
                    ));
                }
                return Ok(chapters);
            }
            ImportScope::Chapter => {
                let index = options
                    .chapter_id
                    .unwrap_or(self.chapters.len() as u64)
                    .checked_sub(1)
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| {
                        KomaError::ImportDenied(
                            "connector feed does not expose that chapter".to_owned(),
                        )
                    })?;
                return self
                    .chapters
                    .get(index)
                    .cloned()
                    .map(|chapter| vec![chapter])
                    .ok_or_else(|| {
                        KomaError::ImportDenied(
                            "connector feed does not expose that chapter".to_owned(),
                        )
                    });
            }
            ImportScope::Volume => {}
        }
        let volumes = self.volumes();
        let requested = options
            .volume_id
            .or_else(|| volumes.first().map(|volume| volume.id));
        let volume = volumes
            .into_iter()
            .find(|volume| Some(volume.id) == requested)
            .ok_or_else(|| {
                KomaError::ImportDenied("connector feed does not expose that volume".to_owned())
            })?
            .number;
        Ok(self
            .chapters
            .iter()
            .filter(|chapter| chapter.volume == Some(volume))
            .cloned()
            .collect())
    }
}

fn map_feed(value: &Value, manifest: &ConnectorManifest) -> Result<MappedFeed> {
    let mapping = &manifest.mapping;
    let title = pointer_string(value, &mapping.title, "title")?;
    let language = mapping
        .language
        .as_deref()
        .and_then(|pointer| value.pointer(pointer))
        .and_then(Value::as_str)
        .unwrap_or("und")
        .trim()
        .to_owned();
    let chapters = value
        .pointer(&mapping.chapters)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            KomaError::ProviderChanged("connector chapter mapping is not an array".to_owned())
        })?;
    if chapters.is_empty() || chapters.len() > MAX_CONNECTOR_CHAPTERS {
        return Err(KomaError::ProviderChanged(format!(
            "connector feed must contain between 1 and {MAX_CONNECTOR_CHAPTERS} entries"
        )));
    }
    let mut mapped = Vec::with_capacity(chapters.len());
    for chapter in chapters {
        let number = pointer_number(chapter, &mapping.chapter_number, "chapter number")?;
        let volume = mapping
            .chapter_volume
            .as_deref()
            .and_then(|pointer| chapter.pointer(pointer))
            .and_then(value_number);
        let mapped_pages = if let Some(pointer) = mapping.chapter_pages.as_deref() {
            let pages = chapter
                .pointer(pointer)
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    KomaError::ProviderChanged("connector page mapping is not an array".to_owned())
                })?;
            map_pages(pages, mapping)?
        } else {
            Vec::new()
        };
        let page_request_url = manifest
            .page_request_url
            .as_deref()
            .map(|template| expand_value_template(template, chapter))
            .transpose()?;
        mapped.push(MappedChapter {
            number,
            volume,
            pages: mapped_pages,
            page_request_url,
        });
    }
    mapped.sort_by(|left, right| {
        left.number
            .partial_cmp(&right.number)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if mapped.is_empty() {
        return Err(KomaError::ProviderChanged(
            "connector feed contains no chapters".to_owned(),
        ));
    }
    Ok(MappedFeed {
        title,
        language,
        chapters: mapped,
    })
}

fn map_pages(pages: &[Value], mapping: &ConnectorMapping) -> Result<Vec<MappedPage>> {
    let mut mapped_pages = Vec::with_capacity(pages.len());
    for page in pages {
        let url = match mapping.page_url.as_deref() {
            Some(pointer) => pointer_string(page, pointer, "page URL")?,
            None => page.as_str().map(str::to_owned).ok_or_else(|| {
                KomaError::ProviderChanged("connector page must be a URL string".to_owned())
            })?,
        };
        let width = mapping
            .page_width
            .as_deref()
            .and_then(|pointer| page.pointer(pointer))
            .and_then(value_number)
            .and_then(|value| u32::try_from(value as i64).ok())
            .filter(|value| *value > 0);
        let height = mapping
            .page_height
            .as_deref()
            .and_then(|pointer| page.pointer(pointer))
            .and_then(value_number)
            .and_then(|value| u32::try_from(value as i64).ok())
            .filter(|value| *value > 0);
        mapped_pages.push(MappedPage { url, width, height });
    }
    Ok(mapped_pages)
}

fn expand_value_template(template: &str, value: &Value) -> Result<String> {
    let mut expanded = String::with_capacity(template.len());
    let mut remainder = template;
    while let Some(start) = remainder.find('{') {
        expanded.push_str(&remainder[..start]);
        let after_start = &remainder[start + 1..];
        let end = after_start.find('}').ok_or_else(|| {
            KomaError::ImportDenied("connector page request template has an open brace".to_owned())
        })?;
        let pointer = &after_start[..end];
        if !pointer.starts_with('/') {
            return Err(KomaError::ImportDenied(
                "connector page request placeholders must be JSON pointers".to_owned(),
            ));
        }
        let replacement = value
            .pointer(pointer)
            .and_then(value_scalar)
            .ok_or_else(|| {
                KomaError::ProviderChanged(format!(
                    "connector page request value {pointer} is missing"
                ))
            })?;
        expanded.push_str(&replacement);
        remainder = &after_start[end + 1..];
    }
    if remainder.contains('}') {
        return Err(KomaError::ImportDenied(
            "connector page request template has a closing brace without an opening brace"
                .to_owned(),
        ));
    }
    expanded.push_str(remainder);
    Ok(expanded)
}

fn value_scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn pointer_string(value: &Value, pointer: &str, label: &str) -> Result<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| KomaError::ProviderChanged(format!("connector {label} is missing")))
}

fn pointer_number(value: &Value, pointer: &str, label: &str) -> Result<f64> {
    value
        .pointer(pointer)
        .and_then(value_number)
        .ok_or_else(|| KomaError::ProviderChanged(format!("connector {label} is missing")))
}

fn value_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn validate_script(script: &str) -> Result<()> {
    script_engine()
        .compile(script)
        .map(|_| ())
        .map_err(|error| {
            KomaError::ImportDenied(format!("connector Rhai transform is invalid: {error}"))
        })
}

#[cfg(test)]
fn run_transform(
    script: &str,
    response: Value,
    source: &str,
    captures: BTreeMap<String, String>,
) -> Result<Value> {
    run_transform_with_network(script, response, source, captures, BTreeMap::new(), None)
}

#[derive(Clone)]
struct ScriptNetworkPolicy {
    allowed_hosts: Vec<String>,
    allow_local_network: bool,
}

fn run_transform_with_network(
    script: &str,
    response: Value,
    source: &str,
    captures: BTreeMap<String, String>,
    settings: BTreeMap<String, String>,
    network: Option<ScriptNetworkPolicy>,
) -> Result<Value> {
    let mut engine = script_engine();
    if let Some(policy) = network {
        let requests = Arc::new(AtomicUsize::new(0));
        engine.register_fn(
            "http",
            move |method: &str,
                  url: &str,
                  headers: Map,
                  body: &str|
                  -> std::result::Result<Dynamic, Box<EvalAltResult>> {
                let count = requests.fetch_add(1, Ordering::Relaxed) + 1;
                if count > 64 {
                    return Err("connector exceeded the 64 request limit".into());
                }
                script_http_request(&policy, method, url, headers, body)
                    .map_err(|error| error.to_string().into())
            },
        );
    }
    let started = std::time::Instant::now();
    engine.on_progress(move |_| {
        (started.elapsed() > std::time::Duration::from_secs(MAX_SCRIPT_SECONDS))
            .then(|| Dynamic::from("connector script timed out"))
    });
    let ast = engine.compile(script).map_err(|error| {
        KomaError::ProviderChanged(format!(
            "connector Rhai transform could not compile: {error}"
        ))
    })?;
    let response = rhai::serde::to_dynamic(response).map_err(|error| {
        KomaError::ProviderChanged(format!(
            "connector response could not be passed to Rhai: {error}"
        ))
    })?;
    let captures = rhai::serde::to_dynamic(captures).map_err(|error| {
        KomaError::ProviderChanged(format!(
            "connector captures could not be passed to Rhai: {error}"
        ))
    })?;
    let settings = rhai::serde::to_dynamic(settings).map_err(|error| {
        KomaError::ProviderChanged(format!(
            "connector settings could not be passed to Rhai: {error}"
        ))
    })?;
    let mut scope = Scope::new();
    scope.push_dynamic("response", response);
    scope.push("source", source.to_owned());
    scope.push_dynamic("captures", captures);
    scope.push_dynamic("settings", settings);
    let output = engine
        .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
        .map_err(|error| {
            KomaError::ProviderChanged(format!("connector Rhai transform failed: {error}"))
        })?;
    let value: Value = rhai::serde::from_dynamic(&output).map_err(|error| {
        KomaError::ProviderChanged(format!(
            "connector Rhai transform returned unsupported data: {error}"
        ))
    })?;
    let encoded = serde_json::to_vec(&value).map_err(|error| {
        KomaError::ProviderChanged(format!(
            "connector Rhai transform could not be measured: {error}"
        ))
    })?;
    if encoded.len() as u64 > MAX_JSON_BYTES {
        return Err(KomaError::ProviderChanged(
            "connector Rhai transform exceeded the 32 MiB output limit".to_owned(),
        ));
    }
    Ok(value)
}

fn script_http_request(
    policy: &ScriptNetworkPolicy,
    method: &str,
    raw_url: &str,
    headers: Map,
    body: &str,
) -> Result<Dynamic> {
    let url = Url::parse(raw_url)?;
    if url.username() != "" || url.password().is_some() || url.fragment().is_some() {
        return Err(KomaError::ImportDenied(
            "connector HTTP URL contains credentials or a fragment".to_owned(),
        ));
    }
    if url.scheme() != "https" && !(policy.allow_local_network && url.scheme() == "http") {
        return Err(KomaError::ImportDenied(
            "connector HTTP requests must use HTTPS".to_owned(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| KomaError::ImportDenied("connector HTTP URL has no host".to_owned()))?
        .to_ascii_lowercase();
    if !policy
        .allowed_hosts
        .iter()
        .any(|allowed| host_matches(&host, allowed))
    {
        return Err(KomaError::ImportDenied(format!(
            "connector did not declare HTTP access to {host}"
        )));
    }
    let port = url.port_or_known_default().ok_or_else(|| {
        KomaError::ImportDenied("connector HTTP URL has no recognized port".to_owned())
    })?;
    let addresses = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| KomaError::ProviderUnavailable(error.to_string()))?
        .collect::<Vec<_>>();
    if addresses.is_empty()
        || (!policy.allow_local_network
            && addresses
                .iter()
                .any(|address| is_non_public_ip(address.ip())))
    {
        return Err(KomaError::ImportDenied(
            "connector HTTP host did not resolve to an allowed address".to_owned(),
        ));
    }
    let mut builder = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("Koma/", env!("CARGO_PKG_VERSION")));
    for address in addresses {
        builder = builder.resolve(&host, address);
    }
    let client = builder.build()?;
    let method = reqwest::Method::from_bytes(method.trim().to_ascii_uppercase().as_bytes())
        .map_err(|_| KomaError::ImportDenied("connector HTTP method is invalid".to_owned()))?;
    let mut request = client.request(method, url);
    for (name, value) in headers {
        if matches!(
            name.as_str().to_ascii_lowercase().as_str(),
            "host" | "content-length" | "transfer-encoding" | "connection"
        ) {
            return Err(KomaError::ImportDenied(format!(
                "connector HTTP header {name} is managed by Koma"
            )));
        }
        let value = value.into_string().map_err(|_| {
            KomaError::ImportDenied("connector HTTP header values must be strings".to_owned())
        })?;
        request = request.header(name.as_str(), value);
    }
    if !body.is_empty() {
        request = request.body(body.to_owned());
    }
    let response = request.send()?;
    let status = response.status().as_u16();
    let response_headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    let content_length = response.content_length().unwrap_or(0);
    if content_length > MAX_JSON_BYTES {
        return Err(KomaError::ProviderChanged(
            "connector HTTP response exceeded 32 MiB".to_owned(),
        ));
    }
    let bytes = response.bytes()?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(KomaError::ProviderChanged(
            "connector HTTP response exceeded 32 MiB".to_owned(),
        ));
    }
    let text = String::from_utf8(bytes.to_vec()).map_err(|error| {
        KomaError::ProviderChanged(format!("connector HTTP response is not UTF-8: {error}"))
    })?;
    let json = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
    rhai::serde::to_dynamic(serde_json::json!({
        "status": status,
        "headers": response_headers,
        "body": text,
        "json": json,
    }))
    .map_err(|error| KomaError::ProviderChanged(error.to_string()))
}

fn script_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(MAX_SCRIPT_OPERATIONS);
    engine.set_max_call_levels(32);
    engine.set_max_expr_depths(64, 32);
    engine.set_max_string_size(8 * 1024 * 1024);
    engine.set_max_array_size(MAX_PAGES);
    engine.set_max_map_size(16_384);
    engine.disable_symbol("eval");
    engine.disable_symbol("import");
    engine.on_print(|_| {});
    engine.on_debug(|_, _, _| {});
    engine.register_fn("sha256", |value: &str| -> String {
        hex_bytes(&Sha256::digest(value.as_bytes()))
    });
    engine.register_fn("hmac_sha256", |secret: &str, value: &str| -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("HMAC accepts keys of every size");
        mac.update(value.as_bytes());
        hex_bytes(&mac.finalize().into_bytes())
    });
    engine.register_fn("base64", |value: &str| -> String {
        STANDARD.encode(value.as_bytes())
    });
    engine.register_fn("url_encode", |value: &str| -> String {
        url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
    });
    engine.register_fn(
        "regex_capture",
        |value: &str, pattern: &str, group: i64| -> String {
            let Ok(group) = usize::try_from(group) else {
                return String::new();
            };
            regex::Regex::new(pattern)
                .ok()
                .and_then(|regex| regex.captures(value))
                .and_then(|captures| captures.get(group))
                .map(|capture| capture.as_str().to_owned())
                .unwrap_or_default()
        },
    );
    engine.register_fn("regex_find_all", |value: &str, pattern: &str| -> Array {
        regex::Regex::new(pattern)
            .map(|regex| {
                regex
                    .find_iter(value)
                    .take(MAX_PAGES)
                    .map(|capture| Dynamic::from(capture.as_str().to_owned()))
                    .collect()
            })
            .unwrap_or_default()
    });
    engine.register_fn(
        "html_select",
        |html: &str, selector: &str, attribute: &str| -> Array {
            let Some(selector) = Selector::parse(selector).ok() else {
                return Array::new();
            };
            Html::parse_document(html)
                .select(&selector)
                .take(MAX_PAGES)
                .filter_map(|element| {
                    if attribute.is_empty() {
                        Some(element.text().collect::<String>().trim().to_owned())
                    } else {
                        element
                            .value()
                            .attr(attribute)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned)
                    }
                })
                .map(Dynamic::from)
                .collect()
        },
    );
    engine
}

fn hex_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn host_matches(host: &str, allowed: &str) -> bool {
    let allowed = allowed.trim().to_ascii_lowercase();
    if let Some(suffix) = allowed.strip_prefix("*.") {
        host.ends_with(&format!(".{suffix}")) && host != suffix
    } else {
        host == allowed
    }
}

fn valid_host_permission(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    let host = value.strip_prefix("*.").unwrap_or(&value);
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

pub fn bundled_mangafire_summary() -> ConnectorSummary {
    ConnectorSummary {
        id: "mangafire".to_owned(),
        name: "MangaFire".to_owned(),
        version: ADAPTER_VERSION.to_owned(),
        description: None,
        kind: ConnectorKind::Bundled,
        enabled: true,
        removable: false,
        schema_version: 0,
        runs_code: false,
        capabilities: vec![
            ConnectorCapability::Chapter,
            ConnectorCapability::Volume,
            ConnectorCapability::Series,
        ],
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::{
        ConnectorManifest, DeclarativeImporter, ScriptNetworkPolicy, map_feed, run_transform,
        run_transform_with_network,
    };
    use crate::importer::{ImportOptions, ImportScope, LinkImporter, fetch_remote_page};

    fn manifest() -> ConnectorManifest {
        ConnectorManifest {
            json_schema: None,
            schema_version: 1,
            id: "fixture-feed".to_owned(),
            name: "Fixture Feed".to_owned(),
            version: "1.0.0".to_owned(),
            description: None,
            source_pattern: r"^https://example\.com/series/(?P<id>[a-z0-9-]+)$".to_owned(),
            request_url: "https://api.example.com/series/$id".to_owned(),
            page_request_url: None,
            allowed_request_hosts: vec!["api.example.com".to_owned()],
            allowed_page_hosts: vec!["*.example-cdn.com".to_owned()],
            allow_local_network: false,
            response_type: super::ConnectorResponseType::Json,
            capabilities: super::default_capabilities(),
            mapping: super::ConnectorMapping {
                title: "/data/title".to_owned(),
                language: Some("/data/language".to_owned()),
                chapters: "/data/chapters".to_owned(),
                chapter_number: "/number".to_owned(),
                chapter_volume: Some("/volume".to_owned()),
                chapter_pages: Some("/pages".to_owned()),
                page_response_pages: None,
                page_url: Some("/url".to_owned()),
                page_width: None,
                page_height: None,
            },
            transform_script: None,
            settings: BTreeMap::new(),
        }
    }

    #[test]
    fn validates_and_maps_fractional_chapters_in_numeric_order() {
        let manifest = manifest();
        manifest.validate().expect("manifest");
        let value = serde_json::json!({
            "data": {
                "title": "Fixture",
                "language": "en",
                "chapters": [
                    {"number": 1, "volume": 1, "pages": [{"url":"https://a.example-cdn.com/1.jpg"}]},
                    {"number": 0.5, "volume": 1, "pages": [{"url":"https://a.example-cdn.com/0.jpg"}]}
                ]
            }
        });
        let feed = map_feed(&value, &manifest).expect("feed");
        assert_eq!(feed.chapters[0].number, 0.5);
        assert_eq!(feed.page_count(), 2);
        assert_eq!(feed.volumes()[0].chapter_count, Some(2));
    }

    #[test]
    fn rejects_executable_or_undeclared_connector_shapes() {
        let mut invalid_id = manifest();
        invalid_id.id = "../plugin".to_owned();
        assert!(invalid_id.validate().is_err());
        let mut missing_hosts = manifest();
        missing_hosts.allowed_page_hosts.clear();
        assert!(missing_hosts.validate().is_err());
    }

    #[test]
    fn shipped_connector_examples_parse_and_validate() {
        for source in [
            include_str!("../../../../connectors/examples/koma-feed-v1.koma-connector.json"),
            include_str!("../../../../connectors/examples/koma-staged-feed-v1.koma-connector.json"),
            include_str!("../../../../connectors/examples/relative-pages-v2.koma-connector.json"),
        ] {
            let manifest: ConnectorManifest =
                serde_json::from_str(source).expect("example connector parses");
            manifest.validate().expect("example connector validates");
        }
    }

    #[test]
    fn schema_v2_rhai_normalizes_relative_page_paths() {
        let source =
            include_str!("../../../../connectors/examples/relative-pages-v2.koma-connector.json");
        let manifest = ConnectorManifest::from_json(source.as_bytes()).expect("v2 connector");
        assert!(manifest.summary().runs_code);
        let input = serde_json::json!({
            "title": "Relative Fixture",
            "language": "en",
            "entries": [{
                "number": 0.5,
                "volume": 1,
                "pages": [{"path": "gallery/1.webp", "width": 1200, "height": 1800}]
            }]
        });
        let output = run_transform(
            manifest.transform_script.as_deref().expect("script"),
            input,
            "https://reader.example/series/42",
            std::collections::BTreeMap::from([("id".to_owned(), "42".to_owned())]),
        )
        .expect("transform");
        let feed = map_feed(&output, &manifest).expect("mapped feed");
        assert_eq!(feed.title, "Relative Fixture");
        assert_eq!(feed.chapters[0].number, 0.5);
        assert_eq!(
            feed.chapters[0].pages[0].url,
            "https://images.reader.example/gallery/1.webp"
        );
    }

    #[test]
    fn schema_v2_rhai_stops_runaway_scripts() {
        let result = run_transform(
            "loop {}",
            serde_json::json!({}),
            "https://reader.example/series/42",
            std::collections::BTreeMap::new(),
        );
        assert!(result.is_err(), "runaway connector script must be stopped");
    }

    #[test]
    fn schema_v2_rhai_http_is_guarded_and_usable() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut request = [0_u8; 4096];
            let read = socket.read(&mut request).expect("read");
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.starts_with("POST /catalog "));
            assert!(request.contains("ping"));
            let body = r#"{"chapters":[]}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("response");
        });
        let script = format!(
            r#"let result = http("POST", "http://{address}/catalog", #{{ "Content-Type": "text/plain" }}, "ping"); result.json"#
        );
        let output = run_transform_with_network(
            &script,
            serde_json::json!({}),
            "https://reader.example/title/test",
            BTreeMap::new(),
            BTreeMap::new(),
            Some(ScriptNetworkPolicy {
                allowed_hosts: vec!["127.0.0.1".to_owned()],
                allow_local_network: true,
            }),
        )
        .expect("script HTTP");
        assert_eq!(output, serde_json::json!({"chapters": []}));
        server.join().expect("server");
    }

    #[tokio::test]
    async fn imports_a_local_json_connector_from_source_to_cbz() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("address");
        let png = Arc::new(
            STANDARD
                .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
                .expect("png"),
        );
        let page_half = format!("http://{address}/page-1.png");
        let page_one = format!("http://{address}/page-2.png");
        let feed = String::from(
            r#"{
                "title": "Connector Fixture",
                "language": "en",
                "entries": [
                    {"id": "1", "number": 1},
                    {"id": "half", "number": 0.5}
                ]
            }"#,
        );
        let served_png = Arc::clone(&png);
        let server = tokio::spawn(async move {
            for _ in 0..12 {
                let (mut socket, _) = listener.accept().await.expect("accept");
                let mut request = vec![0_u8; 4096];
                let read = socket.read(&mut request).await.expect("request");
                let request = String::from_utf8_lossy(&request[..read]);
                let (content_type, body) = if request.starts_with("GET /feed/fixture ") {
                    ("application/json", feed.as_bytes().to_vec())
                } else if request.starts_with("GET /pages/half ") {
                    (
                        "application/json",
                        format!(r#"{{"pages":["{page_half}"]}}"#).into_bytes(),
                    )
                } else if request.starts_with("GET /pages/1 ") {
                    (
                        "application/json",
                        format!(r#"{{"pages":["{page_one}"]}}"#).into_bytes(),
                    )
                } else {
                    ("image/png", served_png.as_ref().clone())
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("headers");
                socket.write_all(&body).await.expect("body");
            }
        });
        let manifest = ConnectorManifest {
            json_schema: None,
            schema_version: 1,
            id: "local-fixture".to_owned(),
            name: "Local Fixture".to_owned(),
            version: "1.0.0".to_owned(),
            description: None,
            source_pattern: format!(
                r"^http://127\.0\.0\.1:{}/source/(?P<id>[a-z0-9-]+)$",
                address.port()
            ),
            request_url: format!("http://127.0.0.1:{}/feed/$id", address.port()),
            page_request_url: Some(format!("http://127.0.0.1:{}/pages/{{/id}}", address.port())),
            allowed_request_hosts: vec!["127.0.0.1".to_owned()],
            allowed_page_hosts: vec!["127.0.0.1".to_owned()],
            allow_local_network: true,
            response_type: super::ConnectorResponseType::Json,
            capabilities: vec![super::ConnectorCapability::Series],
            mapping: super::ConnectorMapping {
                title: "/title".to_owned(),
                language: Some("/language".to_owned()),
                chapters: "/entries".to_owned(),
                chapter_number: "/number".to_owned(),
                chapter_volume: None,
                chapter_pages: None,
                page_response_pages: Some("/pages".to_owned()),
                page_url: None,
                page_width: None,
                page_height: None,
            },
            transform_script: None,
            settings: BTreeMap::new(),
        };
        let importer = DeclarativeImporter::new(manifest).expect("importer");
        let destination = tempfile::tempdir().expect("destination");
        let mut options = ImportOptions::new(destination.path());
        options.scope = ImportScope::Series;
        let source = format!("http://{address}/source/fixture");
        let preview = importer.preview(&source).await.expect("preview");
        assert_eq!(preview.series_chapter_count, Some(2));
        assert_eq!(preview.series_page_count, Some(2));
        let online = importer
            .resolve_online(&source, &options)
            .await
            .expect("online publication");
        assert_eq!(online.chapter_ranges().len(), 2);
        assert!(online.allow_local_network);
        assert_eq!(
            fetch_remote_page(&online, 0)
                .await
                .expect("streamed page")
                .bytes,
            *png,
        );
        let receipt = importer
            .import(&source, &options, None)
            .await
            .expect("import");
        let publication =
            crate::formats::open_publication(&receipt.output_path, None).expect("open CBZ");
        assert_eq!(publication.manifest().pages.len(), 2);
        assert!(
            publication.manifest().pages[0]
                .source_name
                .contains("ch0.5")
        );
        assert!(publication.manifest().pages[1].source_name.contains("ch1"));
        server.await.expect("server");
    }
}
