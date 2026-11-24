use crate::images::{
    decode_rounded_rgba, make_collage_cover, make_hero_backdrop_from_cover,
    make_hero_backdrop_image, make_placeholder_image, spawn_fetch_cover,
    spawn_fetch_cover_decoded, CoverMemoryCache, DecodedCover,
};
use crate::{
    ArtistItem, DownloadItem, GenreChip, LyricLineItem, MainWindow, MixChipItem, ShelfItem,
    SidebarPlaylistItem, TrackItem, TrackMenuInfo,
};
use kwhale_core::{
    format_duration_ms, format_duration_player_ms, Album, Artist, KwhaleCore, KwhalePlural,
    Playlist, Rgb, Track, TrackDownloadStatus, UiTrack,
};
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, Image, Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct CachedHomeSnapshot {
    #[serde(default)]
    pub recent_title: String,
    #[serde(default)]
    pub chart_title: String,
    #[serde(default)]
    pub tracks_title: String,
    pub recent_shelves: Vec<ShelfSeed>,
    pub chart_shelves: Vec<ShelfSeed>,
    pub mixes: Vec<MixSeed>,
    pub hero_slides: Vec<HeroSlideSeed>,
    pub tracks: Vec<UiTrack>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ShelfSeed {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub seed: String,
    pub cover_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ArtistSeed {
    pub id: String,
    pub name: String,
    pub followers_text: String,
    pub avatar_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MixSeed {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: String,
    pub cover_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HeroSlideSeed {
    pub kind: String, // "track" | "mix"
    pub id: String,
    pub badge: String,
    pub title: String,
    pub subtitle: String,
    pub time_text: String,
    pub progress: f32,
    pub track: Option<UiTrack>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct CachedLibrarySnapshot {
    pub playlists: Vec<Playlist>,
    pub albums: Vec<Album>,
    pub tracks: Vec<UiTrack>,
    pub total_tracks: i32,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct CachedSearchBootstrap {
    pub history: Vec<String>,
    pub genres: Vec<(String, String)>,
    pub artists: Vec<ArtistSeed>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct CachedProfileSnapshot {
    pub name: String,
    pub email: String,
    pub avatar_id: Option<String>,
    pub track_count: i32,
    pub playlist_count: i32,
}

#[derive(Clone)]
pub struct TrackMenuContext {
    pub track: UiTrack,
    pub source: String,
}

pub struct AppRuntimeState {
    pub now_dominant: Mutex<Rgb>,
    pub current_accent: Mutex<Rgb>,
    pub lyrics: Mutex<Vec<(u64, String)>>,
    pub hero_slides: Mutex<Vec<HeroSlideSeed>>,
    pub home_all_recent: Mutex<Vec<ShelfSeed>>,
    pub home_all_chart: Mutex<Vec<ShelfSeed>>,
    pub home_recent_offset: Mutex<usize>,
    pub home_chart_offset: Mutex<usize>,
    pub home_mixes: Mutex<Vec<MixSeed>>,
    pub home_tracks: Mutex<Vec<UiTrack>>,
    pub search_tracks: Mutex<Vec<UiTrack>>,
    pub search_artists: Mutex<Vec<ArtistSeed>>,
    pub search_shelves: Mutex<Vec<ShelfSeed>>,
    pub library_tracks: Mutex<Vec<UiTrack>>,
    pub library_playlists: Mutex<Vec<Playlist>>,
    pub library_albums: Mutex<Vec<Album>>,
    pub library_next_cursor: Mutex<Option<String>>,
    pub history_tracks: Mutex<Vec<UiTrack>>,
    pub history_next_cursor: Mutex<Option<String>>,
    pub download_tracks: Mutex<Vec<UiTrack>>,
    pub collection_tracks: Mutex<Vec<UiTrack>>,
    pub collection_albums: Mutex<Vec<ShelfSeed>>,
    pub collection_singles: Mutex<Vec<ShelfSeed>>,
    pub collection_kind: Mutex<String>,
    pub collection_id: Mutex<String>,
    pub collection_saved: Mutex<bool>,
    pub collection_can_edit: Mutex<bool>,
    pub nav_history: Mutex<Vec<String>>,
    pub menu_context: Mutex<Option<TrackMenuContext>>,
    pub search_debounce_seq: Mutex<u64>,
    pub hero_cover_decoded: Mutex<Option<(String, DecodedCover)>>,
    pub demo_mode: bool,
}

impl AppRuntimeState {
    pub fn new(now_dom: Rgb, accent: Rgb, demo_mode: bool) -> Arc<Self> {
        Arc::new(Self {
            now_dominant: Mutex::new(now_dom),
            current_accent: Mutex::new(accent),
            lyrics: Mutex::new(Vec::new()),
            hero_slides: Mutex::new(Vec::new()),
            home_all_recent: Mutex::new(Vec::new()),
            home_all_chart: Mutex::new(Vec::new()),
            home_recent_offset: Mutex::new(0),
            home_chart_offset: Mutex::new(0),
            home_mixes: Mutex::new(Vec::new()),
            home_tracks: Mutex::new(Vec::new()),
            search_tracks: Mutex::new(Vec::new()),
            search_artists: Mutex::new(Vec::new()),
            search_shelves: Mutex::new(Vec::new()),
            library_tracks: Mutex::new(Vec::new()),
            library_playlists: Mutex::new(Vec::new()),
            library_albums: Mutex::new(Vec::new()),
            library_next_cursor: Mutex::new(None),
            history_tracks: Mutex::new(Vec::new()),
            history_next_cursor: Mutex::new(None),
            download_tracks: Mutex::new(Vec::new()),
            collection_tracks: Mutex::new(Vec::new()),
            collection_albums: Mutex::new(Vec::new()),
            collection_singles: Mutex::new(Vec::new()),
            collection_kind: Mutex::new(String::new()),
            collection_id: Mutex::new(String::new()),
            collection_saved: Mutex::new(false),
            collection_can_edit: Mutex::new(false),
            nav_history: Mutex::new(vec!["home".to_string()]),
            menu_context: Mutex::new(None),
            search_debounce_seq: Mutex::new(0),
            hero_cover_decoded: Mutex::new(None),
            demo_mode,
        })
    }
}

pub fn to_model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(v)))
}

pub fn initials_for_name(name: &str) -> String {
    let chars: Vec<char> = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .collect();
    if chars.is_empty() {
        "КВ".to_string()
    } else {
        chars.into_iter().collect()
    }
}

pub fn api_track_to_ui(t: &Track, core: &KwhaleCore) -> UiTrack {
    t.to_ui(core.downloads.is_downloaded(&t.id))
}

pub fn ui_tracks_to_slint(
    tracks: &[UiTrack],
    active_id: Option<&str>,
    core: &KwhaleCore,
    dark: bool,
) -> Vec<TrackItem> {
    tracks
        .iter()
        .map(|t| {
            let liked = core.reactions.is_liked(&t.remote_id, t.is_liked);
            let disliked = core.reactions.is_disliked(&t.remote_id, t.is_disliked);
            let downloaded = t.is_offline_cached || core.downloads.is_downloaded(&t.remote_id);
            TrackItem {
                id: SharedString::from(&t.remote_id),
                title: SharedString::from(&t.title),
                artist: SharedString::from(&t.artist),
                album: SharedString::from(t.album.as_deref().unwrap_or_default()),
                duration: SharedString::from(format_duration_ms((t.duration_sec as u64) * 1000)),
                cover: make_placeholder_image(&t.title, dark, 160, 0.0),
                active: active_id.is_some_and(|aid| aid == t.remote_id),
                liked,
                disliked,
                downloaded,
                explicit: t.is_explicit,
            }
        })
        .collect()
}

pub fn shelf_seeds_to_slint(items: &[ShelfSeed], dark: bool) -> Vec<ShelfItem> {
    items
        .iter()
        .map(|s| ShelfItem {
            id: SharedString::from(&s.id),
            kind: SharedString::from(&s.kind),
            title: SharedString::from(&s.title),
            subtitle: SharedString::from(&s.subtitle),
            cover: make_placeholder_image(&s.seed, dark, 320, 0.0),
        })
        .collect()
}

pub fn artist_seeds_to_slint(items: &[ArtistSeed], dark: bool) -> Vec<ArtistItem> {
    items
        .iter()
        .map(|a| ArtistItem {
            id: SharedString::from(&a.id),
            name: SharedString::from(&a.name),
            followers_text: SharedString::from(&a.followers_text),
            cover: make_placeholder_image(&a.name, dark, 240, 0.0),
        })
        .collect()
}

pub fn mixes_to_chips(items: &[MixSeed]) -> Vec<MixChipItem> {
    items
        .iter()
        .take(8)
        .map(|m| MixChipItem {
            id: SharedString::from(&m.id),
            title: SharedString::from(&m.title),
            subtitle: SharedString::from(&m.subtitle),
            kind: SharedString::from(&m.kind),
            active: false,
        })
        .collect()
}

pub fn artist_to_seed(ar: &Artist) -> ArtistSeed {
    ArtistSeed {
        id: ar.id.clone(),
        name: ar.name.clone(),
        followers_text: if ar.followers > 0 {
            KwhalePlural::listeners(ar.followers.min(i32::MAX as i64) as i32)
        } else {
            String::new()
        },
        avatar_id: ar.avatar_image_id().map(ToOwned::to_owned),
    }
}

pub fn album_to_shelf(al: &Album) -> ShelfSeed {
    let yr = al.year.map(|y| format!(" · {y}")).unwrap_or_default();
    let kind_badge = al
        .type_label()
        .map(|l| format!("{l} · "))
        .unwrap_or_default();
    ShelfSeed {
        id: al.id.clone(),
        kind: "album".to_string(),
        title: al.title.clone(),
        subtitle: format!("{kind_badge}{}{yr}", al.artist_line()),
        seed: al.title.clone(),
        cover_id: al.cover_image_id().map(ToOwned::to_owned),
    }
}

pub fn playlist_to_shelf(pl: &Playlist) -> ShelfSeed {
    ShelfSeed {
        id: pl.id.clone(),
        kind: "playlist".to_string(),
        title: pl.title.clone(),
        subtitle: KwhalePlural::tracks(pl.track_count),
        seed: pl.title.clone(),
        cover_id: pl.cover_image_id().map(ToOwned::to_owned),
    }
}

pub fn sidebar_playlists_to_slint(pls: &[Playlist], dark: bool) -> Vec<SidebarPlaylistItem> {
    pls.iter()
        .map(|p| SidebarPlaylistItem {
            id: SharedString::from(&p.id),
            title: SharedString::from(&p.title),
            count_text: SharedString::from(KwhalePlural::tracks(p.track_count)),
            cover: make_placeholder_image(&p.title, dark, 128, 0.0),
            can_edit: p.permissions.can_edit,
        })
        .collect()
}

pub fn genres_to_chips(genres: &[(String, String)]) -> Vec<GenreChip> {
    genres
        .iter()
        .take(12)
        .map(|(id, name)| GenreChip {
            id: SharedString::from(id),
            name: SharedString::from(name),
        })
        .collect()
}

pub fn active_lyric_index(lines: &[(u64, String)], pos_ms: u64) -> Option<usize> {
    lines.iter().rposition(|(start, _)| *start <= pos_ms)
}

pub fn lyrics_to_slint(lines: &[(u64, String)], pos_ms: u64) -> Vec<LyricLineItem> {
    let active = active_lyric_index(lines, pos_ms);
    lines
        .iter()
        .enumerate()
        .map(|(i, (ms, text))| LyricLineItem {
            text: SharedString::from(text),
            active: active == Some(i),
            start_ms: (*ms).min(i32::MAX as u64) as i32,
        })
        .collect()
}

pub fn apply_hero_slide_to_ui(
    ui: &MainWindow,
    slide: &HeroSlideSeed,
    idx: usize,
    accent: Rgb,
    dark: bool,
) {
    ui.set_hero_slide(idx as i32);
    ui.set_hero_badge(SharedString::from(&slide.badge));
    ui.set_hero_title(SharedString::from(&slide.title));
    ui.set_hero_artist(SharedString::from(&slide.subtitle));
    ui.set_hero_time(SharedString::from(&slide.time_text));
    ui.set_hero_progress(slide.progress);
    ui.set_hero_backdrop(make_hero_backdrop_image(&slide.title, accent, dark, 960, 440, 0.0));
    if slide.track.is_some() {
        ui.set_hero_cover(make_placeholder_image(&slide.title, dark, 256, 0.0));
        ui.set_has_hero_cover(true);
    } else {
        ui.set_has_hero_cover(false);
    }
}

pub fn hydrate_hero_slide_cover(
    ui_weak: slint::Weak<MainWindow>,
    rt: &tokio::runtime::Handle,
    core: &Arc<KwhaleCore>,
    cache: &CoverMemoryCache,
    app_state: &Arc<AppRuntimeState>,
    slide: &HeroSlideSeed,
) {
    if core.api.session().is_none() {
        return;
    }
    let slide_title = slide.title.clone();
    let st = app_state.clone();
    let apply_loaded_cover = move |cov: DecodedCover, ui_weak: slint::Weak<MainWindow>, st: Arc<AppRuntimeState>, slide_title: String| {
        let Some(u) = ui_weak.upgrade() else { return };
        if u.get_hero_title().as_str() != slide_title.as_str() {
            return;
        }
        let dk = u.global::<crate::Theme>().get_is_dark();
        let acc = *st.current_accent.lock().expect("acc");
        *st.hero_cover_decoded.lock().expect("hcd") = Some((slide_title, cov.clone()));
        u.set_hero_cover(cov.resized_lanczos(256).to_slint_image());
        u.set_has_hero_cover(true);
        u.set_hero_backdrop(make_hero_backdrop_from_cover(
            &cov.rgba,
            cov.width,
            cov.height,
            cov.dominant,
            acc,
            dk,
            960,
            440,
        ));
    };

    if let Some(cid) = slide.track.as_ref().and_then(|t| t.cover_image_id.clone()) {
        spawn_fetch_cover_decoded(rt, core.clone(), cache.clone(), cid, "lg", 640, 0.0, move |cov| {
            apply_loaded_cover(cov, ui_weak, st, slide_title);
        });
    } else if slide.kind == "mix" {
        let c = core.clone();
        let cc = cache.clone();
        let mid = slide.id.clone();
        rt.spawn(async move {
            let Some(cov) = build_composite_collage(&c, &cc, "mix", &mid, 512).await else {
                *st.hero_cover_decoded.lock().expect("hcd") = None;
                return;
            };
            let _ = slint::invoke_from_event_loop(move || {
                apply_loaded_cover(cov, ui_weak, st, slide_title);
            });
        });
    } else {
        *app_state.hero_cover_decoded.lock().expect("hcd") = None;
    }
}

pub fn shuffle_tracks(mut tracks: Vec<UiTrack>) -> Vec<UiTrack> {
    let n = tracks.len();
    if n <= 1 {
        return tracks;
    }
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(0x5EED);
    for i in (1..n).rev() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = (seed as usize) % (i + 1);
        tracks.swap(i, j);
    }
    tracks
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} МБ", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{} КБ", bytes / 1024)
    } else {
        format!("{bytes} Б")
    }
}

