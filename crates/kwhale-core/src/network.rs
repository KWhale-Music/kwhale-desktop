use crate::error::{CoreError, CoreResult};
use crate::models::*;
use crate::server_url::ServerUrl;
use crate::storage::TokenStore;
use futures_util::StreamExt;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, RANGE};
use reqwest::multipart::{Form, Part};
use reqwest::{Client, Method, Response, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub normalized_base_url: String,
    pub base_host: String,
    pub client_version: String,
    pub device_id: String,
    pub allow_insecure_http: bool,
}

impl ApiConfig {
    pub fn new(
        base_url: &str,
        client_version: impl Into<String>,
        device_id: impl Into<String>,
        allow_insecure_http: bool,
    ) -> CoreResult<Self> {
        let normalized = ServerUrl::normalize(base_url, None)
            .ok_or_else(|| CoreError::InvalidServerUrl(base_url.to_string()))?;
        if ServerUrl::is_http(&normalized)
            && !allow_insecure_http
            && !ServerUrl::is_local_or_lan(&normalized)
        {
            return Err(CoreError::InsecureHttpDisallowed(normalized));
        }
        let host = ServerUrl::extract_host(&normalized)
            .ok_or_else(|| CoreError::InvalidServerUrl(normalized.clone()))?;
        Ok(Self {
            normalized_base_url: normalized,
            base_host: host,
            client_version: client_version.into(),
            device_id: device_id.into(),
            allow_insecure_http,
        })
    }

    pub fn url(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{}", self.normalized_base_url, path)
        } else {
            format!("{}/{}", self.normalized_base_url, path)
        }
    }
}

#[derive(Clone)]
pub struct KwhaleApi {
    config: Arc<RwLock<ApiConfig>>,
    client: Client,
    token_store: TokenStore,
    refresh_lock: Arc<Mutex<()>>,
}

impl KwhaleApi {
    pub fn new(config: ApiConfig, token_store: TokenStore) -> CoreResult<Self> {
        let allowed_host = Arc::new(RwLock::new(config.base_host.clone()));
        let host_guard = allowed_host.clone();
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            // Enforce single-origin even across redirects.
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                let allowed = host_guard.read().expect("host lock").clone();
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if attempt
                    .url()
                    .host_str()
                    .map(|h| h.eq_ignore_ascii_case(&allowed))
                    .unwrap_or(false)
                {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()?;
        let _ = allowed_host;
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            client,
            token_store,
            refresh_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn config(&self) -> ApiConfig {
        self.config.read().expect("config lock").clone()
    }

    pub fn reconfigure(&self, new_config: ApiConfig) -> CoreResult<()> {
        *self.config.write().expect("config lock") = new_config;
        Ok(())
    }

    pub fn token_store(&self) -> &TokenStore {
        &self.token_store
    }

    pub fn session(&self) -> Option<Session> {
        self.token_store.load()
    }

    pub fn stream_url(&self, track_id: &str) -> String {
        self.config().url(&format!("/tracks/{track_id}/stream"))
    }

    pub fn download_url(&self, track_id: &str) -> String {
        self.config().url(&format!("/tracks/{track_id}/download"))
    }

    pub fn image_url(&self, image_id: &str, size: &str) -> String {
        self.config().url(&format!("/images/{image_id}?size={size}"))
    }

    fn verify_host(&self, url: &Url) -> CoreResult<()> {
        let allowed = self.config().base_host;
        let host = url.host_str().unwrap_or_default();
        if !host.eq_ignore_ascii_case(&allowed) {
            return Err(CoreError::HostPolicyViolation {
                host: host.to_string(),
                allowed,
            });
        }
        Ok(())
    }

    fn build_url(&self, path: &str, query: &[(&str, &str)]) -> CoreResult<Url> {
        let full = self.config().url(path);
        let mut url = Url::parse(&full).map_err(|_| CoreError::InvalidServerUrl(full.clone()))?;
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        self.verify_host(&url)?;
        Ok(url)
    }

    fn common_headers(&self, extra: &[(&str, &str)]) -> HeaderMap {
        let cfg = self.config();
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        if let Ok(v) = HeaderValue::from_str(&cfg.client_version) {
            headers.insert("X-Client-Version", v);
        }
        if let Ok(v) = HeaderValue::from_str(&cfg.device_id) {
            headers.insert("X-Device-Id", v);
        }
        for (k, v) in extra {
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                headers.insert(name, val);
            }
        }
        headers
    }

    /// Single-flight session refresh shared across API requests, image loader, and GStreamer streaming.
    pub async fn refresh_session(&self, stale_access_token: Option<&str>) -> Option<Session> {
        let _guard = self.refresh_lock.lock().await;
        let current = self.token_store.load()?;
        if let Some(stale) = stale_access_token {
            if current.access_token != stale {
                return Some(current);
            }
        }
        self.refresh_blocking(&current.refresh_token).await
    }

    async fn refresh_blocking(&self, refresh_token: &str) -> Option<Session> {
        let Ok(url) = self.build_url("/auth/refresh", &[]) else {
            return None;
        };
        let req_body = RefreshRequest {
            refresh_token: refresh_token.to_string(),
        };
        let resp = self
            .client
            .post(url)
            .headers(self.common_headers(&[]))
            .json(&req_body)
            .send()
            .await;

        let Ok(resp) = resp else {
            // Network blip: never log the user out.
            return None;
        };

        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            self.token_store.clear();
            return None;
        }
        if !status.is_success() {
            // Transient 5xx/429: preserve session.
            return None;
        }
        let Ok(auth) = resp.json::<AuthResponse>().await else {
            return None;
        };
        let session = Session::from_auth(auth, now_epoch_sec());
        self.token_store.save(session.clone());
        Some(session)
    }

