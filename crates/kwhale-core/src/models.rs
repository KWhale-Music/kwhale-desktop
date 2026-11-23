use crate::playback_state::UiTrack;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageRef {
    pub id: String,
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
    #[serde(default)]
    pub blur_hash: Option<String>,
}

impl ImageRef {
    pub fn valid_id(&self) -> Option<&str> {
        image_id_or_null(Some(&self.id))
    }

    pub fn path(&self, size: &str) -> Option<String> {
        self.valid_id().map(|id| format!("/images/{id}?size={size}"))
    }
}

pub fn image_id_or_null(id: Option<&str>) -> Option<&str> {
    id.map(str::trim)
        .filter(|s| !s.is_empty() && *s != "placeholder")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumRef {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub cover: Option<ImageRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRef {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub avatar: Option<ImageRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Genre {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub image: Option<ImageRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub avatar: Option<ImageRef>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserUpdate {
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub invite_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogoutRequest {
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    #[serde(default = "default_token_type")]
    pub token_type: String,
    pub user: User,
}

fn default_token_type() -> String {
    "Bearer".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_epoch_sec: i64,
    pub user: User,
}

impl Session {
    pub fn from_auth(auth: AuthResponse, now_epoch_sec: i64) -> Self {
        Self {
            access_token: auth.access_token,
            refresh_token: auth.refresh_token,
            expires_at_epoch_sec: now_epoch_sec + auth.expires_in.max(60),
            user: auth.user,
        }
    }

    pub fn is_access_token_expired(&self, now_epoch_sec: i64) -> bool {
        now_epoch_sec >= self.expires_at_epoch_sec - 30
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<ArtistRef>,
    #[serde(default)]
    pub album: Option<AlbumRef>,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub explicit: bool,
    #[serde(default)]
    pub cover: Option<ImageRef>,
    #[serde(default)]
    pub genre: Option<Genre>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub track_number: Option<i32>,
    #[serde(default)]
    pub disc_number: Option<i32>,
    #[serde(default)]
    pub is_liked: bool,
    #[serde(default)]
    pub is_disliked: bool,
    #[serde(default)]
    pub has_lyrics: bool,
    #[serde(default)]
    pub is_downloaded: bool,
    #[serde(default = "default_download_status")]
    pub download_status: String,
    #[serde(default)]
    pub preview_available: bool,
    #[serde(default)]
    pub play_count: Option<i64>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub is_censorship_reported: bool,
}

fn default_download_status() -> String {
    "pending".to_string()
}

impl Track {
    pub fn artist_line(&self) -> String {
        if self.artists.is_empty() {
            "Неизвестный артист".to_string()
        } else {
            let joined: Vec<&str> = self
                .artists
                .iter()
                .map(|a| a.name.trim())
                .filter(|s| !s.is_empty())
                .collect();
            if joined.is_empty() {
                "Неизвестный артист".to_string()
            } else {
                joined.join(", ")
            }
        }
    }

    pub fn primary_artist_id(&self) -> Option<&str> {
        self.artists
            .iter()
            .find(|a| !a.id.starts_with("unknown-") && !a.name.trim().is_empty())
            .map(|a| a.id.as_str())
    }

    pub fn valid_artists(&self) -> Vec<&ArtistRef> {
        self.artists
            .iter()
            .filter(|a| !a.id.starts_with("unknown-") && !a.name.trim().is_empty())
            .collect()
    }

    pub fn cover_image_id(&self) -> Option<&str> {
        self.cover
            .as_ref()
            .and_then(|c| c.valid_id())
            .or_else(|| {
                self.album
                    .as_ref()
                    .and_then(|a| a.cover.as_ref())
                    .and_then(|c| c.valid_id())
            })
    }

    pub fn to_ui(&self, is_offline_cached: bool) -> UiTrack {
        UiTrack {
            id: UiTrack::stable_id_for(&self.id),
            remote_id: self.id.clone(),
            title: self.title.clone(),
            artist: self.artist_line(),
            artists: self.artists.iter().map(|a| a.name.clone()).collect(),
            artist_remote_id: self.primary_artist_id().map(ToOwned::to_owned),
            album: self.album.as_ref().map(|a| a.title.clone()),
            album_remote_id: self.album.as_ref().map(|a| a.id.clone()),
            duration_sec: (self.duration_ms / 1000) as u32,
            genre: self.genre.as_ref().map(|g| g.name.clone()),
            year: self.year,
            is_explicit: self.explicit,
            stream_path: format!("/tracks/{}/stream", self.id),
            cover_image_id: self.cover_image_id().map(ToOwned::to_owned),
            has_lyrics: self.has_lyrics,
            is_liked: self.is_liked,
            is_disliked: self.is_disliked,
            is_offline_cached,
            is_censorship_reported: self.is_censorship_reported,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackQualityInfo {
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub bitrate_kbps: Option<i32>,
    #[serde(default)]
    pub sample_rate: Option<i32>,
    #[serde(default)]
    pub bit_depth: Option<i32>,
    #[serde(default)]
    pub channels: Option<i32>,
    #[serde(default)]
    pub lossless: bool,
    #[serde(default)]
    pub tier: i32,
    #[serde(default)]
    pub bytes: Option<u64>,
    #[serde(default)]
    pub measured_duration_ms: Option<u64>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSourceInfo {
    pub connector: String,
    #[serde(default = "default_true")]
    pub available: bool,
    #[serde(default)]
    pub quality_tier: Option<i32>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    pub track: Track,
    #[serde(default)]
    pub isrc: Option<String>,
    #[serde(default)]
    pub quality: Option<TrackQualityInfo>,
    #[serde(default)]
    pub sources: Vec<TrackSourceInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<ArtistRef>,
    #[serde(default)]
    pub cover: Option<ImageRef>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub track_count: i32,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub explicit: bool,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub added_at: Option<String>,
}

impl Album {
    pub fn artist_line(&self) -> String {
        let joined: Vec<&str> = self
            .artists
            .iter()
            .map(|a| a.name.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if joined.is_empty() {
            "Неизвестный артист".to_string()
        } else {
            joined.join(", ")
        }
    }

    pub fn primary_artist_id(&self) -> Option<&str> {
        self.artists
            .iter()
            .find(|a| !a.id.starts_with("unknown-") && !a.name.trim().is_empty())
            .map(|a| a.id.as_str())
    }

    pub fn cover_image_id(&self) -> Option<&str> {
        self.cover.as_ref().and_then(|c| c.valid_id())
    }

    pub fn type_label(&self) -> Option<&'static str> {
        match self.r#type.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("single") => Some("Сингл"),
            Some("ep") => Some("EP"),
            Some("compilation") => Some("Сборник"),
            Some("live") => Some("Концерт"),
            Some("album") => None,
            _ => match self.track_count {
                1..=2 => Some("Сингл"),
                3..=6 => Some("EP"),
                _ => None,
            },
        }
    }

    pub fn is_single_like(&self) -> bool {
        match self.r#type.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("single") | Some("ep") => true,
            Some("album") | Some("compilation") | Some("live") => false,
            _ => (1..=6).contains(&self.track_count),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub avatar: Option<ImageRef>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub genres: Vec<Genre>,
    #[serde(default)]
    pub followers: i64,
    #[serde(default)]
    pub album_count: i32,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub latest_release: Option<Album>,
    #[serde(default)]
    pub top_tracks: Vec<Track>,
    #[serde(default)]
    pub albums: Vec<Album>,
}

impl Artist {
    pub fn avatar_image_id(&self) -> Option<&str> {
        self.avatar.as_ref().and_then(|a| a.valid_id())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistPermissions {
    #[serde(default)]
    pub can_edit: bool,
    #[serde(default)]
    pub can_delete: bool,
    #[serde(default)]
    pub can_add_tracks: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub owner: Option<UserRef>,
    #[serde(default)]
    pub cover: Option<ImageRef>,
    #[serde(default)]
    pub track_count: i32,
    #[serde(default = "default_visibility")]
    pub visibility: String,
    #[serde(default)]
    pub is_system: bool,
    #[serde(default)]
    pub is_followed: bool,
    #[serde(default)]
    pub permissions: PlaylistPermissions,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

fn default_visibility() -> String {
    "private".to_string()
}

impl Playlist {
    pub fn cover_image_id(&self) -> Option<&str> {
        self.cover.as_ref().and_then(|c| c.valid_id())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TracksPage {
    #[serde(default)]
    pub items: Vec<Track>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub total: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumsPage {
    #[serde(default)]
    pub items: Vec<Album>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistsPage {
    #[serde(default)]
    pub items: Vec<Artist>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistsPage {
    #[serde(default)]
    pub items: Vec<Playlist>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub total: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub text: String,
    #[serde(default = "default_suggestion_type")]
    pub r#type: String,
}

fn default_suggestion_type() -> String {
    "query".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestions {
    #[serde(default)]
    pub items: Vec<Suggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CensorshipReportRequest {
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CensorshipReportResponse {
    #[serde(default = "default_true")]
    pub ok: bool,
    #[serde(default)]
    pub report_id: String,
    #[serde(default = "default_pending")]
    pub status: String,
}

fn default_pending() -> String {
    "pending".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRelease {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub version_code: i32,
    #[serde(default)]
    pub version_name: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size_bytes: i64,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchTop {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub track: Option<Track>,
    #[serde(default)]
    pub album: Option<Album>,
    #[serde(default)]
    pub artist: Option<Artist>,
    #[serde(default)]
    pub playlist: Option<Playlist>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub albums: Vec<Album>,
    #[serde(default)]
    pub artists: Vec<Artist>,
    #[serde(default)]
    pub playlists: Vec<Playlist>,
    #[serde(default)]
    pub mixes: Vec<Mix>,
    #[serde(default)]
    pub top: Option<SearchTop>,
    #[serde(default)]
    pub total: i32,
    #[serde(default)]
    pub partial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHistory {
    #[serde(default)]
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsLine {
    pub start_ms: u64,
    pub text: String,
    #[serde(default)]
    pub end_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub track_id: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default)]
    pub plain: Option<String>,
    #[serde(default)]
    pub synced: Vec<LyricsLine>,
    #[serde(default)]
    pub has_timestamps: bool,
}

fn default_provider() -> String {
    "internal".to_string()
}

fn default_lang() -> String {
    "ru".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Banner {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub image: Option<ImageRef>,
    #[serde(default)]
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeAction {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeItem {
    pub r#type: String,
    #[serde(default)]
    pub track: Option<Track>,
    #[serde(default)]
    pub album: Option<Album>,
    #[serde(default)]
    pub artist: Option<Artist>,
    #[serde(default)]
    pub playlist: Option<Playlist>,
    #[serde(default)]
    pub mix: Option<Mix>,
    #[serde(default)]
    pub banner: Option<Banner>,
    #[serde(default)]
    pub action: Option<HomeAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeSection {
    pub id: String,
    pub kind: String,
    #[serde(default = "default_layout")]
    pub layout: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub items: Vec<HomeItem>,
    #[serde(default)]
    pub see_all: bool,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

fn default_layout() -> String {
    "list".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeResponse {
    #[serde(default)]
    pub sections: Vec<HomeSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistSyncStatus {
    #[serde(default)]
    pub artist_id: String,
    #[serde(default = "default_idle")]
    pub status: String,
    #[serde(default)]
    pub track_count: i32,
    #[serde(default)]
    pub album_count: i32,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

fn default_idle() -> String {
    "idle".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistCreate {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub visibility: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTracksAdd {
    pub track_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistReorder {
    pub track_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeAllResult {
    #[serde(default)]
    pub requested: i32,
    #[serde(default)]
    pub liked: i32,
    #[serde(default)]
    pub already_liked: i32,
    #[serde(default)]
    pub queued: i32,
    #[serde(default)]
    pub failed: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    #[serde(default = "default_ok")]
    pub status: String,
}

fn default_ok() -> String {
    "ok".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub track: Track,
    pub played_at: String,
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    #[serde(default)]
    pub items: Vec<HistoryEntry>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub total: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenresResponse {
    #[serde(default)]
    pub items: Vec<Genre>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedMetadata {
    #[serde(default)]
    pub isrc: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentProposal {
    pub id: i64,
    #[serde(default)]
    pub track_id: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artists: Vec<String>,
    #[serde(default)]
    pub proposed: ProposedMetadata,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentListPage {
    #[serde(default)]
    pub items: Vec<EnrichmentProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedRef {
    pub r#type: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mix {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub cover: Option<ImageRef>,
    #[serde(default = "default_mix_kind")]
    pub kind: String,
    #[serde(default)]
    pub seed: Option<SeedRef>,
    #[serde(default)]
    pub track_count: i32,
    #[serde(default)]
    pub is_followed: bool,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

fn default_mix_kind() -> String {
    "personal".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRecommendation {
    pub track: Track,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub reason_code: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRecommendationsPage {
    #[serde(default)]
    pub items: Vec<TrackRecommendation>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MixesPage {
    #[serde(default)]
    pub items: Vec<Mix>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptMixRequest {
    pub prompt: String,
    pub size: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Onboarding {
    #[serde(default)]
    pub genres: Vec<Genre>,
    #[serde(default)]
    pub artists: Vec<ArtistRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mix_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<EventContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listened_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone_offset_minutes: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clicked_position: Option<i32>,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventBatch {
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFeatures {
    #[serde(default)]
    pub recommendations: bool,
    #[serde(default)]
    pub lyrics: bool,
    #[serde(default)]
    pub downloads: bool,
    #[serde(default = "default_true")]
    pub playlists: bool,
    #[serde(default)]
    pub offline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigLimits {
    #[serde(default = "default_max_playlist_tracks")]
    pub max_playlist_tracks: i32,
    #[serde(default = "default_max_upload_bytes")]
    pub max_upload_bytes: i64,
    #[serde(default = "default_event_batch")]
    pub event_batch_size: i32,
}

fn default_max_playlist_tracks() -> i32 {
    5000
}

fn default_max_upload_bytes() -> i64 {
    10 * 1024 * 1024
}

fn default_event_batch() -> i32 {
    100
}

impl Default for ConfigLimits {
    fn default() -> Self {
        Self {
            max_playlist_tracks: default_max_playlist_tracks(),
            max_upload_bytes: default_max_upload_bytes(),
            event_batch_size: default_event_batch(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaConfig {
    #[serde(default)]
    pub supported_formats: Vec<String>,
    #[serde(default = "default_chunk_size")]
    pub stream_chunk_size: i64,
}

fn default_chunk_size() -> i64 {
    256 * 1024
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            supported_formats: Vec::new(),
            stream_chunk_size: default_chunk_size(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub api_version: String,
    #[serde(default)]
    pub min_client_version: String,
    #[serde(default)]
    pub features: ConfigFeatures,
    #[serde(default)]
    pub limits: ConfigLimits,
    #[serde(default)]
    pub media: MediaConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShareKind {
    Track,
    Album,
    Artist,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePlatformLink {
    pub platform: String,
    pub label: String,
    pub url: String,
    pub go_url: String,
    #[serde(default)]
    pub exact: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareCreateRequest {
    pub kind: ShareKind,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareLink {
    pub id: String,
    pub kind: ShareKind,
    pub entity_id: String,
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub meta: Option<String>,
    #[serde(default = "default_placeholder")]
    pub image_id: String,
    pub share_url: String,
    pub card_url: String,
    pub cover_url: String,
    pub deep_link: String,
    pub expires_at: String,
    #[serde(default = "default_ttl_days")]
    pub ttl_days: i32,
    #[serde(default)]
    pub clicks_total: i64,
    #[serde(default)]
    pub platforms: Vec<SharePlatformLink>,
}

fn default_placeholder() -> String {
    "placeholder".to_string()
}

fn default_ttl_days() -> i32 {
    7
}

/// Formatting and Russian pluralization helpers matching KwhaleDesign.kt & KwhaleComponents.kt.
pub struct KwhalePlural;

impl KwhalePlural {
    pub fn tracks(n: i32) -> String {
        format!("{} {}", Self::formatted(n), Self::form(n, "трек", "трека", "треков"))
    }

    pub fn playlists(n: i32) -> String {
        format!(
            "{} {}",
            Self::formatted(n),
            Self::form(n, "плейлист", "плейлиста", "плейлистов")
        )
    }

    pub fn albums(n: i32) -> String {
        format!(
            "{} {}",
            Self::formatted(n),
            Self::form(n, "альбом", "альбома", "альбомов")
        )
    }

    pub fn artists(n: i32) -> String {
        format!(
            "{} {}",
            Self::formatted(n),
            Self::form(n, "артист", "артиста", "артистов")
        )
    }

    pub fn listeners(n: i32) -> String {
        format!(
            "{} {}",
            Self::formatted(n),
            Self::form(n, "слушатель", "слушателя", "слушателей")
        )
    }

    pub fn playlist_word(n: i32) -> &'static str {
        Self::form(n, "плейлист", "плейлиста", "плейлистов")
    }

    pub fn formatted(n: i32) -> String {
        if n >= 1_000_000 {
            format!("{:.1} млн", n as f32 / 1_000_000.0)
        } else if n >= 10_000 {
            format!("{:.0} тыс.", n as f32 / 1_000.0)
        } else if n >= 1_000 {
            format!("{:.1} тыс.", n as f32 / 1_000.0)
        } else {
            n.to_string()
        }
    }

    fn form(n: i32, one: &'static str, few: &'static str, many: &'static str) -> &'static str {
        let abs = n.abs();
        let mod100 = abs % 100;
        let mod10 = abs % 10;
        if (11..=14).contains(&mod100) {
            many
        } else if mod10 == 1 {
            one
        } else if (2..=4).contains(&mod10) {
            few
        } else {
            many
        }
    }
}

pub fn format_duration_ms(ms: u64) -> String {
    let total_sec = ms / 1000;
    format!("{}:{:02}", total_sec / 60, total_sec % 60)
}

pub fn format_duration_player_ms(ms: u64) -> String {
    let total_sec = ms / 1000;
    format!("{:02}:{:02}", total_sec / 60, total_sec % 60)
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} ГБ", b / GB)
    } else if b >= MB {
        format!("{:.1} МБ", b / MB)
    } else if b >= KB {
        format!("{:.0} КБ", b / KB)
    } else {
        format!("{bytes} Б")
    }
}

pub fn greeting_for_hour(hour: u32) -> &'static str {
    match hour {
        5..=11 => "Доброе утро",
        12..=16 => "Добрый день",
        17..=22 => "Добрый вечер",
        _ => "Доброй ночи",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposed_metadata_uses_camel_case_on_the_wire() {
        let json = r#"{"isrc":"USRC123","album":"X","year":2020,"genre":"Rock","coverUrl":"http://c/img"}"#;
        let parsed: ProposedMetadata = serde_json::from_str(json).expect("deserializes");
        // coverUrl (camelCase) must map to cover_url; without the rename it silently None.
        assert_eq!(parsed.cover_url.as_deref(), Some("http://c/img"));

        let out = serde_json::to_value(&parsed).expect("serializes");
        assert_eq!(out.get("coverUrl"), Some(&serde_json::json!("http://c/img")));
    }
}