pub fn downloads_to_slint(
    snap: &kwhale_core::DownloadSnapshot,
    dark: bool,
) -> (Vec<DownloadItem>, Vec<UiTrack>) {
    let mut items = Vec::new();
    let mut tracks = Vec::new();
    for act in &snap.active {
        let (st_txt, pct, failed) = match &act.status {
            TrackDownloadStatus::Queued => ("В очереди...".to_string(), 0.0, false),
            TrackDownloadStatus::Downloading { percent } => {
                (format!("Загрузка {percent}%"), *percent as f32 / 100.0, false)
            }
            TrackDownloadStatus::Failed { message } => (format!("Ошибка: {message}"), 0.0, true),
            TrackDownloadStatus::Downloaded { .. } => ("Готово".to_string(), 1.0, false),
            TrackDownloadStatus::NotDownloaded => (String::new(), 0.0, false),
        };
        items.push(DownloadItem {
            id: SharedString::from(&act.track.remote_id),
            title: SharedString::from(&act.track.title),
            artist: SharedString::from(&act.track.artist),
            status_text: SharedString::from(st_txt),
            detail_text: SharedString::default(),
            progress: pct,
            active: !failed,
            failed,
            cover: make_placeholder_image(&act.track.title, dark, 160, 0.0),
        });
        tracks.push(act.track.clone());
    }
    for dl in &snap.downloaded {
        if !dl.exists() {
            continue;
        }
        let ui_t = dl.to_ui_track();
        items.push(DownloadItem {
            id: SharedString::from(&dl.remote_id),
            title: SharedString::from(&dl.title),
            artist: SharedString::from(&dl.artist),
            status_text: SharedString::from(format!(
                "Скачано · {}",
                format_bytes(dl.bytes)
            )),
            detail_text: SharedString::from(format_duration_player_ms((dl.duration_sec as u64) * 1000)),
            progress: 1.0,
            active: false,
            failed: false,
            cover: make_placeholder_image(&dl.title, dark, 160, 0.0),
        });
        tracks.push(ui_t);
    }
    (items, tracks)
}