    async fn send_authenticated(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        json_body: Option<&serde_json::Value>,
        extra_headers: &[(&str, &str)],
    ) -> CoreResult<Response> {
        let url = self.build_url(path, query)?;
        let is_auth_route = url.path().contains("/auth/");
        let session = self.token_store.load();

        let build_req = |token: Option<&str>| {
            let mut headers = self.common_headers(extra_headers);
            if let Some(tok) = token {
                if let Ok(val) = HeaderValue::from_str(&format!("Bearer {tok}")) {
                    headers.insert(AUTHORIZATION, val);
                }
            }
            let mut req = self.client.request(method.clone(), url.clone()).headers(headers);
            if let Some(body) = json_body {
                req = req.json(body);
            }
            req
        };

        let resp = build_req(session.as_ref().map(|s| s.access_token.as_str()))
            .send()
            .await?;

        if resp.status() == StatusCode::UNAUTHORIZED && !is_auth_route {
            if let Some(sess) = session {
                if let Some(refreshed) = self.refresh_session(Some(&sess.access_token)).await {
                    let retry = build_req(Some(&refreshed.access_token)).send().await?;
                    return Ok(retry);
                }
            }
        }
        Ok(resp)
    }

    async fn request_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<serde_json::Value>,
        extra_headers: &[(&str, &str)],
    ) -> CoreResult<T> {
        let resp = self
            .send_authenticated(method, path, query, body.as_ref(), extra_headers)
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(CoreError::api(status.as_u16(), &text));
        }
        Ok(serde_json::from_str(&text)?)
    }

    async fn request_unit(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<serde_json::Value>,
        extra_headers: &[(&str, &str)],
    ) -> CoreResult<()> {
        let resp = self
            .send_authenticated(method, path, query, body.as_ref(), extra_headers)
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(CoreError::api(status.as_u16(), &text));
        }
        Ok(())
    }

    fn to_value<B: Serialize>(body: &B) -> CoreResult<serde_json::Value> {
        Ok(serde_json::to_value(body)?)
    }

    // ---- Auth --------------------------------------------------------------

    pub async fn register(
        &self,
        email: &str,
        password: &str,
        display_name: &str,
        invite_token: &str,
    ) -> CoreResult<Session> {
        let req = RegisterRequest {
            email: email.trim().to_string(),
            password: password.to_string(),
            display_name: display_name.trim().to_string(),
            invite_token: invite_token.trim().to_string(),
            device_id: Some(self.config().device_id),
        };
        let auth: AuthResponse = self
            .request_json(Method::POST, "/auth/register", &[], Some(Self::to_value(&req)?), &[])
            .await?;
        let session = Session::from_auth(auth, now_epoch_sec());
        self.token_store.save(session.clone());
        Ok(session)
    }

    pub async fn login(&self, email: &str, password: &str) -> CoreResult<Session> {
        let req = LoginRequest {
            email: email.trim().to_string(),
            password: password.to_string(),
            device_id: Some(self.config().device_id),
        };
        let auth: AuthResponse = self
            .request_json(Method::POST, "/auth/login", &[], Some(Self::to_value(&req)?), &[])
            .await?;
        let session = Session::from_auth(auth, now_epoch_sec());
        self.token_store.save(session.clone());
        Ok(session)
    }

    pub async fn logout(&self) {
        if let Some(sess) = self.token_store.load() {
            if let Ok(val) = Self::to_value(&LogoutRequest {
                refresh_token: sess.refresh_token,
            }) {
                let _ = self
                    .request_unit(Method::POST, "/auth/logout", &[], Some(val), &[])
                    .await;
            }
        }
        self.token_store.clear();
    }

    pub async fn me(&self) -> CoreResult<User> {
        self.request_json(Method::GET, "/me", &[], None, &[]).await
    }

    pub async fn update_me(&self, display_name: &str) -> CoreResult<User> {
        let body = Self::to_value(&UserUpdate {
            display_name: display_name.to_string(),
        })?;
        self.request_json(Method::PATCH, "/me", &[], Some(body), &[])
            .await
    }

    // ---- Home & Search -----------------------------------------------------

    pub async fn home(&self, cursor: Option<&str>, limit: Option<i32>) -> CoreResult<HomeResponse> {
        let mut q = Vec::new();
        let lim_s;
        if let Some(c) = cursor {
            q.push(("cursor", c));
        }
        if let Some(l) = limit {
            lim_s = l.to_string();
            q.push(("limit", lim_s.as_str()));
        }
        self.request_json(Method::GET, "/home", &q, None, &[]).await
    }

    pub async fn search(
        &self,
        query: &str,
        search_type: &str,
        limit: Option<i32>,
        wait: bool,
    ) -> CoreResult<SearchResponse> {
        let mut q = vec![("q", query), ("type", search_type)];
        let lim_s;
        if let Some(l) = limit {
            lim_s = l.to_string();
            q.push(("limit", lim_s.as_str()));
        }
        if wait {
            q.push(("wait", "true"));
        }
        self.request_json(Method::GET, "/search", &q, None, &[]).await
    }

    pub async fn suggestions(&self, query: &str) -> CoreResult<Suggestions> {
        self.request_json(Method::GET, "/search/suggestions", &[("q", query)], None, &[])
            .await
    }

    pub async fn search_history(&self) -> CoreResult<SearchHistory> {
        self.request_json(Method::GET, "/search/history", &[], None, &[])
            .await
    }

    pub async fn clear_search_history(&self) -> CoreResult<()> {
        self.request_unit(Method::DELETE, "/search/history", &[], None, &[])
            .await
    }

    // ---- Catalog -----------------------------------------------------------

    pub async fn track(&self, id: &str) -> CoreResult<Track> {
        self.request_json(Method::GET, &format!("/tracks/{id}"), &[], None, &[])
            .await
    }

    pub async fn track_info(&self, id: &str) -> CoreResult<TrackInfo> {
        self.request_json(Method::GET, &format!("/tracks/{id}/info"), &[], None, &[])
            .await
    }

    pub async fn similar_tracks(&self, id: &str, limit: Option<i32>) -> CoreResult<TracksPage> {
        let lim_s;
        let mut q = Vec::new();
        if let Some(l) = limit {
            lim_s = l.to_string();
            q.push(("limit", lim_s.as_str()));
        }
        self.request_json(Method::GET, &format!("/tracks/{id}/similar"), &q, None, &[])
            .await
    }

    pub async fn album(&self, id: &str) -> CoreResult<Album> {
        self.request_json(Method::GET, &format!("/albums/{id}"), &[], None, &[])
            .await
    }

    pub async fn album_tracks(
        &self,
        id: &str,
        cursor: Option<&str>,
        limit: Option<i32>,
    ) -> CoreResult<TracksPage> {
        self.paged_get(&format!("/albums/{id}/tracks"), cursor, limit)
            .await
    }

    pub async fn artist(&self, id: &str) -> CoreResult<Artist> {
        self.request_json(Method::GET, &format!("/artists/{id}"), &[], None, &[])
            .await
    }

    pub async fn artist_albums(
        &self,
        id: &str,
        cursor: Option<&str>,
        limit: Option<i32>,
    ) -> CoreResult<AlbumsPage> {
        self.paged_get(&format!("/artists/{id}/albums"), cursor, limit)
            .await
    }

    pub async fn artist_top_tracks(&self, id: &str, limit: Option<i32>) -> CoreResult<TracksPage> {
        self.paged_get(&format!("/artists/{id}/top-tracks"), None, limit)
            .await
    }

    pub async fn artist_tracks(
        &self,
        id: &str,
        cursor: Option<&str>,
        limit: Option<i32>,
    ) -> CoreResult<TracksPage> {
        self.paged_get(&format!("/artists/{id}/tracks"), cursor, limit)
            .await
    }

    pub async fn sync_artist(&self, id: &str) -> CoreResult<ArtistSyncStatus> {
        self.request_json(Method::POST, &format!("/artists/{id}/sync"), &[], None, &[])
            .await
    }

    pub async fn artist_sync_status(&self, id: &str) -> CoreResult<ArtistSyncStatus> {
        self.request_json(Method::GET, &format!("/artists/{id}/sync"), &[], None, &[])
            .await
    }

    // ---- Playlists & Library ----------------------------------------------

    pub async fn playlist(&self, id: &str) -> CoreResult<Playlist> {
        self.request_json(Method::GET, &format!("/playlists/{id}"), &[], None, &[])
            .await
    }

    pub async fn playlists(
        &self,
        owner: &str,
        cursor: Option<&str>,
        limit: Option<i32>,
    ) -> CoreResult<PlaylistsPage> {
        let mut q = vec![("owner", owner)];
        let lim_s;
        if let Some(c) = cursor {
            q.push(("cursor", c));
        }
        if let Some(l) = limit {
            lim_s = l.to_string();
            q.push(("limit", lim_s.as_str()));
        }
        self.request_json(Method::GET, "/playlists", &q, None, &[]).await
    }

    pub async fn library_tracks(
        &self,
        cursor: Option<&str>,
        limit: Option<i32>,
        sort: &str,
        query: Option<&str>,
    ) -> CoreResult<TracksPage> {
        let mut q = vec![("sort", sort)];
        let lim_s;
        if let Some(c) = cursor {
            q.push(("cursor", c));
        }
        if let Some(l) = limit {
            lim_s = l.to_string();
            q.push(("limit", lim_s.as_str()));
        }
        if let Some(qs) = query.map(str::trim).filter(|s| !s.is_empty()) {
            q.push(("q", qs));
        }
        self.request_json(Method::GET, "/library/tracks", &q, None, &[])
            .await
    }

    pub async fn library_playlists(
        &self,
        cursor: Option<&str>,
        limit: Option<i32>,
    ) -> CoreResult<PlaylistsPage> {
        self.paged_get("/library/playlists", cursor, limit).await
    }

    pub async fn library_albums(
        &self,
        cursor: Option<&str>,
        limit: Option<i32>,
    ) -> CoreResult<AlbumsPage> {
        self.paged_get("/library/albums", cursor, limit).await
    }

    pub async fn history(&self, cursor: Option<&str>, limit: Option<i32>) -> CoreResult<HistoryPage> {
        self.paged_get("/history", cursor, limit).await
    }

    pub async fn clear_history(&self) -> CoreResult<()> {
        self.request_unit(Method::DELETE, "/history", &[], None, &[])
            .await
    }

    pub async fn create_playlist(&self, create: &PlaylistCreate) -> CoreResult<Playlist> {
        let idem = Uuid::new_v4().to_string();
        self.request_json(
            Method::POST,
            "/playlists",
            &[],
            Some(Self::to_value(create)?),
            &[("Idempotency-Key", &idem)],
        )
        .await
    }

    pub async fn update_playlist(&self, id: &str, update: &PlaylistUpdate) -> CoreResult<Playlist> {
        self.request_json(
            Method::PATCH,
            &format!("/playlists/{id}"),
            &[],
            Some(Self::to_value(update)?),
            &[],
        )
        .await
    }

    pub async fn delete_playlist(&self, id: &str) -> CoreResult<()> {
        self.request_unit(Method::DELETE, &format!("/playlists/{id}"), &[], None, &[])
            .await
    }

    pub async fn playlist_tracks(
        &self,
        id: &str,
        cursor: Option<&str>,
        limit: Option<i32>,
    ) -> CoreResult<TracksPage> {
        self.paged_get(&format!("/playlists/{id}/tracks"), cursor, limit)
            .await
    }

    pub async fn add_playlist_tracks(
        &self,
        id: &str,
        track_ids: Vec<String>,
        position: Option<i32>,
    ) -> CoreResult<Playlist> {
        let idem = Uuid::new_v4().to_string();
        let body = Self::to_value(&PlaylistTracksAdd { track_ids, position })?;
        self.request_json(
            Method::POST,
            &format!("/playlists/{id}/tracks"),
            &[],
            Some(body),
            &[("Idempotency-Key", &idem)],
        )
        .await
    }

    pub async fn remove_playlist_track(&self, id: &str, track_id: &str) -> CoreResult<()> {
        self.request_unit(
            Method::DELETE,
            &format!("/playlists/{id}/tracks/{track_id}"),
            &[],
            None,
            &[],
        )
        .await
    }

    pub async fn reorder_playlist_tracks(
        &self,
        id: &str,
        track_ids: Vec<String>,
    ) -> CoreResult<Playlist> {
        let body = Self::to_value(&PlaylistReorder { track_ids })?;
        self.request_json(
            Method::PUT,
            &format!("/playlists/{id}/tracks/order"),
            &[],
            Some(body),
            &[],
        )
        .await
    }

    pub async fn like_album(&self, id: &str) -> CoreResult<()> {
        self.request_unit(Method::PUT, &format!("/albums/{id}/like"), &[], None, &[])
            .await
    }

    pub async fn unlike_album(&self, id: &str) -> CoreResult<()> {
        self.request_unit(Method::DELETE, &format!("/albums/{id}/like"), &[], None, &[])
            .await
    }

    pub async fn like_artist(&self, id: &str) -> CoreResult<()> {
        self.request_unit(Method::PUT, &format!("/artists/{id}/like"), &[], None, &[])
            .await
    }

    pub async fn unlike_artist(&self, id: &str) -> CoreResult<()> {
        self.request_unit(Method::DELETE, &format!("/artists/{id}/like"), &[], None, &[])
            .await
    }

    pub async fn like_album_tracks(&self, id: &str) -> CoreResult<LikeAllResult> {
        let idem = Uuid::new_v4().to_string();
        self.request_json(
            Method::PUT,
            &format!("/albums/{id}/tracks/like"),
            &[],
            None,
            &[("Idempotency-Key", &idem)],
        )
        .await
    }

    pub async fn like_playlist_tracks(&self, id: &str) -> CoreResult<LikeAllResult> {
        let idem = Uuid::new_v4().to_string();
        self.request_json(
            Method::PUT,
            &format!("/playlists/{id}/tracks/like"),
            &[],
            None,
            &[("Idempotency-Key", &idem)],
        )
        .await
    }

    pub async fn follow_playlist(&self, id: &str) -> CoreResult<()> {
        self.request_unit(Method::PUT, &format!("/playlists/{id}/follow"), &[], None, &[])
            .await
    }

    pub async fn unfollow_playlist(&self, id: &str) -> CoreResult<()> {
        self.request_unit(Method::DELETE, &format!("/playlists/{id}/follow"), &[], None, &[])
            .await
    }

    // ---- Reactions & Lyrics -----------------------------------------------

    pub async fn like(&self, track_id: &str) -> CoreResult<()> {
        self.request_unit(Method::PUT, &format!("/tracks/{track_id}/like"), &[], None, &[])
            .await
    }

    pub async fn unlike(&self, track_id: &str) -> CoreResult<()> {
        self.request_unit(Method::DELETE, &format!("/tracks/{track_id}/like"), &[], None, &[])
            .await
    }

    pub async fn dislike(&self, track_id: &str) -> CoreResult<()> {
        self.request_unit(Method::PUT, &format!("/tracks/{track_id}/dislike"), &[], None, &[])
            .await
    }

    pub async fn undislike(&self, track_id: &str) -> CoreResult<()> {
        self.request_unit(Method::DELETE, &format!("/tracks/{track_id}/dislike"), &[], None, &[])
            .await
    }

    pub async fn report_censorship(
        &self,
        track_id: &str,
        reason: &str,
        comment: Option<&str>,
        position_ms: Option<u64>,
    ) -> CoreResult<CensorshipReportResponse> {
        let idem = Uuid::new_v4().to_string();
        let body = Self::to_value(&CensorshipReportRequest {
            reason: reason.to_string(),
            comment: comment.map(ToOwned::to_owned),
            position_ms,
        })?;
        self.request_json(
            Method::POST,
            &format!("/tracks/{track_id}/censorship"),
            &[],
            Some(body),
            &[("Idempotency-Key", &idem)],
        )
        .await
    }

    pub async fn cancel_censorship_report(&self, track_id: &str) -> CoreResult<()> {
        self.request_unit(
            Method::DELETE,
            &format!("/tracks/{track_id}/censorship"),
            &[],
            None,
            &[],
        )
        .await
    }

    pub async fn lyrics(&self, track_id: &str) -> CoreResult<Lyrics> {
        self.request_json(Method::GET, &format!("/tracks/{track_id}/lyrics"), &[], None, &[])
            .await
    }

    // ---- Recommendations & Mixes ------------------------------------------

    pub async fn top_artists(&self, limit: Option<i32>) -> CoreResult<ArtistsPage> {
        self.paged_get("/recommendations/top-artists", None, limit)
            .await
    }

    pub async fn genres(&self) -> CoreResult<GenresResponse> {
        self.request_json(Method::GET, "/recommendations/genres", &[], None, &[])
            .await
    }

    pub async fn mixes(
        &self,
        kind: Option<&str>,
        cursor: Option<&str>,
        limit: Option<i32>,
    ) -> CoreResult<MixesPage> {
        let mut q = Vec::new();
        let lim_s;
        if let Some(k) = kind {
            q.push(("kind", k));
        }
        if let Some(c) = cursor {
            q.push(("cursor", c));
        }
        if let Some(l) = limit {
            lim_s = l.to_string();
            q.push(("limit", lim_s.as_str()));
        }
        self.request_json(Method::GET, "/mixes", &q, None, &[]).await
    }

    pub async fn mix_tracks(
        &self,
        id: &str,
        cursor: Option<&str>,
        limit: Option<i32>,
    ) -> CoreResult<TrackRecommendationsPage> {
        self.paged_get(&format!("/mixes/{id}/tracks"), cursor, limit)
            .await
    }

    pub async fn prompt_mix(&self, req: &PromptMixRequest) -> CoreResult<TrackRecommendationsPage> {
        self.request_json(
            Method::POST,
            "/mixes/prompt",
            &[],
            Some(Self::to_value(req)?),
            &[],
        )
        .await
    }

    // ---- Uploads & Enrichments --------------------------------------------

    pub async fn enrichments(&self) -> CoreResult<EnrichmentListPage> {
        self.request_json(Method::GET, "/uploads/enrichments", &[], None, &[])
            .await
    }

    pub async fn confirm_enrichment(&self, id: i64) -> CoreResult<()> {
        self.request_unit(
            Method::POST,
            &format!("/uploads/enrichments/{id}/confirm"),
            &[],
            None,
            &[],
        )
        .await
    }

    pub async fn reject_enrichment(&self, id: i64) -> CoreResult<()> {
        self.request_unit(
            Method::POST,
            &format!("/uploads/enrichments/{id}/reject"),
            &[],
            None,
            &[],
        )
        .await
    }

    pub async fn upload_playlist_cover(&self, id: &str, file: &Path) -> CoreResult<Playlist> {
        let idem = Uuid::new_v4().to_string();
        let mime = mime_guess::from_path(file)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        let bytes = tokio::fs::read(file).await?;
        let filename = file
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("cover.jpg")
            .to_string();
        self.post_multipart(
            &format!("/playlists/{id}/cover"),
            filename,
            mime,
            bytes,
            &[],
            &[("Idempotency-Key", &idem)],
        )
        .await
    }

    pub async fn delete_playlist_cover(&self, id: &str) -> CoreResult<()> {
        self.request_unit(Method::DELETE, &format!("/playlists/{id}/cover"), &[], None, &[])
            .await
    }

    pub async fn upload_track(
        &self,
        file: &Path,
        title: Option<&str>,
        artist: Option<&str>,
        album: Option<&str>,
    ) -> CoreResult<Track> {
        let idem = Uuid::new_v4().to_string();
        let mime = mime_guess::from_path(file)
            .first_or_octet_stream()
            .essence_str()
            .to_string();
        let bytes = tokio::fs::read(file).await?;
        let filename = file
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("upload.mp3")
            .to_string();
        let mut fields = Vec::new();
        if let Some(t) = title.map(str::trim).filter(|s| !s.is_empty()) {
            fields.push(("title", t.to_string()));
        }
        if let Some(a) = artist.map(str::trim).filter(|s| !s.is_empty()) {
            fields.push(("artist", a.to_string()));
        }
        if let Some(al) = album.map(str::trim).filter(|s| !s.is_empty()) {
            fields.push(("album", al.to_string()));
        }
        self.post_multipart(
            "/uploads/tracks",
            filename,
            mime,
            bytes,
            &fields,
            &[("Idempotency-Key", &idem)],
        )
        .await
    }

    async fn post_multipart<T: DeserializeOwned>(
        &self,
        path: &str,
        filename: String,
        mime: String,
        bytes: Vec<u8>,
        fields: &[(&str, String)],
        extra_headers: &[(&str, &str)],
    ) -> CoreResult<T> {
        let url = self.build_url(path, &[])?;
        let session = self.token_store.load();
        let mut resp = self
            .send_multipart_once(
                &url,
                &filename,
                &mime,
                &bytes,
                fields,
                extra_headers,
                session.as_ref().map(|s| s.access_token.as_str()),
            )
            .await?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            if let Some(sess) = session {
                if let Some(refreshed) = self.refresh_session(Some(&sess.access_token)).await {
                    resp = self
                        .send_multipart_once(
                            &url,
                            &filename,
                            &mime,
                            &bytes,
                            fields,
                            extra_headers,
                            Some(&refreshed.access_token),
                        )
                        .await?;
                }
            }
        }
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(CoreError::api(status.as_u16(), &text));
        }
        Ok(serde_json::from_str(&text)?)
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_multipart_once(
        &self,
        url: &Url,
        filename: &str,
        mime: &str,
        bytes: &[u8],
        fields: &[(&str, String)],
        extra_headers: &[(&str, &str)],
        token: Option<&str>,
    ) -> CoreResult<Response> {
        let part = Part::bytes(bytes.to_vec())
            .file_name(filename.to_string())
            .mime_str(mime)
            .map_err(|e| CoreError::Other(e.to_string()))?;
        let mut form = Form::new().part("file", part);
        for (k, v) in fields {
            form = form.text((*k).to_string(), v.clone());
        }
        let mut headers = self.common_headers(extra_headers);
        if let Some(tok) = token {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {tok}")) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        Ok(self
            .client
            .post(url.clone())
            .headers(headers)
            .multipart(form)
            .send()
            .await?)
    }

    // ---- Events, Config, Health, Share -------------------------------------

    pub async fn send_events(&self, events: &[Event]) -> CoreResult<()> {
        if events.is_empty() {
            return Ok(());
        }
        let body = Self::to_value(&EventBatch {
            events: events.to_vec(),
        })?;
        self.request_unit(Method::POST, "/events", &[], Some(body), &[])
            .await
    }

    pub async fn server_config(&self) -> CoreResult<Config> {
        self.request_json(Method::GET, "/config", &[], None, &[]).await
    }

    pub async fn health(&self) -> CoreResult<HealthResponse> {
        self.request_json(Method::GET, "/health", &[], None, &[]).await
    }

    pub async fn create_share(&self, kind: ShareKind, id: &str) -> CoreResult<ShareLink> {
        let body = Self::to_value(&ShareCreateRequest {
            kind,
            id: id.to_string(),
        })?;
        self.request_json(Method::POST, "/share", &[], Some(body), &[])
            .await
    }

    pub async fn share_details(&self, share_id: &str) -> CoreResult<ShareLink> {
        self.request_json(Method::GET, &format!("/share/{share_id}"), &[], None, &[])
            .await
    }

    // ---- Binary media (Images, Downloads, Streaming Range) -----------------

    pub async fn fetch_image_bytes(&self, image_id: &str, size: &str) -> CoreResult<Vec<u8>> {
        let url = self.build_url(&format!("/images/{image_id}"), &[("size", size)])?;
        let session = self.token_store.load();
        let mut headers = self.common_headers(&[]);
        headers.insert(ACCEPT, HeaderValue::from_static("image/*,*/*"));
        if let Some(sess) = &session {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", sess.access_token)) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        let mut resp = self.client.get(url.clone()).headers(headers.clone()).send().await?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            if let Some(sess) = session {
                if let Some(refreshed) = self.refresh_session(Some(&sess.access_token)).await {
                    if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", refreshed.access_token)) {
                        headers.insert(AUTHORIZATION, val);
                    }
                    resp = self.client.get(url).headers(headers).send().await?;
                }
            }
        }
        let status = resp.status();
        if !status.is_success() {
            return Err(CoreError::api(status.as_u16(), ""));
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn download_track_to<F>(
        &self,
        track_id: &str,
        destination: &Path,
        mut on_progress: F,
    ) -> CoreResult<Option<String>>
    where
        F: FnMut(u64, Option<u64>),
    {
        let resp = self
            .send_authenticated(
                Method::GET,
                &format!("/tracks/{track_id}/download"),
                &[],
                None,
                &[("Accept", "*/*")],
            )
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(CoreError::api(status.as_u16(), &text));
        }
        let content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        let total = resp.content_length();
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::File::create(destination).await?;
        let mut stream = resp.bytes_stream();
        let mut downloaded = 0u64;
        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            file.write_all(&bytes).await?;
            downloaded += bytes.len() as u64;
            on_progress(downloaded, total);
        }
        file.flush().await?;
        Ok(content_type)
    }

    /// Opens an authenticated stream response (supports optional HTTP Range header) with 401 refresh.
    pub async fn open_stream(
        &self,
        track_id: &str,
        range_header: Option<&str>,
    ) -> CoreResult<Response> {
        let url = self.build_url(&format!("/tracks/{track_id}/stream"), &[])?;
        let make_req = |token: Option<&str>| {
            let mut headers = self.common_headers(&[("Accept", "*/*")]);
            if let Some(r) = range_header {
                if let Ok(val) = HeaderValue::from_str(r) {
                    headers.insert(RANGE, val);
                }
            }
            if let Some(tok) = token {
                if let Ok(val) = HeaderValue::from_str(&format!("Bearer {tok}")) {
                    headers.insert(AUTHORIZATION, val);
                }
            }
            self.client.get(url.clone()).headers(headers)
        };
        let session = self.token_store.load();
        let mut resp = make_req(session.as_ref().map(|s| s.access_token.as_str()))
            .send()
            .await?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            if let Some(sess) = session {
                if let Some(refreshed) = self.refresh_session(Some(&sess.access_token)).await {
                    resp = make_req(Some(&refreshed.access_token)).send().await?;
                }
            }
        }
        if !resp.status().is_success() {
            let st = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(CoreError::api(st, &text));
        }
        Ok(resp)
    }

    async fn paged_get<T: DeserializeOwned>(
        &self,
        path: &str,
        cursor: Option<&str>,
        limit: Option<i32>,
    ) -> CoreResult<T> {
        let mut q = Vec::new();
        let lim_s;
        if let Some(c) = cursor {
            q.push(("cursor", c));
        }
        if let Some(l) = limit {
            lim_s = l.to_string();
            q.push(("limit", lim_s.as_str()));
        }
        self.request_json(Method::GET, path, &q, None, &[]).await
    }
}

