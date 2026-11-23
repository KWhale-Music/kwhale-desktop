#![allow(clippy::collapsible_if)]

pub mod app;
pub mod deep_link;
pub mod downloads;
pub mod error;
pub mod events;
pub mod models;
pub mod network;
pub mod playback_state;
pub mod reactions;
pub mod server_url;
pub mod storage;
pub mod theme;

pub use app::{KwhaleCore, SearchSessionState};
pub use deep_link::DeepLink;
pub use downloads::{DownloadManager, DownloadSnapshot, DownloadedTrack, TrackDownloadStatus};
pub use error::{CoreError, CoreResult, Problem};
pub use events::EventTracker;
pub use models::*;
pub use network::{ApiConfig, KwhaleApi};
pub use playback_state::{PlaybackState, RepeatMode, UiTrack};
pub use reactions::{ReactionOverrides, TrackReactionStore};
pub use server_url::ServerUrl;
pub use storage::{
    file_uri, sanitize_key, AppPaths, AppSettings, ContentCache, ImageCache, PlaybackCache,
    SettingsStore, TokenStore,
};
pub use theme::{
    accent_by_id, accent_families, AccentFamily, KwhaleAccent, KwhaleCoverPalette, KwhalePalette,
    Rgb, ThemeMode, UiBackend, ALL_ACCENTS,
};