pub enum TrackListTarget {
    Home,
    Search,
    Library,
    History,
    Collection,
    Queue,
}

pub enum ShelfListTarget {
    Recent,
    Chart,
    Search,
    Library,
    LibraryRow2,
    CollectionAlbums,
    CollectionSingles,
}

pub fn hydrate_track_covers(
    ui_weak: slint::Weak<MainWindow>,
    rt: &tokio::runtime::Handle,
    core: &Arc<KwhaleCore>,
    cache: &CoverMemoryCache,
    tracks: &[UiTrack],
    target: TrackListTarget,
) {
    if core.api.session().is_none() {
        return;
    }
    for (idx, tr) in tracks.iter().enumerate() {
        let Some(cid) = tr.cover_image_id.clone() else {
            continue;
        };
        let expected_id = tr.remote_id.clone();
        let w = ui_weak.clone();
        let target_tag = match target {
            TrackListTarget::Home => 0u8,
            TrackListTarget::Search => 1,
            TrackListTarget::Library => 2,
            TrackListTarget::History => 3,
            TrackListTarget::Collection => 4,
            TrackListTarget::Queue => 5,
        };
        spawn_fetch_cover(rt, core.clone(), cache.clone(), cid, "sm", 160, 0.0, move |img: Image, _| {
            let Some(u) = w.upgrade() else { return };
            let model = match target_tag {
                0 => u.get_tracks(),
                1 => u.get_search_tracks(),
                2 => u.get_library_tracks(),
                3 => u.get_history_tracks(),
                4 => u.get_collection_tracks(),
                _ => u.get_queue_items(),
            };
            if let Some(mut slot) = model.row_data(idx) {
                if slot.id == expected_id {
                    slot.cover = img;
                    model.set_row_data(idx, slot);
                }
            }
        });
    }
}

