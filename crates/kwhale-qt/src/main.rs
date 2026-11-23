#![allow(clippy::collapsible_if, clippy::too_many_arguments, clippy::type_complexity)]

use chrono::Timelike;
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};
use kwhale_audio::{spawn_mpris, spawn_single_instance_dbus, AudioEngine};
use kwhale_core::{
    accent_by_id, accent_families, format_bytes, format_duration_ms, format_duration_player_ms,
    greeting_for_hour, image_id_or_null, DeepLink, KwhaleCore, KwhalePalette, KwhalePlural,
    Lyrics, PlaylistCreate, PlaylistUpdate, PromptMixRequest, RepeatMode, ServerUrl, ShareKind,
    ThemeMode, TrackDownloadStatus, UiBackend, UiTrack, ALL_ACCENTS,
};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

struct GlobalBackend {
    rt: tokio::runtime::Runtime,
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    state: Arc<Mutex<SharedUiState>>,
}

struct SharedUiState {
    dirty: bool,
    toast_msg: String,
    toast_seq: i32,
    server_status: String,
    active_page: String,
    prev_page: String,
    home_json: String,
    search_partial: bool,
    search_json: String,
    library_json: String,
    sidebar_playlists_json: String,
    collection_kind: String,
    collection_id: String,
    collection_can_edit: bool,
    collection_is_fav: bool,
    collection_track_ids: Vec<String>,
    collection_json: String,
    downloads_json: String,
    history_json: String,
    profile_json: String,
    track_info_json: String,
    share_dialog_json: String,
    lyrics_track_id: Option<String>,
    lyrics: Option<Lyrics>,
    lyrics_json: String,
    tracks_by_id: HashMap<String, UiTrack>,
    context_queues: HashMap<String, Vec<UiTrack>>,
    pending_images: HashSet<String>,
    pending_deep_links: Vec<DeepLink>,
    raise_requested: bool,
}

impl Default for SharedUiState {
    fn default() -> Self {
        Self {
            dirty: true,
            toast_msg: String::new(),
            toast_seq: 0,
            server_status: "Готов к проверке".into(),
            active_page: "home".into(),
            prev_page: "home".into(),
            home_json: r#"{"mixes":[],"sections":[]}"#.into(),
            search_partial: false,
            search_json: r#"{"history":[],"genres":[],"topArtists":[],"tracks":[],"albums":[],"artists":[],"playlists":[]}"#.into(),
            library_json: r#"{"tracks":[],"playlists":[],"albums":[]}"#.into(),
            sidebar_playlists_json: "[]".into(),
            collection_kind: String::new(),
            collection_id: String::new(),
            collection_can_edit: false,
            collection_is_fav: false,
            collection_track_ids: Vec::new(),
            collection_json: "{}".into(),
            downloads_json: r#"{"active":[],"downloaded":[]}"#.into(),
            history_json: "[]".into(),
            profile_json: r#"{"name":"","email":"","enrichments":[]}"#.into(),
            track_info_json: "{}".into(),
            share_dialog_json: "{}".into(),
            lyrics_track_id: None,
            lyrics: None,
            lyrics_json: r#"{"synced":[],"plain":""}"#.into(),
            tracks_by_id: HashMap::new(),
            context_queues: HashMap::new(),
            pending_images: HashSet::new(),
            pending_deep_links: Vec::new(),
            raise_requested: false,
        }
    }
}

static BACKEND: OnceLock<GlobalBackend> = OnceLock::new();

fn backend() -> &'static GlobalBackend {
    BACKEND.get().expect("Backend initialized in main()")
}

fn push_toast(state: &Arc<Mutex<SharedUiState>>, msg: impl Into<String>) {
    let mut g = state.lock().expect("ui lock");
    g.toast_msg = msg.into();
    g.toast_seq += 1;
    g.dirty = true;
}

