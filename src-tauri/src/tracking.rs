use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use chrono::Utc;
use keyring::Entry;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::Url;
use uuid::Uuid;

const KEYRING_SERVICE: &str = "me.pixlox.koma.tracking";
const ANILIST_REDIRECT_URI: &str = "koma://oauth/anilist";
const MAL_REDIRECT_URI: &str = "koma://oauth/myanimelist";
const OAUTH_WINDOW_SECONDS: i64 = 10 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackingProvider {
    AniList,
    MyAnimeList,
}

impl TrackingProvider {
    fn key(self) -> &'static str {
        match self {
            Self::AniList => "anilist",
            Self::MyAnimeList => "myanimelist",
        }
    }

    fn client_id(self) -> Option<&'static str> {
        #[cfg(test)]
        if option_env!("KOMA_ANILIST_CLIENT_ID").is_none()
            && option_env!("KOMA_MAL_CLIENT_ID").is_none()
        {
            return Some("test-client-id");
        }
        let value = match self {
            Self::AniList => option_env!("KOMA_ANILIST_CLIENT_ID"),
            Self::MyAnimeList => option_env!("KOMA_MAL_CLIENT_ID"),
        }?;
        (!value.trim().is_empty()).then_some(value.trim())
    }

    fn redirect_uri(self) -> &'static str {
        match self {
            Self::AniList => ANILIST_REDIRECT_URI,
            Self::MyAnimeList => MAL_REDIRECT_URI,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingAccount {
    pub provider: TrackingProvider,
    pub connected: bool,
    pub username: Option<String>,
    pub oauth_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingCandidate {
    pub id: u64,
    pub title: String,
    pub alternate_titles: Vec<String>,
    pub cover_url: Option<String>,
    pub chapters: Option<u32>,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingSuggestion {
    pub provider: TrackingProvider,
    pub automatic: bool,
    pub candidates: Vec<TrackingCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingMapping {
    pub publication_id: Uuid,
    pub provider: TrackingProvider,
    pub media_id: u64,
    pub media_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingOAuth {
    state: String,
    code_verifier: Option<String>,
    created_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct TrackingConfig {
    accounts: HashMap<String, String>,
    mappings: Vec<TrackingMapping>,
    pending_oauth: HashMap<String, PendingOAuth>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredToken {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

pub struct TrackingService {
    client: Client,
    config_path: PathBuf,
    config: Mutex<TrackingConfig>,
}

impl TrackingService {
    pub fn new(config_path: impl AsRef<Path>) -> Result<Self, String> {
        let config_path = config_path.as_ref().to_path_buf();
        let config = match std::fs::read(&config_path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("could not read tracking settings: {error}"))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => TrackingConfig::default(),
            Err(error) => return Err(format!("could not read tracking settings: {error}")),
        };
        let client = Client::builder()
            .user_agent(concat!("Koma/", env!("CARGO_PKG_VERSION")))
            .https_only(true)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            client,
            config_path,
            config: Mutex::new(config),
        })
    }

    pub fn accounts(&self) -> Result<Vec<TrackingAccount>, String> {
        let config = self
            .config
            .lock()
            .map_err(|_| "tracking settings lock was poisoned".to_owned())?;
        Ok([TrackingProvider::AniList, TrackingProvider::MyAnimeList]
            .into_iter()
            .map(|provider| TrackingAccount {
                provider,
                connected: self.token_bundle(provider).is_ok(),
                username: config.accounts.get(provider.key()).cloned(),
                oauth_configured: provider.client_id().is_some(),
            })
            .collect())
    }

    pub fn begin_oauth(&self, provider: TrackingProvider) -> Result<String, String> {
        let client_id = provider.client_id().ok_or_else(|| {
            format!(
                "{} OAuth is not configured in this build",
                provider_display_name(provider)
            )
        })?;
        let state = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let code_verifier = (provider == TrackingProvider::MyAnimeList).then(|| {
            (0..4)
                .map(|_| Uuid::new_v4().simple().to_string())
                .collect::<String>()
        });
        {
            let mut config = self
                .config
                .lock()
                .map_err(|_| "tracking settings lock was poisoned".to_owned())?;
            config.pending_oauth.insert(
                provider.key().to_owned(),
                PendingOAuth {
                    state: state.clone(),
                    code_verifier: code_verifier.clone(),
                    created_at: Utc::now().timestamp(),
                },
            );
            self.save_config(&config)?;
        }

        let mut url = match provider {
            TrackingProvider::AniList => Url::parse("https://anilist.co/api/v2/oauth/authorize"),
            TrackingProvider::MyAnimeList => {
                Url::parse("https://myanimelist.net/v1/oauth2/authorize")
            }
        }
        .map_err(|error| error.to_string())?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("client_id", client_id)
                .append_pair("redirect_uri", provider.redirect_uri())
                .append_pair("state", &state);
            match provider {
                TrackingProvider::AniList => {
                    query.append_pair("response_type", "token");
                }
                TrackingProvider::MyAnimeList => {
                    query.append_pair("response_type", "code").append_pair(
                        "code_challenge",
                        code_verifier.as_deref().unwrap_or_default(),
                    );
                }
            }
        }
        Ok(url.into())
    }

    pub async fn finish_oauth(&self, callback: &str) -> Result<TrackingAccount, String> {
        let url = Url::parse(callback).map_err(|_| "invalid OAuth callback".to_owned())?;
        if url.scheme() != "koma" || url.host_str() != Some("oauth") {
            return Err("Koma rejected an unexpected OAuth callback".to_owned());
        }
        let provider = match url.path() {
            "/anilist" => TrackingProvider::AniList,
            "/myanimelist" => TrackingProvider::MyAnimeList,
            _ => return Err("Koma rejected an unknown OAuth provider".to_owned()),
        };
        let mut parameters = url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<HashMap<_, _>>();
        if let Some(fragment) = url.fragment() {
            parameters.extend(
                url::form_urlencoded::parse(fragment.as_bytes())
                    .map(|(key, value)| (key.into_owned(), value.into_owned())),
            );
        }
        if let Some(error) = parameters.get("error") {
            let detail = parameters
                .get("error_description")
                .map(String::as_str)
                .unwrap_or(error);
            return Err(format!(
                "{} authorization was cancelled: {detail}",
                provider_display_name(provider)
            ));
        }
        let returned_state = parameters
            .get("state")
            .ok_or_else(|| "OAuth callback did not include its security state".to_owned())?;
        let pending = {
            let mut config = self
                .config
                .lock()
                .map_err(|_| "tracking settings lock was poisoned".to_owned())?;
            let pending = config
                .pending_oauth
                .remove(provider.key())
                .ok_or_else(|| "this OAuth request is no longer active".to_owned())?;
            self.save_config(&config)?;
            pending
        };
        if pending.state != *returned_state {
            return Err("OAuth security state did not match".to_owned());
        }
        if Utc::now().timestamp() - pending.created_at > OAUTH_WINDOW_SECONDS {
            return Err("OAuth authorization expired; start again from Settings".to_owned());
        }

        let token = match provider {
            TrackingProvider::AniList => {
                let access_token = parameters
                    .remove("access_token")
                    .filter(|token| !token.is_empty())
                    .ok_or_else(|| "AniList did not return an access token".to_owned())?;
                let expires_at = parameters
                    .get("expires_in")
                    .and_then(|value| value.parse::<i64>().ok())
                    .map(|seconds| Utc::now().timestamp() + seconds);
                StoredToken {
                    access_token,
                    refresh_token: None,
                    expires_at,
                }
            }
            TrackingProvider::MyAnimeList => {
                let code = parameters
                    .get("code")
                    .filter(|code| !code.is_empty())
                    .ok_or_else(|| "MyAnimeList did not return an authorization code".to_owned())?;
                self.exchange_mal_code(
                    code,
                    pending
                        .code_verifier
                        .as_deref()
                        .ok_or_else(|| "MAL PKCE verifier is missing".to_owned())?,
                )
                .await?
            }
        };
        self.finish_connection(provider, token).await
    }

    async fn finish_connection(
        &self,
        provider: TrackingProvider,
        token: StoredToken,
    ) -> Result<TrackingAccount, String> {
        let username = match provider {
            TrackingProvider::AniList => self.anilist_username(&token.access_token).await?,
            TrackingProvider::MyAnimeList => self.mal_username(&token.access_token).await?,
        };
        self.store_token(provider, &token)?;
        {
            let mut config = self
                .config
                .lock()
                .map_err(|_| "tracking settings lock was poisoned".to_owned())?;
            config
                .accounts
                .insert(provider.key().to_owned(), username.clone());
            self.save_config(&config)?;
        }
        Ok(TrackingAccount {
            provider,
            connected: true,
            username: Some(username),
            oauth_configured: provider.client_id().is_some(),
        })
    }

    pub fn disconnect(&self, provider: TrackingProvider) -> Result<(), String> {
        if let Ok(entry) = Entry::new(KEYRING_SERVICE, provider.key()) {
            let _ = entry.delete_credential();
        }
        let mut config = self
            .config
            .lock()
            .map_err(|_| "tracking settings lock was poisoned".to_owned())?;
        config.accounts.remove(provider.key());
        config
            .mappings
            .retain(|mapping| mapping.provider != provider);
        self.save_config(&config)
    }

    pub async fn suggest(
        &self,
        provider: TrackingProvider,
        query: &str,
    ) -> Result<TrackingSuggestion, String> {
        let mut candidates = match provider {
            TrackingProvider::AniList => self.search_anilist(query).await?,
            TrackingProvider::MyAnimeList => self.search_mal(query).await?,
        };
        for candidate in &mut candidates {
            candidate.score = candidate_score(query, candidate);
        }
        candidates.sort_by(|left, right| right.score.total_cmp(&left.score));
        candidates.truncate(8);
        let first = candidates
            .first()
            .map(|candidate| candidate.score)
            .unwrap_or(0.0);
        let second = candidates
            .get(1)
            .map(|candidate| candidate.score)
            .unwrap_or(0.0);
        Ok(TrackingSuggestion {
            provider,
            automatic: first >= 0.92 && first - second >= 0.08,
            candidates,
        })
    }

    pub fn set_mapping(&self, mapping: TrackingMapping) -> Result<(), String> {
        let mut config = self
            .config
            .lock()
            .map_err(|_| "tracking settings lock was poisoned".to_owned())?;
        config.mappings.retain(|candidate| {
            candidate.publication_id != mapping.publication_id
                || candidate.provider != mapping.provider
        });
        config.mappings.push(mapping);
        self.save_config(&config)
    }

    pub fn mappings(&self, publication_id: Uuid) -> Result<Vec<TrackingMapping>, String> {
        let config = self
            .config
            .lock()
            .map_err(|_| "tracking settings lock was poisoned".to_owned())?;
        Ok(config
            .mappings
            .iter()
            .filter(|mapping| mapping.publication_id == publication_id)
            .cloned()
            .collect())
    }

    pub async fn sync_progress(&self, publication_id: Uuid, chapter: u32) {
        let mappings = match self.mappings(publication_id) {
            Ok(mappings) => mappings,
            Err(_) => return,
        };
        for mapping in mappings {
            let Ok(token) = self.access_token(mapping.provider).await else {
                continue;
            };
            let _ = match mapping.provider {
                TrackingProvider::AniList => {
                    self.sync_anilist(&token, mapping.media_id, chapter).await
                }
                TrackingProvider::MyAnimeList => {
                    self.sync_mal(&token, mapping.media_id, chapter).await
                }
            };
        }
    }

    fn token_bundle(&self, provider: TrackingProvider) -> Result<StoredToken, String> {
        let stored = Entry::new(KEYRING_SERVICE, provider.key())
            .map_err(|error| error.to_string())?
            .get_password()
            .map_err(|error| error.to_string())?;
        Ok(serde_json::from_str(&stored).unwrap_or(StoredToken {
            access_token: stored,
            refresh_token: None,
            expires_at: None,
        }))
    }

    fn store_token(&self, provider: TrackingProvider, token: &StoredToken) -> Result<(), String> {
        let encoded = serde_json::to_string(token).map_err(|error| error.to_string())?;
        Entry::new(KEYRING_SERVICE, provider.key())
            .map_err(|error| error.to_string())?
            .set_password(&encoded)
            .map_err(|error| format!("could not store the OAuth token securely: {error}"))
    }

    async fn access_token(&self, provider: TrackingProvider) -> Result<String, String> {
        let token = self.token_bundle(provider)?;
        if token
            .expires_at
            .is_none_or(|expires_at| expires_at > Utc::now().timestamp() + 60)
        {
            return Ok(token.access_token);
        }
        if provider != TrackingProvider::MyAnimeList {
            return Err("AniList authorization expired; connect the account again".to_owned());
        }
        self.refresh_mal(token).await
    }

    async fn exchange_mal_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<StoredToken, String> {
        let client_id = TrackingProvider::MyAnimeList
            .client_id()
            .ok_or_else(|| "MyAnimeList OAuth is not configured".to_owned())?;
        let response = self
            .client
            .post("https://myanimelist.net/v1/oauth2/token")
            .basic_auth(client_id, Some(""))
            .form(&[
                ("client_id", client_id),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", MAL_REDIRECT_URI),
                ("code_verifier", code_verifier),
            ])
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| format!("MyAnimeList token exchange failed: {error}"))?
            .json::<OAuthTokenResponse>()
            .await
            .map_err(|error| error.to_string())?;
        Ok(StoredToken {
            access_token: response.access_token,
            refresh_token: response.refresh_token,
            expires_at: response
                .expires_in
                .map(|seconds| Utc::now().timestamp() + seconds),
        })
    }

    async fn refresh_mal(&self, token: StoredToken) -> Result<String, String> {
        let client_id = TrackingProvider::MyAnimeList
            .client_id()
            .ok_or_else(|| "MyAnimeList OAuth is not configured".to_owned())?;
        let refresh_token = token
            .refresh_token
            .as_deref()
            .ok_or_else(|| "MyAnimeList authorization expired; connect again".to_owned())?;
        let response = self
            .client
            .post("https://myanimelist.net/v1/oauth2/token")
            .basic_auth(client_id, Some(""))
            .form(&[
                ("client_id", client_id),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| format!("MyAnimeList token refresh failed: {error}"))?
            .json::<OAuthTokenResponse>()
            .await
            .map_err(|error| error.to_string())?;
        let refreshed = StoredToken {
            access_token: response.access_token,
            refresh_token: response.refresh_token.or(token.refresh_token),
            expires_at: response
                .expires_in
                .map(|seconds| Utc::now().timestamp() + seconds),
        };
        self.store_token(TrackingProvider::MyAnimeList, &refreshed)?;
        Ok(refreshed.access_token)
    }

    fn account_name(&self, provider: TrackingProvider) -> Result<String, String> {
        self.config
            .lock()
            .map_err(|_| "tracking settings lock was poisoned".to_owned())?
            .accounts
            .get(provider.key())
            .cloned()
            .ok_or_else(|| "tracking account is not connected".to_owned())
    }

    fn save_config(&self, config: &TrackingConfig) -> Result<(), String> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?;
        let temporary = self.config_path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
        std::fs::rename(temporary, &self.config_path).map_err(|error| error.to_string())
    }

    async fn anilist_username(&self, token: &str) -> Result<String, String> {
        let response = self
            .client
            .post("https://graphql.anilist.co")
            .bearer_auth(token)
            .json(&json!({ "query": "query { Viewer { name } }" }))
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| format!("AniList rejected this token: {error}"))?
            .json::<serde_json::Value>()
            .await
            .map_err(|error| error.to_string())?;
        response["data"]["Viewer"]["name"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "AniList did not return an account name".to_owned())
    }

    async fn mal_username(&self, token: &str) -> Result<String, String> {
        let response = self
            .client
            .get("https://api.myanimelist.net/v2/users/@me")
            .bearer_auth(token)
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| format!("MyAnimeList rejected this token: {error}"))?
            .json::<serde_json::Value>()
            .await
            .map_err(|error| error.to_string())?;
        response["name"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "MyAnimeList did not return an account name".to_owned())
    }

    async fn search_anilist(&self, query: &str) -> Result<Vec<TrackingCandidate>, String> {
        let body = json!({
            "query": "query ($search: String) { Page(page: 1, perPage: 10) { media(search: $search, type: MANGA) { id title { romaji english native } synonyms coverImage { medium } chapters } } }",
            "variables": { "search": query }
        });
        let response = self
            .client
            .post("https://graphql.anilist.co")
            .json(&body)
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json::<serde_json::Value>()
            .await
            .map_err(|error| error.to_string())?;
        let media = response["data"]["Page"]["media"]
            .as_array()
            .ok_or_else(|| "AniList returned an unexpected response".to_owned())?;
        Ok(media
            .iter()
            .filter_map(|entry| {
                let title = entry["title"]["english"]
                    .as_str()
                    .or_else(|| entry["title"]["romaji"].as_str())
                    .or_else(|| entry["title"]["native"].as_str())?
                    .to_owned();
                let mut alternate_titles = ["romaji", "english", "native"]
                    .into_iter()
                    .filter_map(|key| entry["title"][key].as_str().map(str::to_owned))
                    .collect::<Vec<_>>();
                alternate_titles.extend(
                    entry["synonyms"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|value| value.as_str().map(str::to_owned)),
                );
                Some(TrackingCandidate {
                    id: entry["id"].as_u64()?,
                    title,
                    alternate_titles,
                    cover_url: entry["coverImage"]["medium"].as_str().map(str::to_owned),
                    chapters: entry["chapters"]
                        .as_u64()
                        .and_then(|value| value.try_into().ok()),
                    score: 0.0,
                })
            })
            .collect())
    }

    async fn search_mal(&self, query: &str) -> Result<Vec<TrackingCandidate>, String> {
        let token = self.access_token(TrackingProvider::MyAnimeList).await?;
        let response = self
            .client
            .get("https://api.myanimelist.net/v2/manga")
            .bearer_auth(token)
            .query(&[
                ("q", query),
                ("limit", "10"),
                ("fields", "alternative_titles,num_chapters"),
            ])
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json::<serde_json::Value>()
            .await
            .map_err(|error| error.to_string())?;
        Ok(response["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let node = &entry["node"];
                let mut alternate_titles = Vec::new();
                if let Some(values) = node["alternative_titles"]["synonyms"].as_array() {
                    alternate_titles.extend(
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_owned)),
                    );
                }
                for key in ["en", "ja"] {
                    if let Some(value) = node["alternative_titles"][key].as_str() {
                        alternate_titles.push(value.to_owned());
                    }
                }
                Some(TrackingCandidate {
                    id: node["id"].as_u64()?,
                    title: node["title"].as_str()?.to_owned(),
                    alternate_titles,
                    cover_url: node["main_picture"]["medium"].as_str().map(str::to_owned),
                    chapters: node["num_chapters"]
                        .as_u64()
                        .and_then(|value| value.try_into().ok()),
                    score: 0.0,
                })
            })
            .collect())
    }

    async fn sync_anilist(&self, token: &str, media_id: u64, chapter: u32) -> Result<(), String> {
        let username = self.account_name(TrackingProvider::AniList)?;
        let current = self
            .client
            .post("https://graphql.anilist.co")
            .bearer_auth(token)
            .json(&json!({
                "query": "query ($userName: String, $mediaId: Int) { MediaList(userName: $userName, mediaId: $mediaId) { progress } }",
                "variables": { "userName": username, "mediaId": media_id }
            }))
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json::<serde_json::Value>()
            .await
            .map_err(|error| error.to_string())?;
        if current["data"]["MediaList"]["progress"]
            .as_u64()
            .is_some_and(|progress| progress >= u64::from(chapter))
        {
            return Ok(());
        }
        self.client
            .post("https://graphql.anilist.co")
            .bearer_auth(token)
            .json(&json!({
                "query": "mutation ($mediaId: Int, $progress: Int) { SaveMediaListEntry(mediaId: $mediaId, progress: $progress) { id } }",
                "variables": { "mediaId": media_id, "progress": chapter }
            }))
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    async fn sync_mal(&self, token: &str, media_id: u64, chapter: u32) -> Result<(), String> {
        let current = self
            .client
            .get(format!("https://api.myanimelist.net/v2/manga/{media_id}"))
            .bearer_auth(token)
            .query(&[("fields", "my_list_status")])
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json::<serde_json::Value>()
            .await
            .map_err(|error| error.to_string())?;
        if current["my_list_status"]["num_chapters_read"]
            .as_u64()
            .is_some_and(|progress| progress >= u64::from(chapter))
        {
            return Ok(());
        }
        self.client
            .patch(format!(
                "https://api.myanimelist.net/v2/manga/{media_id}/my_list_status"
            ))
            .bearer_auth(token)
            .form(&[("num_chapters_read", chapter.to_string())])
            .send()
            .await
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn candidate_score(query: &str, candidate: &TrackingCandidate) -> f64 {
    let query = normalize_title(query);
    if query.is_empty() {
        return 0.0;
    }
    candidate
        .alternate_titles
        .iter()
        .chain(std::iter::once(&candidate.title))
        .map(|title| {
            let title = normalize_title(title);
            if title == query {
                return 1.0;
            }
            let query_tokens = query.split_whitespace().collect::<Vec<_>>();
            let title_tokens = title.split_whitespace().collect::<Vec<_>>();
            let shared = query_tokens
                .iter()
                .filter(|token| title_tokens.contains(token))
                .count();
            2.0 * shared as f64 / (query_tokens.len() + title_tokens.len()).max(1) as f64
        })
        .fold(0.0, f64::max)
}

fn normalize_title(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn provider_display_name(provider: TrackingProvider) -> &'static str {
    match provider {
        TrackingProvider::AniList => "AniList",
        TrackingProvider::MyAnimeList => "MyAnimeList",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_requires_an_exact_or_decisive_title() {
        let exact = TrackingCandidate {
            id: 1,
            title: "Frieren: Beyond Journey's End".to_owned(),
            alternate_titles: vec!["Sousou no Frieren".to_owned()],
            cover_url: None,
            chapters: None,
            score: 0.0,
        };
        assert_eq!(candidate_score("Sousou no Frieren", &exact), 1.0);
        assert!(candidate_score("Frieren", &exact) < 0.92);
    }

    #[test]
    fn oauth_urls_use_provider_specific_secure_flows() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let service =
            TrackingService::new(directory.path().join("tracking.json")).expect("service");

        let anilist = Url::parse(
            &service
                .begin_oauth(TrackingProvider::AniList)
                .expect("AniList OAuth URL"),
        )
        .expect("valid AniList URL");
        let anilist_query = anilist.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(anilist.scheme(), "https");
        assert_eq!(anilist.host_str(), Some("anilist.co"));
        assert_eq!(
            anilist_query
                .get("redirect_uri")
                .map(|value| value.as_ref()),
            Some(ANILIST_REDIRECT_URI)
        );
        assert_eq!(
            anilist_query
                .get("response_type")
                .map(|value| value.as_ref()),
            Some("token")
        );
        assert!(
            anilist_query
                .get("state")
                .is_some_and(|state| state.len() == 64)
        );

        let mal = Url::parse(
            &service
                .begin_oauth(TrackingProvider::MyAnimeList)
                .expect("MAL OAuth URL"),
        )
        .expect("valid MAL URL");
        let mal_query = mal.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(mal.host_str(), Some("myanimelist.net"));
        assert_eq!(
            mal_query.get("redirect_uri").map(|value| value.as_ref()),
            Some(MAL_REDIRECT_URI)
        );
        assert_eq!(
            mal_query.get("response_type").map(|value| value.as_ref()),
            Some("code")
        );
        assert!(
            mal_query
                .get("code_challenge")
                .is_some_and(|challenge| challenge.len() == 128)
        );
    }

    #[tokio::test]
    async fn oauth_callback_rejects_an_unmatched_state_before_token_exchange() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let service =
            TrackingService::new(directory.path().join("tracking.json")).expect("service");
        service
            .begin_oauth(TrackingProvider::MyAnimeList)
            .expect("begin OAuth");
        let error = service
            .finish_oauth("koma://oauth/myanimelist?code=proof&state=wrong")
            .await
            .expect_err("wrong state must fail");
        assert_eq!(error, "OAuth security state did not match");
    }
}