static COLLAGE_SEM: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

pub async fn build_collage_from_cover_ids(
    core: &Arc<KwhaleCore>,
    cache: &CoverMemoryCache,
    cache_key_id: &str,
    ids: Vec<String>,
    target_size: u32,
) -> Option<DecodedCover> {
    let collage_key = format!("collage:{cache_key_id}:{target_size}");
    if let Some(hit) = cache.get(&collage_key) {
        return Some(hit);
    }
    let unique_ids: Vec<String> = ids.into_iter().take(4).collect();
    if unique_ids.is_empty() {
        return None;
    }

    let cell_sz = if unique_ids.len() == 1 {
        target_size
    } else {
        (target_size / 2).max(64)
    };

    let mut join_set = tokio::task::JoinSet::new();
    for (slot_idx, cid) in unique_ids.into_iter().enumerate() {
        let cell_key = format!("{cid}:md:{cell_sz}:0.0");
        if let Some(cached) = cache.get(&cell_key) {
            join_set.spawn(async move { Some((slot_idx, cached)) });
            continue;
        }
        let c = core.clone();
        let cc = cache.clone();
        join_set.spawn(async move {
            let bytes = match c.get_or_fetch_image(&cid, "md").await {
                Ok((_, b)) => b,
                Err(err) => {
                    tracing::warn!(image_id = %cid, error = %err, "failed to fetch collage cell cover");
                    return None;
                }
            };
            let decoded = tokio::task::spawn_blocking(move || decode_rounded_rgba(&bytes, cell_sz, 0.0))
                .await
                .ok()
                .flatten()?;
            cc.put(cell_key, decoded.clone());
            Some((slot_idx, decoded))
        });
    }

    let mut indexed: Vec<(usize, DecodedCover)> = Vec::new();
    while let Some(res) = join_set.join_next().await {
        if let Ok(Some(item)) = res {
            indexed.push(item);
        }
    }
    if indexed.is_empty() {
        return None;
    }
    indexed.sort_by_key(|(idx, _)| *idx);
    let covers: Vec<DecodedCover> = indexed.into_iter().map(|(_, c)| c).collect();
    let collage = make_collage_cover(&covers, target_size);
    cache.put(collage_key, collage.clone());
    Some(collage)
}

