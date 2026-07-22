use std::{
    cmp::Ordering as CmpOrdering,
    collections::{HashMap, HashSet},
    io::Read,
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, stream};
use reqwest::{
    Client, Response, StatusCode,
    header::{ACCEPT, CONTENT_TYPE, REFERER, RETRY_AFTER},
    redirect::Policy,
};
use scraper::Html;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::sync::mpsc::UnboundedSender;
use url::Url;
use uuid::Uuid;

use crate::{
    error::{KomaError, Result},
    formats::{MAX_PAGES, ZipPublication, validate_page_bytes},
    metadata::ComicInfo,
    model::{ChapterRange, ImportReceipt, KomaArchiveMetadata, PageData},
};

mod connector;
pub use connector::{
    ConnectorCapability, ConnectorKind, ConnectorManifest, ConnectorSummary, DeclarativeImporter,
    bundled_mangafire_summary,
};

pub const IMPORT_WARNING: &str =
    "Only import properly released works that you own or have permission to download.";
const MANGAFIRE_ORIGIN: &str = "https://mangafire.to/";
const ADAPTER_VERSION: &str = "mangafire-api-2026.07-chapter-series.2";
const MAX_JSON_BYTES: u64 = 32 * 1024 * 1024;
const MAX_IMPORT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_IMPORT_PAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CHAPTER_FALLBACKS: usize = 500;
const MAX_CHAPTER_LIST_PAGES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOptions {
    pub destination_directory: PathBuf,
    pub volume_id: Option<u64>,
    #[serde(default)]
    pub chapter_id: Option<u64>,
    #[serde(default)]
    pub selected_chapter_ids: Vec<u64>,
    #[serde(default)]
    pub scope: ImportScope,
    pub preferred_language: Option<String>,
    pub overwrite_existing: bool,
    pub download_concurrency: usize,
}

