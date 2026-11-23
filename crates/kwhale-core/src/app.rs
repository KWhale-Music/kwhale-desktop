use crate::downloads::DownloadManager;
use crate::error::CoreResult;
use crate::events::EventTracker;
use crate::models::{Album, Artist, HomeResponse, Mix, Playlist, SearchResponse, Track, User};
use crate::network::{ApiConfig, KwhaleApi};
use crate::reactions::TrackReactionStore;
use crate::server_url::ServerUrl;
use crate::storage::{
    AppPaths, ContentCache, ImageCache, PlaybackCache, SettingsStore, TokenStore,
};
use crate::theme::{KwhaleCoverPalette, ThemeMode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::sync::watch;

pub struct HomeSnapshot {
    pub home: Option<HomeResponse>,
    pub mixes: Vec<Mix>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,
    pub albums: Vec<Album>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBootstrapSnapshot {
    pub history: Vec<String>,
    pub genres: Vec<crate::models::Genre>,
    pub artists: Vec<Artist>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSnapshot {
    pub name: String,
    pub email: String,
    pub avatar_image_id: Option<String>,
    pub track_count: i32,
    pub playlist_count: i32,
    pub download_count: usize,
    pub enrichments: Vec<crate::models::EnrichmentProposal>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSnapshot {
    pub title: String,
    pub subtitle: String,
    pub meta: String,
    pub cover_image_id: Option<String>,
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub latest_release: Option<Album>,
    pub can_edit: bool,
    pub is_favorite: bool,
    pub is_followed: bool,
    pub is_owned: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SearchSessionState {
    pub query: String,
    pub selected_type: String,
    pub results: Option<SearchResponse>,
    pub results_type: String,
}

#[derive(Clone)]
pub struct KwhaleCore {
    pub paths: AppPaths,
    pub settings: SettingsStore,
    pub token_store: TokenStore,
    pub api: KwhaleApi,
    pub cache: ContentCache,
    pub image_cache: ImageCache,
    pub playback_cache: PlaybackCache,
    pub events: EventTracker,
    pub reactions: TrackReactionStore,
    pub downloads: DownloadManager,
    pub search_session: Arc<RwLock<SearchSessionState>>,
    pub client_version: String,
    theme_tx: watch::Sender<(ThemeMode, i32, bool)>,
    auth_tx: watch::Sender<Option<User>>,
}

impl KwhaleCore {
    pub fn init(client_version: impl Into<String>) -> CoreResult<Arc<Self>> {
        let paths = AppPaths::resolve();
        Self::init_with_paths(paths, client_version)
    }

    pub fn init_with_paths(paths: AppPaths, client_version: impl Into<String>) -> CoreResult<Arc<Self>> {
        let client_version = client_version.into();
        let settings = SettingsStore::load(&paths);
        let snap = settings.snapshot();
        let token_store = TokenStore::new(&paths);
        let initial_user = token_store.load().map(|s| s.user);
        let config = ApiConfig::new(
            &snap.api_base_url,
            &client_version,
            &snap.device_id,
            snap.allow_insecure_http,
        )
        // Recover from a poisoned/invalid stored URL instead of refusing to start.
        .or_else(|_| {
            ApiConfig::new(ServerUrl::DEFAULT_PROD, &client_version, &snap.device_id, false)
        })?;
        let api = KwhaleApi::new(config, token_store.clone())?;
        let cache = ContentCache::new(&paths);
        let image_cache = ImageCache::new(&paths);
        let playback_cache = PlaybackCache::new(&paths);
        let events = EventTracker::new(api.clone(), &paths);
        let reactions = TrackReactionStore::new(api.clone(), events.clone());
        let downloads = DownloadManager::new(api.clone(), &paths);

        let (theme_tx, _) = watch::channel((
            snap.theme_mode,
            snap.accent_id,
            snap.follow_system_accent,
        ));
        let (auth_tx, _) = watch::channel(initial_user);

        Ok(Arc::new(Self {
            paths,
            settings,
            token_store,
            api,
            cache,
            image_cache,
            playback_cache,
            events,
            reactions,
            downloads,
            search_session: Arc::new(RwLock::new(SearchSessionState {
                selected_type: "all".to_string(),
                results_type: "all".to_string(),
                ..Default::default()
            })),
            client_version,
            theme_tx,
            auth_tx,
        }))
    }

    pub fn reconfigure_api(&self, new_base_url: &str, allow_insecure_http: bool) -> CoreResult<()> {
        // Validate before persisting so an invalid URL is never stored (a persisted bad
        // URL would otherwise brick the next launch).
        let device_id = self.settings.snapshot().device_id;
        let cfg = ApiConfig::new(new_base_url, &self.client_version, &device_id, allow_insecure_http)?;
        self.settings.update(|s| {
            s.api_base_url = new_base_url.to_string();
            s.allow_insecure_http = allow_insecure_http;
        });
        self.api.reconfigure(cfg)?;
        Ok(())
    }

    pub fn subscribe_theme(&self) -> watch::Receiver<(ThemeMode, i32, bool)> {
        self.theme_tx.subscribe()
    }

    pub fn update_theme(&self, mode: ThemeMode, accent_id: i32, follow_system_accent: bool) {
        self.settings.update(|s| {
            s.theme_mode = mode;
            s.accent_id = accent_id;
            s.follow_system_accent = follow_system_accent;
        });
        let _ = self.theme_tx.send((mode, accent_id, follow_system_accent));
    }

    pub fn subscribe_auth(&self) -> watch::Receiver<Option<User>> {
        self.auth_tx.subscribe()
    }

    pub fn notify_auth_user(&self, user: Option<User>) {
        let _ = self.auth_tx.send(user);
    }

    /// Returns cached or freshly downloaded cover bytes (and path on disk for MPRIS/GTK/Qt).
    pub async fn get_or_fetch_image(&self, image_id: &str, size: &str) -> CoreResult<(PathBuf, Vec<u8>)> {
        let path = self.image_cache.path_for(image_id, size);
        if let Some(bytes) = self.image_cache.get(image_id, size) {
            return Ok((path, bytes));
        }
        let bytes = self.api.fetch_image_bytes(image_id, size).await?;
        let saved = self.image_cache.put(image_id, size, &bytes)?;
        Ok((saved, bytes))
    }

    pub fn placeholder_svg(&self, seed: &str, dark: bool, show_initial: bool, size: u32) -> String {
        KwhaleCoverPalette::placeholder_svg(seed, dark, show_initial, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconfigure_rejects_invalid_url_without_persisting() {
        let dir = tempfile::tempdir().unwrap();
        let core = KwhaleCore::init_with_paths(AppPaths::for_base(dir.path()), "0.1.0").unwrap();
        let before = core.settings.snapshot().api_base_url;

        // Plain HTTP to a public host with insecure disallowed must be rejected.
        let res = core.reconfigure_api("http://music.example.org", false);
        assert!(res.is_err());
        // Settings must be unchanged: a rejected URL is never persisted.
        assert_eq!(core.settings.snapshot().api_base_url, before);
    }

    #[test]
    fn init_recovers_from_poisoned_server_url() {
        let dir = tempfile::tempdir().unwrap();
        let core = KwhaleCore::init_with_paths(AppPaths::for_base(dir.path()), "0.1.0").unwrap();
        // Simulate a poisoned stored URL reaching disk.
        core.settings
            .update(|s| s.api_base_url = "ftp://not-a-server".to_string());
        drop(core);

        // Re-init must not fail even though the stored URL is invalid.
        let re = KwhaleCore::init_with_paths(AppPaths::for_base(dir.path()), "0.1.0");
        assert!(re.is_ok(), "init must recover from an invalid stored URL");
    }
}