fn push_unique_cover_id(ids: &mut Vec<String>, cid: Option<&str>) -> bool {
    if let Some(s) = cid {
        if !ids.iter().any(|x| x == s) {
            ids.push(s.to_string());
        }
    }
    ids.len() >= 4
}

async fn build_composite_collage(
    core: &Arc<KwhaleCore>,
    cache: &CoverMemoryCache,
    kind: &str,
    id: &str,
    target_size: u32,
) -> Option<DecodedCover> {
    let key_id = format!("{kind}:{id}");
    let collage_key = format!("collage:{key_id}:{target_size}");
    if let Some(hit) = cache.get(&collage_key) {
        return Some(hit);
    }
    let _permit = COLLAGE_SEM.acquire().await.ok()?;
    if let Some(hit) = cache.get(&collage_key) {
        return Some(hit);
    }

    let mut ids: Vec<String> = Vec::new();
    match kind {
        "mix" => {
            if let Ok(pg) = core.api.mix_tracks(id, None, Some(16)).await {
                for r in &pg.items {
                    if push_unique_cover_id(&mut ids, r.track.cover_image_id()) {
                        break;
                    }
                }
            }
        }
        "playlist" => {
            if let Ok(pg) = core.api.playlist_tracks(id, None, Some(16)).await {
                for t in &pg.items {
                    if push_unique_cover_id(&mut ids, t.cover_image_id()) {
                        break;
                    }
                }
            }
        }
        _ => return None,
    }
    build_collage_from_cover_ids(core, cache, &key_id, ids, target_size).await
}

pub fn hydrate_collection_collage<F>(
    rt: &tokio::runtime::Handle,
    core: Arc<KwhaleCore>,
    cache: CoverMemoryCache,
    kind: String,
    id: String,
    tracks: &[UiTrack],
    target_size: u32,
    on_ready: F,
) where
    F: FnOnce(Image) + Send + 'static,
{
    let mut ids: Vec<String> = Vec::new();
    for t in tracks {
        if push_unique_cover_id(&mut ids, t.cover_image_id.as_deref()) {
            break;
        }
    }
    if ids.is_empty() {
        return;
    }
    let key_id = format!("{kind}:{id}");
    rt.spawn(async move {
        let Some(cov) = build_collage_from_cover_ids(&core, &cache, &key_id, ids, target_size).await else {
            return;
        };
        let _ = slint::invoke_from_event_loop(move || {
            on_ready(cov.to_slint_image());
        });
    });
}

fn shelf_model_for_tag(u: &MainWindow, tag: u8) -> ModelRc<ShelfItem> {
    match tag {
        0 => u.get_recent_releases(),
        1 => u.get_chart_items(),
        2 => u.get_search_shelves(),
        3 => u.get_library_collections(),
        4 => u.get_collection_albums(),
        5 => u.get_collection_singles(),
        _ => u.get_library_collections_row2(),
    }
}

pub fn hydrate_shelf_covers(
    ui_weak: slint::Weak<MainWindow>,
    rt: &tokio::runtime::Handle,
    core: &Arc<KwhaleCore>,
    cache: &CoverMemoryCache,
    shelves: &[ShelfSeed],
    target: ShelfListTarget,
) {
    if core.api.session().is_none() {
        return;
    }
    for (idx, sh) in shelves.iter().enumerate() {
        let expected_id = sh.id.clone();
        let w = ui_weak.clone();
        let tag = match target {
            ShelfListTarget::Recent => 0u8,
            ShelfListTarget::Chart => 1,
            ShelfListTarget::Search => 2,
            ShelfListTarget::Library => 3,
            ShelfListTarget::CollectionAlbums => 4,
            ShelfListTarget::CollectionSingles => 5,
            ShelfListTarget::LibraryRow2 => 6,
        };
        if let Some(cid) = sh.cover_id.clone() {
            spawn_fetch_cover(rt, core.clone(), cache.clone(), cid, "md", 512, 0.0, move |img: Image, _| {
                let Some(u) = w.upgrade() else { return };
                let model = shelf_model_for_tag(&u, tag);
                if let Some(mut slot) = model.row_data(idx) {
                    if slot.id == expected_id {
                        slot.cover = img;
                        model.set_row_data(idx, slot);
                    }
                }
            });
        } else if matches!(sh.kind.as_str(), "mix" | "playlist") {
            let c = core.clone();
            let cc = cache.clone();
            let kind = sh.kind.clone();
            rt.spawn(async move {
                let Some(cov) = build_composite_collage(&c, &cc, &kind, &expected_id, 512).await else {
                    return;
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(u) = w.upgrade() else { return };
                    let model = shelf_model_for_tag(&u, tag);
                    if let Some(mut slot) = model.row_data(idx) {
                        if slot.id == expected_id {
                            slot.cover = cov.to_slint_image();
                            model.set_row_data(idx, slot);
                        }
                    }
                });
            });
        }
    }
}