fn svg_data_uri(svg: &str) -> String {
    let mut out = String::from("data:image/svg+xml;utf8,");
    for b in svg.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b':'
            | b'/'
            | b'='
            | b' ' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn cover_url_for(
    core: &Arc<KwhaleCore>,
    rt: &tokio::runtime::Handle,
    state: &Arc<Mutex<SharedUiState>>,
    image_id: Option<&str>,
    seed: &str,
    tier: &str,
) -> String {
    if let Some(id) = image_id_or_null(image_id) {
        let path = core.image_cache.path_for(id, tier);
        if path.is_file() {
            return format!("file://{}", path.to_string_lossy());
        }
        let key = format!("{id}:{tier}");
        let should_fetch = {
            let mut g = state.lock().expect("ui lock");
            g.pending_images.insert(key.clone())
        };
        if should_fetch {
            let c = core.clone();
            let st = state.clone();
            let id_owned = id.to_string();
            let tier_owned = tier.to_string();
            rt.spawn(async move {
                if c.get_or_fetch_image(&id_owned, &tier_owned).await.is_ok() {
                    let mut g = st.lock().expect("ui lock");
                    g.dirty = true;
                }
            });
        }
    }
    let dark = core.settings.snapshot().theme_mode != ThemeMode::Light;
    let svg = core.placeholder_svg(seed, dark, true, 160);
    svg_data_uri(&svg)
}

fn ui_track_to_json(
    core: &Arc<KwhaleCore>,
    rt: &tokio::runtime::Handle,
    state: &Arc<Mutex<SharedUiState>>,
    ut: &UiTrack,
    index: usize,
) -> serde_json::Value {
    let is_liked = core.reactions.is_liked(&ut.remote_id, ut.is_liked);
    let is_disliked = core.reactions.is_disliked(&ut.remote_id, ut.is_disliked);
    let is_reported = core
        .reactions
        .is_censorship_reported(&ut.remote_id, ut.is_censorship_reported);
    let is_downloaded = core.downloads.is_downloaded(&ut.remote_id);
    let cover = cover_url_for(
        core,
        rt,
        state,
        ut.cover_image_id.as_deref(),
        &ut.title,
        "sm",
    );
    json!({
        "index": index,
        "id": ut.remote_id,
        "title": ut.title,
        "artist": ut.artist,
        "artistId": ut.artist_remote_id.clone().unwrap_or_default(),
        "album": ut.album.clone().unwrap_or_default(),
        "albumId": ut.album_remote_id.clone().unwrap_or_default(),
        "durationSec": ut.duration_sec,
        "durationText": format_duration_ms((ut.duration_sec as u64) * 1000),
        "explicit": ut.is_explicit,
        "coverUrl": cover,
        "isLiked": is_liked,
        "isDisliked": is_disliked,
        "isCensorshipReported": is_reported,
        "isDownloaded": is_downloaded,
    })
}

fn register_tracks(
    state: &Arc<Mutex<SharedUiState>>,
    context_key: &str,
    tracks: &[UiTrack],
) {
    let mut g = state.lock().expect("ui lock");
    for t in tracks {
        g.tracks_by_id.insert(t.remote_id.clone(), t.clone());
    }
    g.context_queues
        .insert(context_key.to_string(), tracks.to_vec());
}

fn build_accents_catalog_json() -> String {
    let mut families = Vec::new();
    for fam in accent_families() {
        let items: Vec<_> = fam
            .accents
            .iter()
            .map(|a| {
                json!({
                    "id": a.id,
                    "name": a.name,
                    "hex": a.rgb().to_hex(),
                })
            })
            .collect();
        families.push(json!({
            "title": fam.title,
            "items": items,
        }));
    }
    serde_json::to_string(&families).unwrap_or_else(|_| "[]".into())
}

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // Theme & Palette
        #[qproperty(QString, theme_mode)]
        #[qproperty(i32, accent_id)]
        #[qproperty(bool, follow_system_accent)]
        #[qproperty(QString, ui_backend)]
        #[qproperty(bool, is_dark)]
        #[qproperty(bool, is_amoled)]
        #[qproperty(QString, color_primary)]
        #[qproperty(QString, color_on_primary)]
        #[qproperty(QString, color_primary_container)]
        #[qproperty(QString, color_on_primary_container)]
        #[qproperty(QString, color_secondary_container)]
        #[qproperty(QString, color_surface)]
        #[qproperty(QString, color_surface_low)]
        #[qproperty(QString, color_surface_container)]
        #[qproperty(QString, color_surface_high)]
        #[qproperty(QString, color_on_surface)]
        #[qproperty(QString, color_on_surface_variant)]
        #[qproperty(QString, color_outline_variant)]
        #[qproperty(QString, color_error)]
        #[qproperty(QString, accents_json)]
        // Auth & Server & Settings
        #[qproperty(bool, authenticated)]
        #[qproperty(QString, user_name)]
        #[qproperty(QString, user_email)]
        #[qproperty(QString, greeting)]
        #[qproperty(QString, server_url)]
        #[qproperty(bool, allow_insecure_http)]
        #[qproperty(QString, server_status)]
        #[qproperty(QString, llm_endpoint)]
        #[qproperty(QString, llm_api_key)]
        #[qproperty(QString, llm_model)]
        #[qproperty(bool, prefetch_enabled)]
        #[qproperty(QString, cache_used_label)]
        // Navigation & UI state
        #[qproperty(QString, active_page)]
        #[qproperty(bool, can_go_back)]
        #[qproperty(bool, inspector_open)]
        #[qproperty(QString, inspector_tab)]
        #[qproperty(bool, now_playing_open)]
        #[qproperty(QString, toast_message)]
        #[qproperty(i32, toast_seq)]
        // Playback, Queue & Lyrics
        #[qproperty(bool, is_playing)]
        #[qproperty(bool, is_buffering)]
        #[qproperty(bool, shuffle)]
        #[qproperty(QString, repeat_mode)]
        #[qproperty(i32, progress_permille)]
        #[qproperty(QString, position_text)]
        #[qproperty(QString, duration_text)]
        #[qproperty(f64, volume)]
        #[qproperty(bool, muted)]
        #[qproperty(QString, current_track_id)]
        #[qproperty(QString, current_title)]
        #[qproperty(QString, current_artist)]
        #[qproperty(QString, current_artist_id)]
        #[qproperty(QString, current_album)]
        #[qproperty(QString, current_album_id)]
        #[qproperty(QString, current_cover_url)]
        #[qproperty(bool, current_liked)]
        #[qproperty(bool, current_disliked)]
        #[qproperty(bool, current_censorship_reported)]
        #[qproperty(bool, current_downloaded)]
        #[qproperty(QString, queue_json)]
        #[qproperty(QString, lyrics_json)]
        #[qproperty(i32, active_lyric_index)]
        // Screen JSON Models
        #[qproperty(QString, home_json)]
        #[qproperty(bool, search_partial)]
        #[qproperty(QString, search_json)]
        #[qproperty(QString, library_json)]
        #[qproperty(QString, sidebar_playlists_json)]
        #[qproperty(QString, collection_json)]
        #[qproperty(QString, downloads_json)]
        #[qproperty(QString, history_json)]
        #[qproperty(QString, profile_json)]
        #[qproperty(QString, track_info_json)]
        #[qproperty(QString, share_dialog_json)]
        type KwhaleBridge = super::KwhaleBridgeRust;

        #[qinvokable]
        fn init(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn poll_updates(self: Pin<&mut KwhaleBridge>);

        // Auth & Server
        #[qinvokable]
        fn login(self: Pin<&mut KwhaleBridge>, url: QString, allow_http: bool, email: QString, password: QString);
        #[qinvokable]
        fn register_account(
            self: Pin<&mut KwhaleBridge>,
            url: QString,
            allow_http: bool,
            name: QString,
            email: QString,
            password: QString,
            invite_token: QString,
        );
        #[qinvokable]
        fn logout(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn save_server(self: Pin<&mut KwhaleBridge>, url: QString, allow_http: bool);
        #[qinvokable]
        fn check_server_health(self: Pin<&mut KwhaleBridge>);

        // Navigation
        #[qinvokable]
        fn navigate(self: Pin<&mut KwhaleBridge>, page: QString);
        #[qinvokable]
        fn navigate_back(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn toggle_inspector(self: Pin<&mut KwhaleBridge>, tab: QString);
        #[qinvokable]
        fn toggle_now_playing(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn open_album(self: Pin<&mut KwhaleBridge>, album_id: QString);
        #[qinvokable]
        fn open_artist(self: Pin<&mut KwhaleBridge>, artist_id: QString);
        #[qinvokable]
        fn open_artist_all_tracks(self: Pin<&mut KwhaleBridge>, artist_id: QString, artist_name: QString);
        #[qinvokable]
        fn open_playlist(self: Pin<&mut KwhaleBridge>, playlist_id: QString);
        #[qinvokable]
        fn open_mix(self: Pin<&mut KwhaleBridge>, mix_id: QString);
        #[qinvokable]
        fn open_liked_tracks(self: Pin<&mut KwhaleBridge>);

        // Playback & Queue & Lyrics
        #[qinvokable]
        fn play_pause(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn next_track(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn previous_track(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn seek_permille(self: Pin<&mut KwhaleBridge>, permille: i32);
        #[qinvokable]
        fn seek_ms(self: Pin<&mut KwhaleBridge>, ms: i64);
        #[qinvokable]
        fn seek_relative_ms(self: Pin<&mut KwhaleBridge>, delta_ms: i64);
        #[qinvokable]
        fn set_volume_level(self: Pin<&mut KwhaleBridge>, vol: f64);
        #[qinvokable]
        fn toggle_mute(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn toggle_shuffle(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn cycle_repeat(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn jump_to_queue(self: Pin<&mut KwhaleBridge>, index: i32);
        #[qinvokable]
        fn move_queue_item(self: Pin<&mut KwhaleBridge>, from_idx: i32, to_idx: i32);
        #[qinvokable]
        fn remove_queue_item(self: Pin<&mut KwhaleBridge>, index: i32);
        #[qinvokable]
        fn play_personal_wave(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn play_prompt_mix(self: Pin<&mut KwhaleBridge>, prompt: QString, size: i32);

        // Track & Context Actions
        #[qinvokable]
        fn play_context_track(self: Pin<&mut KwhaleBridge>, context_key: QString, index: i32, surface: QString);
        #[qinvokable]
        fn play_collection_all(self: Pin<&mut KwhaleBridge>, shuffle: bool);
        #[qinvokable]
        fn play_track_now(self: Pin<&mut KwhaleBridge>, track_id: QString, surface: QString);
        #[qinvokable]
        fn play_track_next(self: Pin<&mut KwhaleBridge>, track_id: QString, surface: QString);
        #[qinvokable]
        fn add_track_to_queue(self: Pin<&mut KwhaleBridge>, track_id: QString, surface: QString);
        #[qinvokable]
        fn start_track_radio(self: Pin<&mut KwhaleBridge>, track_id: QString);
        #[qinvokable]
        fn toggle_track_like(self: Pin<&mut KwhaleBridge>, track_id: QString, surface: QString);
        #[qinvokable]
        fn toggle_track_dislike(self: Pin<&mut KwhaleBridge>, track_id: QString, surface: QString);
        #[qinvokable]
        fn report_track_censorship(self: Pin<&mut KwhaleBridge>, track_id: QString);
        #[qinvokable]
        fn cancel_track_censorship(self: Pin<&mut KwhaleBridge>, track_id: QString);
        #[qinvokable]
        fn download_track(self: Pin<&mut KwhaleBridge>, track_id: QString);
        #[qinvokable]
        fn remove_downloaded_track(self: Pin<&mut KwhaleBridge>, track_id: QString);
        #[qinvokable]
        fn cancel_active_download(self: Pin<&mut KwhaleBridge>, track_id: QString);
        #[qinvokable]
        fn request_track_info(self: Pin<&mut KwhaleBridge>, track_id: QString);
        #[qinvokable]
        fn request_share(self: Pin<&mut KwhaleBridge>, kind: QString, entity_id: QString, title: QString);

        // Collection & Playlist Management & Upload
        #[qinvokable]
        fn toggle_collection_favorite(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn like_all_collection_tracks(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn sync_current_artist(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn create_playlist(
            self: Pin<&mut KwhaleBridge>,
            title: QString,
            desc: QString,
            is_public: bool,
            initial_track_id: QString,
        );
        #[qinvokable]
        fn update_current_playlist(
            self: Pin<&mut KwhaleBridge>,
            title: QString,
            desc: QString,
            is_public: bool,
        );
        #[qinvokable]
        fn delete_current_playlist(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn add_track_to_playlist(self: Pin<&mut KwhaleBridge>, playlist_id: QString, track_id: QString);
        #[qinvokable]
        fn remove_track_from_current_playlist(self: Pin<&mut KwhaleBridge>, track_id: QString);
        #[qinvokable]
        fn move_current_playlist_track(self: Pin<&mut KwhaleBridge>, index: i32, delta: i32);
        #[qinvokable]
        fn upload_current_playlist_cover(self: Pin<&mut KwhaleBridge>, file_url: QString);
        #[qinvokable]
        fn upload_track_file(
            self: Pin<&mut KwhaleBridge>,
            file_url: QString,
            title: QString,
            artist: QString,
            album: QString,
        );

        // Search, History, Profile, Appearance, Settings
        #[qinvokable]
        fn run_search(self: Pin<&mut KwhaleBridge>, query: QString, filter_type: QString);
        #[qinvokable]
        fn clear_search_history(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn clear_play_history(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn update_profile_name(self: Pin<&mut KwhaleBridge>, display_name: QString);
        #[qinvokable]
        fn confirm_enrichment(self: Pin<&mut KwhaleBridge>, proposal_id: i64);
        #[qinvokable]
        fn reject_enrichment(self: Pin<&mut KwhaleBridge>, proposal_id: i64);
        #[qinvokable]
        fn set_theme(self: Pin<&mut KwhaleBridge>, mode: QString, accent_id: i32, follow_sys: bool);
        #[qinvokable]
        fn set_ui_backend_pref(self: Pin<&mut KwhaleBridge>, backend: QString);
        #[qinvokable]
        fn save_llm_settings(
            self: Pin<&mut KwhaleBridge>,
            endpoint: QString,
            api_key: QString,
            model: QString,
        );
        #[qinvokable]
        fn set_prefetch(self: Pin<&mut KwhaleBridge>, enabled: bool);
        #[qinvokable]
        fn clear_playback_cache(self: Pin<&mut KwhaleBridge>);
        #[qinvokable]
        fn reload_home(self: Pin<&mut KwhaleBridge>);
    }
}

pub struct KwhaleBridgeRust {
    theme_mode: QString,
    accent_id: i32,
    follow_system_accent: bool,
    ui_backend: QString,
    is_dark: bool,
    is_amoled: bool,
    color_primary: QString,
    color_on_primary: QString,
    color_primary_container: QString,
    color_on_primary_container: QString,
    color_secondary_container: QString,
    color_surface: QString,
    color_surface_low: QString,
    color_surface_container: QString,
    color_surface_high: QString,
    color_on_surface: QString,
    color_on_surface_variant: QString,
    color_outline_variant: QString,
    color_error: QString,
    accents_json: QString,

    authenticated: bool,
    user_name: QString,
    user_email: QString,
    greeting: QString,
    server_url: QString,
    allow_insecure_http: bool,
    server_status: QString,
    llm_endpoint: QString,
    llm_api_key: QString,
    llm_model: QString,
    prefetch_enabled: bool,
    cache_used_label: QString,

    active_page: QString,
    can_go_back: bool,
    inspector_open: bool,
    inspector_tab: QString,
    now_playing_open: bool,
    toast_message: QString,
    toast_seq: i32,

    is_playing: bool,
    is_buffering: bool,
    shuffle: bool,
    repeat_mode: QString,
    progress_permille: i32,
    position_text: QString,
    duration_text: QString,
    volume: f64,
    muted: bool,
    current_track_id: QString,
    current_title: QString,
    current_artist: QString,
    current_artist_id: QString,
    current_album: QString,
    current_album_id: QString,
    current_cover_url: QString,
    current_liked: bool,
    current_disliked: bool,
    current_censorship_reported: bool,
    current_downloaded: bool,
    queue_json: QString,
    lyrics_json: QString,
    active_lyric_index: i32,

    home_json: QString,
    search_partial: bool,
    search_json: QString,
    library_json: QString,
    sidebar_playlists_json: QString,
    collection_json: QString,
    downloads_json: QString,
    history_json: QString,
    profile_json: QString,
    track_info_json: QString,
    share_dialog_json: QString,
}

impl Default for KwhaleBridgeRust {
    fn default() -> Self {
        Self {
            theme_mode: QString::from("dark"),
            accent_id: 2,
            follow_system_accent: false,
            ui_backend: QString::from("auto"),
            is_dark: true,
            is_amoled: false,
            color_primary: QString::from("#7ab4ff"),
            color_on_primary: QString::from("#0b0b0b"),
            color_primary_container: QString::from("#243954"),
            color_on_primary_container: QString::from("#e8f1ff"),
            color_secondary_container: QString::from("#222b38"),
            color_surface: QString::from("#121212"),
            color_surface_low: QString::from("#1a1a1a"),
            color_surface_container: QString::from("#1e1e1e"),
            color_surface_high: QString::from("#2a2a2a"),
            color_on_surface: QString::from("#e6e1e5"),
            color_on_surface_variant: QString::from("#cac4d0"),
            color_outline_variant: QString::from("#49454f"),
            color_error: QString::from("#ffb4ab"),
            accents_json: QString::from(&build_accents_catalog_json()),

            authenticated: false,
            user_name: QString::from("Слушатель"),
            user_email: QString::from(""),
            greeting: QString::from(greeting_for_hour(chrono::Local::now().hour())),
            server_url: QString::from(ServerUrl::DEFAULT_PROD),
            allow_insecure_http: false,
            server_status: QString::from("Готов к проверке"),
            llm_endpoint: QString::from(""),
            llm_api_key: QString::from(""),
            llm_model: QString::from(""),
            prefetch_enabled: true,
            cache_used_label: QString::from("0 Б"),

            active_page: QString::from("home"),
            can_go_back: false,
            inspector_open: false,
            inspector_tab: QString::from("queue"),
            now_playing_open: false,
            toast_message: QString::from(""),
            toast_seq: 0,

            is_playing: false,
            is_buffering: false,
            shuffle: false,
            repeat_mode: QString::from("off"),
            progress_permille: 0,
            position_text: QString::from("00:00"),
            duration_text: QString::from("00:00"),
            volume: 1.0,
            muted: false,
            current_track_id: QString::from(""),
            current_title: QString::from("Ничего не играет"),
            current_artist: QString::from("Выберите трек"),
            current_artist_id: QString::from(""),
            current_album: QString::from(""),
            current_album_id: QString::from(""),
            current_cover_url: QString::from(""),
            current_liked: false,
            current_disliked: false,
            current_censorship_reported: false,
            current_downloaded: false,
            queue_json: QString::from("[]"),
            lyrics_json: QString::from(r#"{"synced":[],"plain":""}"#),
            active_lyric_index: -1,

            home_json: QString::from(r#"{"mixes":[],"sections":[]}"#),
            search_partial: false,
            search_json: QString::from(
                r#"{"history":[],"genres":[],"topArtists":[],"tracks":[],"albums":[],"artists":[],"playlists":[]}"#,
            ),
            library_json: QString::from(r#"{"tracks":[],"playlists":[],"albums":[]}"#),
            sidebar_playlists_json: QString::from("[]"),
            collection_json: QString::from("{}"),
            downloads_json: QString::from(r#"{"active":[],"downloaded":[]}"#),
            history_json: QString::from("[]"),
            profile_json: QString::from(r#"{"name":"","email":"","enrichments":[]}"#),
            track_info_json: QString::from("{}"),
            share_dialog_json: QString::from("{}"),
        }
    }
}

fn file_url_to_path(raw: &str) -> PathBuf {
    if let Some(stripped) = raw.trim().strip_prefix("file://") {
        let mut decoded = Vec::with_capacity(stripped.len());
        let bytes = stripped.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Ok(hex_str) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                    if let Ok(val) = u8::from_str_radix(hex_str, 16) {
                        decoded.push(val);
                        i += 3;
                        continue;
                    }
                }
            }
            decoded.push(bytes[i]);
            i += 1;
        }
        PathBuf::from(String::from_utf8_lossy(&decoded).to_string())
    } else {
        PathBuf::from(raw.trim())
    }
}

fn spawn_reload_home() {
    let b = backend();
    let core = b.core.clone();
    let state = b.state.clone();
    let rt_h = b.rt.handle().clone();
    b.rt.spawn(async move {
        let home = core.api.home(None, Some(12)).await.ok();
        let mixes = core
            .api
            .mixes(None, None, Some(8))
            .await
            .map(|p| p.items)
            .unwrap_or_default();

        let mixes_val: Vec<_> = mixes
            .iter()
            .map(|m| {
                let cov = cover_url_for(
                    &core,
                    &rt_h,
                    &state,
                    m.cover.as_ref().and_then(|c| c.valid_id()),
                    &m.title,
                    "md",
                );
                json!({
                    "id": m.id,
                    "title": m.title,
                    "subtitle": m.subtitle.clone().unwrap_or_else(|| "Персональный микс".into()),
                    "coverUrl": cov,
                })
            })
            .collect();

        let mut sections_val = Vec::new();
        if let Some(h) = home {
            for (s_idx, sec) in h.sections.into_iter().enumerate() {
                if sec.items.is_empty() {
                    continue;
                }
                let ctx_key = format!("home_sec_{s_idx}");
                let tracks: Vec<UiTrack> = sec
                    .items
                    .iter()
                    .filter_map(|i| i.track.as_ref())
                    .map(|t| t.to_ui(core.downloads.is_downloaded(&t.id)))
                    .collect();
                if !tracks.is_empty() {
                    register_tracks(&state, &ctx_key, &tracks);
                }
                let tracks_json: Vec<_> = tracks
                    .iter()
                    .take(8)
                    .enumerate()
                    .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
                    .collect();
                let albums_json: Vec<_> = sec
                    .items
                    .iter()
                    .filter_map(|i| i.album.as_ref())
                    .take(12)
                    .map(|al| {
                        let cov = cover_url_for(
                            &core,
                            &rt_h,
                            &state,
                            al.cover_image_id(),
                            &al.title,
                            "md",
                        );
                        json!({
                            "id": al.id,
                            "title": al.title,
                            "subtitle": al.artist_line(),
                            "badge": al.type_label().unwrap_or(""),
                            "coverUrl": cov,
                        })
                    })
                    .collect();
                let artists_json: Vec<_> = sec
                    .items
                    .iter()
                    .filter_map(|i| i.artist.as_ref())
                    .take(12)
                    .map(|ar| {
                        let cov = cover_url_for(
                            &core,
                            &rt_h,
                            &state,
                            ar.avatar_image_id(),
                            &ar.name,
                            "md",
                        );
                        json!({
                            "id": ar.id,
                            "name": ar.name,
                            "coverUrl": cov,
                        })
                    })
                    .collect();

                sections_val.push(json!({
                    "contextKey": ctx_key,
                    "title": sec.title.unwrap_or_else(|| "Рекомендации".into()),
                    "tracks": tracks_json,
                    "albums": albums_json,
                    "artists": artists_json,
                }));
            }
        }

        let payload = json!({
            "mixes": mixes_val,
            "sections": sections_val,
        });
        let mut g = state.lock().expect("ui lock");
        g.home_json = payload.to_string();
        g.dirty = true;
    });
}

fn spawn_reload_library() {
    let b = backend();
    let core = b.core.clone();
    let state = b.state.clone();
    let rt_h = b.rt.handle().clone();
    b.rt.spawn(async move {
        let tracks = core
            .api
            .library_tracks(None, Some(150), "recent", None)
            .await
            .map(|p| p.items)
            .unwrap_or_default();
        let playlists = core
            .api
            .library_playlists(None, Some(100))
            .await
            .map(|p| p.items)
            .unwrap_or_default();
        let albums = core
            .api
            .library_albums(None, Some(100))
            .await
            .map(|p| p.items)
            .unwrap_or_default();

        let ui_tracks: Vec<UiTrack> = tracks
            .iter()
            .map(|t| t.to_ui(core.downloads.is_downloaded(&t.id)))
            .collect();
        register_tracks(&state, "library_tracks", &ui_tracks);

        let tracks_json: Vec<_> = ui_tracks
            .iter()
            .enumerate()
            .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
            .collect();
        let pl_json: Vec<_> = playlists
            .iter()
            .map(|pl| {
                let cov = cover_url_for(
                    &core,
                    &rt_h,
                    &state,
                    pl.cover_image_id(),
                    &pl.title,
                    "md",
                );
                json!({
                    "id": pl.id,
                    "title": pl.title,
                    "trackCount": pl.track_count,
                    "subtitle": KwhalePlural::tracks(pl.track_count),
                    "coverUrl": cov,
                })
            })
            .collect();
        let al_json: Vec<_> = albums
            .iter()
            .map(|al| {
                let cov = cover_url_for(
                    &core,
                    &rt_h,
                    &state,
                    al.cover_image_id(),
                    &al.title,
                    "md",
                );
                json!({
                    "id": al.id,
                    "title": al.title,
                    "subtitle": al.artist_line(),
                    "badge": al.type_label().unwrap_or(""),
                    "coverUrl": cov,
                })
            })
            .collect();

        let sb_str = serde_json::to_string(&pl_json).unwrap_or_else(|_| "[]".into());
        let lib_str = json!({
            "tracks": tracks_json,
            "playlists": pl_json,
            "albums": al_json,
        })
        .to_string();

        let mut g = state.lock().expect("ui lock");
        g.sidebar_playlists_json = sb_str;
        g.library_json = lib_str;
        g.dirty = true;
    });
}

fn spawn_reload_search_bootstrap() {
    let b = backend();
    let core = b.core.clone();
    let state = b.state.clone();
    let rt_h = b.rt.handle().clone();
    b.rt.spawn(async move {
        let hist = core
            .api
            .search_history()
            .await
            .map(|h| h.items)
            .unwrap_or_default();
        let genres: Vec<_> = core
            .api
            .genres()
            .await
            .map(|g| g.items)
            .unwrap_or_default()
            .into_iter()
            .map(|g| json!({ "id": g.id, "name": g.name }))
            .collect();
        let top_artists: Vec<_> = core
            .api
            .top_artists(Some(12))
            .await
            .map(|a| a.items)
            .unwrap_or_default()
            .into_iter()
            .map(|ar| {
                let cov =
                    cover_url_for(&core, &rt_h, &state, ar.avatar_image_id(), &ar.name, "md");
                json!({ "id": ar.id, "name": ar.name, "coverUrl": cov })
            })
            .collect();

        let payload = json!({
            "history": hist,
            "genres": genres,
            "topArtists": top_artists,
            "tracks": [],
            "albums": [],
            "artists": [],
            "playlists": [],
        });
        let mut g = state.lock().expect("ui lock");
        g.search_partial = false;
        g.search_json = payload.to_string();
        g.dirty = true;
    });
}

fn spawn_reload_downloads() {
    let b = backend();
    let core = b.core.clone();
    let state = b.state.clone();
    let rt_h = b.rt.handle().clone();
    let snap = core.downloads.snapshot();

    let active_json: Vec<_> = snap
        .active
        .iter()
        .map(|a| {
            let status_str = match &a.status {
                TrackDownloadStatus::Queued => "В очереди…".to_string(),
                TrackDownloadStatus::Downloading { percent } => format!("Загрузка: {percent}%"),
                TrackDownloadStatus::Failed { message } => format!("Ошибка: {message}"),
                _ => String::new(),
            };
            json!({
                "id": a.track.remote_id,
                "title": a.track.title,
                "artist": a.track.artist,
                "status": status_str,
            })
        })
        .collect();

    let ui_tracks: Vec<UiTrack> = snap.downloaded.iter().map(|d| d.to_ui_track()).collect();
    register_tracks(&state, "downloads_tracks", &ui_tracks);
    let dl_json: Vec<_> = ui_tracks
        .iter()
        .enumerate()
        .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
        .collect();

    let mut g = state.lock().expect("ui lock");
    g.downloads_json = json!({
        "active": active_json,
        "downloaded": dl_json,
    })
    .to_string();
    g.dirty = true;
}

fn spawn_reload_history() {
    let b = backend();
    let core = b.core.clone();
    let state = b.state.clone();
    let rt_h = b.rt.handle().clone();
    b.rt.spawn(async move {
        let items = core
            .api
            .history(None, Some(100))
            .await
            .map(|p| p.items)
            .unwrap_or_default();
        let ui_tracks: Vec<UiTrack> = items
            .iter()
            .map(|e| e.track.to_ui(core.downloads.is_downloaded(&e.track.id)))
            .collect();
        register_tracks(&state, "history_tracks", &ui_tracks);
        let list: Vec<_> = ui_tracks
            .iter()
            .enumerate()
            .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
            .collect();
        let mut g = state.lock().expect("ui lock");
        g.history_json = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
        g.dirty = true;
    });
}

fn spawn_reload_profile() {
    let b = backend();
    let core = b.core.clone();
    let state = b.state.clone();
    b.rt.spawn(async move {
        let sess = core.api.session();
        let name = sess
            .as_ref()
            .map(|s| s.user.display_name.clone())
            .unwrap_or_default();
        let email = sess
            .as_ref()
            .and_then(|s| s.user.email.clone())
            .unwrap_or_default();
        let enrichments: Vec<_> = core
            .api
            .enrichments()
            .await
            .map(|p| p.items)
            .unwrap_or_default()
            .into_iter()
            .map(|e| {
                let title = e
                    .title
                    .unwrap_or_else(|| format!("Трек #{:?}", e.track_id));
                let summary = format!(
                    "Альбом: {} • Год: {} • Жанр: {}",
                    e.proposed.album.as_deref().unwrap_or("—"),
                    e.proposed
                        .year
                        .map(|y| y.to_string())
                        .unwrap_or_else(|| "—".into()),
                    e.proposed.genre.as_deref().unwrap_or("—")
                );
                json!({
                    "id": e.id,
                    "title": title,
                    "summary": summary,
                })
            })
            .collect();

        let mut g = state.lock().expect("ui lock");
        g.profile_json = json!({
            "name": name,
            "email": email,
            "enrichments": enrichments,
        })
        .to_string();
        g.dirty = true;
    });
}

fn spawn_load_collection(kind: String, id: String) {
    let b = backend();
    let core = b.core.clone();
    let state = b.state.clone();
    let rt_h = b.rt.handle().clone();

    {
        let mut g = state.lock().expect("ui lock");
        if g.active_page != "collection" {
            g.prev_page = g.active_page.clone();
        }
        g.active_page = "collection".into();
        g.collection_kind = kind.clone();
        g.collection_id = id.clone();
        g.dirty = true;
    }

    b.rt.spawn(async move {
        match kind.as_str() {
            "liked" => {
                if let Ok(page) = core.api.library_tracks(None, Some(200), "recent", None).await {
                    let ui_tracks: Vec<UiTrack> = page
                        .items
                        .iter()
                        .map(|t| t.to_ui(core.downloads.is_downloaded(&t.id)))
                        .collect();
                    register_tracks(&state, "collection_tracks", &ui_tracks);
                    let tracks_json: Vec<_> = ui_tracks
                        .iter()
                        .enumerate()
                        .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
                        .collect();
                    let cov = cover_url_for(&core, &rt_h, &state, None, "Любимые треки", "lg");
                    let payload = json!({
                        "kind": "liked",
                        "id": "liked",
                        "title": "Любимые треки",
                        "subtitle": "Ваша коллекция избранного",
                        "meta": KwhalePlural::tracks(ui_tracks.len() as i32),
                        "coverUrl": cov,
                        "canEdit": false,
                        "isFavorite": true,
                        "tracks": tracks_json,
                        "albums": [],
                    });
                    let mut g = state.lock().expect("ui lock");
                    g.collection_track_ids =
                        ui_tracks.iter().map(|t| t.remote_id.clone()).collect();
                    g.collection_json = payload.to_string();
                    g.dirty = true;
                }
            }
            "album" => {
                if let Ok(album) = core.api.album(&id).await {
                    let tracks = if album.tracks.is_empty() {
                        core.api
                            .album_tracks(&id, None, Some(100))
                            .await
                            .map(|p| p.items)
                            .unwrap_or_default()
                    } else {
                        album.tracks.clone()
                    };
                    let ui_tracks: Vec<UiTrack> = tracks
                        .iter()
                        .map(|t| t.to_ui(core.downloads.is_downloaded(&t.id)))
                        .collect();
                    register_tracks(&state, "collection_tracks", &ui_tracks);
                    let tracks_json: Vec<_> = ui_tracks
                        .iter()
                        .enumerate()
                        .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
                        .collect();
                    let cov = cover_url_for(
                        &core,
                        &rt_h,
                        &state,
                        album.cover_image_id(),
                        &album.title,
                        "lg",
                    );
                    let meta = format!(
                        "{}{} • {}",
                        album.type_label().unwrap_or("Альбом"),
                        album.year.map(|y| format!(" • {y}")).unwrap_or_default(),
                        KwhalePlural::tracks(ui_tracks.len() as i32)
                    );
                    let payload = json!({
                        "kind": "album",
                        "id": album.id,
                        "title": album.title,
                        "subtitle": album.artist_line(),
                        "meta": meta,
                        "coverUrl": cov,
                        "canEdit": false,
                        "isFavorite": album.is_favorite,
                        "tracks": tracks_json,
                        "albums": [],
                    });
                    let mut g = state.lock().expect("ui lock");
                    g.collection_is_fav = album.is_favorite;
                    g.collection_track_ids =
                        ui_tracks.iter().map(|t| t.remote_id.clone()).collect();
                    g.collection_json = payload.to_string();
                    g.dirty = true;
                }
            }
            "playlist" => {
                if let Ok(pl) = core.api.playlist(&id).await {
                    let tracks = if pl.tracks.is_empty() {
                        core.api
                            .playlist_tracks(&id, None, Some(200))
                            .await
                            .map(|p| p.items)
                            .unwrap_or_default()
                    } else {
                        pl.tracks.clone()
                    };
                    let ui_tracks: Vec<UiTrack> = tracks
                        .iter()
                        .map(|t| t.to_ui(core.downloads.is_downloaded(&t.id)))
                        .collect();
                    register_tracks(&state, "collection_tracks", &ui_tracks);
                    let tracks_json: Vec<_> = ui_tracks
                        .iter()
                        .enumerate()
                        .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
                        .collect();
                    let can_edit = pl.permissions.can_edit
                        || core
                            .api
                            .session()
                            .and_then(|s| pl.owner.as_ref().map(|o| o.id == s.user.id))
                            .unwrap_or(false);
                    let cov = cover_url_for(
                        &core,
                        &rt_h,
                        &state,
                        pl.cover_image_id(),
                        &pl.title,
                        "lg",
                    );
                    let meta = format!(
                        "{} • {}",
                        if pl.visibility == "public" {
                            "Публичный"
                        } else {
                            "Приватный"
                        },
                        KwhalePlural::tracks(ui_tracks.len() as i32)
                    );
                    let payload = json!({
                        "kind": "playlist",
                        "id": pl.id,
                        "title": pl.title,
                        "subtitle": pl.description.clone().unwrap_or_else(|| "Плейлист KWhale".into()),
                        "meta": meta,
                        "coverUrl": cov,
                        "canEdit": can_edit,
                        "isPublic": pl.visibility == "public",
                        "isFavorite": pl.is_followed,
                        "tracks": tracks_json,
                        "albums": [],
                    });
                    let mut g = state.lock().expect("ui lock");
                    g.collection_can_edit = can_edit;
                    g.collection_is_fav = pl.is_followed;
                    g.collection_track_ids =
                        ui_tracks.iter().map(|t| t.remote_id.clone()).collect();
                    g.collection_json = payload.to_string();
                    g.dirty = true;
                }
            }
            "mix" => {
                if let Ok(page) = core.api.mix_tracks(&id, None, Some(50)).await {
                    let ui_tracks: Vec<UiTrack> = page
                        .items
                        .iter()
                        .map(|r| r.track.to_ui(core.downloads.is_downloaded(&r.track.id)))
                        .collect();
                    register_tracks(&state, "collection_tracks", &ui_tracks);
                    let tracks_json: Vec<_> = ui_tracks
                        .iter()
                        .enumerate()
                        .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
                        .collect();
                    let cov = cover_url_for(
                        &core,
                        &rt_h,
                        &state,
                        ui_tracks.first().and_then(|t| t.cover_image_id.as_deref()),
                        "Персональный микс",
                        "lg",
                    );
                    let payload = json!({
                        "kind": "mix",
                        "id": id,
                        "title": "Персональный микс",
                        "subtitle": "Поток рекомендаций KWhale",
                        "meta": KwhalePlural::tracks(ui_tracks.len() as i32),
                        "coverUrl": cov,
                        "canEdit": false,
                        "isFavorite": false,
                        "tracks": tracks_json,
                        "albums": [],
                    });
                    let mut g = state.lock().expect("ui lock");
                    g.collection_track_ids =
                        ui_tracks.iter().map(|t| t.remote_id.clone()).collect();
                    g.collection_json = payload.to_string();
                    g.dirty = true;
                }
            }
            "artist" => {
                if let Ok(artist) = core.api.artist(&id).await {
                    let top_tracks = if artist.top_tracks.is_empty() {
                        core.api
                            .artist_top_tracks(&id, Some(25))
                            .await
                            .map(|p| p.items)
                            .unwrap_or_default()
                    } else {
                        artist.top_tracks.clone()
                    };
                    let albums = if artist.albums.is_empty() {
                        core.api
                            .artist_albums(&id, None, Some(60))
                            .await
                            .map(|p| p.items)
                            .unwrap_or_default()
                    } else {
                        artist.albums.clone()
                    };
                    let ui_tracks: Vec<UiTrack> = top_tracks
                        .iter()
                        .map(|t| t.to_ui(core.downloads.is_downloaded(&t.id)))
                        .collect();
                    register_tracks(&state, "collection_tracks", &ui_tracks);
                    let tracks_json: Vec<_> = ui_tracks
                        .iter()
                        .enumerate()
                        .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
                        .collect();
                    let albums_json: Vec<_> = albums
                        .iter()
                        .map(|al| {
                            let cov = cover_url_for(
                                &core,
                                &rt_h,
                                &state,
                                al.cover_image_id(),
                                &al.title,
                                "md",
                            );
                            json!({
                                "id": al.id,
                                "title": al.title,
                                "subtitle": al.year.map(|y| y.to_string()).unwrap_or_else(|| al.artist_line()),
                                "badge": al.type_label().unwrap_or(""),
                                "isSingleLike": al.is_single_like(),
                                "coverUrl": cov,
                            })
                        })
                        .collect();
                    let cov = cover_url_for(
                        &core,
                        &rt_h,
                        &state,
                        artist.avatar_image_id(),
                        &artist.name,
                        "lg",
                    );
                    let meta = format!(
                        "{} • {}",
                        KwhalePlural::albums(albums.len() as i32),
                        KwhalePlural::tracks(ui_tracks.len() as i32)
                    );
                    let payload = json!({
                        "kind": "artist",
                        "id": artist.id,
                        "title": artist.name,
                        "subtitle": artist.bio.clone().unwrap_or_else(|| "Артист KWhale".into()),
                        "meta": meta,
                        "coverUrl": cov,
                        "canEdit": false,
                        "isFavorite": artist.is_favorite,
                        "tracks": tracks_json,
                        "albums": albums_json,
                    });
                    let mut g = state.lock().expect("ui lock");
                    g.collection_is_fav = artist.is_favorite;
                    g.collection_track_ids =
                        ui_tracks.iter().map(|t| t.remote_id.clone()).collect();
                    g.collection_json = payload.to_string();
                    g.dirty = true;
                }
            }
            _ => {}
        }
    });
}

fn handle_deep_link_internal(dl: DeepLink) {
    match dl {
        DeepLink::Album(id) => spawn_load_collection("album".into(), id),
        DeepLink::Artist(id) => spawn_load_collection("artist".into(), id),
        DeepLink::Playlist(id) => spawn_load_collection("playlist".into(), id),
        DeepLink::Track(tid) => {
            let b = backend();
            let core = b.core.clone();
            let eng = b.engine.clone();
            let st = b.state.clone();
            b.rt.spawn(async move {
                match core.api.track(&tid).await {
                    Ok(t) => {
                        let ut = t.to_ui(core.downloads.is_downloaded(&t.id));
                        eng.play_queue(vec![ut], 0, "player", None);
                    }
                    Err(e) => push_toast(&st, e.user_message()),
                }
            });
        }
        DeepLink::Share(sid) => {
            let b = backend();
            let core = b.core.clone();
            let eng = b.engine.clone();
            let st = b.state.clone();
            b.rt.spawn(async move {
                match core.api.share_details(&sid).await {
                    Ok(link) => match link.kind {
                        ShareKind::Album => spawn_load_collection("album".into(), link.entity_id),
                        ShareKind::Artist => spawn_load_collection("artist".into(), link.entity_id),
                        ShareKind::Track => {
                            if let Ok(t) = core.api.track(&link.entity_id).await {
                                let ut = t.to_ui(core.downloads.is_downloaded(&t.id));
                                eng.play_queue(vec![ut], 0, "player", None);
                                push_toast(&st, format!("Воспроизводится: {}", t.title));
                            }
                        }
                    },
                    Err(e) => push_toast(&st, e.user_message()),
                }
            });
        }
    }
}

impl qobject::KwhaleBridge {
    pub fn init(mut self: Pin<&mut Self>) {
        self.as_mut().sync_theme_and_settings();
        if backend().core.api.session().is_some() {
            spawn_reload_home();
            spawn_reload_library();
            spawn_reload_search_bootstrap();
            spawn_reload_downloads();
        }
        self.poll_updates();
    }

    fn sync_theme_and_settings(mut self: Pin<&mut Self>) {
        let b = backend();
        let snap = b.core.settings.snapshot();
        let accent = if snap.follow_system_accent {
            ALL_ACCENTS
                .iter()
                .find(|a| a.id == 106)
                .copied()
                .unwrap_or_else(|| accent_by_id(snap.accent_id))
        } else {
            accent_by_id(snap.accent_id)
        };
        let pal = KwhalePalette::build(snap.theme_mode, accent.rgb(), true);

        self.as_mut()
            .set_theme_mode(QString::from(snap.theme_mode.as_str()));
        self.as_mut().set_accent_id(snap.accent_id);
        self.as_mut()
            .set_follow_system_accent(snap.follow_system_accent);
        self.as_mut()
            .set_ui_backend(QString::from(snap.ui_backend.as_str()));
        self.as_mut().set_is_dark(pal.is_dark);
        self.as_mut().set_is_amoled(pal.is_amoled);

        self.as_mut()
            .set_color_primary(QString::from(&pal.primary.to_hex()));
        self.as_mut()
            .set_color_on_primary(QString::from(&pal.on_primary.to_hex()));
        self.as_mut()
            .set_color_primary_container(QString::from(&pal.primary_container.to_hex()));
        self.as_mut()
            .set_color_on_primary_container(QString::from(&pal.on_primary_container.to_hex()));
        self.as_mut()
            .set_color_secondary_container(QString::from(&pal.secondary_container.to_hex()));
        self.as_mut()
            .set_color_surface(QString::from(&pal.surface.to_hex()));
        self.as_mut()
            .set_color_surface_low(QString::from(&pal.surface_container_low.to_hex()));
        self.as_mut()
            .set_color_surface_container(QString::from(&pal.surface_container.to_hex()));
        self.as_mut()
            .set_color_surface_high(QString::from(&pal.surface_container_high.to_hex()));
        self.as_mut()
            .set_color_on_surface(QString::from(&pal.on_surface.to_hex()));
        self.as_mut()
            .set_color_on_surface_variant(QString::from(&pal.on_surface_variant.to_hex()));
        self.as_mut()
            .set_color_outline_variant(QString::from(&pal.outline_variant.to_hex()));
        self.as_mut()
            .set_color_error(QString::from(&pal.error.to_hex()));

        self.as_mut()
            .set_server_url(QString::from(&snap.api_base_url));
        self.as_mut()
            .set_allow_insecure_http(snap.allow_insecure_http);
        self.as_mut()
            .set_llm_endpoint(QString::from(&snap.llm_endpoint));
        self.as_mut()
            .set_llm_api_key(QString::from(&snap.llm_api_key));
        self.as_mut().set_llm_model(QString::from(&snap.llm_model));
        self.as_mut().set_prefetch_enabled(snap.prefetch_enabled);
        self.as_mut().set_cache_used_label(QString::from(&format_bytes(
            b.core.playback_cache.used_bytes(),
        )));

        if let Some(sess) = b.core.api.session() {
            self.as_mut().set_authenticated(true);
            self.as_mut()
                .set_user_name(QString::from(&sess.user.display_name));
            self.as_mut().set_user_email(QString::from(
                sess.user.email.as_deref().unwrap_or_default(),
            ));
        } else {
            self.as_mut().set_authenticated(false);
        }
    }

    pub fn poll_updates(mut self: Pin<&mut Self>) {
        let b = backend();
        let pb = b.engine.state();
        let rt_h = b.rt.handle().clone();

        // Drain any D-Bus / CLI deep links
        let pending_links: Vec<DeepLink> = {
            let mut g = b.state.lock().expect("ui lock");
            std::mem::take(&mut g.pending_deep_links)
        };
        for dl in pending_links {
            handle_deep_link_internal(dl);
        }

        // Sync playback state
        self.as_mut().set_is_playing(pb.is_playing);
        self.as_mut().set_is_buffering(pb.is_buffering);
        self.as_mut().set_shuffle(pb.shuffle);
        let rep_str = match pb.repeat_mode {
            RepeatMode::Off => "off",
            RepeatMode::All => "all",
            RepeatMode::One => "one",
        };
        self.as_mut().set_repeat_mode(QString::from(rep_str));
        self.as_mut()
            .set_progress_permille((pb.progress() * 1000.0) as i32);
        self.as_mut()
            .set_position_text(QString::from(&format_duration_player_ms(pb.position_ms)));
        self.as_mut()
            .set_duration_text(QString::from(&format_duration_player_ms(pb.duration_ms)));
        self.as_mut().set_volume(pb.volume);
        self.as_mut().set_muted(pb.muted);

        if let Some(cur) = pb.current() {
            let cur_id = cur.remote_id.clone();
            self.as_mut().set_current_track_id(QString::from(&cur_id));
            self.as_mut().set_current_title(QString::from(&cur.title));
            self.as_mut().set_current_artist(QString::from(&cur.artist));
            self.as_mut().set_current_artist_id(QString::from(
                cur.artist_remote_id.as_deref().unwrap_or_default(),
            ));
            self.as_mut().set_current_album(QString::from(
                cur.album.as_deref().unwrap_or_default(),
            ));
            self.as_mut().set_current_album_id(QString::from(
                cur.album_remote_id.as_deref().unwrap_or_default(),
            ));
            let cov = cover_url_for(
                &b.core,
                &rt_h,
                &b.state,
                cur.cover_image_id.as_deref(),
                &cur.title,
                "lg",
            );
            self.as_mut().set_current_cover_url(QString::from(&cov));
            self.as_mut()
                .set_current_liked(b.core.reactions.is_liked(&cur_id, cur.is_liked));
            self.as_mut()
                .set_current_disliked(b.core.reactions.is_disliked(&cur_id, cur.is_disliked));
            self.as_mut().set_current_censorship_reported(
                b.core
                    .reactions
                    .is_censorship_reported(&cur_id, cur.is_censorship_reported),
            );
            self.as_mut()
                .set_current_downloaded(b.core.downloads.is_downloaded(&cur_id));

            // Check if lyrics need fetching for current track
            let need_lyrics = {
                let mut g = b.state.lock().expect("ui lock");
                if g.lyrics_track_id.as_deref() != Some(cur_id.as_str()) {
                    g.lyrics_track_id = Some(cur_id.clone());
                    g.lyrics = None;
                    g.lyrics_json = r#"{"synced":[],"plain":"Загрузка текста песни…"}"#.into();
                    g.dirty = true;
                    true
                } else {
                    false
                }
            };
            if need_lyrics {
                let c = b.core.clone();
                let st = b.state.clone();
                let tid = cur_id.clone();
                b.rt.spawn(async move {
                    match c.api.lyrics(&tid).await {
                        Ok(lyr) => {
                            let synced_val: Vec<_> = lyr
                                .synced
                                .iter()
                                .enumerate()
                                .map(|(idx, l)| {
                                    json!({
                                        "index": idx,
                                        "startMs": l.start_ms,
                                        "text": if l.text.trim().is_empty() { "♪" } else { l.text.as_str() },
                                    })
                                })
                                .collect();
                            let plain = lyr.plain.clone().unwrap_or_default();
                            let mut g = st.lock().expect("ui lock");
                            g.lyrics_json = json!({
                                "synced": synced_val,
                                "plain": plain,
                            })
                            .to_string();
                            g.lyrics = Some(lyr);
                            g.dirty = true;
                        }
                        Err(_) => {
                            let mut g = st.lock().expect("ui lock");
                            g.lyrics_json =
                                r#"{"synced":[],"plain":"Текст песни недоступен"}"#.into();
                            g.lyrics = None;
                            g.dirty = true;
                        }
                    }
                });
            }

            // Active karaoke line index
            let active_idx = {
                let g = b.state.lock().expect("ui lock");
                g.lyrics
                    .as_ref()
                    .and_then(|l| {
                        if l.synced.is_empty() {
                            None
                        } else {
                            l.synced
                                .iter()
                                .rposition(|line| line.start_ms <= pb.position_ms)
                                .map(|i| i as i32)
                        }
                    })
                    .unwrap_or(-1)
            };
            self.as_mut().set_active_lyric_index(active_idx);
        }

        // Queue JSON
        let q_items: Vec<_> = pb
            .queue
            .iter()
            .enumerate()
            .map(|(idx, tr)| {
                json!({
                    "index": idx,
                    "id": tr.remote_id,
                    "title": tr.title,
                    "artist": tr.artist,
                    "durationText": format_duration_ms((tr.duration_sec as u64) * 1000),
                    "isCurrent": idx == pb.index,
                })
            })
            .collect();
        let q_str = serde_json::to_string(&q_items).unwrap_or_else(|_| "[]".into());
        if self.as_ref().queue_json().to_string() != q_str {
            self.as_mut().set_queue_json(QString::from(&q_str));
        }

        // Check shared UI state dirty flag
        let snap_opt = {
            let mut g = b.state.lock().expect("ui lock");
            if g.dirty {
                g.dirty = false;
                Some((
                    g.toast_msg.clone(),
                    g.toast_seq,
                    g.server_status.clone(),
                    g.active_page.clone(),
                    g.home_json.clone(),
                    g.search_partial,
                    g.search_json.clone(),
                    g.library_json.clone(),
                    g.sidebar_playlists_json.clone(),
                    g.collection_json.clone(),
                    g.downloads_json.clone(),
                    g.history_json.clone(),
                    g.profile_json.clone(),
                    g.track_info_json.clone(),
                    g.share_dialog_json.clone(),
                    g.lyrics_json.clone(),
                ))
            } else {
                None
            }
        };

        if let Some((
            toast_msg,
            toast_seq,
            server_status,
            active_page,
            home_json,
            search_partial,
            search_json,
            library_json,
            sidebar_playlists_json,
            collection_json,
            downloads_json,
            history_json,
            profile_json,
            track_info_json,
            share_dialog_json,
            lyrics_json,
        )) = snap_opt
        {
            self.as_mut().set_toast_message(QString::from(&toast_msg));
            self.as_mut().set_toast_seq(toast_seq);
            self.as_mut()
                .set_server_status(QString::from(&server_status));
            self.as_mut()
                .set_can_go_back(active_page == "collection");
            self.as_mut().set_active_page(QString::from(&active_page));
            self.as_mut().set_home_json(QString::from(&home_json));
            self.as_mut().set_search_partial(search_partial);
            self.as_mut().set_search_json(QString::from(&search_json));
            self.as_mut().set_library_json(QString::from(&library_json));
            self.as_mut()
                .set_sidebar_playlists_json(QString::from(&sidebar_playlists_json));
            self.as_mut()
                .set_collection_json(QString::from(&collection_json));
            self.as_mut()
                .set_downloads_json(QString::from(&downloads_json));
            self.as_mut().set_history_json(QString::from(&history_json));
            self.as_mut().set_profile_json(QString::from(&profile_json));
            self.as_mut()
                .set_track_info_json(QString::from(&track_info_json));
            self.as_mut()
                .set_share_dialog_json(QString::from(&share_dialog_json));
            self.as_mut().set_lyrics_json(QString::from(&lyrics_json));
            self.as_mut().sync_theme_and_settings();
        }
    }

    pub fn login(
        self: Pin<&mut Self>,
        url: QString,
        allow_http: bool,
        email: QString,
        password: QString,
    ) {
        let b = backend();
        let raw_url = url.to_string();
        let Some(norm_url) = ServerUrl::normalize(&raw_url, None) else {
            push_toast(&b.state, "Некорректный адрес сервера");
            return;
        };
        if let Err(e) = b.core.reconfigure_api(&norm_url, allow_http) {
            push_toast(&b.state, e.user_message());
            return;
        }
        let em = email.to_string().trim().to_string();
        let pw = password.to_string();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.login(&em, &pw).await {
                Ok(sess) => {
                    c.notify_auth_user(Some(sess.user.clone()));
                    push_toast(&st, format!("Добро пожаловать, {}!", sess.user.display_name));
                    spawn_reload_home();
                    spawn_reload_library();
                    spawn_reload_search_bootstrap();
                    spawn_reload_downloads();
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn register_account(
        self: Pin<&mut Self>,
        url: QString,
        allow_http: bool,
        name: QString,
        email: QString,
        password: QString,
        invite_token: QString,
    ) {
        let b = backend();
        let raw_url = url.to_string();
        let Some(norm_url) = ServerUrl::normalize(&raw_url, None) else {
            push_toast(&b.state, "Некорректный адрес сервера");
            return;
        };
        if let Err(e) = b.core.reconfigure_api(&norm_url, allow_http) {
            push_toast(&b.state, e.user_message());
            return;
        }
        let nm = name.to_string().trim().to_string();
        let em = email.to_string().trim().to_string();
        let pw = password.to_string();
        let inv = invite_token.to_string().trim().to_string();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.register(&em, &pw, &nm, &inv).await {
                Ok(sess) => {
                    c.notify_auth_user(Some(sess.user.clone()));
                    push_toast(&st, format!("Аккаунт создан: {}!", sess.user.display_name));
                    spawn_reload_home();
                    spawn_reload_library();
                    spawn_reload_search_bootstrap();
                    spawn_reload_downloads();
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn logout(mut self: Pin<&mut Self>) {
        let b = backend();
        b.engine.stop();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            c.api.logout().await;
            push_toast(&st, "Вы вышли из аккаунта");
        });
        self.as_mut().set_authenticated(false);
    }

    pub fn save_server(mut self: Pin<&mut Self>, url: QString, allow_http: bool) {
        let b = backend();
        let raw = url.to_string();
        let Some(norm) = ServerUrl::normalize(&raw, None) else {
            push_toast(&b.state, "Некорректный URL сервера");
            return;
        };
        match b.core.reconfigure_api(&norm, allow_http) {
            Ok(()) => {
                self.as_mut().set_server_url(QString::from(&norm));
                push_toast(&b.state, "Адрес сервера сохранён");
            }
            Err(e) => push_toast(&b.state, e.user_message()),
        }
    }

    pub fn check_server_health(self: Pin<&mut Self>) {
        let b = backend();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.health().await {
                Ok(h) => {
                    let ver = c
                        .api
                        .server_config()
                        .await
                        .ok()
                        .map(|cfg| cfg.api_version)
                        .unwrap_or_else(|| "v1".into());
                    let msg = format!("Статус: {} • API: {}", h.status, ver);
                    let mut g = st.lock().expect("ui lock");
                    g.server_status = msg.clone();
                    g.toast_msg = msg;
                    g.toast_seq += 1;
                    g.dirty = true;
                }
                Err(e) => {
                    let msg = format!("Ошибка: {}", e.user_message());
                    let mut g = st.lock().expect("ui lock");
                    g.server_status = msg.clone();
                    g.toast_msg = msg;
                    g.toast_seq += 1;
                    g.dirty = true;
                }
            }
        });
    }

    pub fn navigate(mut self: Pin<&mut Self>, page: QString) {
        let p = page.to_string();
        {
            let mut g = backend().state.lock().expect("ui lock");
            g.prev_page = p.clone();
            g.active_page = p.clone();
            g.dirty = true;
        }
        self.as_mut().set_can_go_back(false);
        self.as_mut().set_active_page(page);
        match p.as_str() {
            "home" => spawn_reload_home(),
            "library" => spawn_reload_library(),
            "downloads" => spawn_reload_downloads(),
            "history" => spawn_reload_history(),
            "profile" => spawn_reload_profile(),
            _ => {}
        }
    }

    pub fn navigate_back(mut self: Pin<&mut Self>) {
        let prev = {
            let mut g = backend().state.lock().expect("ui lock");
            let p = g.prev_page.clone();
            g.active_page = p.clone();
            g.dirty = true;
            p
        };
        self.as_mut().set_can_go_back(false);
        self.as_mut().set_active_page(QString::from(&prev));
    }

    pub fn toggle_inspector(mut self: Pin<&mut Self>, tab: QString) {
        let cur_open = *self.as_ref().inspector_open();
        let cur_tab = self.as_ref().inspector_tab().to_string();
        let req_tab = tab.to_string();
        if cur_open && cur_tab == req_tab {
            self.as_mut().set_inspector_open(false);
        } else {
            self.as_mut().set_inspector_tab(tab);
            self.as_mut().set_inspector_open(true);
        }
    }

    pub fn toggle_now_playing(mut self: Pin<&mut Self>) {
        let next = !*self.as_ref().now_playing_open();
        self.as_mut().set_now_playing_open(next);
    }

    pub fn open_album(self: Pin<&mut Self>, album_id: QString) {
        let id = album_id.to_string();
        if !id.is_empty() {
            spawn_load_collection("album".into(), id);
        }
    }

    pub fn open_artist(self: Pin<&mut Self>, artist_id: QString) {
        let id = artist_id.to_string();
        if !id.is_empty() {
            spawn_load_collection("artist".into(), id);
        }
    }

    pub fn open_artist_all_tracks(
        self: Pin<&mut Self>,
        artist_id: QString,
        artist_name: QString,
    ) {
        let b = backend();
        let core = b.core.clone();
        let state = b.state.clone();
        let rt_h = b.rt.handle().clone();
        let aid = artist_id.to_string();
        let aname = artist_name.to_string();
        b.rt.spawn(async move {
            if let Ok(page) = core.api.artist_tracks(&aid, None, Some(150)).await {
                let ui_tracks: Vec<UiTrack> = page
                    .items
                    .iter()
                    .map(|t| t.to_ui(core.downloads.is_downloaded(&t.id)))
                    .collect();
                register_tracks(&state, "collection_tracks", &ui_tracks);
                let tracks_json: Vec<_> = ui_tracks
                    .iter()
                    .enumerate()
                    .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
                    .collect();
                let cov = cover_url_for(&core, &rt_h, &state, None, &aname, "lg");
                let payload = json!({
                    "kind": "artist_tracks",
                    "id": aid,
                    "title": format!("Все треки: {aname}"),
                    "subtitle": aname,
                    "meta": KwhalePlural::tracks(ui_tracks.len() as i32),
                    "coverUrl": cov,
                    "canEdit": false,
                    "isFavorite": false,
                    "tracks": tracks_json,
                    "albums": [],
                });
                let mut g = state.lock().expect("ui lock");
                g.collection_track_ids =
                    ui_tracks.iter().map(|t| t.remote_id.clone()).collect();
                g.collection_json = payload.to_string();
                g.dirty = true;
            }
        });
    }

    pub fn open_playlist(self: Pin<&mut Self>, playlist_id: QString) {
        let id = playlist_id.to_string();
        if !id.is_empty() {
            spawn_load_collection("playlist".into(), id);
        }
    }

    pub fn open_mix(self: Pin<&mut Self>, mix_id: QString) {
        let id = mix_id.to_string();
        if !id.is_empty() {
            spawn_load_collection("mix".into(), id);
        }
    }

    pub fn open_liked_tracks(self: Pin<&mut Self>) {
        spawn_load_collection("liked".into(), "liked".into());
    }

    pub fn play_pause(self: Pin<&mut Self>) {
        backend().engine.toggle();
    }

    pub fn next_track(self: Pin<&mut Self>) {
        backend().engine.next();
    }

    pub fn previous_track(self: Pin<&mut Self>) {
        backend().engine.previous();
    }

    pub fn seek_permille(self: Pin<&mut Self>, permille: i32) {
        let dur = backend().engine.state().duration_ms;
        if dur > 0 {
            let ms = ((permille.clamp(0, 1000) as f64 / 1000.0) * dur as f64) as u64;
            backend().engine.seek_ms(ms);
        }
    }

    pub fn seek_ms(self: Pin<&mut Self>, ms: i64) {
        if ms >= 0 {
            backend().engine.seek_ms(ms as u64);
        }
    }

    pub fn seek_relative_ms(self: Pin<&mut Self>, delta_ms: i64) {
        let cur = backend().engine.state().position_ms as i64;
        let target = (cur + delta_ms).max(0) as u64;
        backend().engine.seek_ms(target);
    }

    pub fn set_volume_level(self: Pin<&mut Self>, vol: f64) {
        let b = backend();
        b.engine.set_muted(false);
        b.engine.set_volume(vol.clamp(0.0, 1.0));
    }

    pub fn toggle_mute(self: Pin<&mut Self>) {
        let b = backend();
        let m = b.engine.state().muted;
        b.engine.set_muted(!m);
    }

    pub fn toggle_shuffle(self: Pin<&mut Self>) {
        backend().engine.toggle_shuffle();
    }

    pub fn cycle_repeat(self: Pin<&mut Self>) {
        backend().engine.cycle_repeat();
    }

    pub fn jump_to_queue(self: Pin<&mut Self>, index: i32) {
        if index >= 0 {
            backend().engine.jump_to(index as usize);
        }
    }

    pub fn move_queue_item(self: Pin<&mut Self>, from_idx: i32, to_idx: i32) {
        if from_idx >= 0 && to_idx >= 0 {
            backend()
                .engine
                .move_in_queue(from_idx as usize, to_idx as usize);
        }
    }

    pub fn remove_queue_item(self: Pin<&mut Self>, index: i32) {
        if index >= 0 {
            backend().engine.remove_from_queue(index as usize);
        }
    }

    pub fn play_personal_wave(self: Pin<&mut Self>) {
        let b = backend();
        let c = b.core.clone();
        let eng = b.engine.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            if let Ok(mixes) = c.api.mixes(None, None, Some(5)).await {
                if let Some(first) = mixes.items.first() {
                    if let Ok(page) = c.api.mix_tracks(&first.id, None, Some(40)).await {
                        if !page.items.is_empty() {
                            let q: Vec<UiTrack> = page
                                .items
                                .iter()
                                .map(|r| r.track.to_ui(c.downloads.is_downloaded(&r.track.id)))
                                .collect();
                            eng.play_queue(q, 0, "mix", Some(&first.id));
                            push_toast(&st, "Моя волна запущена");
                            return;
                        }
                    }
                }
            }
            match c.api.library_tracks(None, Some(40), "recent", None).await {
                Ok(lib) if !lib.items.is_empty() => {
                    let q: Vec<UiTrack> = lib
                        .items
                        .iter()
                        .map(|t| t.to_ui(c.downloads.is_downloaded(&t.id)))
                        .collect();
                    eng.play_queue(q, 0, "mix", None);
                    push_toast(&st, "Моя волна запущена");
                }
                _ => push_toast(&st, "На сервере пока нет треков для волны"),
            }
        });
    }

    pub fn play_prompt_mix(self: Pin<&mut Self>, prompt: QString, size: i32) {
        let p = prompt.to_string().trim().to_string();
        let b = backend();
        if p.is_empty() {
            push_toast(&b.state, "Введите описание настроения для AI-микса");
            return;
        }
        let snap = b.core.settings.snapshot();
        let req = PromptMixRequest {
            prompt: p,
            size: size.clamp(5, 50),
            llm_api_key: if snap.llm_api_key.is_empty() {
                None
            } else {
                Some(snap.llm_api_key)
            },
            llm_endpoint: if snap.llm_endpoint.is_empty() {
                None
            } else {
                Some(snap.llm_endpoint)
            },
            llm_model: if snap.llm_model.is_empty() {
                None
            } else {
                Some(snap.llm_model)
            },
        };
        let c = b.core.clone();
        let eng = b.engine.clone();
        let st = b.state.clone();
        push_toast(&st, "Генерация AI-микса…");
        b.rt.spawn(async move {
            match c.api.prompt_mix(&req).await {
                Ok(page) if !page.items.is_empty() => {
                    let q: Vec<UiTrack> = page
                        .items
                        .iter()
                        .map(|r| r.track.to_ui(c.downloads.is_downloaded(&r.track.id)))
                        .collect();
                    eng.play_queue(q, 0, "mix", None);
                    push_toast(&st, format!("AI-микс запущен ({} треков)", page.items.len()));
                }
                Ok(_) => push_toast(&st, "По этому запросу треки не найдены"),
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn play_context_track(
        self: Pin<&mut Self>,
        context_key: QString,
        index: i32,
        surface: QString,
    ) {
        let b = backend();
        let key = context_key.to_string();
        let surf = surface.to_string();
        let q_opt = {
            let g = b.state.lock().expect("ui lock");
            g.context_queues.get(&key).cloned()
        };
        if let Some(q) = q_opt {
            if !q.is_empty() {
                let idx = (index.max(0) as usize).min(q.len() - 1);
                b.engine.play_queue(q, idx, &surf, None);
            }
        }
    }

    pub fn play_collection_all(self: Pin<&mut Self>, shuffle: bool) {
        let b = backend();
        let q_opt = {
            let g = b.state.lock().expect("ui lock");
            g.context_queues.get("collection_tracks").cloned()
        };
        if let Some(q) = q_opt {
            if !q.is_empty() {
                b.engine.play_queue(q, 0, "playlist", None);
                if shuffle && !b.engine.state().shuffle {
                    b.engine.toggle_shuffle();
                }
            }
        }
    }

    pub fn play_track_now(self: Pin<&mut Self>, track_id: QString, surface: QString) {
        let b = backend();
        let tid = track_id.to_string();
        let surf = surface.to_string();
        let tr_opt = {
            let g = b.state.lock().expect("ui lock");
            g.tracks_by_id.get(&tid).cloned()
        };
        if let Some(tr) = tr_opt {
            b.engine.play_queue(vec![tr], 0, &surf, None);
        }
    }

    pub fn play_track_next(self: Pin<&mut Self>, track_id: QString, surface: QString) {
        let b = backend();
        let tid = track_id.to_string();
        let surf = surface.to_string();
        let tr_opt = {
            let g = b.state.lock().expect("ui lock");
            g.tracks_by_id.get(&tid).cloned()
        };
        if let Some(tr) = tr_opt {
            b.engine.play_next(tr, &surf);
            push_toast(&b.state, "Будет воспроизведено следующим");
        }
    }

    pub fn add_track_to_queue(self: Pin<&mut Self>, track_id: QString, surface: QString) {
        let b = backend();
        let tid = track_id.to_string();
        let surf = surface.to_string();
        let tr_opt = {
            let g = b.state.lock().expect("ui lock");
            g.tracks_by_id.get(&tid).cloned()
        };
        if let Some(tr) = tr_opt {
            b.engine.add_queue(vec![tr], &surf);
            push_toast(&b.state, "Добавлено в очередь");
        }
    }

    pub fn start_track_radio(self: Pin<&mut Self>, track_id: QString) {
        let b = backend();
        let tid = track_id.to_string();
        let seed_opt = {
            let g = b.state.lock().expect("ui lock");
            g.tracks_by_id.get(&tid).cloned()
        };
        let c = b.core.clone();
        let eng = b.engine.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.similar_tracks(&tid, Some(25)).await {
                Ok(page) => {
                    let mut q = Vec::new();
                    if let Some(seed) = seed_opt {
                        q.push(seed);
                    }
                    for t in page.items {
                        q.push(t.to_ui(c.downloads.is_downloaded(&t.id)));
                    }
                    if !q.is_empty() {
                        eng.play_queue(q, 0, "mix", None);
                        push_toast(&st, "Запущена волна похожих треков");
                    }
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn toggle_track_like(self: Pin<&mut Self>, track_id: QString, surface: QString) {
        let b = backend();
        let tid = track_id.to_string();
        let surf = surface.to_string();
        let tr_opt = {
            let g = b.state.lock().expect("ui lock");
            g.tracks_by_id
                .get(&tid)
                .cloned()
                .or_else(|| b.engine.state().current().cloned().filter(|c| c.remote_id == tid))
        };
        let Some(tr) = tr_opt else {
            return;
        };
        let c = b.core.clone();
        let eng = b.engine.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.reactions.toggle_like(&tr, &surf).await {
                Ok(liked) => {
                    eng.update_track_liked(&tid, liked);
                    push_toast(
                        &st,
                        if liked {
                            "Добавлено в Любимые треки"
                        } else {
                            "Удалено из Любимых треков"
                        },
                    );
                }
                Err(e) => push_toast(&st, e),
            }
        });
    }

    pub fn toggle_track_dislike(self: Pin<&mut Self>, track_id: QString, surface: QString) {
        let b = backend();
        let tid = track_id.to_string();
        let surf = surface.to_string();
        let tr_opt = {
            let g = b.state.lock().expect("ui lock");
            g.tracks_by_id
                .get(&tid)
                .cloned()
                .or_else(|| b.engine.state().current().cloned().filter(|c| c.remote_id == tid))
        };
        let Some(tr) = tr_opt else {
            return;
        };
        let c = b.core.clone();
        let eng = b.engine.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.reactions.toggle_dislike(&tr, &surf).await {
                Ok(disliked) => {
                    if disliked {
                        if eng
                            .state()
                            .current()
                            .map(|cur| cur.remote_id == tid)
                            .unwrap_or(false)
                        {
                            eng.next();
                        }
                        push_toast(&st, "Трек отмечен «Не нравится»");
                    } else {
                        push_toast(&st, "Отметка «Не нравится» снята");
                    }
                }
                Err(e) => push_toast(&st, e),
            }
        });
    }

    pub fn report_track_censorship(self: Pin<&mut Self>, track_id: QString) {
        let b = backend();
        let tid = track_id.to_string();
        let tr_opt = {
            let g = b.state.lock().expect("ui lock");
            g.tracks_by_id
                .get(&tid)
                .cloned()
                .or_else(|| b.engine.state().current().cloned().filter(|c| c.remote_id == tid))
        };
        let Some(tr) = tr_opt else {
            return;
        };
        let pos = Some(b.engine.state().position_ms);
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.reactions.report_censorship_one_click(&tr, pos).await {
                Ok(msg) => push_toast(&st, msg),
                Err(e) => push_toast(&st, e),
            }
        });
    }

    pub fn cancel_track_censorship(self: Pin<&mut Self>, track_id: QString) {
        let b = backend();
        let tid = track_id.to_string();
        let tr_opt = {
            let g = b.state.lock().expect("ui lock");
            g.tracks_by_id
                .get(&tid)
                .cloned()
                .or_else(|| b.engine.state().current().cloned().filter(|c| c.remote_id == tid))
        };
        let Some(tr) = tr_opt else {
            return;
        };
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.reactions.cancel_censorship_report(&tr).await {
                Ok(msg) => push_toast(&st, msg),
                Err(e) => push_toast(&st, e),
            }
        });
    }

    pub fn download_track(self: Pin<&mut Self>, track_id: QString) {
        let b = backend();
        let tid = track_id.to_string();
        let tr_opt = {
            let g = b.state.lock().expect("ui lock");
            g.tracks_by_id
                .get(&tid)
                .cloned()
                .or_else(|| b.engine.state().current().cloned().filter(|c| c.remote_id == tid))
        };
        if let Some(tr) = tr_opt {
            b.core
                .downloads
                .set_duration_probe(Arc::new(kwhale_audio::measure_duration_sec));
            b.core.downloads.enqueue(tr, false);
            push_toast(&b.state, "Загрузка трека запущена");
            spawn_reload_downloads();
        }
    }

    pub fn remove_downloaded_track(self: Pin<&mut Self>, track_id: QString) {
        let b = backend();
        let tid = track_id.to_string();
        b.core.downloads.remove(&tid);
        push_toast(&b.state, "Офлайн-копия удалена");
        spawn_reload_downloads();
    }

    pub fn cancel_active_download(self: Pin<&mut Self>, track_id: QString) {
        let b = backend();
        b.core.downloads.cancel(&track_id.to_string());
        spawn_reload_downloads();
    }

    pub fn request_track_info(self: Pin<&mut Self>, track_id: QString) {
        let b = backend();
        let tid = track_id.to_string();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.track_info(&tid).await {
                Ok(info) => {
                    let q = info.quality;
                    let codec = q
                        .as_ref()
                        .and_then(|x| x.codec.clone())
                        .unwrap_or_else(|| "Неизвестно".into());
                    let lossless = q.as_ref().map(|x| x.lossless).unwrap_or(false);
                    let tier = q.as_ref().map(|x| x.tier).unwrap_or(0);
                    let bitrate = q
                        .as_ref()
                        .and_then(|x| x.bitrate_kbps)
                        .map(|b| format!("{b} кбит/с"))
                        .unwrap_or_else(|| "—".into());
                    let sample_rate = q
                        .as_ref()
                        .and_then(|x| x.sample_rate)
                        .map(|s| format!("{s} Гц"))
                        .unwrap_or_else(|| "—".into());
                    let size_str = q
                        .as_ref()
                        .and_then(|x| x.bytes)
                        .map(format_bytes)
                        .unwrap_or_else(|| "—".into());
                    let sources: Vec<_> = info
                        .sources
                        .iter()
                        .map(|s| {
                            json!({
                                "connector": s.connector,
                                "status": if s.available { "Доступен" } else { "Недоступен" },
                            })
                        })
                        .collect();
                    let payload = json!({
                        "open": true,
                        "id": info.track.id,
                        "title": info.track.title,
                        "artist": info.track.artist_line(),
                        "isrc": info.isrc.unwrap_or_else(|| "—".into()),
                        "codec": format!("{codec} ({}, tier {tier})", if lossless { "Lossless" } else { "Lossy" }),
                        "bitrate": bitrate,
                        "sampleRate": sample_rate,
                        "size": size_str,
                        "sources": sources,
                    });
                    let mut g = st.lock().expect("ui lock");
                    g.track_info_json = payload.to_string();
                    g.dirty = true;
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn request_share(
        self: Pin<&mut Self>,
        kind: QString,
        entity_id: QString,
        title: QString,
    ) {
        let b = backend();
        let k_str = kind.to_string();
        let sk = match k_str.as_str() {
            "album" => ShareKind::Album,
            "artist" => ShareKind::Artist,
            _ => ShareKind::Track,
        };
        let eid = entity_id.to_string();
        let etitle = title.to_string();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.create_share(sk, &eid).await {
                Ok(link) => {
                    let platforms: Vec<_> = link
                        .platforms
                        .iter()
                        .map(|p| json!({ "label": p.label, "url": p.url }))
                        .collect();
                    let payload = json!({
                        "open": true,
                        "title": etitle,
                        "shareUrl": link.share_url,
                        "deepLink": link.deep_link,
                        "ttlDays": link.ttl_days,
                        "clicks": link.clicks_total,
                        "platforms": platforms,
                    });
                    let mut g = st.lock().expect("ui lock");
                    g.share_dialog_json = payload.to_string();
                    g.dirty = true;
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn toggle_collection_favorite(self: Pin<&mut Self>) {
        let b = backend();
        let (kind, id, cur_fav) = {
            let g = b.state.lock().expect("ui lock");
            (
                g.collection_kind.clone(),
                g.collection_id.clone(),
                g.collection_is_fav,
            )
        };
        let next = !cur_fav;
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            let res = match kind.as_str() {
                "album" => {
                    if next {
                        c.api.like_album(&id).await
                    } else {
                        c.api.unlike_album(&id).await
                    }
                }
                "artist" => {
                    if next {
                        c.api.like_artist(&id).await
                    } else {
                        c.api.unlike_artist(&id).await
                    }
                }
                "playlist" => {
                    if next {
                        c.api.follow_playlist(&id).await
                    } else {
                        c.api.unfollow_playlist(&id).await
                    }
                }
                _ => Ok(()),
            };
            match res {
                Ok(()) => {
                    spawn_load_collection(kind, id);
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn like_all_collection_tracks(self: Pin<&mut Self>) {
        let b = backend();
        let (kind, id) = {
            let g = b.state.lock().expect("ui lock");
            (g.collection_kind.clone(), g.collection_id.clone())
        };
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            let res = match kind.as_str() {
                "album" => c.api.like_album_tracks(&id).await,
                "playlist" => c.api.like_playlist_tracks(&id).await,
                _ => return,
            };
            match res {
                Ok(r) => push_toast(
                    &st,
                    format!("В Любимые добавлено: {} треков", r.liked + r.already_liked),
                ),
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn sync_current_artist(self: Pin<&mut Self>) {
        let b = backend();
        let id = {
            let g = b.state.lock().expect("ui lock");
            g.collection_id.clone()
        };
        if id.is_empty() {
            return;
        }
        let c = b.core.clone();
        let st = b.state.clone();
        push_toast(&st, "Синхронизация дискографии…");
        b.rt.spawn(async move {
            match c.api.sync_artist(&id).await {
                Ok(s) => {
                    push_toast(
                        &st,
                        format!(
                            "Дискография: {} (альбомов: {}, треков: {})",
                            s.status, s.album_count, s.track_count
                        ),
                    );
                    spawn_load_collection("artist".into(), id);
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn create_playlist(
        self: Pin<&mut Self>,
        title: QString,
        desc: QString,
        is_public: bool,
        initial_track_id: QString,
    ) {
        let b = backend();
        let t = title.to_string().trim().to_string();
        if t.is_empty() {
            push_toast(&b.state, "Введите название плейлиста");
            return;
        }
        let d = {
            let s = desc.to_string().trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        let vis = if is_public {
            "public".to_string()
        } else {
            "private".to_string()
        };
        let init_tid = initial_track_id.to_string().trim().to_string();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c
                .api
                .create_playlist(&PlaylistCreate {
                    title: t,
                    description: d,
                    visibility: vis,
                })
                .await
            {
                Ok(pl) => {
                    if !init_tid.is_empty() {
                        let _ = c
                            .api
                            .add_playlist_tracks(&pl.id, vec![init_tid], None)
                            .await;
                    }
                    push_toast(&st, "Плейлист создан");
                    spawn_reload_library();
                    spawn_load_collection("playlist".into(), pl.id);
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn update_current_playlist(
        self: Pin<&mut Self>,
        title: QString,
        desc: QString,
        is_public: bool,
    ) {
        let b = backend();
        let pid = {
            let g = b.state.lock().expect("ui lock");
            g.collection_id.clone()
        };
        if pid.is_empty() {
            return;
        }
        let t = title.to_string().trim().to_string();
        let d = {
            let s = desc.to_string().trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        let vis = if is_public {
            "public".to_string()
        } else {
            "private".to_string()
        };
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c
                .api
                .update_playlist(
                    &pid,
                    &PlaylistUpdate {
                        title: Some(t),
                        description: d,
                        visibility: Some(vis),
                    },
                )
                .await
            {
                Ok(pl) => {
                    push_toast(&st, "Плейлист обновлён");
                    spawn_reload_library();
                    spawn_load_collection("playlist".into(), pl.id);
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn delete_current_playlist(self: Pin<&mut Self>) {
        let b = backend();
        let pid = {
            let g = b.state.lock().expect("ui lock");
            g.collection_id.clone()
        };
        if pid.is_empty() {
            return;
        }
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.delete_playlist(&pid).await {
                Ok(()) => {
                    {
                        let mut g = st.lock().expect("ui lock");
                        g.active_page = "library".into();
                        g.dirty = true;
                    }
                    push_toast(&st, "Плейлист удалён");
                    spawn_reload_library();
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn add_track_to_playlist(self: Pin<&mut Self>, playlist_id: QString, track_id: QString) {
        let b = backend();
        let pid = playlist_id.to_string();
        let tid = track_id.to_string();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.add_playlist_tracks(&pid, vec![tid], None).await {
                Ok(pl) => {
                    push_toast(&st, format!("Добавлено в «{}»", pl.title));
                    spawn_reload_library();
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn remove_track_from_current_playlist(self: Pin<&mut Self>, track_id: QString) {
        let b = backend();
        let pid = {
            let g = b.state.lock().expect("ui lock");
            g.collection_id.clone()
        };
        let tid = track_id.to_string();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.remove_playlist_track(&pid, &tid).await {
                Ok(()) => {
                    push_toast(&st, "Трек удалён из плейлиста");
                    spawn_load_collection("playlist".into(), pid);
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn move_current_playlist_track(self: Pin<&mut Self>, index: i32, delta: i32) {
        let b = backend();
        let (pid, mut ids) = {
            let g = b.state.lock().expect("ui lock");
            (g.collection_id.clone(), g.collection_track_ids.clone())
        };
        let target = index + delta;
        if index < 0 || target < 0 || (index as usize) >= ids.len() || (target as usize) >= ids.len()
        {
            return;
        }
        ids.swap(index as usize, target as usize);
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.reorder_playlist_tracks(&pid, ids).await {
                Ok(_) => spawn_load_collection("playlist".into(), pid),
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn upload_current_playlist_cover(self: Pin<&mut Self>, file_url: QString) {
        let b = backend();
        let pid = {
            let g = b.state.lock().expect("ui lock");
            g.collection_id.clone()
        };
        let path = file_url_to_path(&file_url.to_string());
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.upload_playlist_cover(&pid, &path).await {
                Ok(_) => {
                    push_toast(&st, "Обложка плейлиста обновлена");
                    spawn_reload_library();
                    spawn_load_collection("playlist".into(), pid);
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn upload_track_file(
        self: Pin<&mut Self>,
        file_url: QString,
        title: QString,
        artist: QString,
        album: QString,
    ) {
        let b = backend();
        let path = file_url_to_path(&file_url.to_string());
        if !path.is_file() {
            push_toast(&b.state, "Выберите существующий аудиофайл");
            return;
        }
        let t_opt = {
            let s = title.to_string().trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        let ar_opt = {
            let s = artist.to_string().trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        let al_opt = {
            let s = album.to_string().trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        let c = b.core.clone();
        let st = b.state.clone();
        push_toast(&st, "Загрузка трека на сервер…");
        b.rt.spawn(async move {
            match c
                .api
                .upload_track(
                    &path,
                    t_opt.as_deref(),
                    ar_opt.as_deref(),
                    al_opt.as_deref(),
                )
                .await
            {
                Ok(t) => {
                    push_toast(&st, format!("Трек «{}» успешно загружен", t.title));
                    spawn_reload_library();
                    spawn_reload_home();
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn run_search(self: Pin<&mut Self>, query: QString, filter_type: QString) {
        let q = query.to_string().trim().to_string();
        if q.is_empty() {
            spawn_reload_search_bootstrap();
            return;
        }
        let stype = {
            let f = filter_type.to_string();
            if f.is_empty() { "all".to_string() } else { f }
        };
        let b = backend();
        let core = b.core.clone();
        let state = b.state.clone();
        let rt_h = b.rt.handle().clone();

        b.rt.spawn(async move {
            // Wave 1 (wait = false)
            if let Ok(r1) = core.api.search(&q, &stype, Some(30), false).await {
                let ui_tracks: Vec<UiTrack> = r1
                    .tracks
                    .iter()
                    .map(|t| t.to_ui(core.downloads.is_downloaded(&t.id)))
                    .collect();
                register_tracks(&state, "search_tracks", &ui_tracks);
                let tracks_json: Vec<_> = ui_tracks
                    .iter()
                    .enumerate()
                    .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
                    .collect();
                let albums_json: Vec<_> = r1
                    .albums
                    .iter()
                    .map(|al| {
                        let cov = cover_url_for(
                            &core,
                            &rt_h,
                            &state,
                            al.cover_image_id(),
                            &al.title,
                            "md",
                        );
                        json!({
                            "id": al.id,
                            "title": al.title,
                            "subtitle": al.artist_line(),
                            "badge": al.type_label().unwrap_or(""),
                            "coverUrl": cov,
                        })
                    })
                    .collect();
                let artists_json: Vec<_> = r1
                    .artists
                    .iter()
                    .map(|ar| {
                        let cov = cover_url_for(
                            &core,
                            &rt_h,
                            &state,
                            ar.avatar_image_id(),
                            &ar.name,
                            "md",
                        );
                        json!({ "id": ar.id, "name": ar.name, "coverUrl": cov })
                    })
                    .collect();
                let playlists_json: Vec<_> = r1
                    .playlists
                    .iter()
                    .map(|pl| {
                        let cov = cover_url_for(
                            &core,
                            &rt_h,
                            &state,
                            pl.cover_image_id(),
                            &pl.title,
                            "md",
                        );
                        json!({
                            "id": pl.id,
                            "title": pl.title,
                            "subtitle": KwhalePlural::tracks(pl.track_count),
                            "coverUrl": cov,
                        })
                    })
                    .collect();

                let mut g = state.lock().expect("ui lock");
                g.search_partial = r1.partial;
                g.search_json = json!({
                    "history": [],
                    "genres": [],
                    "topArtists": [],
                    "tracks": tracks_json,
                    "albums": albums_json,
                    "artists": artists_json,
                    "playlists": playlists_json,
                })
                .to_string();
                g.dirty = true;
            }

            // Wave 2 (wait = true)
            if let Ok(r2) = core.api.search(&q, &stype, Some(30), true).await {
                let ui_tracks: Vec<UiTrack> = r2
                    .tracks
                    .iter()
                    .map(|t| t.to_ui(core.downloads.is_downloaded(&t.id)))
                    .collect();
                register_tracks(&state, "search_tracks", &ui_tracks);
                let tracks_json: Vec<_> = ui_tracks
                    .iter()
                    .enumerate()
                    .map(|(i, ut)| ui_track_to_json(&core, &rt_h, &state, ut, i))
                    .collect();
                let albums_json: Vec<_> = r2
                    .albums
                    .iter()
                    .map(|al| {
                        let cov = cover_url_for(
                            &core,
                            &rt_h,
                            &state,
                            al.cover_image_id(),
                            &al.title,
                            "md",
                        );
                        json!({
                            "id": al.id,
                            "title": al.title,
                            "subtitle": al.artist_line(),
                            "badge": al.type_label().unwrap_or(""),
                            "coverUrl": cov,
                        })
                    })
                    .collect();
                let artists_json: Vec<_> = r2
                    .artists
                    .iter()
                    .map(|ar| {
                        let cov = cover_url_for(
                            &core,
                            &rt_h,
                            &state,
                            ar.avatar_image_id(),
                            &ar.name,
                            "md",
                        );
                        json!({ "id": ar.id, "name": ar.name, "coverUrl": cov })
                    })
                    .collect();
                let playlists_json: Vec<_> = r2
                    .playlists
                    .iter()
                    .map(|pl| {
                        let cov = cover_url_for(
                            &core,
                            &rt_h,
                            &state,
                            pl.cover_image_id(),
                            &pl.title,
                            "md",
                        );
                        json!({
                            "id": pl.id,
                            "title": pl.title,
                            "subtitle": KwhalePlural::tracks(pl.track_count),
                            "coverUrl": cov,
                        })
                    })
                    .collect();

                let mut g = state.lock().expect("ui lock");
                g.search_partial = false;
                g.search_json = json!({
                    "history": [],
                    "genres": [],
                    "topArtists": [],
                    "tracks": tracks_json,
                    "albums": albums_json,
                    "artists": artists_json,
                    "playlists": playlists_json,
                })
                .to_string();
                g.dirty = true;
            }
        });
    }

    pub fn clear_search_history(self: Pin<&mut Self>) {
        let b = backend();
        let c = b.core.clone();
        b.rt.spawn(async move {
            let _ = c.api.clear_search_history().await;
            spawn_reload_search_bootstrap();
        });
    }

    pub fn clear_play_history(self: Pin<&mut Self>) {
        let b = backend();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            if c.api.clear_history().await.is_ok() {
                push_toast(&st, "История очищена");
                spawn_reload_history();
            }
        });
    }

    pub fn update_profile_name(self: Pin<&mut Self>, display_name: QString) {
        let name = display_name.to_string().trim().to_string();
        if name.is_empty() {
            return;
        }
        let b = backend();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.update_me(&name).await {
                Ok(_) => {
                    push_toast(&st, "Имя профиля обновлено");
                    spawn_reload_profile();
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn confirm_enrichment(self: Pin<&mut Self>, proposal_id: i64) {
        let b = backend();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.confirm_enrichment(proposal_id).await {
                Ok(()) => {
                    push_toast(&st, "Метаданные трека подтверждены");
                    spawn_reload_profile();
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn reject_enrichment(self: Pin<&mut Self>, proposal_id: i64) {
        let b = backend();
        let c = b.core.clone();
        let st = b.state.clone();
        b.rt.spawn(async move {
            match c.api.reject_enrichment(proposal_id).await {
                Ok(()) => {
                    push_toast(&st, "Предложение отклонено");
                    spawn_reload_profile();
                }
                Err(e) => push_toast(&st, e.user_message()),
            }
        });
    }

    pub fn set_theme(
        mut self: Pin<&mut Self>,
        mode: QString,
        accent_id: i32,
        follow_sys: bool,
    ) {
        let b = backend();
        let tm = ThemeMode::from_str_lossy(&mode.to_string());
        b.core.update_theme(tm, accent_id, follow_sys);
        self.as_mut().sync_theme_and_settings();
    }

    pub fn set_ui_backend_pref(mut self: Pin<&mut Self>, backend_str: QString) {
        let b = backend();
        let ub = UiBackend::from_str_lossy(&backend_str.to_string());
        b.core.settings.update(|s| s.ui_backend = ub);
        self.as_mut().set_ui_backend(QString::from(ub.as_str()));
        push_toast(
            &b.state,
            "Движок интерфейса сохранён (применится при следующем запуске)",
        );
    }

    pub fn save_llm_settings(
        mut self: Pin<&mut Self>,
        endpoint: QString,
        api_key: QString,
        model: QString,
    ) {
        let b = backend();
        let ep = endpoint.to_string();
        let key = api_key.to_string();
        let mdl = model.to_string();
        b.core.settings.update(|s| {
            s.llm_endpoint = ep;
            s.llm_api_key = key;
            s.llm_model = mdl;
        });
        self.as_mut().sync_theme_and_settings();
        push_toast(&b.state, "Настройки AI Промпт-Микса сохранены");
    }

    pub fn set_prefetch(mut self: Pin<&mut Self>, enabled: bool) {
        let b = backend();
        b.core.settings.update(|s| s.prefetch_enabled = enabled);
        self.as_mut().set_prefetch_enabled(enabled);
    }

    pub fn clear_playback_cache(mut self: Pin<&mut Self>) {
        let b = backend();
        b.core.playback_cache.clear();
        self.as_mut().set_cache_used_label(QString::from(&format_bytes(
            b.core.playback_cache.used_bytes(),
        )));
        push_toast(&b.state, "Кэш воспроизведения очищен");
    }

    pub fn reload_home(self: Pin<&mut Self>) {
        spawn_reload_home();
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _guard = rt.enter();

    let core = KwhaleCore::init(env!("CARGO_PKG_VERSION"))?;
    if core.api.session().is_none() {
        let _ = rt.block_on(core.token_store.try_recover_from_keyring());
    }
    let engine = rt.block_on(AudioEngine::start(core.clone()))?;

    let mut ui_state = SharedUiState::default();
    for arg in std::env::args().skip(1) {
        if let Some(dl) = DeepLink::parse(&arg) {
            ui_state.pending_deep_links.push(dl);
        }
    }
    let state = Arc::new(Mutex::new(ui_state));

    {
        let st_raise = state.clone();
        let st_raise2 = state.clone();
        let st_uri = state.clone();
        spawn_mpris(
            core.clone(),
            engine.clone(),
            Arc::new(move || {
                st_raise.lock().expect("ui lock").raise_requested = true;
            }),
        );
        spawn_single_instance_dbus(
            Arc::new(move || {
                st_raise2.lock().expect("ui lock").raise_requested = true;
            }),
            Arc::new(move |uri: String| {
                if let Some(dl) = DeepLink::parse(&uri) {
                    let mut g = st_uri.lock().expect("ui lock");
                    g.pending_deep_links.push(dl);
                    g.dirty = true;
                }
            }),
        );
    }

    let _ = BACKEND.set(GlobalBackend {
        rt,
        core,
        engine,
        state,
    });

    let mut app = QGuiApplication::new();
    let mut qml_engine = QQmlApplicationEngine::new();
    if let Some(eng) = qml_engine.as_mut() {
        eng.load(&QUrl::from("qrc:/qt/qml/org/dueattendant149/kwhale/qml/Main.qml"));
    }
    if let Some(a) = app.as_mut() {
        a.exec();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_file_urls_and_encodes_svg_data_uri() {
        let p = file_url_to_path("file:///home/user/%D0%9C%D1%83%D0%B7%D1%8B%D0%BA%D0%B0/song%201.flac");
        assert_eq!(
            p,
            PathBuf::from("/home/user/Музыка/song 1.flac")
        );
        let uri = svg_data_uri("<svg width=\"64\"></svg>");
        assert!(uri.starts_with("data:image/svg+xml;utf8,"));
        assert!(uri.contains("%3Csvg"));
    }
}