impl ImportOptions {
    pub fn new(destination_directory: impl Into<PathBuf>) -> Self {
        Self {
            destination_directory: destination_directory.into(),
            volume_id: None,
            chapter_id: None,
            selected_chapter_ids: Vec::new(),
            scope: ImportScope::Volume,
            preferred_language: Some("en".to_owned()),
            overwrite_existing: false,
            download_concurrency: 6,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportScope {
    #[default]
    Volume,
    Chapter,
    Series,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePage {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteChapter {
    pub id: Option<String>,
    pub number: f64,
    pub title: Option<String>,
    pub volume: Option<f64>,
    pub pages: Vec<RemotePage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteNavigationItem {
    pub id: u64,
    pub number: f64,
    pub title: Option<String>,
    pub language: String,
}

/// A connector-neutral publication resolved from a web source. Koma owns all
/// network and filesystem side effects; connectors only describe these pages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePublication {
    pub provider: String,
    pub source_url: String,
    pub eligibility_url: String,
    pub eligibility_status: u16,
    pub title: String,
    pub language: Option<String>,
    pub scope: ImportScope,
    pub volume_id: Option<u64>,
    pub chapter_id: Option<u64>,
    pub selected_chapter_ids: Vec<u64>,
    pub chapters: Vec<RemoteChapter>,
    #[serde(default)]
    pub chapter_catalog: Vec<RemoteNavigationItem>,
    #[serde(default)]
    pub volume_catalog: Vec<RemoteNavigationItem>,
    pub allowed_page_hosts: Vec<String>,
    #[serde(default)]
    pub allow_local_network: bool,
}

pub async fn fetch_remote_page(
    publication: &RemotePublication,
    page_index: usize,
) -> Result<PageData> {
    let page = publication
        .chapters
        .iter()
        .flat_map(|chapter| chapter.pages.iter())
        .nth(page_index)
        .ok_or(KomaError::PageOutOfRange { index: page_index })?;
    let url = Url::parse(&page.url)?;
    if (url.scheme() != "https" && !(publication.allow_local_network && url.scheme() == "http"))
        || url.username() != ""
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(KomaError::ImportDenied(
            "online page URL is not permitted by this connector".to_owned(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| KomaError::ImportDenied("online page URL has no host".to_owned()))?
        .to_ascii_lowercase();
    if !publication
        .allowed_page_hosts
        .iter()
        .any(|allowed| host_matches(&host, allowed))
    {
        return Err(KomaError::ImportDenied(format!(
            "online page host {host} is not allowed by this connector"
        )));
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let addresses = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|error| KomaError::ProviderUnavailable(error.to_string()))?
        .collect::<Vec<_>>();
    if addresses.is_empty()
        || (!publication.allow_local_network
            && addresses
                .iter()
                .any(|address| is_non_public_ip(address.ip())))
    {
        return Err(KomaError::ImportDenied(
            "online page host did not resolve to a public address".to_owned(),
        ));
    }
    let mut builder = client_builder();
    for address in addresses {
        builder = builder.resolve(&host, address);
    }
    let client = builder.build()?;
    let mut response = None;
    for attempt in 0..4 {
        match client
            .get(url.clone())
            .header(ACCEPT, "image/avif,image/webp,image/*")
            .header(REFERER, &publication.eligibility_url)
            .send()
            .await
        {
            Ok(next)
                if attempt < 3
                    && (next.status() == StatusCode::TOO_MANY_REQUESTS
                        || next.status().is_server_error()) =>
            {
                let delay = provider_retry_delay(&next, attempt);
                drop(next);
                tokio::time::sleep(delay).await;
            }
            Ok(next) => {
                response = Some(next);
                break;
            }
            Err(error) if attempt < 3 && (error.is_connect() || error.is_timeout()) => {
                tokio::time::sleep(std::time::Duration::from_millis(
                    300 * 2_u64.pow(attempt as u32),
                ))
                .await;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let response = response.ok_or_else(|| {
        KomaError::ProviderUnavailable("online page request exhausted its retry budget".to_owned())
    })?;
    require_success(response.status(), "online page request")?;
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let bytes = read_bounded(response, MAX_IMPORT_PAGE_BYTES, "online page").await?;
    let extension = page_extension(&url, content_type.as_deref());
    validate_page_bytes(&format!("page.{extension}"), &bytes)?;
    Ok(PageData {
        index: page_index,
        mime_type: content_type
            .filter(|value| value.starts_with("image/"))
            .unwrap_or_else(|| format!("image/{extension}")),
        bytes,
    })
}

fn host_matches(host: &str, allowed: &str) -> bool {
    let allowed = allowed.trim().to_ascii_lowercase();
    if let Some(suffix) = allowed.strip_prefix("*.") {
        host != suffix && host.ends_with(&format!(".{suffix}"))
    } else {
        host == allowed
    }
}

impl RemotePublication {
    pub fn page_count(&self) -> usize {
        self.chapters
            .iter()
            .map(|chapter| chapter.pages.len())
            .sum()
    }

    pub fn chapter_ranges(&self) -> Vec<ChapterRange> {
        let mut page = 0;
        self.chapters
            .iter()
            .filter_map(|chapter| {
                if chapter.pages.is_empty() {
                    return None;
                }
                let start_page_index = page;
                page += chapter.pages.len();
                Some(ChapterRange {
                    id: chapter.id.clone(),
                    number: chapter.number,
                    title: chapter.title.clone(),
                    start_page_index,
                    end_page_index: page - 1,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportVolume {
    pub id: u64,
    pub number: f64,
    pub name: Option<String>,
    pub language: String,
    pub chapter_count: Option<usize>,
    pub page_count: Option<usize>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportChapter {
    pub id: u64,
    pub number: f64,
    pub name: Option<String>,
    pub language: String,
    pub page_count: Option<usize>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub provider: String,
    pub title: String,
    pub source_url: String,
    pub eligibility_url: String,
    pub eligibility_status: u16,
    pub eligible: bool,
    pub warning: String,
    pub volumes: Vec<ImportVolume>,
    pub chapters: Vec<ImportChapter>,
    pub selected_volume_id: Option<u64>,
    pub selected_chapter_id: Option<u64>,
    pub estimated_page_count: Option<usize>,
    pub series_chapter_count: Option<usize>,
    pub series_page_count: Option<usize>,
    pub available_scopes: Vec<ImportScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImportEvent {
    Checking {
        url: String,
    },
    Eligible {
        status: u16,
    },
    Discovered {
        title: String,
        volume: String,
        page_count: usize,
    },
    Downloading {
        completed: usize,
        total: usize,
    },
    Recovering {
        failed_pages: usize,
        strategy: String,
    },
    Packaging {
        output_path: PathBuf,
    },
    Completed {
        receipt: ImportReceipt,
    },
}

#[async_trait]
pub trait LinkImporter: Send + Sync {
    fn provider(&self) -> &str;
    fn recognizes(&self, source: &str) -> bool;
    async fn preview(&self, source: &str) -> Result<ImportPreview>;
    async fn resolve_online(
        &self,
        source: &str,
        options: &ImportOptions,
    ) -> Result<RemotePublication>;
    async fn navigate_online(
        &self,
        publication: &RemotePublication,
        target_scope: ImportScope,
        target_id: u64,
    ) -> Result<RemotePublication> {
        let mut options = ImportOptions::new(PathBuf::new());
        options.scope = target_scope;
        options.preferred_language = publication.language.clone();
        match target_scope {
            ImportScope::Chapter => options.chapter_id = Some(target_id),
            ImportScope::Volume => options.volume_id = Some(target_id),
            ImportScope::Series => {
                return Err(KomaError::Other(
                    "choose a chapter or volume to open".to_owned(),
                ));
            }
        }
        self.resolve_online(&publication.source_url, &options).await
    }
    async fn import(
        &self,
        source: &str,
        options: &ImportOptions,
        events: Option<&UnboundedSender<ImportEvent>>,
    ) -> Result<ImportReceipt>;
}

pub struct MangaFireImporter {
    client: Client,
    origin: Url,
    pinned_clients: Mutex<HashMap<String, Client>>,
}

impl MangaFireImporter {
    pub fn new() -> Result<Self> {
        Self::with_origin(Url::parse(MANGAFIRE_ORIGIN)?)
    }

    fn with_origin(origin: Url) -> Result<Self> {
        let client = client_builder().build()?;
        Ok(Self {
            client,
            origin,
            pinned_clients: Mutex::new(HashMap::new()),
        })
    }

    async fn client_for(&self, url: &Url) -> Result<Client> {
        // Only the test fixture can construct a non-HTTPS origin. Production
        // provider and page URLs are always HTTPS and pinned below.
        if url.scheme() != "https" {
            return Ok(self.client.clone());
        }
        let host = url
            .host_str()
            .ok_or_else(|| KomaError::ImportDenied("network URL has no host".to_owned()))?
            .to_ascii_lowercase();
        if let Some(client) = self.pinned_clients()?.get(&host).cloned() {
            return Ok(client);
        }

        let port = url.port_or_known_default().ok_or_else(|| {
            KomaError::ImportDenied("network URL has no recognized port".to_owned())
        })?;
        let addresses = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|error| {
                KomaError::ProviderUnavailable(format!(
                    "could not resolve the MangaFire host {host}: {error}"
                ))
            })?
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            return Err(KomaError::ProviderUnavailable(format!(
                "the MangaFire host {host} resolved to no addresses"
            )));
        }
        if addresses
            .iter()
            .any(|address| is_non_public_ip(address.ip()))
        {
            return Err(KomaError::ImportDenied(format!(
                "the MangaFire host {host} resolved to a non-public address"
            )));
        }

        let client = client_builder()
            .resolve_to_addrs(&host, &addresses)
            .build()?;
        let mut pinned_clients = self.pinned_clients()?;
        Ok(pinned_clients
            .entry(host)
            .or_insert_with(|| client.clone())
            .clone())
    }

    fn pinned_clients(&self) -> Result<MutexGuard<'_, HashMap<String, Client>>> {
        self.pinned_clients
            .lock()
            .map_err(|_| KomaError::Other("MangaFire client cache was poisoned".to_owned()))
    }

    #[cfg(test)]
    fn with_test_origin(origin: Url) -> Result<Self> {
        Self::with_origin(origin)
    }

    async fn resolve(
        &self,
        source: &str,
        requested_volume_id: Option<u64>,
        preferred_language: Option<&str>,
        events: Option<&UnboundedSender<ImportEvent>>,
    ) -> Result<ResolvedImport> {
        let target = self.parse_target(source)?;
        let eligibility_url = target.source_url.clone();
        emit(
            events,
            ImportEvent::Checking {
                url: eligibility_url.to_string(),
            },
        );
        let proof = self.check_eligibility(&eligibility_url).await?;

        let title_url = self.api_url(&format!("titles/{}", target.hid))?;
        let title_response: ApiData<MangaFireTitle> = self.request_json(&title_url).await?;
        if title_response.data.hid != target.hid {
            return Err(KomaError::ProviderChanged(
                "the title response did not match the pasted link".to_owned(),
            ));
        }

        let selected_id = if let Some(volume_id) = target.volume_id {
            volume_id
        } else {
            let volumes_url = self.api_url(&format!("titles/{}/volumes", target.hid))?;
            let response: VolumeListResponse = self.request_json(&volumes_url).await?;
            if response.items.is_empty() {
                return Err(KomaError::ProviderChanged(
                    "this title does not expose any downloadable volumes".to_owned(),
                ));
            }
            let selected = select_volume(
                &response.items,
                requested_volume_id,
                preferred_language.unwrap_or("en"),
            )?;
            selected
        };

        let volume_url = self.api_url(&format!("volumes/{selected_id}"))?;
        let volume_response: ApiData<MangaFireVolume> = self.request_json(&volume_url).await?;
        let volume = volume_response.data;
        if volume.id != selected_id || volume.title.hid != target.hid {
            return Err(KomaError::ProviderChanged(
                "the selected volume did not belong to the pasted title".to_owned(),
            ));
        }
        if volume.pages.is_empty() {
            return Err(KomaError::ProviderChanged(
                "the selected volume contains no pages".to_owned(),
            ));
        }
        if volume.pages.len() > MAX_PAGES {
            return Err(KomaError::ProviderChanged(format!(
                "the selected volume contains more than {MAX_PAGES} pages"
            )));
        }
        for page in &volume.pages {
            let page_url = Url::parse(&page.url)?;
            validate_page_url(&page_url, self.origin.host_str())?;
        }

        emit(
            events,
            ImportEvent::Eligible {
                status: proof.status,
            },
        );
        emit(
            events,
            ImportEvent::Discovered {
                title: title_response.data.title.clone(),
                volume: volume_number_label(volume.number),
                page_count: volume.pages.len(),
            },
        );
        Ok(ResolvedImport {
            target,
            proof,
            title: title_response.data,
            volume,
        })
    }

    async fn catalog(
        &self,
        source: &str,
        requested_volume_id: Option<u64>,
        preferred_language: Option<&str>,
    ) -> Result<MangaFireCatalog> {
        let target = self.parse_target(source)?;
        let proof = self.check_eligibility(&target.source_url).await?;
        let title_url = self.api_url(&format!("titles/{}", target.hid))?;
        let title_response: ApiData<MangaFireTitle> = self.request_json(&title_url).await?;
        if title_response.data.hid != target.hid {
            return Err(KomaError::ProviderChanged(
                "the title response did not match the pasted link".to_owned(),
            ));
        }

        let volumes_url = self.api_url(&format!("titles/{}/volumes", target.hid))?;
        let volume_response: VolumeListResponse = self.request_json(&volumes_url).await?;
        let mut volumes = volume_response.items;
        volumes.sort_by(|left, right| {
            left.number
                .partial_cmp(&right.number)
                .unwrap_or(CmpOrdering::Equal)
                .then_with(|| left.id.cmp(&right.id))
        });
        let requested_volume_id = target.volume_id.or(requested_volume_id);
        let requested_volume =
            requested_volume_id.and_then(|id| volumes.iter().find(|volume| volume.id == id));
        if requested_volume_id.is_some() && requested_volume.is_none() && !volumes.is_empty() {
            return Err(KomaError::ImportDenied(
                "the requested volume is not exposed by this title".to_owned(),
            ));
        }
        let preferred_language = preferred_language.unwrap_or("en");
        let language = requested_volume
            .or_else(|| {
                volumes
                    .iter()
                    .find(|volume| volume.language.eq_ignore_ascii_case(preferred_language))
            })
            .or_else(|| volumes.first())
            .map(|volume| volume.language.clone())
            .unwrap_or_else(|| preferred_language.to_owned());
        let chapters = self
            .chapter_summaries(&target.hid, &language, false)
            .await?;
        if volumes.is_empty() && chapters.is_empty() {
            return Err(KomaError::ProviderChanged(
                "this title exposes no readable chapters or volumes".to_owned(),
            ));
        }
        Ok(MangaFireCatalog {
            target,
            proof,
            title: title_response.data,
            language,
            volumes,
            chapters,
        })
    }

    async fn load_volume(&self, title_hid: &str, volume_id: u64) -> Result<MangaFireVolume> {
        let volume_url = self.api_url(&format!("volumes/{volume_id}"))?;
        let response: ApiData<MangaFireVolume> = self.request_json(&volume_url).await?;
        let volume = response.data;
        if volume.id != volume_id || volume.title.hid != title_hid {
            return Err(KomaError::ProviderChanged(
                "the selected volume did not belong to the pasted title".to_owned(),
            ));
        }
        if volume.pages.is_empty() {
            return Err(KomaError::ProviderChanged(
                "the selected volume contains no pages".to_owned(),
            ));
        }
        if volume.pages.len() > MAX_PAGES {
            return Err(KomaError::ProviderChanged(format!(
                "the selected volume contains more than {MAX_PAGES} pages"
            )));
        }
        for page in &volume.pages {
            validate_page_url(&Url::parse(&page.url)?, self.origin.host_str())?;
        }
        Ok(volume)
    }

    fn resolved_from_catalog(catalog: MangaFireCatalog) -> ResolvedImport {
        let selected = catalog.volumes.iter().find(|volume| {
            Some(volume.id) == catalog.target.volume_id
                || volume.language.eq_ignore_ascii_case(&catalog.language)
        });
        let volume = MangaFireVolume {
            id: selected.map(|volume| volume.id).unwrap_or(0),
            number: selected.map(|volume| volume.number).unwrap_or(0.0),
            name: selected
                .map(|volume| volume.name.clone())
                .unwrap_or_default(),
            language: catalog.language,
            pages: Vec::new(),
            title: MangaFireVolumeTitle {
                hid: catalog.target.hid.clone(),
            },
        };
        ResolvedImport {
            target: catalog.target,
            proof: catalog.proof,
            title: catalog.title,
            volume,
        }
    }

    fn parse_target(&self, source: &str) -> Result<MangaFireTarget> {
        let mut url = Url::parse(source.trim())?;
        if url.username() != "" || url.password().is_some() {
            return Err(KomaError::ImportDenied(
                "links containing credentials are not accepted".to_owned(),
            ));
        }
        if url.scheme() != self.origin.scheme() || url.host_str() != self.origin.host_str() {
            return Err(KomaError::UnsupportedFormat(
                "Koma currently accepts MangaFire links from mangafire.to only".to_owned(),
            ));
        }
        if url.port() != self.origin.port() {
            return Err(KomaError::ImportDenied(
                "links using a custom network port are not accepted".to_owned(),
            ));
        }
        url.set_fragment(None);
        url.set_query(None);
        let segments = url
            .path_segments()
            .ok_or_else(|| KomaError::UnsupportedFormat("invalid MangaFire link".to_owned()))?
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.len() != 2 && segments.len() != 4 {
            return Err(KomaError::UnsupportedFormat(
                "paste a MangaFire title or volume link".to_owned(),
            ));
        }
        if segments[0] != "title" {
            return Err(KomaError::UnsupportedFormat(
                "paste a MangaFire title or volume link".to_owned(),
            ));
        }
        let hid = segments[1]
            .split('-')
            .next()
            .filter(|value| {
                !value.is_empty()
                    && value
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric())
            })
            .ok_or_else(|| KomaError::UnsupportedFormat("invalid MangaFire title key".to_owned()))?
            .to_owned();
        let volume_id = if segments.len() == 4 {
            if segments[2] != "volume" {
                return Err(KomaError::UnsupportedFormat(
                    "chapter links are not volume imports".to_owned(),
                ));
            }
            Some(
                segments[3]
                    .parse::<u64>()
                    .map_err(|_| KomaError::UnsupportedFormat("invalid volume id".to_owned()))?,
            )
        } else {
            None
        };
        Ok(MangaFireTarget {
            hid,
            volume_id,
            source_url: url,
        })
    }

    fn api_url(&self, path: &str) -> Result<Url> {
        Ok(self.origin.join(&format!("api/{path}"))?)
    }

    async fn check_eligibility(&self, url: &Url) -> Result<EligibilityProof> {
        let response = self
            .client_for(url)
            .await?
            .get(url.clone())
            .header(ACCEPT, "text/html,application/xhtml+xml")
            .send()
            .await?;
        let status = response.status();
        require_success(status, "eligibility check")?;
        Ok(EligibilityProof {
            url: url.clone(),
            status: status.as_u16(),
            checked_at: Utc::now(),
        })
    }

    async fn request_json<T: DeserializeOwned>(&self, url: &Url) -> Result<T> {
        for attempt in 0..7 {
            let response = self
                .client_for(url)
                .await?
                .get(url.clone())
                .header(ACCEPT, "application/json")
                .header("X-Requested-With", "XMLHttpRequest")
                .send()
                .await?;
            let status = response.status();
            if (status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()) && attempt < 6
            {
                let delay = provider_retry_delay(&response, attempt);
                drop(response);
                tokio::time::sleep(delay).await;
                continue;
            }
            require_success(status, "MangaFire data request")?;
            let bytes = match read_bounded(response, MAX_JSON_BYTES, "MangaFire response").await {
                Ok(bytes) => bytes,
                Err(KomaError::Network(_)) if attempt < 6 => {
                    self.evict_pinned_client(url);
                    tokio::time::sleep(std::time::Duration::from_millis(
                        400 * 2_u64.pow(attempt as u32),
                    ))
                    .await;
                    continue;
                }
                Err(error) => return Err(error),
            };
            return serde_json::from_slice(&bytes).map_err(|error| {
                KomaError::ProviderChanged(format!("invalid JSON response: {error}"))
            });
        }
        Err(KomaError::ProviderUnavailable(
            "MangaFire data request exhausted its retry budget".to_owned(),
        ))
    }

    async fn download_page(
        &self,
        page: MangaFirePage,
        index: usize,
        name_stem: &str,
        staging_directory: &Path,
        referer: &Url,
        downloaded_bytes: &AtomicU64,
    ) -> Result<(String, PathBuf)> {
        let page_url = Url::parse(&page.url)?;
        validate_page_url(&page_url, self.origin.host_str())?;
        if let Some(existing) = existing_staged_page(staging_directory, name_stem)? {
            let bytes = std::fs::read(&existing.1)?;
            if validate_page_bytes(&existing.0, &bytes).is_ok() {
                downloaded_bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                return Ok(existing);
            }
            std::fs::remove_file(existing.1)?;
        }
        let response = self.send_page_request(&page_url, referer, index).await?;
        require_success(response.status(), &format!("page {} download", index + 1))?;
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = read_bounded(response, MAX_IMPORT_PAGE_BYTES, "page image").await?;
        let byte_size = bytes.len() as u64;
        downloaded_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(byte_size)
                    .filter(|updated| *updated <= MAX_IMPORT_BYTES)
            })
            .map_err(|_| {
                KomaError::ProviderChanged(format!(
                    "the volume exceeded Koma's {} GiB import safety limit",
                    MAX_IMPORT_BYTES / 1024 / 1024 / 1024
                ))
            })?;
        let extension = page_extension(&page_url, content_type.as_deref());
        let name = format!("{name_stem}.{extension}");
        let (decoded_width, decoded_height) = validate_page_bytes(&name, &bytes)?;
        if let (Some(expected_width), Some(expected_height), Some(width), Some(height)) = (
            positive(page.width),
            positive(page.height),
            decoded_width,
            decoded_height,
        ) && (expected_width != width || expected_height != height)
        {
            return Err(KomaError::InvalidImage(format!(
                "page {} dimensions did not match the provider manifest",
                index + 1
            )));
        }
        let path = staging_directory.join(&name);
        tokio::fs::write(&path, bytes).await?;
        Ok((name, path))
    }

    async fn send_page_request(
        &self,
        page_url: &Url,
        referer: &Url,
        index: usize,
    ) -> Result<Response> {
        for attempt in 0..5 {
            let result = self
                .client_for(page_url)
                .await?
                .get(page_url.clone())
                .header(ACCEPT, "image/avif,image/webp,image/*")
                .header(REFERER, referer.as_str())
                .send()
                .await;
            match result {
                Ok(response)
                    if (response.status() == StatusCode::TOO_MANY_REQUESTS
                        || response.status().is_server_error())
                        && attempt < 4 =>
                {
                    let delay = provider_retry_delay(&response, attempt);
                    drop(response);
                    tokio::time::sleep(delay).await;
                }
                Ok(response) => return Ok(response),
                Err(error) if attempt < 4 && (error.is_connect() || error.is_timeout()) => {
                    self.evict_pinned_client(page_url);
                    tokio::time::sleep(std::time::Duration::from_millis(
                        300 * 2_u64.pow(attempt as u32),
                    ))
                    .await;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(KomaError::ProviderUnavailable(format!(
            "page {} exhausted its retry budget",
            index + 1
        )))
    }

    fn evict_pinned_client(&self, url: &Url) {
        if let Some(host) = url.host_str()
            && let Ok(mut clients) = self.pinned_clients.lock()
        {
            clients.remove(&host.to_ascii_lowercase());
        }
    }

    async fn official_chapter_summaries(
        &self,
        title_hid: &str,
        language: &str,
    ) -> Result<Vec<MangaFireChapterSummary>> {
        self.chapter_summaries(title_hid, language, true).await
    }

    async fn chapter_summaries(
        &self,
        title_hid: &str,
        language: &str,
        require_any: bool,
    ) -> Result<Vec<MangaFireChapterSummary>> {
        let mut summaries = Vec::new();
        let mut page_number = 1;
        loop {
            if page_number > MAX_CHAPTER_LIST_PAGES {
                return Err(KomaError::ProviderChanged(
                    "the chapter list exceeded its pagination safety limit".to_owned(),
                ));
            }
            let mut chapters_url = self.api_url(&format!("titles/{title_hid}/chapters"))?;
            chapters_url.query_pairs_mut().extend_pairs([
                ("language", language),
                ("type", "all"),
                ("sort", "number"),
                ("order", "asc"),
                ("limit", "100"),
                ("page", &page_number.to_string()),
            ]);
            let response: ChapterListResponse = self.request_json(&chapters_url).await?;
            summaries.extend(response.items.into_iter().filter(|chapter| {
                chapter.release_type.eq_ignore_ascii_case("official")
                    && chapter.language.eq_ignore_ascii_case(language)
            }));
            if summaries.len() > MAX_CHAPTER_FALLBACKS {
                return Err(KomaError::ProviderChanged(
                    "the series exceeds Koma's chapter safety limit".to_owned(),
                ));
            }
            if page_number >= response.meta.last_page {
                break;
            }
            page_number += 1;
        }
        summaries.sort_by(|left, right| {
            left.number
                .partial_cmp(&right.number)
                .unwrap_or(CmpOrdering::Equal)
                .then_with(|| left.id.cmp(&right.id))
        });
        if require_any && summaries.is_empty() {
            return Err(KomaError::ProviderChanged(
                "this language exposes no official chapters".to_owned(),
            ));
        }
        Ok(summaries)
    }

    async fn load_official_chapters(
        &self,
        title_hid: &str,
        language: &str,
        summaries: Vec<MangaFireChapterSummary>,
    ) -> Result<Vec<ResolvedChapter>> {
        let mut chapters = Vec::with_capacity(summaries.len());
        for (index, summary) in summaries.into_iter().enumerate() {
            if index > 0 && self.origin.scheme() == "https" {
                tokio::time::sleep(std::time::Duration::from_millis(180)).await;
            }
            chapters.push(
                self.load_official_chapter(title_hid, language, summary)
                    .await?,
            );
        }
        Ok(chapters)
    }

    async fn load_official_chapter(
        &self,
        title_hid: &str,
        language: &str,
        summary: MangaFireChapterSummary,
    ) -> Result<ResolvedChapter> {
        let chapter_url = self.api_url(&format!("chapters/{}", summary.id))?;
        let response: ApiData<MangaFireChapter> = self.request_json(&chapter_url).await?;
        let chapter = response.data;
        if chapter.id != summary.id
            || chapter.title.hid != title_hid
            || !chapter.language.eq_ignore_ascii_case(language)
        {
            return Err(KomaError::ProviderChanged(
                "an official chapter response did not match the selected series".to_owned(),
            ));
        }
        if chapter.pages.is_empty() {
            return Err(KomaError::ProviderChanged(format!(
                "chapter {} contains no pages",
                volume_number_label(summary.number)
            )));
        }
        for page in &chapter.pages {
            validate_page_url(&Url::parse(&page.url)?, self.origin.host_str())?;
        }
        Ok(ResolvedChapter {
            number: summary.number,
            name: summary.name,
            chapter,
        })
    }

    async fn import_chapter(
        &self,
        resolved: ResolvedImport,
        options: &ImportOptions,
        events: Option<&UnboundedSender<ImportEvent>>,
    ) -> Result<ImportReceipt> {
        let summaries = self
            .official_chapter_summaries(&resolved.target.hid, &resolved.volume.language)
            .await?;
        let selected_id = options
            .chapter_id
            .or_else(|| summaries.last().map(|chapter| chapter.id))
            .ok_or_else(|| {
                KomaError::ProviderChanged("this language exposes no official chapters".to_owned())
            })?;
        let summary = summaries
            .into_iter()
            .find(|chapter| chapter.id == selected_id)
            .ok_or_else(|| {
                KomaError::ImportDenied(
                    "the selected chapter is not exposed for this language".to_owned(),
                )
            })?;
        let chapter_name = clean_string(&summary.name);
        let chapter = self
            .load_official_chapter(
                &resolved.target.hid,
                &resolved.volume.language,
                summary.clone(),
            )
            .await?;
        let total = chapter.chapter.pages.len();
        if total == 0 || total > MAX_PAGES {
            return Err(KomaError::ProviderChanged(format!(
                "the chapter must contain between 1 and {MAX_PAGES} pages"
            )));
        }
        emit(
            events,
            ImportEvent::Discovered {
                title: resolved.title.title.clone(),
                volume: format!("Chapter {}", volume_number_label(chapter.number)),
                page_count: total,
            },
        );

        std::fs::create_dir_all(&options.destination_directory)?;
        let staging_pages = persistent_staging_pages(
            &options.destination_directory,
            &format!("chapter-{}-{selected_id}", resolved.target.hid),
        )?;
        let name_width = total.to_string().len().max(3);
        let concurrency = options.download_concurrency.clamp(1, 8);
        let referer = resolved.proof.url.clone();
        let event_sender = events.cloned();
        let completed_pages = Arc::new(AtomicUsize::new(0));
        let downloaded_bytes = Arc::new(AtomicU64::new(0));
        let results = stream::iter(chapter.chapter.pages.clone().into_iter().enumerate())
            .map(|(index, page)| {
                let staging_pages = staging_pages.clone();
                let referer = referer.clone();
                let event_sender = event_sender.clone();
                let completed_pages = Arc::clone(&completed_pages);
                let downloaded_bytes = Arc::clone(&downloaded_bytes);
                async move {
                    let stem = format!("{:0name_width$}", index + 1);
                    let result = self
                        .download_page(
                            page,
                            index,
                            &stem,
                            &staging_pages,
                            &referer,
                            &downloaded_bytes,
                        )
                        .await;
                    if result.is_ok() {
                        let completed = completed_pages.fetch_add(1, Ordering::Relaxed) + 1;
                        emit(
                            event_sender.as_ref(),
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
                        "chapter page {} was not downloaded",
                        index + 1
                    ))
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let output_path = choose_output_path(
            &options.destination_directory.join(chapter_output_file_name(
                &resolved.title.title,
                chapter.number,
                &resolved.volume.language,
            )),
            options.overwrite_existing,
        );
        emit(
            events,
            ImportEvent::Packaging {
                output_path: output_path.clone(),
            },
        );
        let comic_info = chapter_comic_info(&resolved, chapter.number, chapter_name, total);
        let koma_metadata = KomaArchiveMetadata::new(vec![ChapterRange {
            id: Some(selected_id.to_string()),
            number: chapter.number,
            title: clean_string(&summary.name),
            start_page_index: 0,
            end_page_index: total - 1,
        }]);
        let package_path = output_path.clone();
        tokio::task::spawn_blocking(move || {
            ZipPublication::write_cbz_from_files_with_metadata(
                &package_path,
                downloaded,
                &comic_info,
                Some(&koma_metadata),
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
            source_url: resolved.target.source_url.to_string(),
            eligibility_url: resolved.proof.url.to_string(),
            eligibility_status: resolved.proof.status,
            checked_at: resolved.proof.checked_at,
            page_count: total,
            output_path,
            output_hash,
            adapter_version: ADAPTER_VERSION.to_owned(),
        };
        emit(
            events,
            ImportEvent::Completed {
                receipt: receipt.clone(),
            },
        );
        remove_completed_staging(&staging_pages);
        Ok(receipt)
    }

    async fn import_series(
        &self,
        resolved: ResolvedImport,
        options: &ImportOptions,
        events: Option<&UnboundedSender<ImportEvent>>,
    ) -> Result<ImportReceipt> {
        let mut summaries = self
            .official_chapter_summaries(&resolved.target.hid, &resolved.volume.language)
            .await?;
        if !options.selected_chapter_ids.is_empty() {
            let selected = options
                .selected_chapter_ids
                .iter()
                .copied()
                .collect::<HashSet<_>>();
            summaries.retain(|chapter| selected.contains(&chapter.id));
            if summaries.len() != selected.len() {
                return Err(KomaError::ImportDenied(
                    "one or more selected chapters do not belong to this series".to_owned(),
                ));
            }
        }
        if summaries.is_empty() {
            return Err(KomaError::ImportDenied(
                "select at least one chapter to import".to_owned(),
            ));
        }
        let chapters = self
            .load_official_chapters(&resolved.target.hid, &resolved.volume.language, summaries)
            .await?;
        self.package_series(resolved, options, events, chapters)
            .await
    }

    async fn import_volume_series(
        &self,
        catalog: MangaFireCatalog,
        options: &ImportOptions,
        events: Option<&UnboundedSender<ImportEvent>>,
    ) -> Result<ImportReceipt> {
        let summaries = catalog
            .volumes
            .iter()
            .filter(|volume| volume.language.eq_ignore_ascii_case(&catalog.language))
            .cloned()
            .collect::<Vec<_>>();
        if summaries.is_empty() {
            return Err(KomaError::ProviderChanged(
                "this language exposes no readable volumes".to_owned(),
            ));
        }
        let mut chapters = Vec::with_capacity(summaries.len());
        for (index, summary) in summaries.into_iter().enumerate() {
            if index > 0 && self.origin.scheme() == "https" {
                tokio::time::sleep(std::time::Duration::from_millis(180)).await;
            }
            let volume = self.load_volume(&catalog.target.hid, summary.id).await?;
            chapters.push(ResolvedChapter {
                number: volume.number,
                name: volume.name,
                chapter: MangaFireChapter {
                    id: volume.id,
                    language: volume.language,
                    pages: volume.pages,
                    title: volume.title,
                },
            });
        }
        let resolved = Self::resolved_from_catalog(catalog);
        self.package_series(resolved, options, events, chapters)
            .await
    }

    async fn package_series(
        &self,
        resolved: ResolvedImport,
        options: &ImportOptions,
        events: Option<&UnboundedSender<ImportEvent>>,
        chapters: Vec<ResolvedChapter>,
    ) -> Result<ImportReceipt> {
        let total = chapters.iter().try_fold(0_usize, |count, chapter| {
            count.checked_add(chapter.chapter.pages.len())
        });
        let total = total
            .filter(|count| *count > 0 && *count <= MAX_PAGES)
            .ok_or_else(|| {
                KomaError::ProviderChanged(format!(
                    "the series must contain between 1 and {MAX_PAGES} pages"
                ))
            })?;
        emit(
            events,
            ImportEvent::Discovered {
                title: resolved.title.title.clone(),
                volume: "Series".to_owned(),
                page_count: total,
            },
        );

        std::fs::create_dir_all(&options.destination_directory)?;
        let staging_pages = persistent_staging_pages(
            &options.destination_directory,
            &format!(
                "series-{}-{}",
                resolved.target.hid, resolved.volume.language
            ),
        )?;
        let global_width = total.to_string().len().max(4);
        let mut specifications = Vec::with_capacity(total);
        let mut chapter_ranges = Vec::with_capacity(chapters.len());
        for chapter in &chapters {
            let start_page_index = specifications.len();
            let page_width = chapter.chapter.pages.len().to_string().len().max(3);
            for (page_index, page) in chapter.chapter.pages.iter().cloned().enumerate() {
                let global_index = specifications.len();
                specifications.push((
                    page,
                    format!(
                        "{:0global_width$}-ch{}-{:0page_width$}",
                        global_index + 1,
                        volume_number_label(chapter.number),
                        page_index + 1,
                    ),
                ));
            }
            chapter_ranges.push(ChapterRange {
                id: Some(chapter.chapter.id.to_string()),
                number: chapter.number,
                title: clean_string(&chapter.name),
                start_page_index,
                end_page_index: specifications.len() - 1,
            });
        }

        let concurrency = options.download_concurrency.clamp(1, 8);
        let referer = resolved.proof.url.clone();
        let event_sender = events.cloned();
        let completed_pages = Arc::new(AtomicUsize::new(0));
        let downloaded_bytes = Arc::new(AtomicU64::new(0));
        let results = stream::iter(specifications.into_iter().enumerate())
            .map(|(index, (page, stem))| {
                let staging_pages = staging_pages.clone();
                let referer = referer.clone();
                let event_sender = event_sender.clone();
                let completed_pages = Arc::clone(&completed_pages);
                let downloaded_bytes = Arc::clone(&downloaded_bytes);
                async move {
                    let result = self
                        .download_page(
                            page,
                            index,
                            &stem,
                            &staging_pages,
                            &referer,
                            &downloaded_bytes,
                        )
                        .await;
                    if result.is_ok() {
                        let completed = completed_pages.fetch_add(1, Ordering::Relaxed) + 1;
                        emit(
                            event_sender.as_ref(),
                            ImportEvent::Downloading { completed, total },
                        );
                    }
                    (index, result)
                }
            })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;
        let mut downloaded = vec![None; total];
        for (index, result) in results {
            downloaded[index] = Some(result?);
        }
        let downloaded = downloaded
            .into_iter()
            .enumerate()
            .map(|(index, page)| {
                page.ok_or_else(|| {
                    KomaError::ProviderUnavailable(format!(
                        "series page {} was not downloaded",
                        index + 1
                    ))
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let file_name = series_output_file_name(&resolved.title.title, &resolved.volume.language);
        let output_path = choose_output_path(
            &options.destination_directory.join(file_name),
            options.overwrite_existing,
        );
        emit(
            events,
            ImportEvent::Packaging {
                output_path: output_path.clone(),
            },
        );
        let comic_info = series_comic_info(&resolved, total);
        let koma_metadata = KomaArchiveMetadata::new(chapter_ranges);
        let package_path = output_path.clone();
        tokio::task::spawn_blocking(move || {
            ZipPublication::write_cbz_from_files_with_metadata(
                &package_path,
                downloaded,
                &comic_info,
                Some(&koma_metadata),
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
            source_url: resolved.target.source_url.to_string(),
            eligibility_url: resolved.proof.url.to_string(),
            eligibility_status: resolved.proof.status,
            checked_at: resolved.proof.checked_at,
            page_count: total,
            output_path,
            output_hash,
            adapter_version: ADAPTER_VERSION.to_owned(),
        };
        emit(
            events,
            ImportEvent::Completed {
                receipt: receipt.clone(),
            },
        );
        remove_completed_staging(&staging_pages);
        Ok(receipt)
    }

    async fn chapter_fallback_pages(
        &self,
        resolved: &ResolvedImport,
    ) -> Result<Option<Vec<MangaFirePage>>> {
        let summaries = match self
            .official_chapter_summaries(&resolved.target.hid, &resolved.volume.language)
            .await
        {
            Ok(summaries) => summaries,
            Err(_) => return Ok(None),
        };
        let chapters = self
            .load_official_chapters(&resolved.target.hid, &resolved.volume.language, summaries)
            .await?
            .into_iter()
            .map(|resolved| resolved.chapter)
            .collect::<Vec<_>>();

        let target_page_count = resolved.volume.pages.len();
        let chapter_page_counts = chapters
            .iter()
            .map(|chapter| chapter.pages.len())
            .collect::<Vec<_>>();
        let Some((start, end)) =
            unique_contiguous_page_range(&chapter_page_counts, target_page_count)
        else {
            return Ok(None);
        };
        let fallback = chapters[start..=end]
            .iter()
            .flat_map(|chapter| chapter.pages.iter().cloned())
            .collect::<Vec<_>>();
        if fallback.len() != target_page_count
            || !fallback
                .iter()
                .zip(&resolved.volume.pages)
                .all(|(candidate, original)| compatible_page_geometry(candidate, original))
        {
            return Ok(None);
        }
        for page in &fallback {
            validate_page_url(&Url::parse(&page.url)?, self.origin.host_str())?;
        }
        Ok(Some(fallback))
    }

    async fn volume_chapter_ranges(&self, resolved: &ResolvedImport) -> Result<Vec<ChapterRange>> {
        let summaries = self
            .official_chapter_summaries(&resolved.target.hid, &resolved.volume.language)
            .await?;
        if summaries.len() > 40 {
            return Ok(Vec::new());
        }
        let chapters = self
            .load_official_chapters(&resolved.target.hid, &resolved.volume.language, summaries)
            .await?;
        let counts = chapters
            .iter()
            .map(|chapter| chapter.chapter.pages.len())
            .collect::<Vec<_>>();
        let Some((start, end)) = unique_contiguous_page_range(&counts, resolved.volume.pages.len())
        else {
            return Ok(Vec::new());
        };
        if !chapters[start..=end]
            .iter()
            .flat_map(|chapter| chapter.chapter.pages.iter())
            .zip(&resolved.volume.pages)
            .all(|(chapter, volume)| compatible_page_geometry(chapter, volume))
        {
            return Ok(Vec::new());
        }
        let mut page = 0_usize;
        Ok(chapters[start..=end]
            .iter()
            .map(|chapter| {
                let start_page_index = page;
                page += chapter.chapter.pages.len();
                ChapterRange {
                    id: Some(chapter.chapter.id.to_string()),
                    number: chapter.number,
                    title: clean_string(&chapter.name),
                    start_page_index,
                    end_page_index: page - 1,
                }
            })
            .collect())
    }
}

fn client_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .redirect(Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(45))
        .user_agent(concat!("Koma/", env!("CARGO_PKG_VERSION")))
}

fn persistent_staging_pages(destination: &Path, key: &str) -> Result<PathBuf> {
    let id = blake3::hash(key.as_bytes()).to_hex();
    let pages = destination
        .join(".koma-downloads")
        .join(&id.as_str()[..20])
        .join("pages");
    std::fs::create_dir_all(&pages)?;
    Ok(pages)
}

fn existing_staged_page(directory: &Path, stem: &str) -> Result<Option<(String, PathBuf)>> {
    let prefix = format!("{stem}.");
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_file() && name.starts_with(&prefix) {
            return Ok(Some((name, entry.path())));
        }
    }
    Ok(None)
}

fn remove_completed_staging(pages: &Path) {
    if let Some(job) = pages.parent() {
        let _ = std::fs::remove_dir_all(job);
        if let Some(root) = job.parent() {
            let _ = std::fs::remove_dir(root);
        }
    }
}

fn provider_retry_delay(response: &Response, attempt: usize) -> std::time::Duration {
    if let Some(seconds) = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
    {
        return std::time::Duration::from_secs(seconds.min(15));
    }
    std::time::Duration::from_millis((400 * 2_u64.pow(attempt as u32)).min(12_000))
}

#[async_trait]
impl LinkImporter for MangaFireImporter {
    fn provider(&self) -> &'static str {
        "MangaFire"
    }

    fn recognizes(&self, source: &str) -> bool {
        self.parse_target(source).is_ok()
    }

    async fn preview(&self, source: &str) -> Result<ImportPreview> {
        let catalog = self.catalog(source, None, Some("en")).await?;
        let selected_volume_id = catalog
            .target
            .volume_id
            .or_else(|| catalog.volumes.first().map(|volume| volume.id));
        let selected_chapter_id = catalog.chapters.last().map(|chapter| chapter.id);
        let chapters = catalog
            .chapters
            .iter()
            .map(|chapter| ImportChapter {
                id: chapter.id,
                number: chapter.number,
                name: clean_string(&chapter.name),
                language: chapter.language.clone(),
                page_count: None,
                selected: Some(chapter.id) == selected_chapter_id,
            })
            .collect::<Vec<_>>();
        let volumes = catalog
            .volumes
            .iter()
            .map(|volume| ImportVolume {
                id: volume.id,
                number: volume.number,
                name: clean_string(&volume.name),
                language: volume.language.clone(),
                chapter_count: Some(volume.chapter_count),
                page_count: None,
                selected: Some(volume.id) == selected_volume_id,
            })
            .collect::<Vec<_>>();
        let mut available_scopes = Vec::new();
        if !chapters.is_empty() {
            available_scopes.push(ImportScope::Chapter);
        }
        if !volumes.is_empty() {
            available_scopes.push(ImportScope::Volume);
        }
        available_scopes.push(ImportScope::Series);
        Ok(ImportPreview {
            provider: self.provider().to_owned(),
            title: catalog.title.title,
            source_url: catalog.target.source_url.to_string(),
            eligibility_url: catalog.proof.url.to_string(),
            eligibility_status: catalog.proof.status,
            eligible: true,
            warning: IMPORT_WARNING.to_owned(),
            volumes,
            chapters,
            selected_volume_id,
            selected_chapter_id,
            estimated_page_count: None,
            series_chapter_count: Some(catalog.chapters.len()),
            series_page_count: None,
            available_scopes,
        })
    }

    async fn resolve_online(
        &self,
        source: &str,
        options: &ImportOptions,
    ) -> Result<RemotePublication> {
        let catalog = self
            .catalog(
                source,
                options.volume_id,
                options.preferred_language.as_deref(),
            )
            .await?;
        let provider = self.provider().to_owned();
        let source_url = catalog.target.source_url.to_string();
        let eligibility_url = catalog.proof.url.to_string();
        let eligibility_status = catalog.proof.status;
        let title = catalog.title.title.clone();
        let language = Some(catalog.language.clone());
        let mut active_chapter_id = options.chapter_id;
        let mut active_volume_id = options.volume_id.or(catalog.target.volume_id);
        let selected_chapters = if options.selected_chapter_ids.is_empty() {
            catalog.chapters.clone()
        } else {
            let selected = options
                .selected_chapter_ids
                .iter()
                .copied()
                .collect::<HashSet<_>>();
            let filtered = catalog
                .chapters
                .iter()
                .filter(|chapter| selected.contains(&chapter.id))
                .cloned()
                .collect::<Vec<_>>();
            if filtered.len() != selected.len() {
                return Err(KomaError::ImportDenied(
                    "one or more selected chapters do not belong to this series".to_owned(),
                ));
            }
            filtered
        };

        // Online reading resolves a catalog plus one active section. Adjacent
        // sections are fetched only when the reader enters them.
        let chapters = match options.scope {
            ImportScope::Chapter => {
                let selected_id = options
                    .chapter_id
                    .or_else(|| catalog.chapters.last().map(|chapter| chapter.id))
                    .ok_or_else(|| {
                        KomaError::ProviderChanged(
                            "this language exposes no official chapters".to_owned(),
                        )
                    })?;
                active_chapter_id = Some(selected_id);
                let summary = catalog
                    .chapters
                    .iter()
                    .find(|chapter| chapter.id == selected_id)
                    .cloned()
                    .ok_or_else(|| {
                        KomaError::ImportDenied(
                            "the selected chapter is not exposed for this language".to_owned(),
                        )
                    })?;
                let chapter = self
                    .load_official_chapter(&catalog.target.hid, &catalog.language, summary)
                    .await?;
                vec![remote_mangafire_chapter(chapter, None)]
            }
            ImportScope::Series => {
                if let Some(summary) = selected_chapters.first().cloned() {
                    active_chapter_id = Some(summary.id);
                    let chapter = self
                        .load_official_chapter(&catalog.target.hid, &catalog.language, summary)
                        .await?;
                    vec![remote_mangafire_chapter(chapter, None)]
                } else {
                    let selected_id = active_volume_id
                        .or_else(|| catalog.volumes.first().map(|volume| volume.id))
                        .ok_or_else(|| {
                            KomaError::ProviderChanged(
                                "this title exposes no readable chapters or volumes".to_owned(),
                            )
                        })?;
                    active_volume_id = Some(selected_id);
                    let volume = self.load_volume(&catalog.target.hid, selected_id).await?;
                    vec![remote_mangafire_volume(volume)]
                }
            }
            ImportScope::Volume => {
                let selected_id = active_volume_id
                    .or_else(|| catalog.volumes.first().map(|volume| volume.id))
                    .ok_or_else(|| {
                        KomaError::ProviderChanged(
                            "this title does not expose any readable volumes".to_owned(),
                        )
                    })?;
                active_volume_id = Some(selected_id);
                vec![remote_mangafire_volume(
                    self.load_volume(&catalog.target.hid, selected_id).await?,
                )]
            }
        };
        let mut allowed_page_hosts = chapters
            .iter()
            .flat_map(|chapter| chapter.pages.iter())
            .filter_map(|page| Url::parse(&page.url).ok())
            .filter_map(|url| url.host_str().map(str::to_ascii_lowercase))
            .collect::<Vec<_>>();
        allowed_page_hosts.sort();
        allowed_page_hosts.dedup();
        Ok(RemotePublication {
            provider,
            source_url,
            eligibility_url,
            eligibility_status,
            title,
            language,
            scope: options.scope,
            volume_id: active_volume_id,
            chapter_id: active_chapter_id,
            selected_chapter_ids: options.selected_chapter_ids.clone(),
            chapters,
            chapter_catalog: selected_chapters
                .iter()
                .map(remote_chapter_navigation)
                .collect(),
            volume_catalog: catalog
                .volumes
                .iter()
                .filter(|volume| volume.language.eq_ignore_ascii_case(&catalog.language))
                .map(remote_volume_navigation)
                .collect(),
            allowed_page_hosts,
            allow_local_network: false,
        })
    }

    async fn navigate_online(
        &self,
        publication: &RemotePublication,
        target_scope: ImportScope,
        target_id: u64,
    ) -> Result<RemotePublication> {
        let target = self.parse_target(&publication.source_url)?;
        let mut next = publication.clone();
        next.chapters = match target_scope {
            ImportScope::Chapter => {
                let item = publication
                    .chapter_catalog
                    .iter()
                    .find(|item| item.id == target_id)
                    .ok_or_else(|| {
                        KomaError::ImportDenied(
                            "the selected chapter is not exposed by this title".to_owned(),
                        )
                    })?;
                let chapter = self
                    .load_official_chapter(
                        &target.hid,
                        &item.language,
                        MangaFireChapterSummary {
                            id: item.id,
                            number: item.number,
                            name: item.title.clone().unwrap_or_default(),
                            language: item.language.clone(),
                            release_type: "official".to_owned(),
                        },
                    )
                    .await?;
                next.chapter_id = Some(target_id);
                vec![remote_mangafire_chapter(chapter, None)]
            }
            ImportScope::Volume => {
                if !publication
                    .volume_catalog
                    .iter()
                    .any(|item| item.id == target_id)
                {
                    return Err(KomaError::ImportDenied(
                        "the selected volume is not exposed by this title".to_owned(),
                    ));
                }
                next.volume_id = Some(target_id);
                vec![remote_mangafire_volume(
                    self.load_volume(&target.hid, target_id).await?,
                )]
            }
            ImportScope::Series => {
                return Err(KomaError::Other(
                    "choose a chapter or volume to open".to_owned(),
                ));
            }
        };
        next.allowed_page_hosts = next
            .chapters
            .iter()
            .flat_map(|chapter| chapter.pages.iter())
            .filter_map(|page| Url::parse(&page.url).ok())
            .filter_map(|url| url.host_str().map(str::to_ascii_lowercase))
            .collect();
        next.allowed_page_hosts.sort();
        next.allowed_page_hosts.dedup();
        Ok(next)
    }

    async fn import(
        &self,
        source: &str,
        options: &ImportOptions,
        events: Option<&UnboundedSender<ImportEvent>>,
    ) -> Result<ImportReceipt> {
        if options.scope == ImportScope::Chapter {
            let catalog = self
                .catalog(
                    source,
                    options.volume_id,
                    options.preferred_language.as_deref(),
                )
                .await?;
            return self
                .import_chapter(Self::resolved_from_catalog(catalog), options, events)
                .await;
        }
        if options.scope == ImportScope::Series {
            let catalog = self
                .catalog(
                    source,
                    options.volume_id,
                    options.preferred_language.as_deref(),
                )
                .await?;
            if catalog.chapters.is_empty() {
                return self.import_volume_series(catalog, options, events).await;
            }
            return self
                .import_series(Self::resolved_from_catalog(catalog), options, events)
                .await;
        }
        let resolved = self
            .resolve(
                source,
                options.volume_id,
                options.preferred_language.as_deref(),
                events,
            )
            .await?;

        let chapter_ranges = self
            .volume_chapter_ranges(&resolved)
            .await
            .unwrap_or_default();

        // No destination directory or page file exists until every guarded
        // provider request above has succeeded.
        std::fs::create_dir_all(&options.destination_directory)?;
        let staging_pages = persistent_staging_pages(
            &options.destination_directory,
            &format!("volume-{}-{}", resolved.target.hid, resolved.volume.id),
        )?;

        let total = resolved.volume.pages.len();
        let concurrency = options.download_concurrency.clamp(1, 8);
        let referer = resolved.proof.url.clone();
        let event_sender = events.cloned();
        let completed_pages = Arc::new(AtomicUsize::new(0));
        let downloaded_bytes = Arc::new(AtomicU64::new(0));
        let name_width = total.to_string().len().max(3);
        let direct_results = stream::iter(resolved.volume.pages.clone().into_iter().enumerate())
            .map(|(index, page)| {
                let staging_pages = staging_pages.clone();
                let referer = referer.clone();
                let event_sender = event_sender.clone();
                let completed_pages = Arc::clone(&completed_pages);
                let downloaded_bytes = Arc::clone(&downloaded_bytes);
                async move {
                    let stem = format!("{:0name_width$}", index + 1);
                    let result = self
                        .download_page(
                            page,
                            index,
                            &stem,
                            &staging_pages,
                            &referer,
                            &downloaded_bytes,
                        )
                        .await;
                    if result.is_ok() {
                        let completed = completed_pages.fetch_add(1, Ordering::Relaxed) + 1;
                        emit(
                            event_sender.as_ref(),
                            ImportEvent::Downloading { completed, total },
                        );
                    }
                    (index, result)
                }
            })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;
        let mut downloaded = vec![None; total];
        let mut recoverable_failures = Vec::new();
        for (index, result) in direct_results {
            match result {
                Ok(page) => downloaded[index] = Some(page),
                Err(error)
                    if matches!(
                        error,
                        KomaError::ProviderUnavailable(_) | KomaError::Network(_)
                    ) =>
                {
                    recoverable_failures.push((index, error.to_string()));
                }
                Err(error) => return Err(error),
            }
        }

        if !recoverable_failures.is_empty() {
            let fallback_pages = self
                .chapter_fallback_pages(&resolved)
                .await?
                .ok_or_else(|| {
                    KomaError::ProviderUnavailable(format!(
                        "{} volume page(s) failed and no unambiguous official chapter fallback was available: {}",
                        recoverable_failures.len(),
                        recoverable_failures
                            .iter()
                            .map(|(index, error)| format!("page {} ({error})", index + 1))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                })?;
            emit(
                events,
                ImportEvent::Recovering {
                    failed_pages: recoverable_failures.len(),
                    strategy: "verified official chapter sequence".to_owned(),
                },
            );
            let fallback_results = stream::iter(recoverable_failures.into_iter())
                .map(|(index, _)| {
                    let staging_pages = staging_pages.clone();
                    let referer = referer.clone();
                    let event_sender = event_sender.clone();
                    let completed_pages = Arc::clone(&completed_pages);
                    let downloaded_bytes = Arc::clone(&downloaded_bytes);
                    let page = fallback_pages[index].clone();
                    async move {
                        let stem = format!("{:0name_width$}", index + 1);
                        let result = self
                            .download_page(
                                page,
                                index,
                                &stem,
                                &staging_pages,
                                &referer,
                                &downloaded_bytes,
                            )
                            .await;
                        if result.is_ok() {
                            let completed = completed_pages.fetch_add(1, Ordering::Relaxed) + 1;
                            emit(
                                event_sender.as_ref(),
                                ImportEvent::Downloading { completed, total },
                            );
                        }
                        (index, result)
                    }
                })
                .buffer_unordered(concurrency)
                .collect::<Vec<_>>()
                .await;
            for (index, result) in fallback_results {
                downloaded[index] = Some(result?);
            }
        }
        let downloaded = downloaded
            .into_iter()
            .enumerate()
            .map(|(index, page)| {
                page.ok_or_else(|| {
                    KomaError::ProviderUnavailable(format!("page {} was not downloaded", index + 1))
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let file_name = output_file_name(
            &resolved.title.title,
            resolved.volume.number,
            &resolved.volume.language,
        );
        let output_path = choose_output_path(
            &options.destination_directory.join(file_name),
            options.overwrite_existing,
        );
        emit(
            events,
            ImportEvent::Packaging {
                output_path: output_path.clone(),
            },
        );
        let comic_info = comic_info(&resolved);
        let koma_metadata =
            (!chapter_ranges.is_empty()).then(|| KomaArchiveMetadata::new(chapter_ranges));
        let package_path = output_path.clone();
        tokio::task::spawn_blocking(move || {
            ZipPublication::write_cbz_from_files_with_metadata(
                &package_path,
                downloaded,
                &comic_info,
                koma_metadata.as_ref(),
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
            source_url: resolved.target.source_url.to_string(),
            eligibility_url: resolved.proof.url.to_string(),
            eligibility_status: resolved.proof.status,
            checked_at: resolved.proof.checked_at,
            page_count: total,
            output_path,
            output_hash,
            adapter_version: ADAPTER_VERSION.to_owned(),
        };
        emit(
            events,
            ImportEvent::Completed {
                receipt: receipt.clone(),
            },
        );
        remove_completed_staging(&staging_pages);
        Ok(receipt)
    }
}

fn remote_mangafire_chapter(chapter: ResolvedChapter, volume: Option<f64>) -> RemoteChapter {
    RemoteChapter {
        id: Some(chapter.chapter.id.to_string()),
        number: chapter.number,
        title: clean_string(&chapter.name),
        volume,
        pages: chapter
            .chapter
            .pages
            .into_iter()
            .map(remote_mangafire_page)
            .collect(),
    }
}

fn remote_mangafire_volume(volume: MangaFireVolume) -> RemoteChapter {
    RemoteChapter {
        id: Some(format!("volume:{}", volume.id)),
        number: volume.number,
        title: clean_string(&volume.name),
        volume: Some(volume.number),
        pages: volume
            .pages
            .into_iter()
            .map(remote_mangafire_page)
            .collect(),
    }
}

fn remote_chapter_navigation(chapter: &MangaFireChapterSummary) -> RemoteNavigationItem {
    RemoteNavigationItem {
        id: chapter.id,
        number: chapter.number,
        title: clean_string(&chapter.name),
        language: chapter.language.clone(),
    }
}

fn remote_volume_navigation(volume: &MangaFireVolumeSummary) -> RemoteNavigationItem {
    RemoteNavigationItem {
        id: volume.id,
        number: volume.number,
        title: clean_string(&volume.name),
        language: volume.language.clone(),
    }
}

fn remote_mangafire_page(page: MangaFirePage) -> RemotePage {
    RemotePage {
        url: page.url,
        width: positive(page.width),
        height: positive(page.height),
    }
}

#[derive(Debug)]
struct MangaFireTarget {
    hid: String,
    volume_id: Option<u64>,
    source_url: Url,
}

#[derive(Debug)]
struct EligibilityProof {
    url: Url,
    status: u16,
    checked_at: DateTime<Utc>,
}

#[derive(Debug)]
struct ResolvedImport {
    target: MangaFireTarget,
    proof: EligibilityProof,
    title: MangaFireTitle,
    volume: MangaFireVolume,
}

#[derive(Debug)]
struct MangaFireCatalog {
    target: MangaFireTarget,
    proof: EligibilityProof,
    title: MangaFireTitle,
    language: String,
    volumes: Vec<MangaFireVolumeSummary>,
    chapters: Vec<MangaFireChapterSummary>,
}

#[derive(Debug)]
struct ResolvedChapter {
    number: f64,
    name: String,
    chapter: MangaFireChapter,
}

#[derive(Debug, Deserialize)]
struct ApiData<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct VolumeListResponse {
    items: Vec<MangaFireVolumeSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaFireVolumeSummary {
    id: u64,
    number: f64,
    #[serde(default)]
    name: String,
    language: String,
    chapter_count: usize,
}

#[derive(Debug, Deserialize)]
struct ChapterListResponse {
    items: Vec<MangaFireChapterSummary>,
    meta: PaginationMeta,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaFireChapterSummary {
    id: u64,
    number: f64,
    #[serde(default)]
    name: String,
    language: String,
    #[serde(rename = "type")]
    release_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaginationMeta {
    last_page: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaFireChapter {
    id: u64,
    language: String,
    pages: Vec<MangaFirePage>,
    title: MangaFireVolumeTitle,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaFireTitle {
    hid: String,
    title: String,
    #[serde(default)]
    synopsis_html: String,
    #[serde(default)]
    authors: Vec<NamedValue>,
    #[serde(default)]
    artists: Vec<NamedValue>,
    #[serde(default)]
    genres: Vec<NamedValue>,
    #[serde(default)]
    themes: Vec<NamedValue>,
    #[serde(default)]
    r#type: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MangaFireVolume {
    id: u64,
    number: f64,
    #[serde(default)]
    name: String,
    language: String,
    pages: Vec<MangaFirePage>,
    title: MangaFireVolumeTitle,
}

#[derive(Debug, Clone, Deserialize)]
struct MangaFirePage {
    url: String,
    width: i64,
    height: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct MangaFireVolumeTitle {
    hid: String,
}

#[derive(Debug, Deserialize)]
struct NamedValue {
    title: String,
}

fn select_volume(
    volumes: &[MangaFireVolumeSummary],
    requested_id: Option<u64>,
    preferred_language: &str,
) -> Result<u64> {
    if let Some(requested_id) = requested_id {
        return volumes
            .iter()
            .find(|volume| volume.id == requested_id)
            .map(|volume| volume.id)
            .ok_or_else(|| {
                KomaError::ImportDenied(
                    "the requested volume is not exposed by this title".to_owned(),
                )
            });
    }
    let preferred = volumes
        .iter()
        .filter(|volume| volume.language.eq_ignore_ascii_case(preferred_language))
        .collect::<Vec<_>>();
    preferred
        .first()
        .copied()
        .or_else(|| volumes.first())
        .map(|volume| volume.id)
        .ok_or_else(|| KomaError::ProviderChanged("the title exposes no volumes".to_owned()))
}

fn require_success(status: StatusCode, context: &str) -> Result<()> {
    if status.is_success() {
        return Ok(());
    }
    if status == StatusCode::FORBIDDEN {
        return Err(KomaError::ImportDenied(format!(
            "{context} returned 403; MangaFire denied the request"
        )));
    }
    if status == StatusCode::NOT_FOUND
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
    {
        return Err(KomaError::ProviderUnavailable(format!(
            "{context} returned HTTP {}",
            status.as_u16()
        )));
    }
    if status.is_client_error() {
        return Err(KomaError::ImportDenied(format!(
            "{context} was refused with HTTP {}",
            status.as_u16()
        )));
    }
    Err(KomaError::ProviderChanged(format!(
        "{context} returned unexpected HTTP {}",
        status.as_u16()
    )))
}

async fn read_bounded(response: Response, limit: u64, label: &str) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(KomaError::ProviderChanged(format!(
            "{label} exceeded the {} MiB limit",
            limit / 1024 / 1024
        )));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err(KomaError::ProviderChanged(format!(
                "{label} exceeded the {} MiB limit",
                limit / 1024 / 1024
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate_page_url(url: &Url, provider_host: Option<&str>) -> Result<()> {
    if url.scheme() != "https" || url.username() != "" || url.password().is_some() {
        return Err(KomaError::ImportDenied(
            "the provider returned an unsafe page URL".to_owned(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| KomaError::ImportDenied("page URL has no host".to_owned()))?
        .to_ascii_lowercase();
    let provider_host = provider_host.unwrap_or_default().to_ascii_lowercase();
    let allowed =
        host == provider_host || host.ends_with(".mangafire.to") || is_mangafire_cdn_host(&host);
    if !allowed {
        return Err(KomaError::ImportDenied(format!(
            "the provider returned an untrusted image host: {host}"
        )));
    }
    if host == "localhost" || host.ends_with(".local") || host.ends_with(".internal") {
        return Err(KomaError::ImportDenied(
            "local network page URLs are not accepted".to_owned(),
        ));
    }
    if let Ok(address) = host.parse::<IpAddr>()
        && is_non_public_ip(address)
    {
        return Err(KomaError::ImportDenied(
            "private network page URLs are not accepted".to_owned(),
        ));
    }
    Ok(())
}

fn is_mangafire_cdn_host(host: &str) -> bool {
    ["mfcdn.nl", "mfcdn1.xyz", "mfcdn2.xyz", "mfcdn3.xyz"]
        .iter()
        .any(|root| host == *root || host.ends_with(&format!(".{root}")))
}

fn is_non_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_multicast()
                || address.is_unspecified()
                || address.is_broadcast()
                || octets[0] == 0
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 2)
                || (octets[0] == 198 && (18..=19).contains(&octets[1]))
                || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
                || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
                || octets[0] >= 240
        }
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_non_public_ip(IpAddr::V4(mapped));
            }
            let segments = address.segments();
            address.is_loopback()
                || address.is_multicast()
                || address.is_unspecified()
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80
                || (segments[0] & 0xffc0) == 0xfec0
                || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        }
    }
}

fn page_extension(url: &Url, content_type: Option<&str>) -> &'static str {
    let mime = content_type
        .and_then(|value| value.split(';').next())
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "image/avif" => "avif",
        "image/bmp" => "bmp",
        "image/gif" => "gif",
        "image/png" => "png",
        "image/tiff" => "tiff",
        "image/webp" => "webp",
        "image/jpeg" | "image/jpg" => "jpg",
        _ => match Path::new(url.path())
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("avif") => "avif",
            Some("bmp") => "bmp",
            Some("gif") => "gif",
            Some("png") => "png",
            Some("tif" | "tiff") => "tiff",
            Some("webp") => "webp",
            _ => "jpg",
        },
    }
}

fn comic_info(resolved: &ResolvedImport) -> ComicInfo {
    let number = volume_number_label(resolved.volume.number);
    let title = clean_string(&resolved.volume.name)
        .unwrap_or_else(|| format!("{} — Volume {number}", resolved.title.title));
    let summary = strip_html(&resolved.title.synopsis_html);
    ComicInfo {
        title: Some(title),
        series: Some(resolved.title.title.clone()),
        number: Some(number),
        volume: whole_i32(resolved.volume.number),
        summary,
        writer: join_named(&resolved.title.authors),
        penciller: join_named(&resolved.title.artists),
        genre: join_named(&resolved.title.genres),
        tags: join_named(&resolved.title.themes),
        web: Some(resolved.target.source_url.to_string()),
        language_iso: Some(resolved.volume.language.clone()),
        manga: (resolved.title.r#type == "manga").then(|| "YesAndRightToLeft".to_owned()),
        page_count: Some(resolved.volume.pages.len()),
        ..ComicInfo::default()
    }
}

fn series_comic_info(resolved: &ResolvedImport, page_count: usize) -> ComicInfo {
    ComicInfo {
        title: Some(resolved.title.title.clone()),
        series: Some(resolved.title.title.clone()),
        summary: strip_html(&resolved.title.synopsis_html),
        writer: join_named(&resolved.title.authors),
        penciller: join_named(&resolved.title.artists),
        genre: join_named(&resolved.title.genres),
        tags: join_named(&resolved.title.themes),
        web: Some(resolved.target.source_url.to_string()),
        language_iso: Some(resolved.volume.language.clone()),
        manga: (resolved.title.r#type == "manga").then(|| "YesAndRightToLeft".to_owned()),
        page_count: Some(page_count),
        ..ComicInfo::default()
    }
}

fn chapter_comic_info(
    resolved: &ResolvedImport,
    chapter_number: f64,
    chapter_name: Option<String>,
    page_count: usize,
) -> ComicInfo {
    let number = volume_number_label(chapter_number);
    ComicInfo {
        title: Some(
            chapter_name.unwrap_or_else(|| format!("{} — Chapter {number}", resolved.title.title)),
        ),
        series: Some(resolved.title.title.clone()),
        number: Some(number),
        summary: strip_html(&resolved.title.synopsis_html),
        writer: join_named(&resolved.title.authors),
        penciller: join_named(&resolved.title.artists),
        genre: join_named(&resolved.title.genres),
        tags: join_named(&resolved.title.themes),
        web: Some(resolved.target.source_url.to_string()),
        language_iso: Some(resolved.volume.language.clone()),
        manga: (resolved.title.r#type == "manga").then(|| "YesAndRightToLeft".to_owned()),
        page_count: Some(page_count),
        ..ComicInfo::default()
    }
}

fn strip_html(source: &str) -> Option<String> {
    let document = Html::parse_fragment(source);
    let text = document
        .root_element()
        .text()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    clean_string(&text)
}

fn join_named(values: &[NamedValue]) -> Option<String> {
    let value = values
        .iter()
        .map(|value| value.title.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    clean_string(&value)
}

fn output_file_name(title: &str, number: f64, language: &str) -> String {
    let title = sanitize_file_component(title);
    let language = sanitize_file_component(language);
    format!(
        "{title} — Vol. {} [{language}].cbz",
        volume_number_label(number)
    )
}

fn series_output_file_name(title: &str, language: &str) -> String {
    let title = sanitize_file_component(title);
    let language = sanitize_file_component(language);
    format!("{title} — Complete [{language}].cbz")
}

fn chapter_output_file_name(title: &str, number: f64, language: &str) -> String {
    let title = sanitize_file_component(title);
    let language = sanitize_file_component(language);
    format!(
        "{title} — Ch. {} [{language}].cbz",
        volume_number_label(number)
    )
}

fn sanitize_file_component(value: &str) -> String {
    let mut cleaned = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut truncate_at = cleaned.len().min(96);
    while truncate_at > 0 && !cleaned.is_char_boundary(truncate_at) {
        truncate_at -= 1;
    }
    cleaned.truncate(truncate_at);
    cleaned = cleaned.trim_matches([' ', '.']).to_owned();
    if cleaned.is_empty() {
        cleaned.push_str("Untitled");
    }
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if reserved
        .iter()
        .any(|reserved| cleaned.eq_ignore_ascii_case(reserved))
    {
        cleaned.insert(0, '_');
    }
    cleaned
}

fn choose_output_path(requested: &Path, overwrite: bool) -> PathBuf {
    if overwrite || !requested.exists() {
        return requested.to_path_buf();
    }
    let parent = requested.parent().unwrap_or_else(|| Path::new(""));
    let stem = requested
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Imported volume");
    for number in 2..10_000 {
        let candidate = parent.join(format!("{stem} ({number}).cbz"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem} [{}].cbz", Uuid::new_v4()))
}

fn volume_number_label(number: f64) -> String {
    if number.fract().abs() < f64::EPSILON {
        format!("{number:.0}")
    } else {
        let value = format!("{number:.4}");
        value.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

fn whole_i32(number: f64) -> Option<i32> {
    (number.fract().abs() < f64::EPSILON && number >= i32::MIN as f64 && number <= i32::MAX as f64)
        .then_some(number as i32)
}

fn positive(value: i64) -> Option<u32> {
    u32::try_from(value).ok().filter(|value| *value > 0)
}

fn compatible_page_geometry(candidate: &MangaFirePage, original: &MangaFirePage) -> bool {
    let Some(candidate_width) = positive(candidate.width) else {
        return false;
    };
    let Some(candidate_height) = positive(candidate.height) else {
        return false;
    };
    let Some(original_width) = positive(original.width) else {
        return false;
    };
    let Some(original_height) = positive(original.height) else {
        return false;
    };
    let candidate_ratio = f64::from(candidate_width) / f64::from(candidate_height);
    let original_ratio = f64::from(original_width) / f64::from(original_height);
    (candidate_ratio - original_ratio).abs() <= 0.0025
}

fn unique_contiguous_page_range(
    page_counts: &[usize],
    target_page_count: usize,
) -> Option<(usize, usize)> {
    if target_page_count == 0 || page_counts.contains(&0) {
        return None;
    }
    let mut candidate = None;
    for start in 0..page_counts.len() {
        let mut count = 0_usize;
        for (end, page_count) in page_counts.iter().enumerate().skip(start) {
            count = count.checked_add(*page_count)?;
            if count == target_page_count {
                if candidate.is_some() {
                    return None;
                }
                candidate = Some((start, end));
                break;
            }
            if count > target_page_count {
                break;
            }
        }
    }
    candidate
}

fn clean_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn emit(events: Option<&UnboundedSender<ImportEvent>>, event: ImportEvent) {
    if let Some(events) = events {
        let _ = events.send(event);
    }
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use tempfile::tempdir;

    use super::{
        ImportOptions, ImportScope, LinkImporter, MangaFireImporter, MangaFirePage,
        MangaFireVolume, MangaFireVolumeSummary, compatible_page_geometry, is_non_public_ip,
        select_volume, unique_contiguous_page_range, validate_page_url,
    };
    use crate::error::KomaError;

    #[test]
    fn title_links_default_to_the_first_preferred_language_volume() {
        let volumes = [
            MangaFireVolumeSummary {
                id: 1,
                number: 1.0,
                name: String::new(),
                language: "ja".to_owned(),
                chapter_count: 8,
            },
            MangaFireVolumeSummary {
                id: 2,
                number: 1.0,
                name: String::new(),
                language: "en".to_owned(),
                chapter_count: 8,
            },
            MangaFireVolumeSummary {
                id: 3,
                number: 2.0,
                name: String::new(),
                language: "en".to_owned(),
                chapter_count: 9,
            },
        ];
        assert_eq!(select_volume(&volumes, None, "en").expect("default"), 2);
        assert_eq!(
            select_volume(&volumes, Some(3), "en").expect("requested"),
            3
        );
        assert!(select_volume(&volumes, Some(99), "en").is_err());
    }

    #[test]
    fn parses_current_volume_response_shape() {
        let source = r#"{
          "data": {
            "id": 339405,
            "number": 1,
            "name": "",
            "language": "en",
            "pages": [
              {"url":"https://l1n.mfcdn2.xyz/page.jpg","width":1096,"height":1600}
            ],
            "title": {"hid":"70ox7"}
          }
        }"#;
        let response: super::ApiData<MangaFireVolume> =
            serde_json::from_str(source).expect("fixture parses");
        assert_eq!(response.data.id, 339405);
        assert_eq!(response.data.pages.len(), 1);
    }

    #[test]
    fn trusts_only_the_observed_mangafire_cdn_roots() {
        for source in [
            "https://o48.mfcdn1.xyz/page.jpg",
            "https://l1n.mfcdn2.xyz/page.jpg",
            "https://o48.mfcdn3.xyz/page.jpg",
            "https://img.mfcdn.nl/page.jpg",
        ] {
            let url = url::Url::parse(source).expect("valid fixture URL");
            validate_page_url(&url, Some("mangafire.to")).expect("trusted MangaFire CDN");
        }

        for source in [
            "https://mfcdn1.xyz.evil.example/page.jpg",
            "https://notmfcdn1.xyz/page.jpg",
            "https://127.0.0.1/page.jpg",
            "http://o48.mfcdn1.xyz/page.jpg",
        ] {
            let url = url::Url::parse(source).expect("valid fixture URL");
            assert!(validate_page_url(&url, Some("mangafire.to")).is_err());
        }
    }

    #[test]
    fn accepts_only_one_exact_contiguous_official_chapter_sequence() {
        assert_eq!(
            unique_contiguous_page_range(
                &[
                    59, 51, 29, 33, 21, 25, 23, 19, 19, 21, 19, 19, 21, 19, 19, 20
                ],
                397
            ),
            Some((0, 14))
        );
        assert_eq!(unique_contiguous_page_range(&[10, 10, 10], 20), None);
        assert_eq!(unique_contiguous_page_range(&[10, 0, 20], 30), None);
        assert_eq!(unique_contiguous_page_range(&[10, 20], 31), None);
    }

    #[test]
    fn fallback_geometry_allows_resolution_changes_but_not_different_pages() {
        let original = MangaFirePage {
            url: "https://o48.mfcdn3.xyz/original.jpg".to_owned(),
            width: 1096,
            height: 1600,
        };
        let same_page_at_higher_resolution = MangaFirePage {
            url: "https://nw8.mfcdn3.xyz/fallback.jpg".to_owned(),
            width: 1400,
            height: 2043,
        };
        let landscape_page = MangaFirePage {
            url: "https://nw8.mfcdn3.xyz/wrong.jpg".to_owned(),
            width: 2043,
            height: 1400,
        };
        assert!(compatible_page_geometry(
            &same_page_at_higher_resolution,
            &original
        ));
        assert!(!compatible_page_geometry(&landscape_page, &original));
    }

    #[test]
    fn rejects_private_reserved_and_documentation_network_addresses() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "169.254.1.1",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "240.0.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(is_non_public_ip(address.parse().expect("fixture address")));
        }
        for address in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(!is_non_public_ip(address.parse().expect("fixture address")));
        }
    }

    #[tokio::test]
    async fn forbidden_check_writes_no_destination_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        let address = listener.local_addr().expect("fixture address");
        let server = thread::spawn(move || {
            let (mut connection, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 4096];
            let count = connection.read(&mut request).expect("read request");
            connection
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 6\r\nConnection: close\r\n\r\ndenied",
                )
                .expect("write response");
            String::from_utf8_lossy(&request[..count]).into_owned()
        });
        let origin = url::Url::parse(&format!("http://{address}/")).expect("origin");
        let importer = MangaFireImporter::with_test_origin(origin).expect("importer");
        let directory = tempdir().expect("temp directory");
        let destination = directory.path().join("downloads");
        let source = format!(
            "http://{address}/title/70ox7-hatori-to-furuta-no-hinichijou-sahanji/volume/339405"
        );
        let error = importer
            .import(&source, &ImportOptions::new(&destination), None)
            .await
            .expect_err("403 is denied");
        assert!(matches!(error, KomaError::ImportDenied(_)));
        assert!(!destination.exists());
        let request = server.join().expect("server thread");
        assert!(
            request.starts_with(
                "GET /title/70ox7-hatori-to-furuta-no-hinichijou-sahanji/volume/339405 "
            )
        );
    }

    #[tokio::test]
    async fn retries_a_temporary_mangafire_rate_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        let address = listener.local_addr().expect("fixture address");
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for attempt in 0..2 {
                let (mut connection, _) = listener.accept().expect("accept request");
                let mut request = [0_u8; 4096];
                let count = connection.read(&mut request).expect("read request");
                requests.push(String::from_utf8_lossy(&request[..count]).into_owned());
                if attempt == 0 {
                    connection
                        .write_all(
                            b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .expect("write rate-limit response");
                } else {
                    connection
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 13\r\nConnection: close\r\n\r\n{\"data\":\"ok\"}",
                        )
                        .expect("write JSON response");
                }
            }
            requests
        });
        let origin = url::Url::parse(&format!("http://{address}/")).expect("origin");
        let importer = MangaFireImporter::with_test_origin(origin.clone()).expect("importer");
        let response: serde_json::Value = importer
            .request_json(&origin.join("api/test").expect("request URL"))
            .await
            .expect("temporary 429 should be retried");
        assert_eq!(response["data"], "ok");
        let requests = server.join().expect("server thread");
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| request.starts_with("GET /api/test "))
        );
    }

    #[tokio::test]
    async fn online_series_opens_one_chapter_from_a_large_chapter_only_catalog() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        let address = listener.local_addr().expect("fixture address");
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..6 {
                let (mut connection, _) = listener.accept().expect("accept request");
                let mut request = [0_u8; 8192];
                let count = connection.read(&mut request).expect("read request");
                let request = String::from_utf8_lossy(&request[..count]).into_owned();
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or_default()
                    .to_owned();
                requests.push(path.clone());
                let body = if path == "/api/titles/abc" {
                    r#"{"data":{"hid":"abc","title":"Lazy proof"}}"#.to_owned()
                } else if path == "/api/titles/abc/volumes" {
                    r#"{"items":[]}"#.to_owned()
                } else if path.starts_with("/api/titles/abc/chapters?") {
                    let items = (1..=250)
                        .map(|id| {
                            format!(
                                r#"{{"id":{id},"number":{id},"name":"","language":"en","type":"official"}}"#
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    format!(r#"{{"items":[{items}],"meta":{{"lastPage":1}}}}"#)
                } else if path == "/api/chapters/1" || path == "/api/chapters/2" {
                    let id = if path.ends_with("/2") { 2 } else { 1 };
                    format!(
                        r#"{{"data":{{"id":{id},"language":"en","pages":[{{"url":"https://l1n.mfcdn2.xyz/page-{id}.jpg","width":800,"height":1200}}],"title":{{"hid":"abc"}}}}}}"#
                    )
                } else {
                    "ok".to_owned()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                connection
                    .write_all(response.as_bytes())
                    .expect("write response");
            }
            requests
        });
        let origin = url::Url::parse(&format!("http://{address}/")).expect("origin");
        let importer = MangaFireImporter::with_test_origin(origin).expect("importer");
        let mut options = ImportOptions::new(tempdir().expect("temp").path());
        options.scope = ImportScope::Series;
        options.preferred_language = Some("en".to_owned());
        let publication = importer
            .resolve_online(&format!("http://{address}/title/abc-lazy-proof"), &options)
            .await
            .expect("online publication");
        assert_eq!(publication.chapter_catalog.len(), 250);
        assert_eq!(publication.chapters.len(), 1);
        assert_eq!(publication.chapter_id, Some(1));
        assert_eq!(publication.page_count(), 1);
        let next = importer
            .navigate_online(&publication, ImportScope::Chapter, 2)
            .await
            .expect("next chapter");
        assert_eq!(next.chapter_id, Some(2));
        assert_eq!(next.chapters[0].number, 2.0);
        let requests = server.join().expect("server thread");
        assert_eq!(requests.len(), 6);
        assert_eq!(
            requests
                .iter()
                .filter(|path| path.starts_with("/api/chapters/"))
                .count(),
            2
        );
    }
}