pub fn apply_library_shelves(
    ui: &MainWindow,
    ui_weak: slint::Weak<MainWindow>,
    rt: &tokio::runtime::Handle,
    core: &Arc<KwhaleCore>,
    cache: &CoverMemoryCache,
    cols: Vec<ShelfSeed>,
    dark: bool,
) {
    let row1: Vec<ShelfSeed> = cols.iter().take(5).cloned().collect();
    let row2: Vec<ShelfSeed> = cols.iter().skip(5).take(5).cloned().collect();
    ui.set_library_collections(to_model(shelf_seeds_to_slint(&row1, dark)));
    ui.set_library_collections_row2(to_model(shelf_seeds_to_slint(&row2, dark)));
    hydrate_shelf_covers(ui_weak.clone(), rt, core, cache, &row1, ShelfListTarget::Library);
    hydrate_shelf_covers(ui_weak, rt, core, cache, &row2, ShelfListTarget::LibraryRow2);
}

pub fn hydrate_artist_covers(
    ui_weak: slint::Weak<MainWindow>,
    rt: &tokio::runtime::Handle,
    core: &Arc<KwhaleCore>,
    cache: &CoverMemoryCache,
    artists: &[ArtistSeed],
) {
    if core.api.session().is_none() {
        return;
    }
    for (idx, ar) in artists.iter().enumerate() {
        let Some(cid) = ar.avatar_id.clone() else {
            continue;
        };
        let expected_id = ar.id.clone();
        let w = ui_weak.clone();
        spawn_fetch_cover(rt, core.clone(), cache.clone(), cid, "md", 320, 0.0, move |img: Image, _| {
            let Some(u) = w.upgrade() else { return };
            let model = u.get_search_artists();
            if let Some(mut slot) = model.row_data(idx) {
                if slot.id == expected_id {
                    slot.cover = img;
                    model.set_row_data(idx, slot);
                }
            }
        });
    }
}

pub fn hydrate_sidebar_playlist_covers(
    ui_weak: slint::Weak<MainWindow>,
    rt: &tokio::runtime::Handle,
    core: &Arc<KwhaleCore>,
    cache: &CoverMemoryCache,
    pls: &[Playlist],
) {
    if core.api.session().is_none() {
        return;
    }
    for (idx, pl) in pls.iter().enumerate() {
        let expected_id = pl.id.clone();
        let w = ui_weak.clone();
        if let Some(cid) = pl.cover_image_id().map(ToOwned::to_owned) {
            spawn_fetch_cover(rt, core.clone(), cache.clone(), cid, "sm", 128, 0.0, move |img: Image, _| {
                let Some(u) = w.upgrade() else { return };
                let model = u.get_sidebar_playlists();
                if let Some(mut slot) = model.row_data(idx) {
                    if slot.id == expected_id {
                        slot.cover = img;
                        model.set_row_data(idx, slot);
                    }
                }
            });
        } else {
            let c = core.clone();
            let cc = cache.clone();
            rt.spawn(async move {
                let Some(cov) = build_composite_collage(&c, &cc, "playlist", &expected_id, 128).await else {
                    return;
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(u) = w.upgrade() else { return };
                    let model = u.get_sidebar_playlists();
                    if let Some(mut slot) = model.row_data(idx) {
                        if slot.id == expected_id {
                            slot.cover = cov.to_slint_image();
                            model.set_row_data(idx, slot);
                        }
                    }
                });
            });
        }
    }
}

pub fn track_menu_info_from_track(
    tr: &UiTrack,
    core: &KwhaleCore,
    dark: bool,
    in_editable_playlist: bool,
) -> TrackMenuInfo {
    TrackMenuInfo {
        visible: true,
        track_id: SharedString::from(&tr.remote_id),
        title: SharedString::from(&tr.title),
        artist: SharedString::from(&tr.artist),
        album: SharedString::from(tr.album.as_deref().unwrap_or_default()),
        album_id: SharedString::from(tr.album_remote_id.as_deref().unwrap_or_default()),
        artist_id: SharedString::from(tr.artist_remote_id.as_deref().unwrap_or_default()),
        cover: make_placeholder_image(&tr.title, dark, 96, 0.0),
        liked: core.reactions.is_liked(&tr.remote_id, tr.is_liked),
        disliked: core.reactions.is_disliked(&tr.remote_id, tr.is_disliked),
        downloaded: tr.is_offline_cached || core.downloads.is_downloaded(&tr.remote_id),
        in_editable_playlist,
        details_text: SharedString::default(),
        playlist_picker_open: false,
    }
}