pub fn now_epoch_sec() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn single_flight_refresh_runs_once_and_preserves_session_on_5xx() {
        let server = MockServer::start().await;
        let refresh_calls = Arc::new(AtomicUsize::new(0));
        let rc = refresh_calls.clone();

        Mock::given(method("GET"))
            .and(path("/v1/me"))
            .and(header("authorization", "Bearer old-token"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/me"))
            .and(header("authorization", "Bearer new-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "1",
                "displayName": "Atte",
                "avatar": null
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(move |_req: &wiremock::Request| {
                rc.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "accessToken": "new-token",
                    "refreshToken": "new-refresh",
                    "expiresIn": 900,
                    "tokenType": "Bearer",
                    "user": { "id": "1", "displayName": "Atte", "avatar": null }
                }))
            })
            .mount(&server)
            .await;

        let store = TokenStore::in_memory(Some(Session {
            access_token: "old-token".into(),
            refresh_token: "old-refresh".into(),
            expires_at_epoch_sec: 9999999999,
            user: User {
                id: "1".into(),
                display_name: "Atte".into(),
                email: None,
                avatar: None,
                created_at: None,
            },
        }));
        let cfg = ApiConfig::new(&server.uri(), "0.1.0", "test", true).unwrap();
        let api = KwhaleApi::new(cfg, store.clone()).unwrap();

        let (r1, r2) = tokio::join!(api.me(), api.me());
        assert!(r1.is_ok());
        assert!(r2.is_ok());
        assert_eq!(refresh_calls.load(Ordering::SeqCst), 1);

        // Now test 500 on /auth/refresh does NOT clear session
        let server2 = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server2)
            .await;
        let cfg2 = ApiConfig::new(&server2.uri(), "0.1.0", "test", true).unwrap();
        api.reconfigure(cfg2).unwrap();
        let res = api.refresh_session(None).await;
        assert!(res.is_none());
        assert!(store.load().is_some());
    }
}