/// Seeds screenshot demo content ONLY when `--demo-playing` is passed.
pub fn apply_demo_seed(
    ui: &MainWindow,
    core: &Arc<KwhaleCore>,
    app_state: &Arc<AppRuntimeState>,
    dark: bool,
    accent: Rgb,
) {
    let recent = vec![
        ShelfSeed { id: "personal".into(), kind: "mix".into(), title: "Для тебя".into(), subtitle: "Персональный микс · 40 треков".into(), seed: "Для тебя".into(), cover_id: None },
        ShelfSeed { id: "discovery".into(), kind: "mix".into(), title: "Открытия".into(), subtitle: "Новые имена · 35 треков".into(), seed: "Открытия".into(), cover_id: None },
        ShelfSeed { id: "mood:focus".into(), kind: "mix".into(), title: "Утренний фокус".into(), subtitle: "Спокойный ритм · 78 треков".into(), seed: "Утро".into(), cover_id: None },
        ShelfSeed { id: "genre:electronic".into(), kind: "mix".into(), title: "Электроника".into(), subtitle: "Жанровый микс · 64 трека".into(), seed: "Электроника".into(), cover_id: None },
        ShelfSeed { id: "mood:road".into(), kind: "mix".into(), title: "В дороге".into(), subtitle: "Бодрый микс · 52 трека".into(), seed: "В дороге".into(), cover_id: None },
    ];
    let chart = vec![
        ShelfSeed { id: "c1".into(), kind: "album".into(), title: "Сигнал из космоса".into(), subtitle: "Космос · 2025".into(), seed: "Космос".into(), cover_id: None },
        ShelfSeed { id: "c2".into(), kind: "album".into(), title: "Морская".into(), subtitle: "Мумий Тролль · 1997".into(), seed: "Морская".into(), cover_id: None },
        ShelfSeed { id: "c3".into(), kind: "album".into(), title: "Группа крови".into(), subtitle: "Кино · 1988".into(), seed: "Кино".into(), cover_id: None },
        ShelfSeed { id: "c4".into(), kind: "album".into(), title: "Синоптик".into(), subtitle: "Cream Soda · 2024".into(), seed: "Cream Soda".into(), cover_id: None },
        ShelfSeed { id: "c5".into(), kind: "album".into(), title: "Пыяла".into(), subtitle: "AIGEL · 2020".into(), seed: "AIGEL".into(), cover_id: None },
    ];
    let rows = [
        ("t1", "Вечерний город", "Молчат Дома", "Этажи", 252, true),
        ("t2", "Птицы", "Кино", "Чёрный альбом", 218, false),
        ("t3", "Когда твоя девушка больна", "Кино", "Чёрный альбом", 259, false),
        ("t4", "Звезда по имени Солнце", "Кино", "Звезда по имени Солнце", 225, true),
        ("t5", "Группа крови", "Кино", "Группа крови", 284, false),
        ("t6", "Пачка сигарет", "Кино", "Звезда по имени Солнце", 268, false),
        ("t7", "Восьмиклассница", "Кино", "45", 165, false),
        ("t8", "Спокойная ночь", "Молчат Дома", "Монумент", 367, true),
    ];
    let tracks: Vec<UiTrack> = rows
        .into_iter()
        .map(|(id, title, artist, album, dur, liked)| UiTrack {
            id: UiTrack::stable_id_for(id),
            remote_id: id.into(),
            title: title.into(),
            artist: artist.into(),
            artists: vec![artist.into()],
            artist_remote_id: None,
            album: Some(album.into()),
            album_remote_id: None,
            duration_sec: dur,
            genre: None,
            year: None,
            is_explicit: false,
            stream_path: format!("/tracks/{id}/stream"),
            cover_image_id: None,
            has_lyrics: false,
            is_liked: liked,
            is_disliked: false,
            is_offline_cached: false,
            is_censorship_reported: false,
        })
        .collect();

    let hero = vec![HeroSlideSeed {
        kind: "track".into(),
        id: "t1".into(),
        badge: "Продолжить слушать".into(),
        title: "Вечерний город".into(),
        subtitle: "Молчат Дома".into(),
        time_text: "1:24 / 4:12".into(),
        progress: 0.34,
        track: tracks.first().cloned(),
    }];

    *app_state.home_all_recent.lock().expect("recent") = recent.clone();
    *app_state.home_all_chart.lock().expect("chart") = chart.clone();
    *app_state.home_tracks.lock().expect("ht") = tracks.clone();
    *app_state.collection_tracks.lock().expect("ct") = tracks.clone();
    *app_state.hero_slides.lock().expect("hero") = hero.clone();

    let slint_tracks = ui_tracks_to_slint(&tracks, Some("t1"), core, dark);
    ui.set_recent_section_title("Для тебя и открытия".into());
    ui.set_chart_section_title("Недавно добавленные альбомы".into());
    ui.set_tracks_section_title("Недавно прослушанное".into());
    ui.set_recent_releases(to_model(shelf_seeds_to_slint(&recent, dark)));
    ui.set_chart_items(to_model(shelf_seeds_to_slint(&chart, dark)));
    ui.set_tracks(to_model(slint_tracks.clone()));
    ui.set_queue_items(to_model(slint_tracks.clone()));
    ui.set_history_tracks(to_model(slint_tracks.clone()));
    ui.set_collection_tracks(to_model(slint_tracks.clone()));
    ui.set_library_collections(to_model(shelf_seeds_to_slint(&recent, dark)));
    ui.set_library_collections_row2(to_model(shelf_seeds_to_slint(&chart, dark)));
    ui.set_library_tracks(to_model(slint_tracks));
    ui.set_collection_kind("mix".into());
    ui.set_collection_title("Для тебя".into());
    ui.set_collection_subtitle("Персональный микс".into());
    ui.set_collection_cover(make_placeholder_image("Для тебя", dark, 320, 0.0));
    ui.set_liked_count_text("412 треков".into());
    ui.set_sidebar_playlists(to_model(vec![
        SidebarPlaylistItem { id: "p1".into(), title: "Для работы".into(), count_text: "87 треков".into(), cover: make_placeholder_image("Для работы", dark, 128, 0.0), can_edit: true },
        SidebarPlaylistItem { id: "p2".into(), title: "Вечерний микс".into(), count_text: "124 трека".into(), cover: make_placeholder_image("Вечерний микс", dark, 128, 0.0), can_edit: true },
        SidebarPlaylistItem { id: "p3".into(), title: "Путешествия".into(), count_text: "76 треков".into(), cover: make_placeholder_image("Путешествия", dark, 128, 0.0), can_edit: true },
    ]));
    apply_hero_slide_to_ui(ui, &hero[0], 0, accent, dark);
    ui.set_user_initials("ВД".into());
    ui.set_user_name("Демо пользователь".into());
    ui.set_has_current_track(true);
    ui.set_now_title("Вечерний город".into());
    ui.set_now_artist("Молчат Дома".into());
    ui.set_now_cover(make_placeholder_image("Вечерний город", dark, 256, 0.0));
    ui.set_now_cover_hd(make_placeholder_image("Вечерний город", dark, 512, 0.0));
    ui.set_now_liked(true);
    ui.set_now_pos_text("1:24".into());
    ui.set_now_dur_text("4:12".into());
    ui.set_progress(0.34);
    let demo_lyr = vec![
        (0, "Огни зажигаются вдоль пустых проспектов".to_string()),
        (22_000, "Электрический ветер несёт нас сквозь ночь".to_string()),
        (48_000, "Над крышами гаснет закат фиолетовый".to_string()),
        (76_000, "Вечерний город дышит в такт с тобой".to_string()),
        (105_000, "Неон отражается в мокром асфальте".to_string()),
        (138_000, "И музыка держит нас до утра".to_string()),
    ];
    *app_state.lyrics.lock().expect("lyr") = demo_lyr.clone();
    ui.set_lyric_lines(to_model(lyrics_to_slint(&demo_lyr, 84_000)));

    ui.set_mix_chips(to_model(vec![
        MixChipItem { id: "m1".into(), title: "Ночная дорога".into(), subtitle: "Подборка для позднего вечера".into(), kind: "mix".into(), active: false },
        MixChipItem { id: "m2".into(), title: "Пост-панк волна".into(), subtitle: "Настроение и любимые группы".into(), kind: "mix".into(), active: false },
        MixChipItem { id: "m3".into(), title: "Спокойный фокус".into(), subtitle: "Инструменталы и эмбиент".into(), kind: "mix".into(), active: false },
    ]));
    ui.set_search_history(to_model(vec![
        SharedString::from("Молчат Дома"),
        SharedString::from("Кино"),
        SharedString::from("Сигнал"),
    ]));
    ui.set_search_genres(to_model(vec![
        GenreChip { id: "postpunk".into(), name: "Пост-панк".into() },
        GenreChip { id: "synth".into(), name: "Синтвейв".into() },
        GenreChip { id: "indie".into(), name: "Инди-рок".into() },
        GenreChip { id: "electronic".into(), name: "Электроника".into() },
        GenreChip { id: "ambient".into(), name: "Эмбиент".into() },
    ]));
    let artists = vec![
        ArtistSeed { id: "a1".into(), name: "Молчат Дома".into(), followers_text: "84 120 слушателей".into(), avatar_id: None },
        ArtistSeed { id: "a2".into(), name: "Кино".into(), followers_text: "215 400 слушателей".into(), avatar_id: None },
        ArtistSeed { id: "a3".into(), name: "Cream Soda".into(), followers_text: "62 300 слушателей".into(), avatar_id: None },
        ArtistSeed { id: "a4".into(), name: "AIGEL".into(), followers_text: "48 900 слушателей".into(), avatar_id: None },
    ];
    ui.set_search_artists(to_model(artist_seeds_to_slint(&artists, dark)));
}
