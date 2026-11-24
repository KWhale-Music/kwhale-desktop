use crate::images::{
    make_hero_backdrop_from_cover, make_hero_backdrop_image, make_placeholder_image,
    make_player_backdrop_image, spawn_fetch_cover, CoverMemoryCache,
};
use crate::platform;
use crate::state::{
    album_to_shelf, api_track_to_ui, apply_hero_slide_to_ui, apply_library_shelves,
    artist_seeds_to_slint, artist_to_seed, genres_to_chips, hydrate_artist_covers,
    hydrate_collection_collage, hydrate_hero_slide_cover, hydrate_shelf_covers,
    hydrate_sidebar_playlist_covers, hydrate_track_covers, initials_for_name, mixes_to_chips,
    playlist_to_shelf, shelf_seeds_to_slint, shuffle_tracks, sidebar_playlists_to_slint, to_model,
    track_menu_info_from_track, ui_tracks_to_slint, AppRuntimeState, ArtistSeed,
    CachedHomeSnapshot, CachedLibrarySnapshot, CachedProfileSnapshot, CachedSearchBootstrap,
    HeroSlideSeed, MixSeed, ShelfListTarget, ShelfSeed, TrackListTarget, TrackMenuContext,
};
use crate::MainWindow;
use crate::Theme;
use kwhale_audio::AudioEngine;
use kwhale_core::{
    accent_by_id, format_duration_player_ms, ContentCache, KwhaleCore,
    KwhalePalette, KwhalePlural, PlatformStyle, PlaylistCreate, PlaylistUpdate,
    PromptMixRequest, Rgb, ServerUrl, ShareKind, ThemeMode, UiTrack,
};
use slint::{Color, ComponentHandle, Image, SharedString};
use std::sync::Arc;
use std::time::Duration;

pub type ToastFn = Arc<dyn Fn(String) + Send + Sync>;

pub fn rgb_to_slint(c: Rgb) -> Color {
    Color::from_rgb_u8(c.r, c.g, c.b)
}

pub fn apply_theme_to_ui(
    ui: &MainWindow,
    core: &KwhaleCore,
    app_state: &AppRuntimeState,
    system_is_dark: bool,
    system_accent: Option<Rgb>,
    platform_override: Option<PlatformStyle>,
    theme_override: Option<ThemeMode>,
    accent_override: Option<i32>,
) -> Rgb {
    let snap = core.settings.snapshot();
    let active_mode = theme_override.unwrap_or(snap.theme_mode);
    let active_platform = platform_override.unwrap_or(snap.platform_style);
    let eff_accent_id = accent_override.unwrap_or(snap.accent_id);
    let accent_rgb = if accent_override.is_some() || !snap.follow_system_accent {
        accent_by_id(eff_accent_id).rgb()
    } else {
        system_accent.unwrap_or_else(|| accent_by_id(eff_accent_id).rgb())
    };
    let pal = KwhalePalette::build(active_mode, accent_rgb, system_is_dark);
    let resolved_platform = platform::resolve_platform_style(active_platform);

    let theme = ui.global::<Theme>();
    theme.set_is_dark(pal.is_dark);
    theme.set_is_amoled(pal.is_amoled);
    theme.set_surface(rgb_to_slint(pal.surface));
    theme.set_surface_lowest(rgb_to_slint(pal.surface_container_lowest));
    theme.set_surface_low(rgb_to_slint(pal.surface_container_low));
    theme.set_surface_container(rgb_to_slint(pal.surface_container));
    theme.set_surface_high(rgb_to_slint(pal.surface_container_high));
    theme.set_surface_highest(rgb_to_slint(pal.surface_container_highest));
    theme.set_on_surface(rgb_to_slint(pal.on_surface));
    theme.set_on_surface_variant(rgb_to_slint(pal.on_surface_variant));
    theme.set_outline(rgb_to_slint(pal.outline));
    theme.set_outline_variant(rgb_to_slint(pal.outline_variant));
    theme.set_primary(rgb_to_slint(pal.primary));
    theme.set_on_primary(rgb_to_slint(pal.on_primary));
    theme.set_primary_container(rgb_to_slint(pal.primary_container));
    theme.set_on_primary_container(rgb_to_slint(pal.on_primary_container));
    theme.set_secondary_container(rgb_to_slint(pal.secondary_container));
    theme.set_on_secondary_container(rgb_to_slint(pal.on_secondary_container));
    theme.set_error(rgb_to_slint(pal.error));
    theme.set_error_container(rgb_to_slint(pal.error_container));
    let white = Rgb::from_u32(0xFFFFFF);
    let black = Rgb::from_u32(0x000000);
    let primary_hover = if pal.is_dark {
        pal.primary.blend(white, 0.16)
    } else {
        pal.primary.blend(black, 0.14)
    };
    let primary_pressed = if pal.is_dark {
        pal.primary.blend(black, 0.16)
    } else {
        pal.primary.blend(black, 0.28)
    };
    let overlay_accent = if pal.primary.luminance() < 0.45 {
        accent_rgb.blend(white, 0.38)
    } else {
        pal.primary
    };
    let overlay_on_accent = overlay_accent.on_color();

    theme.set_primary_hover(rgb_to_slint(primary_hover));
    theme.set_primary_pressed(rgb_to_slint(primary_pressed));
    theme.set_overlay_accent(rgb_to_slint(overlay_accent));
    theme.set_overlay_on_accent(rgb_to_slint(overlay_on_accent));
    theme.set_platform_id(SharedString::from(resolved_platform.as_str()));

    ui.set_platform_setting(SharedString::from(active_platform.as_str()));
    ui.set_theme_mode(SharedString::from(active_mode.as_str()));
    ui.set_follow_system_accent(accent_override.is_none() && snap.follow_system_accent);
    ui.set_close_to_tray(snap.close_to_tray);

    let featured_ids = [10, 0, 2, 13, 14, 15, 50, 56, 71];
    let swatches = featured_ids
        .into_iter()
        .map(|id| {
            let acc = accent_by_id(id);
            crate::AccentOption {
                id,
                name: SharedString::from(acc.name),
                swatch: rgb_to_slint(acc.rgb()),
                active: (accent_override.is_some() || !snap.follow_system_accent) && eff_accent_id == id,
            }
        })
        .collect();
    ui.set_accent_swatches(to_model(swatches));

    *app_state.current_accent.lock().expect("acc") = accent_rgb;
    let now_dom = *app_state.now_dominant.lock().expect("dom");
    let hero_seed = ui.get_hero_title().to_string();
    let seed = if hero_seed.is_empty() { "kwhale" } else { hero_seed.as_str() };
    let cover_hero_opt = app_state.hero_cover_decoded.lock().expect("hcd").clone();
    if let Some((cov_title, cov)) = cover_hero_opt {
        if cov_title == hero_seed {
            ui.set_hero_backdrop(make_hero_backdrop_from_cover(
                &cov.rgba,
                cov.width,
                cov.height,
                cov.dominant,
                accent_rgb,
                pal.is_dark,
                960,
                440,
            ));
        } else {
            ui.set_hero_backdrop(make_hero_backdrop_image(seed, accent_rgb, pal.is_dark, 960, 440, 0.0));
        }
    } else {
        ui.set_hero_backdrop(make_hero_backdrop_image(seed, accent_rgb, pal.is_dark, 960, 440, 0.0));
    }
    ui.set_player_backdrop(make_player_backdrop_image(now_dom, accent_rgb, pal.is_dark, 800, 450));
    accent_rgb
}

fn push_nav(app_state: &AppRuntimeState, route: &str) {
    let mut hist = app_state.nav_history.lock().expect("nav");
    if hist.last().map(String::as_str) != Some(route) {
        hist.push(route.to_string());
        if hist.len() > 24 {
            hist.remove(0);
        }
    }
}

fn mix_to_shelf(m: &MixSeed) -> ShelfSeed {
    ShelfSeed {
        id: m.id.clone(),
        kind: "mix".to_string(),
        title: m.title.clone(),
        subtitle: m.subtitle.clone(),
        seed: m.title.clone(),
        cover_id: m.cover_id.clone(),
    }
}

fn apply_home_snapshot_to_ui(
    ui: &MainWindow,
    core: &Arc<KwhaleCore>,
    app_state: &Arc<AppRuntimeState>,
    cover_cache: &CoverMemoryCache,
    rt: &tokio::runtime::Handle,
    snap: &CachedHomeSnapshot,
    active_track_id: Option<&str>,
) {
    let dark = ui.global::<Theme>().get_is_dark();
    let accent = *app_state.current_accent.lock().expect("acc");

    *app_state.home_all_recent.lock().expect("hr") = snap.recent_shelves.clone();
    *app_state.home_all_chart.lock().expect("hc") = snap.chart_shelves.clone();
    *app_state.home_recent_offset.lock().expect("hro") = 0;
    *app_state.home_chart_offset.lock().expect("hco") = 0;
    *app_state.home_mixes.lock().expect("hm") = snap.mixes.clone();
    *app_state.home_tracks.lock().expect("ht") = snap.tracks.clone();
    *app_state.hero_slides.lock().expect("hero") = snap.hero_slides.clone();

    let rec_title = if !snap.recent_title.is_empty() {
        snap.recent_title.as_str()
    } else if snap.recent_shelves.first().is_some_and(|s| s.kind == "mix") {
        "Для тебя и открытия"
    } else {
        "Недавно добавленные альбомы"
    };
    let ch_title = if !snap.chart_title.is_empty() {
        snap.chart_title.as_str()
    } else if snap.chart_shelves.first().is_some_and(|s| s.kind == "mix") {
        "Миксы по настроению и жанрам"
    } else {
        "Твои альбомы"
    };
    let tr_title = if !snap.tracks_title.is_empty() {
        snap.tracks_title.as_str()
    } else {
        "Недавно прослушанное"
    };
    ui.set_recent_section_title(SharedString::from(rec_title));
    ui.set_chart_section_title(SharedString::from(ch_title));
    ui.set_tracks_section_title(SharedString::from(tr_title));

    let rec_slice: Vec<ShelfSeed> = snap.recent_shelves.iter().take(5).cloned().collect();
    let ch_slice: Vec<ShelfSeed> = snap.chart_shelves.iter().take(5).cloned().collect();
    ui.set_recent_has_shift(snap.recent_shelves.len() > 5);
    ui.set_chart_has_shift(snap.chart_shelves.len() > 5);
    ui.set_recent_releases(to_model(shelf_seeds_to_slint(&rec_slice, dark)));
    ui.set_chart_items(to_model(shelf_seeds_to_slint(&ch_slice, dark)));
    ui.set_mix_chips(to_model(mixes_to_chips(&snap.mixes)));
    ui.set_tracks(to_model(ui_tracks_to_slint(&snap.tracks, active_track_id, core, dark)));
    ui.set_hero_slide_count(snap.hero_slides.len().clamp(1, 3) as i32);

    let weak = ui.as_weak();
    if ui.get_hero_badge() != "Сейчас играет" {
        if let Some(first) = snap.hero_slides.first() {
            apply_hero_slide_to_ui(ui, first, 0, accent, dark);
            hydrate_hero_slide_cover(weak.clone(), rt, core, cover_cache, app_state, first);
        }
    }

    hydrate_shelf_covers(weak.clone(), rt, core, cover_cache, &rec_slice, ShelfListTarget::Recent);
    hydrate_shelf_covers(weak.clone(), rt, core, cover_cache, &ch_slice, ShelfListTarget::Chart);
    hydrate_track_covers(weak, rt, core, cover_cache, &snap.tracks, TrackListTarget::Home);
}

fn apply_library_snapshot_to_ui(
    ui: &MainWindow,
    core: &Arc<KwhaleCore>,
    app_state: &Arc<AppRuntimeState>,
    cover_cache: &CoverMemoryCache,
    rt: &tokio::runtime::Handle,
    snap: &CachedLibrarySnapshot,
    active_track_id: Option<&str>,
) {
    let dark = ui.global::<Theme>().get_is_dark();
    *app_state.library_playlists.lock().expect("lpl") = snap.playlists.clone();
    *app_state.library_albums.lock().expect("lal") = snap.albums.clone();
    *app_state.library_tracks.lock().expect("ltr") = snap.tracks.clone();

    ui.set_liked_count_text(SharedString::from(KwhalePlural::tracks(
        snap.total_tracks.max(snap.tracks.len() as i32),
    )));
    ui.set_sidebar_playlists(to_model(sidebar_playlists_to_slint(&snap.playlists, dark)));
    ui.set_library_tracks(to_model(ui_tracks_to_slint(&snap.tracks, active_track_id, core, dark)));

    let weak = ui.as_weak();
    let tab = ui.get_library_tab().to_string();
    let cols = build_library_shelves(&tab, &snap.playlists, &snap.albums);
    apply_library_shelves(ui, weak.clone(), rt, core, cover_cache, cols, dark);

    hydrate_sidebar_playlist_covers(weak.clone(), rt, core, cover_cache, &snap.playlists);
    hydrate_track_covers(weak, rt, core, cover_cache, &snap.tracks, TrackListTarget::Library);
}

fn build_library_shelves(
    tab: &str,
    playlists: &[kwhale_core::Playlist],
    albums: &[kwhale_core::Album],
) -> Vec<ShelfSeed> {
    match tab {
        "playlists" => playlists.iter().map(playlist_to_shelf).collect(),
        "albums" => albums.iter().map(album_to_shelf).collect(),
        "tracks" => Vec::new(),
        _ => {
            let mut combined: Vec<ShelfSeed> = playlists.iter().map(playlist_to_shelf).collect();
            combined.extend(albums.iter().map(album_to_shelf));
            combined
        }
    }
}

pub fn load_all_backend_data(
    ui_weak: slint::Weak<MainWindow>,
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    app_state: Arc<AppRuntimeState>,
    cover_cache: CoverMemoryCache,
    rt: tokio::runtime::Handle,
    force_network: bool,
) {
    if core.api.session().is_none() {
        return;
    }

    // Step 1: Hydrate UI immediately from disk cache if available
    if !force_network {
        let active_id = engine.state().current().map(|t| t.remote_id.clone());
        if let Some(prof) = core.cache.read::<CachedProfileSnapshot>(ContentCache::PROFILE) {
            let w = ui_weak.clone();
            let c_p = core.clone();
            let rt_p = rt.clone();
            let cc_p = cover_cache.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = w.upgrade() {
                    u.set_user_name(SharedString::from(&prof.name));
                    u.set_user_email(SharedString::from(&prof.email));
                    u.set_user_initials(SharedString::from(initials_for_name(&prof.name)));
                    let dl_cnt = c_p.downloads.snapshot().downloaded.len();
                    u.set_profile_stats_text(SharedString::from(format!(
                        "{} · {} · {} офлайн",
                        KwhalePlural::tracks(prof.track_count),
                        KwhalePlural::playlists(prof.playlist_count),
                        dl_cnt
                    )));
                    if let Some(cid) = prof.avatar_id {
                        let w2 = w.clone();
                        spawn_fetch_cover(&rt_p, c_p, cc_p, cid, "sm", 128, 0.0, move |img, _| {
                            if let Some(u2) = w2.upgrade() {
                                u2.set_user_avatar(img);
                                u2.set_user_has_avatar(true);
                            }
                        });
                    }
                }
            });
        }
        if let Some(h_snap) = core.cache.read::<CachedHomeSnapshot>(ContentCache::HOME) {
            let w = ui_weak.clone();
            let c_h = core.clone();
            let st_h = app_state.clone();
            let cc_h = cover_cache.clone();
            let rt_h = rt.clone();
            let aid = active_id.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = w.upgrade() {
                    apply_home_snapshot_to_ui(&u, &c_h, &st_h, &cc_h, &rt_h, &h_snap, aid.as_deref());
                }
            });
        }
        if let Some(l_snap) = core.cache.read::<CachedLibrarySnapshot>(ContentCache::LIBRARY) {
            let w = ui_weak.clone();
            let c_l = core.clone();
            let st_l = app_state.clone();
            let cc_l = cover_cache.clone();
            let rt_l = rt.clone();
            let aid = active_id.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = w.upgrade() {
                    apply_library_snapshot_to_ui(&u, &c_l, &st_l, &cc_l, &rt_l, &l_snap, aid.as_deref());
                }
            });
        }
        if let Some(sb) = core.cache.read::<CachedSearchBootstrap>(ContentCache::SEARCH_BOOTSTRAP) {
            let w = ui_weak.clone();
            let c_s = core.clone();
            let st_s = app_state.clone();
            let cc_s = cover_cache.clone();
            let rt_s = rt.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = w.upgrade() {
                    let dk = u.global::<Theme>().get_is_dark();
                    *st_s.search_artists.lock().expect("sa") = sb.artists.clone();
                    u.set_search_history(to_model(sb.history.into_iter().map(SharedString::from).collect()));
                    u.set_search_genres(to_model(genres_to_chips(&sb.genres)));
                    u.set_search_artists(to_model(artist_seeds_to_slint(&sb.artists, dk)));
                    hydrate_artist_covers(w, &rt_s, &c_s, &cc_s, &sb.artists);
                }
            });
        }
    }

    // Step 2: Fetch fresh live data concurrently from KWhale server
    {
        let w = ui_weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(u) = w.upgrade() {
                u.set_home_loading(true);
                u.set_home_error(SharedString::default());
            }
        });
    }

    let c = core.clone();
    let eng = engine.clone();
    let st = app_state.clone();
    let cc = cover_cache.clone();
    let rt2 = rt.clone();
    let w = ui_weak.clone();

    rt.spawn(async move {
        let (home_res, mixes_res, hist_res, me_res, lib_tr_res, lib_pl_res, lib_al_res, sh_res, gn_res, ar_res) = tokio::join!(
            c.api.home(None, Some(40)),
            c.api.mixes(None, None, Some(30)),
            c.api.history(None, Some(25)),
            c.api.me(),
            c.api.library_tracks(None, Some(100), "default", None),
            c.api.library_playlists(None, Some(100)),
            c.api.library_albums(None, Some(100)),
            c.api.search_history(),
            c.api.genres(),
            c.api.top_artists(Some(10)),
        );

        let active_id = eng.state().current().map(|t| t.remote_id.clone());

        // Profile update
        if let Ok(me) = me_res {
            let prof = CachedProfileSnapshot {
                name: me.display_name.clone(),
                email: me.email.clone().unwrap_or_default(),
                avatar_id: me.avatar.as_ref().and_then(|a| a.valid_id().map(ToOwned::to_owned)),
                track_count: lib_tr_res.as_ref().ok().and_then(|p| p.total).unwrap_or(0),
                playlist_count: lib_pl_res.as_ref().ok().and_then(|p| p.total).unwrap_or(0),
            };
            c.cache.write(ContentCache::PROFILE, &prof);
            let w_p = w.clone();
            let c_p = c.clone();
            let cc_p = cc.clone();
            let rt_p = rt2.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = w_p.upgrade() {
                    u.set_user_name(SharedString::from(&prof.name));
                    u.set_user_email(SharedString::from(&prof.email));
                    u.set_user_initials(SharedString::from(initials_for_name(&prof.name)));
                    let dl_cnt = c_p.downloads.snapshot().downloaded.len();
                    u.set_profile_stats_text(SharedString::from(format!(
                        "{} · {} · {} офлайн",
                        KwhalePlural::tracks(prof.track_count),
                        KwhalePlural::playlists(prof.playlist_count),
                        dl_cnt
                    )));
                    if let Some(cid) = prof.avatar_id {
                        let w2 = w_p.clone();
                        spawn_fetch_cover(&rt_p, c_p, cc_p, cid, "sm", 128, 0.0, move |img, _| {
                            if let Some(u2) = w2.upgrade() {
                                u2.set_user_avatar(img);
                                u2.set_user_has_avatar(true);
                            }
                        });
                    }
                }
            });
        }

        // Home + Mixes + History
        let mixes_vec: Vec<MixSeed> = mixes_res
            .map(|pg| {
                pg.items
                    .into_iter()
                    .map(|m| MixSeed {
                        id: m.id,
                        title: m.title,
                        subtitle: m
                            .subtitle
                            .unwrap_or_else(|| KwhalePlural::tracks(m.track_count)),
                        kind: m.kind,
                        cover_id: m.cover.as_ref().and_then(|cv| cv.valid_id().map(ToOwned::to_owned)),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let history_vec: Vec<UiTrack> = hist_res
            .as_ref()
            .map(|pg| pg.items.iter().map(|h| api_track_to_ui(&h.track, &c)).collect())
            .unwrap_or_default();
        let history_cursor = hist_res.as_ref().ok().and_then(|pg| pg.next_cursor.clone());
        *st.history_tracks.lock().expect("hist") = history_vec.clone();
        *st.history_next_cursor.lock().expect("hc") = history_cursor.clone();

        {
            let w_hi = w.clone();
            let c_hi = c.clone();
            let cc_hi = cc.clone();
            let rt_hi = rt2.clone();
            let hv = history_vec.clone();
            let has_more_h = history_cursor.is_some();
            let aid = active_id.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = w_hi.upgrade() {
                    let dk = u.global::<Theme>().get_is_dark();
                    u.set_history_has_more(has_more_h);
                    u.set_history_tracks(to_model(ui_tracks_to_slint(&hv, aid.as_deref(), &c_hi, dk)));
                    hydrate_track_covers(w_hi, &rt_hi, &c_hi, &cc_hi, &hv, TrackListTarget::History);
                }
            });
        }

        match home_res {
            Ok(home) => {
                let mut sec_album_groups: Vec<(String, Vec<ShelfSeed>)> = Vec::new();
                let mut home_tracks: Vec<UiTrack> = Vec::new();
                let mut tr_title = "Недавно прослушанное".to_string();

                for section in &home.sections {
                    let sec_id = section.id.as_str();
                    if matches!(sec_id, "playlists" | "albums" | "liked" | "for_you" | "discovery" | "moods") {
                        continue;
                    }
                    let mut sec_albums: Vec<ShelfSeed> = Vec::new();
                    let mut sec_tracks: Vec<UiTrack> = Vec::new();
                    for item in &section.items {
                        if let Some(al) = &item.album {
                            sec_albums.push(album_to_shelf(al));
                        } else if let Some(pl) = &item.playlist {
                            sec_albums.push(playlist_to_shelf(pl));
                        } else if let Some(tr) = &item.track {
                            sec_tracks.push(api_track_to_ui(tr, &c));
                        }
                    }
                    if !sec_albums.is_empty() {
                        let stitle = section
                            .title
                            .clone()
                            .unwrap_or_else(|| "Подборки".to_string());
                        sec_album_groups.push((stitle, sec_albums));
                    }
                    if !sec_tracks.is_empty() {
                        if home_tracks.is_empty() {
                            if let Some(t) = &section.title {
                                tr_title = t.clone();
                            }
                        }
                        for tr in sec_tracks {
                            if !home_tracks.iter().any(|x| x.remote_id == tr.remote_id) {
                                home_tracks.push(tr);
                            }
                        }
                    }
                }

                // Shelf 1: real personalized mixes ("Для тебя", "Открытия", artist/genre/mood mixes)
                let mut ordered_mixes: Vec<ShelfSeed> = Vec::new();
                for kind_pri in ["personal", "discovery", "artist", "genre", "mood"] {
                    for m in &mixes_vec {
                        if m.kind == kind_pri && !ordered_mixes.iter().any(|s| s.id == m.id) {
                            ordered_mixes.push(mix_to_shelf(m));
                        }
                    }
                }
                for m in &mixes_vec {
                    if !ordered_mixes.iter().any(|s| s.id == m.id) {
                        ordered_mixes.push(mix_to_shelf(m));
                    }
                }

                let mut recent_shelves: Vec<ShelfSeed> = Vec::new();
                let mut rec_title = "Для тебя и открытия".to_string();
                let mut chart_shelves: Vec<ShelfSeed> = Vec::new();
                let mut ch_title = "Твои альбомы".to_string();

                let mut used_first_sec = false;
                let mut used_lib_albums_as_primary = false;

                if !ordered_mixes.is_empty() {
                    recent_shelves = ordered_mixes;
                } else if let Some((stitle, items)) = sec_album_groups.first() {
                    rec_title = stitle.clone();
                    recent_shelves = items.clone();
                    used_first_sec = true;
                } else if let Ok(al_pg) = &lib_al_res {
                    if !al_pg.items.is_empty() {
                        recent_shelves = al_pg.items.iter().map(album_to_shelf).collect();
                        rec_title = "Недавно добавленные альбомы".to_string();
                        used_lib_albums_as_primary = true;
                    }
                }

                // Shelf 2: secondary Home section, or real saved albums / playlists (never fake charts)
                if !used_first_sec {
                    if let Some((stitle, items)) = sec_album_groups.first() {
                        ch_title = stitle.clone();
                        chart_shelves = items.clone();
                    }
                } else if let Some((stitle, items)) = sec_album_groups.get(1) {
                    ch_title = stitle.clone();
                    chart_shelves = items.clone();
                }

                if chart_shelves.is_empty() && !used_lib_albums_as_primary {
                    if let Ok(al_pg) = &lib_al_res {
                        if !al_pg.items.is_empty() {
                            chart_shelves = al_pg.items.iter().map(album_to_shelf).collect();
                            ch_title = "Твои альбомы".to_string();
                        }
                    }
                }
                if chart_shelves.is_empty() {
                    if let Ok(pl_pg) = &lib_pl_res {
                        if !pl_pg.items.is_empty() {
                            chart_shelves = pl_pg.items.iter().map(playlist_to_shelf).collect();
                            ch_title = "Твои плейлисты".to_string();
                        }
                    }
                }

                if home_tracks.is_empty() {
                    home_tracks = history_vec.clone();
                    if !home_tracks.is_empty() {
                        tr_title = "Недавно прослушанное".to_string();
                    }
                }

                // Build Hero slides from Continue Listening (history) + Personal mixes
                let mut hero_slides: Vec<HeroSlideSeed> = Vec::new();
                if let Some(last) = history_vec.first() {
                    hero_slides.push(HeroSlideSeed {
                        kind: "track".into(),
                        id: last.remote_id.clone(),
                        badge: "Продолжить слушать".into(),
                        title: last.title.clone(),
                        subtitle: last.artist.clone(),
                        time_text: format_duration_player_ms((last.duration_sec as u64) * 1000),
                        progress: 0.0,
                        track: Some(last.clone()),
                    });
                }
                for mx in mixes_vec.iter().filter(|m| m.kind == "personal" || m.kind == "discovery").take(3) {
                    if hero_slides.len() >= 3 {
                        break;
                    }
                    hero_slides.push(HeroSlideSeed {
                        kind: "mix".into(),
                        id: mx.id.clone(),
                        badge: "Персональный микс".into(),
                        title: mx.title.clone(),
                        subtitle: mx.subtitle.clone(),
                        time_text: String::new(),
                        progress: 0.0,
                        track: None,
                    });
                }

                let snap = CachedHomeSnapshot {
                    recent_title: rec_title,
                    chart_title: ch_title,
                    tracks_title: tr_title,
                    recent_shelves,
                    chart_shelves,
                    mixes: mixes_vec.clone(),
                    hero_slides,
                    tracks: home_tracks,
                };
                c.cache.write(ContentCache::HOME, &snap);

                let w_h = w.clone();
                let c_h = c.clone();
                let st_h = st.clone();
                let cc_h = cc.clone();
                let rt_h = rt2.clone();
                let aid = active_id.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(u) = w_h.upgrade() {
                        u.set_home_loading(false);
                        u.set_home_error(SharedString::default());
                        apply_home_snapshot_to_ui(&u, &c_h, &st_h, &cc_h, &rt_h, &snap, aid.as_deref());
                    }
                });
            }
            Err(err) => {
                let msg = err.user_message();
                let w_e = w.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(u) = w_e.upgrade() {
                        u.set_home_loading(false);
                        u.set_home_error(SharedString::from(msg));
                    }
                });
            }
        }

        // Library update
        let lib_pls = lib_pl_res.map(|p| p.items).unwrap_or_default();
        let lib_als = lib_al_res.map(|p| p.items).unwrap_or_default();
        let (lib_trs, lib_cursor, lib_total) = match lib_tr_res {
            Ok(pg) => {
                let total = pg.total.unwrap_or(pg.items.len() as i32);
                let trs: Vec<UiTrack> = pg.items.iter().map(|t| api_track_to_ui(t, &c)).collect();
                (trs, pg.next_cursor, total)
            }
            Err(_) => (Vec::new(), None, 0),
        };
        *st.library_next_cursor.lock().expect("lnc") = lib_cursor.clone();
        let lib_snap = CachedLibrarySnapshot {
            playlists: lib_pls,
            albums: lib_als,
            tracks: lib_trs,
            total_tracks: lib_total,
        };
        c.cache.write(ContentCache::LIBRARY, &lib_snap);
        {
            let w_l = w.clone();
            let c_l = c.clone();
            let st_l = st.clone();
            let cc_l = cc.clone();
            let rt_l = rt2.clone();
            let aid = active_id.clone();
            let has_more = lib_cursor.is_some();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = w_l.upgrade() {
                    u.set_library_has_more(has_more);
                    apply_library_snapshot_to_ui(&u, &c_l, &st_l, &cc_l, &rt_l, &lib_snap, aid.as_deref());
                }
            });
        }

        // Search bootstrap update (history + genres + top artists)
        let sb = CachedSearchBootstrap {
            history: sh_res.map(|h| h.items).unwrap_or_default(),
            genres: gn_res
                .map(|g| g.items.into_iter().map(|x| (x.id, x.name)).collect())
                .unwrap_or_default(),
            artists: ar_res
                .map(|a| a.items.iter().map(artist_to_seed).collect())
                .unwrap_or_default(),
        };
        c.cache.write(ContentCache::SEARCH_BOOTSTRAP, &sb);
        {
            let w_s = w.clone();
            let c_s = c.clone();
            let st_s = st.clone();
            let cc_s = cc.clone();
            let rt_s = rt2.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = w_s.upgrade() {
                    let dk = u.global::<Theme>().get_is_dark();
                    *st_s.search_artists.lock().expect("sa") = sb.artists.clone();
                    u.set_search_history(to_model(sb.history.into_iter().map(SharedString::from).collect()));
                    u.set_search_genres(to_model(genres_to_chips(&sb.genres)));
                    u.set_search_artists(to_model(artist_seeds_to_slint(&sb.artists, dk)));
                    hydrate_artist_covers(w_s, &rt_s, &c_s, &cc_s, &sb.artists);
                }
            });
        }
    });
}

pub fn open_collection_view(
    kind: String,
    id: String,
    ui_weak: slint::Weak<MainWindow>,
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    app_state: Arc<AppRuntimeState>,
    cover_cache: CoverMemoryCache,
    rt: tokio::runtime::Handle,
) {
    if let Some(u) = ui_weak.upgrade() {
        push_nav(&app_state, "collection");
        let dark = u.global::<Theme>().get_is_dark();
        u.set_active_nav("collection".into());
        u.set_collection_kind(SharedString::from(&kind));
        u.set_collection_id(SharedString::from(&id));
        u.set_collection_loading(true);
        u.set_collection_sync_text(SharedString::default());
        u.set_collection_albums(to_model(Vec::new()));
        u.set_collection_singles(to_model(Vec::new()));

        if kind == "favorites" {
            let fav_tracks = app_state.library_tracks.lock().expect("lt").clone();
            *app_state.collection_kind.lock().expect("ck") = "favorites".into();
            *app_state.collection_id.lock().expect("cid") = "favorites".into();
            *app_state.collection_tracks.lock().expect("ct") = fav_tracks.clone();
            *app_state.collection_can_edit.lock().expect("cce") = false;
            u.set_collection_title("Избранное".into());
            u.set_collection_subtitle("Мои треки".into());
            u.set_collection_meta(SharedString::from(KwhalePlural::tracks(fav_tracks.len() as i32)));
            u.set_collection_artist_id(SharedString::default());
            u.set_collection_cover(make_placeholder_image("Избранное", dark, 320, 0.0));
            u.set_collection_can_edit(false);
            u.set_collection_loading(false);
            let active_id = engine.state().current().map(|t| t.remote_id.clone());
            u.set_collection_tracks(to_model(ui_tracks_to_slint(&fav_tracks, active_id.as_deref(), &core, dark)));
            hydrate_track_covers(ui_weak, &rt, &core, &cover_cache, &fav_tracks, TrackListTarget::Collection);
            return;
        }
    }

    if core.api.session().is_none() {
        if let Some(u) = ui_weak.upgrade() {
            u.set_collection_loading(false);
        }
        return;
    }

    let c = core.clone();
    let st = app_state.clone();
    let cc = cover_cache.clone();
    let rt2 = rt.clone();
    let w = ui_weak.clone();
    let eng = engine.clone();
    let k = kind.clone();
    let cid_s = id.clone();

    rt.spawn(async move {
        let mut title = String::new();
        let mut subtitle = String::new();
        let mut meta = String::new();
        let mut artist_id = String::new();
        let mut cover_id: Option<String> = None;
        let mut saved = false;
        let mut can_edit = false;
        let mut tracks_ui: Vec<UiTrack> = Vec::new();
        let mut albums_seed: Vec<ShelfSeed> = Vec::new();
        let mut singles_seed: Vec<ShelfSeed> = Vec::new();

        match k.as_str() {
            "album" => {
                if let Ok(al) = c.api.album(&cid_s).await {
                    title = al.title.clone();
                    subtitle = al.artist_line();
                    artist_id = al.primary_artist_id().unwrap_or_default().to_string();
                    meta = [
                        al.year.map(|y| y.to_string()),
                        Some(KwhalePlural::tracks(al.track_count)),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · ");
                    cover_id = al.cover_image_id().map(ToOwned::to_owned);
                    saved = al.is_favorite;
                    let raw = if al.tracks.is_empty() {
                        c.api.album_tracks(&cid_s, None, Some(100)).await.map(|p| p.items).unwrap_or_default()
                    } else {
                        al.tracks
                    };
                    tracks_ui = raw
                        .iter()
                        .map(|t| {
                            let mut u = api_track_to_ui(t, &c);
                            if u.cover_image_id.is_none() {
                                u.cover_image_id = cover_id.clone();
                            }
                            if u.album.is_none() {
                                u.album = Some(title.clone());
                            }
                            if u.album_remote_id.is_none() {
                                u.album_remote_id = Some(cid_s.clone());
                            }
                            u
                        })
                        .collect();
                }
            }
            "playlist" => {
                if let Ok(pl) = c.api.playlist(&cid_s).await {
                    title = pl.title.clone();
                    subtitle = pl
                        .owner
                        .as_ref()
                        .map(|o| o.display_name.clone())
                        .unwrap_or_else(|| "Вы".into());
                    meta = KwhalePlural::tracks(pl.track_count);
                    cover_id = pl.cover_image_id().map(ToOwned::to_owned);
                    saved = pl.is_followed;
                    can_edit = pl.permissions.can_edit;
                    let raw = if pl.tracks.is_empty() {
                        c.api.playlist_tracks(&cid_s, None, Some(150)).await.map(|p| p.items).unwrap_or_default()
                    } else {
                        pl.tracks
                    };
                    tracks_ui = raw
                        .iter()
                        .map(|t| {
                            let mut u = api_track_to_ui(t, &c);
                            if u.cover_image_id.is_none() {
                                u.cover_image_id = cover_id.clone();
                            }
                            if u.album.is_none() {
                                u.album = Some(title.clone());
                            }
                            if u.album_remote_id.is_none() {
                                u.album_remote_id = Some(cid_s.clone());
                            }
                            u
                        })
                        .collect();
                }
            }
            "artist" => {
                if let Ok(ar) = c.api.artist(&cid_s).await {
                    title = ar.name.clone();
                    subtitle = ar.genres.iter().map(|g| g.name.as_str()).collect::<Vec<_>>().join(", ");
                    meta = KwhalePlural::listeners(ar.followers.clamp(0, i32::MAX as i64) as i32);
                    cover_id = ar.avatar_image_id().map(ToOwned::to_owned);
                    saved = ar.is_favorite;
                    let top_raw = if ar.top_tracks.is_empty() {
                        c.api.artist_top_tracks(&cid_s, Some(10)).await.map(|p| p.items).unwrap_or_default()
                    } else {
                        ar.top_tracks
                    };
                    tracks_ui = top_raw.iter().map(|t| api_track_to_ui(t, &c)).collect();
                    let rels = if ar.albums.is_empty() {
                        c.api.artist_albums(&cid_s, None, Some(50)).await.map(|p| p.items).unwrap_or_default()
                    } else {
                        ar.albums
                    };
                    for a in rels {
                        if a.is_single_like() {
                            singles_seed.push(album_to_shelf(&a));
                        } else {
                            albums_seed.push(album_to_shelf(&a));
                        }
                    }

                    // Background catalog sync matching Android CollectionScreen.kt
                    let c_sync = c.clone();
                    let w_sync = w.clone();
                    let cid_sync = cid_s.clone();
                    let rt_sync = rt2.clone();
                    rt2.spawn(async move {
                        if let Ok(st_sync) = c_sync.api.sync_artist(&cid_sync).await {
                            if st_sync.status == "syncing" || st_sync.status == "queued" {
                                let w_m = w_sync.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(u) = w_m.upgrade() {
                                        u.set_collection_sync_text("Синхронизация каталога...".into());
                                    }
                                });
                                for _ in 0..8 {
                                    tokio::time::sleep(Duration::from_secs(2)).await;
                                    if let Ok(poll) = c_sync.api.artist_sync_status(&cid_sync).await {
                                        if matches!(poll.status.as_str(), "done" | "failed" | "unavailable") {
                                            let w_d = w_sync.clone();
                                            let _ = slint::invoke_from_event_loop(move || {
                                                if let Some(u) = w_d.upgrade() {
                                                    u.set_collection_sync_text(SharedString::default());
                                                }
                                            });
                                            break;
                                        }
                                    } else {
                                        break;
                                    }
                                }
                                drop(rt_sync);
                            }
                        }
                    });
                }
            }
            "mix" => {
                c.events.mix_click(&cid_s);
                if let Ok(mix_pg) = c.api.mixes(None, None, Some(50)).await {
                    if let Some(mx) = mix_pg.items.into_iter().find(|m| m.id == cid_s) {
                        title = mx.title;
                        subtitle = mx.subtitle.unwrap_or_else(|| "Персональный микс".into());
                        meta = KwhalePlural::tracks(mx.track_count);
                        cover_id = mx.cover.as_ref().and_then(|cv| cv.valid_id().map(ToOwned::to_owned));
                    }
                }
                if let Ok(pg) = c.api.mix_tracks(&cid_s, None, Some(60)).await {
                    tracks_ui = pg.items.iter().map(|r| api_track_to_ui(&r.track, &c)).collect();
                    if title.is_empty() {
                        title = if cid_s == "personal" {
                            "Для тебя".into()
                        } else if cid_s == "discovery" {
                            "Открытия".into()
                        } else {
                            "Микс".into()
                        };
                        subtitle = "Персональный микс".into();
                        meta = KwhalePlural::tracks(tracks_ui.len() as i32);
                    }
                }
            }
            _ => {}
        }

        if title.is_empty() {
            let w_err = w.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = w_err.upgrade() {
                    u.set_collection_loading(false);
                }
            });
            return;
        }

        *st.collection_kind.lock().expect("ck") = k.clone();
        *st.collection_id.lock().expect("cid") = cid_s.clone();
        *st.collection_saved.lock().expect("cs") = saved;
        *st.collection_can_edit.lock().expect("cce") = can_edit;
        *st.collection_tracks.lock().expect("ct") = tracks_ui.clone();
        *st.collection_albums.lock().expect("ca") = albums_seed.clone();
        *st.collection_singles.lock().expect("csg") = singles_seed.clone();

        let active_id = eng.state().current().map(|t| t.remote_id.clone());
        let w_ui = w.clone();
        let c_ui = c.clone();
        let cc_ui = cc.clone();
        let rt_ui = rt2.clone();
        let k_col = k.clone();
        let id_col = cid_s.clone();

        let _ = slint::invoke_from_event_loop(move || {
            let Some(u) = w_ui.upgrade() else { return };
            let dk = u.global::<Theme>().get_is_dark();
            u.set_collection_loading(false);
            u.set_collection_title(SharedString::from(&title));
            u.set_collection_subtitle(SharedString::from(&subtitle));
            u.set_collection_meta(SharedString::from(&meta));
            u.set_collection_artist_id(SharedString::from(&artist_id));
            u.set_collection_saved(saved);
            u.set_collection_can_edit(can_edit);
            u.set_collection_cover(make_placeholder_image(&title, dk, 320, 0.0));
            u.set_collection_tracks(to_model(ui_tracks_to_slint(&tracks_ui, active_id.as_deref(), &c_ui, dk)));
            let al_vis: Vec<ShelfSeed> = albums_seed.iter().take(5).cloned().collect();
            let sg_vis: Vec<ShelfSeed> = singles_seed.iter().take(5).cloned().collect();
            u.set_collection_albums(to_model(shelf_seeds_to_slint(&al_vis, dk)));
            u.set_collection_singles(to_model(shelf_seeds_to_slint(&sg_vis, dk)));

            if let Some(cov_id) = cover_id {
                let w2 = w_ui.clone();
                spawn_fetch_cover(&rt_ui, c_ui.clone(), cc_ui.clone(), cov_id, "lg", 512, 0.0, move |img: Image, _| {
                    if let Some(u2) = w2.upgrade() {
                        u2.set_collection_cover(img);
                    }
                });
            } else if matches!(k_col.as_str(), "mix" | "playlist") {
                let w2 = w_ui.clone();
                hydrate_collection_collage(
                    &rt_ui,
                    c_ui.clone(),
                    cc_ui.clone(),
                    k_col,
                    id_col,
                    &tracks_ui,
                    512,
                    move |img| {
                        if let Some(u2) = w2.upgrade() {
                            u2.set_collection_cover(img);
                        }
                    },
                );
            }
            hydrate_track_covers(w_ui.clone(), &rt_ui, &c_ui, &cc_ui, &tracks_ui, TrackListTarget::Collection);
            hydrate_shelf_covers(w_ui.clone(), &rt_ui, &c_ui, &cc_ui, &al_vis, ShelfListTarget::CollectionAlbums);
            hydrate_shelf_covers(w_ui, &rt_ui, &c_ui, &cc_ui, &sg_vis, ShelfListTarget::CollectionSingles);
        });
    });
}

pub fn bind_all_callbacks(
    ui: &MainWindow,
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    app_state: Arc<AppRuntimeState>,
    cover_cache: CoverMemoryCache,
    rt_handle: tokio::runtime::Handle,
    system_dark: Arc<std::sync::Mutex<bool>>,
    system_accent: Arc<std::sync::Mutex<Option<Rgb>>>,
    toast: ToastFn,
) {
    // ---- Transport ----
    {
        let eng = engine.clone();
        ui.on_toggle_play(move || eng.toggle());
    }
    {
        let eng = engine.clone();
        ui.on_prev_track(move || eng.previous());
    }
    {
        let eng = engine.clone();
        ui.on_next_track(move || eng.next());
    }
    {
        let eng = engine.clone();
        ui.on_toggle_shuffle(move || eng.toggle_shuffle());
    }
    {
        let eng = engine.clone();
        ui.on_cycle_repeat(move || eng.cycle_repeat());
    }
    {
        let eng = engine.clone();
        ui.on_seek_to(move |ratio| {
            let dur = eng.state().duration_ms;
            if dur > 0 {
                eng.seek_ms((dur as f64 * ratio.clamp(0.0, 1.0) as f64).round() as u64);
            }
        });
    }
    {
        let eng = engine.clone();
        ui.on_set_volume(move |v| {
            eng.set_muted(false);
            eng.set_volume(v.clamp(0.0, 1.0) as f64);
        });
    }
    {
        let eng = engine.clone();
        ui.on_toggle_mute(move || {
            let m = eng.state().muted;
            eng.set_muted(!m);
        });
    }
    {
        let eng = engine.clone();
        ui.on_jump_queue(move |idx| {
            if idx >= 0 {
                eng.jump_to(idx as usize);
            }
        });
    }
    {
        let eng = engine.clone();
        ui.on_remove_queue_item(move |idx| {
            if idx >= 0 {
                eng.remove_from_queue(idx as usize);
            }
        });
    }
    {
        let eng = engine.clone();
        let w = ui.as_weak();
        ui.on_clear_queue(move || {
            eng.stop();
            if let Some(u) = w.upgrade() {
                u.set_queue_items(to_model(Vec::new()));
                u.set_has_current_track(false);
            }
        });
    }
    {
        let eng = engine.clone();
        ui.on_seek_lyric(move |ms| {
            if ms >= 0 {
                eng.seek_ms(ms as u64);
            }
        });
    }

    // ---- Auth: check server, login, register, logout ----
    {
        let c = core.clone();
        let w = ui.as_weak();
        let rt = rt_handle.clone();
        ui.on_check_server(move |url_raw, allow_http| {
            let Some(norm) = ServerUrl::normalize(url_raw.trim(), None) else {
                if let Some(u) = w.upgrade() {
                    u.set_server_check_status("Некорректный адрес сервера".into());
                }
                return;
            };
            if !allow_http && norm.starts_with("http://") && !ServerUrl::is_local_or_lan(&norm) {
                if let Some(u) = w.upgrade() {
                    u.set_server_check_status("Для публичного HTTP включите галочку HTTP".into());
                }
                return;
            }
            if let Some(u) = w.upgrade() {
                u.set_server_check_status("Проверяем сервер...".into());
            }
            let c2 = c.clone();
            let w2 = w.clone();
            rt.spawn(async move {
                let status = if c2.reconfigure_api(&norm, allow_http).is_ok() {
                    match c2.api.health().await {
                        Ok(h) => format!("Сервер доступен ({})", h.status),
                        Err(e) => format!("Не отвечает: {}", e.user_message()),
                    }
                } else {
                    "Отклонён адрес сервера".into()
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(u) = w2.upgrade() {
                        u.set_server_check_status(SharedString::from(status));
                    }
                });
            });
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let w = ui.as_weak();
        let rt = rt_handle.clone();
        let t_fn = toast.clone();
        ui.on_submit_login(move |url_raw, allow_http, email, password| {
            if email.trim().is_empty() || password.is_empty() {
                if let Some(u) = w.upgrade() {
                    u.set_auth_error("Заполните email и пароль".into());
                }
                return;
            }
            let Some(norm) = ServerUrl::normalize(url_raw.trim(), None) else {
                if let Some(u) = w.upgrade() {
                    u.set_auth_error("Некорректный адрес сервера".into());
                }
                return;
            };
            if let Err(err) = c.reconfigure_api(&norm, allow_http) {
                if let Some(u) = w.upgrade() {
                    u.set_auth_error(SharedString::from(err.user_message()));
                }
                return;
            }
            if let Some(u) = w.upgrade() {
                u.set_auth_loading(true);
                u.set_auth_error(SharedString::default());
            }
            let c2 = c.clone();
            let eng2 = eng.clone();
            let st2 = st.clone();
            let cc2 = cc.clone();
            let rt2 = rt.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            let em = email.to_string();
            let pw = password.to_string();
            rt.spawn(async move {
                match c2.api.login(&em, &pw).await {
                    Ok(sess) => {
                        c2.notify_auth_user(Some(sess.user.clone()));
                        let name = sess.user.display_name.clone();
                        let mail = sess.user.email.clone().unwrap_or_default();
                        let init = initials_for_name(&name);
                        let srv = c2.settings.snapshot().api_base_url;
                        let w3 = w2.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w3.upgrade() {
                                u.set_authenticated(true);
                                u.set_auth_loading(false);
                                u.set_auth_password(SharedString::default());
                                u.set_user_name(SharedString::from(name));
                                u.set_user_email(SharedString::from(mail));
                                u.set_user_initials(SharedString::from(init));
                                u.set_server_url(SharedString::from(srv));
                                u.set_auth_error(SharedString::default());
                            }
                        });
                        load_all_backend_data(w2, c2, eng2, st2, cc2, rt2, true);
                        t2("Вход выполнен".into());
                    }
                    Err(err) => {
                        let msg = err.user_message();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w2.upgrade() {
                                u.set_auth_loading(false);
                                u.set_auth_error(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let w = ui.as_weak();
        let rt = rt_handle.clone();
        let t_fn = toast.clone();
        ui.on_submit_register(move |url_raw, allow_http, email, password, display_name, invite| {
            if email.trim().is_empty() || password.len() < 8 || display_name.trim().is_empty() || invite.trim().is_empty() {
                if let Some(u) = w.upgrade() {
                    u.set_auth_error("Укажите имя, invite-токен, email и пароль >= 8 символов".into());
                }
                return;
            }
            let Some(norm) = ServerUrl::normalize(url_raw.trim(), None) else {
                if let Some(u) = w.upgrade() {
                    u.set_auth_error("Некорректный адрес сервера".into());
                }
                return;
            };
            if let Err(err) = c.reconfigure_api(&norm, allow_http) {
                if let Some(u) = w.upgrade() {
                    u.set_auth_error(SharedString::from(err.user_message()));
                }
                return;
            }
            if let Some(u) = w.upgrade() {
                u.set_auth_loading(true);
                u.set_auth_error(SharedString::default());
            }
            let c2 = c.clone();
            let eng2 = eng.clone();
            let st2 = st.clone();
            let cc2 = cc.clone();
            let rt2 = rt.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            let em = email.to_string();
            let pw = password.to_string();
            let dn = display_name.to_string();
            let inv = invite.to_string();
            rt.spawn(async move {
                match c2.api.register(&em, &pw, &dn, &inv).await {
                    Ok(sess) => {
                        c2.notify_auth_user(Some(sess.user.clone()));
                        let name = sess.user.display_name.clone();
                        let init = initials_for_name(&name);
                        let srv = c2.settings.snapshot().api_base_url;
                        let w3 = w2.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w3.upgrade() {
                                u.set_authenticated(true);
                                u.set_auth_loading(false);
                                u.set_auth_password(SharedString::default());
                                u.set_user_name(SharedString::from(name));
                                u.set_user_initials(SharedString::from(init));
                                u.set_server_url(SharedString::from(srv));
                                u.set_auth_error(SharedString::default());
                            }
                        });
                        load_all_backend_data(w2, c2, eng2, st2, cc2, rt2, true);
                        t2("Аккаунт создан".into());
                    }
                    Err(err) => {
                        let msg = err.user_message();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w2.upgrade() {
                                u.set_auth_loading(false);
                                u.set_auth_error(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        let rt = rt_handle.clone();
        ui.on_logout(move || {
            eng.stop();
            let c2 = c.clone();
            let st2 = st.clone();
            let w2 = w.clone();
            rt.spawn(async move {
                c2.api.logout().await;
                c2.cache.invalidate_home();
                c2.cache.invalidate_library();
                c2.cache.invalidate_profile();
                c2.cache.invalidate_search_bootstrap();
                c2.notify_auth_user(None);
                *st2.home_tracks.lock().expect("ht") = Vec::new();
                *st2.home_all_recent.lock().expect("hr") = Vec::new();
                *st2.home_all_chart.lock().expect("hc") = Vec::new();
                *st2.library_tracks.lock().expect("lt") = Vec::new();
                *st2.library_playlists.lock().expect("lp") = Vec::new();
                *st2.history_tracks.lock().expect("hi") = Vec::new();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(u) = w2.upgrade() {
                        u.set_demo_mode(false);
                        u.set_authenticated(false);
                        u.set_player_open(false);
                        u.set_has_current_track(false);
                        u.set_queue_items(to_model(Vec::new()));
                        u.set_tracks(to_model(Vec::new()));
                        u.set_recent_releases(to_model(Vec::new()));
                        u.set_chart_items(to_model(Vec::new()));
                        u.set_sidebar_playlists(to_model(Vec::new()));
                        u.set_user_name(SharedString::default());
                        u.set_user_initials(SharedString::default());
                        u.set_user_has_avatar(false);
                    }
                });
            });
        });
    }

    // ---- Navigation & Back ----
    {
        let w = ui.as_weak();
        let st = app_state.clone();
        let c = core.clone();
        let eng = engine.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        ui.on_nav(move |target| {
            let route = target.to_string();
            push_nav(&st, &route);
            if let Some(u) = w.upgrade() {
                u.set_active_nav(SharedString::from(&route));
            }
            if route == "history" && c.api.session().is_some() {
                let c2 = c.clone();
                let st2 = st.clone();
                let cc2 = cc.clone();
                let rt2 = rt.clone();
                let w2 = w.clone();
                let eng2 = eng.clone();
                rt.spawn(async move {
                    if let Ok(pg) = c2.api.history(None, Some(50)).await {
                        let hv: Vec<UiTrack> = pg.items.iter().map(|h| api_track_to_ui(&h.track, &c2)).collect();
                        *st2.history_tracks.lock().expect("hist") = hv.clone();
                        *st2.history_next_cursor.lock().expect("hc") = pg.next_cursor.clone();
                        let aid = eng2.state().current().map(|t| t.remote_id.clone());
                        let has_more = pg.next_cursor.is_some();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w2.upgrade() {
                                let dk = u.global::<Theme>().get_is_dark();
                                u.set_history_has_more(has_more);
                                u.set_history_tracks(to_model(ui_tracks_to_slint(&hv, aid.as_deref(), &c2, dk)));
                                hydrate_track_covers(w2, &rt2, &c2, &cc2, &hv, TrackListTarget::History);
                            }
                        });
                    }
                });
            }
        });
    }
    {
        let w = ui.as_weak();
        let st = app_state.clone();
        ui.on_navigate_back(move || {
            let prev = {
                let mut hist = st.nav_history.lock().expect("nav");
                if hist.len() > 1 {
                    hist.pop();
                }
                hist.last().cloned().unwrap_or_else(|| "home".to_string())
            };
            if let Some(u) = w.upgrade() {
                u.set_active_nav(SharedString::from(prev));
            }
        });
    }

    // ---- Home: Refresh, Hero Slides, Play Hero, My Wave, Prompt Mix, Shelf Shift ----
    {
        let w = ui.as_weak();
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let t_fn = toast.clone();
        ui.on_refresh_home(move || {
            if c.api.session().is_some() {
                c.cache.invalidate_home();
                c.cache.invalidate_library();
                c.cache.invalidate_profile();
                load_all_backend_data(w.clone(), c.clone(), eng.clone(), st.clone(), cc.clone(), rt.clone(), true);
                t_fn("Обновление данных с сервера...".into());
            } else {
                t_fn("Войдите в аккаунт для обновления".into());
            }
        });
    }
    {
        let w = ui.as_weak();
        let st = app_state.clone();
        let c = core.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        ui.on_cycle_hero_slide(move |idx| {
            let slides = st.hero_slides.lock().expect("hero").clone();
            if slides.is_empty() {
                return;
            }
            let i = (idx.max(0) as usize) % slides.len();
            let acc = *st.current_accent.lock().expect("acc");
            if let Some(u) = w.upgrade() {
                let dk = u.global::<Theme>().get_is_dark();
                apply_hero_slide_to_ui(&u, &slides[i], i, acc, dk);
                hydrate_hero_slide_cover(w.clone(), &rt, &c, &cc, &st, &slides[i]);
            }
        });
    }
    {
        let eng = engine.clone();
        let c = core.clone();
        let st = app_state.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_play_hero(move || {
            let cur_st = eng.state();
            if !cur_st.queue.is_empty() {
                eng.toggle();
                return;
            }
            let slides = st.hero_slides.lock().expect("hero").clone();
            let idx = w.upgrade().map(|u| u.get_hero_slide().max(0) as usize).unwrap_or(0);
            if let Some(sl) = slides.get(idx) {
                if sl.kind == "mix" {
                    let mid = sl.id.clone();
                    let c2 = c.clone();
                    let e2 = eng.clone();
                    let t2 = t_fn.clone();
                    rt.spawn(async move {
                        if let Ok(pg) = c2.api.mix_tracks(&mid, None, Some(40)).await {
                            let q: Vec<UiTrack> = pg.items.iter().map(|r| api_track_to_ui(&r.track, &c2)).collect();
                            if !q.is_empty() {
                                e2.play_queue(q, 0, "mix", Some(&mid));
                                return;
                            }
                        }
                        t2("В миксе пока нет треков".into());
                    });
                    return;
                } else if let Some(tr) = &sl.track {
                    let mut q = st.history_tracks.lock().expect("hist").clone();
                    if q.is_empty() {
                        q.push(tr.clone());
                    }
                    eng.play_queue(q, 0, "home", None);
                    return;
                }
            }
            let fallback = st.home_tracks.lock().expect("ht").clone();
            if !fallback.is_empty() {
                eng.play_queue(fallback, 0, "home", None);
            } else {
                t_fn("Нет треков для запуска".into());
            }
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let rt = rt_handle.clone();
        let t_fn = toast.clone();
        ui.on_start_wave(move || {
            if c.api.session().is_none() {
                let q = st.home_tracks.lock().expect("ht").clone();
                if !q.is_empty() {
                    eng.play_queue(q, 0, "mix", None);
                    t_fn("Волна запущена (демо-режим)".into());
                } else {
                    t_fn("Войдите в аккаунт для запуска волны".into());
                }
                return;
            }
            let c2 = c.clone();
            let e2 = eng.clone();
            let t2 = t_fn.clone();
            rt.spawn(async move {
                if let Ok(mixes) = c2.api.mixes(Some("personal"), None, Some(5)).await {
                    if let Some(first) = mixes.items.first() {
                        if let Ok(pg) = c2.api.mix_tracks(&first.id, None, Some(45)).await {
                            let q: Vec<UiTrack> = pg.items.iter().map(|r| api_track_to_ui(&r.track, &c2)).collect();
                            if !q.is_empty() {
                                e2.play_queue(q, 0, "mix", Some(&first.id));
                                t2(format!("Волна: {}", first.title));
                                return;
                            }
                        }
                    }
                }
                if let Ok(mixes) = c2.api.mixes(None, None, Some(5)).await {
                    if let Some(first) = mixes.items.first() {
                        if let Ok(pg) = c2.api.mix_tracks(&first.id, None, Some(45)).await {
                            let q: Vec<UiTrack> = pg.items.iter().map(|r| api_track_to_ui(&r.track, &c2)).collect();
                            if !q.is_empty() {
                                e2.play_queue(q, 0, "mix", Some(&first.id));
                                t2(format!("Запущено: {}", first.title));
                                return;
                            }
                        }
                    }
                }
                match c2.api.library_tracks(None, Some(50), "recent", None).await {
                    Ok(pg) if !pg.items.is_empty() => {
                        let q = shuffle_tracks(pg.items.iter().map(|tr| api_track_to_ui(tr, &c2)).collect());
                        e2.play_queue(q, 0, "mix", None);
                        t2("Моя волна по библиотеке запущена".into());
                    }
                    Ok(_) => t2("Добавьте треки или прослушайте музыку для волны".into()),
                    Err(e) => t2(e.user_message()),
                }
            });
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_submit_prompt_mix(move |prompt_raw| {
            let prompt = prompt_raw.trim().to_string();
            if prompt.is_empty() {
                return;
            }
            if c.api.session().is_none() {
                t_fn("Войдите в аккаунт для сборки волны по запросу".into());
                return;
            }
            if let Some(u) = w.upgrade() {
                u.set_prompt_loading(true);
            }
            let c2 = c.clone();
            let e2 = eng.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            let snap = c.settings.snapshot();
            let req = PromptMixRequest {
                prompt: prompt.clone(),
                size: 30,
                llm_api_key: (!snap.llm_api_key.is_empty()).then_some(snap.llm_api_key),
                llm_endpoint: (!snap.llm_endpoint.is_empty()).then_some(snap.llm_endpoint),
                llm_model: (!snap.llm_model.is_empty()).then_some(snap.llm_model),
            };
            rt.spawn(async move {
                match c2.api.prompt_mix(&req).await {
                    Ok(pg) => {
                        let q: Vec<UiTrack> = pg.items.iter().map(|r| api_track_to_ui(&r.track, &c2)).collect();
                        if q.is_empty() {
                            t2("Ничего не нашлось — попробуйте другой запрос".into());
                        } else {
                            let mid = format!("prompt:{}", prompt.chars().take(64).collect::<String>());
                            e2.play_queue(q, 0, "mix", Some(&mid));
                            t2("Плейлист под настроение запущен".into());
                        }
                    }
                    Err(err) => t2(err.user_message()),
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(u) = w2.upgrade() {
                        u.set_prompt_loading(false);
                    }
                });
            });
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let rt = rt_handle.clone();
        let t_fn = toast.clone();
        ui.on_play_mix(move |mix_id| {
            if c.api.session().is_none() {
                return;
            }
            let c2 = c.clone();
            let e2 = eng.clone();
            let t2 = t_fn.clone();
            let mid = mix_id.to_string();
            rt.spawn(async move {
                c2.events.mix_click(&mid);
                match c2.api.mix_tracks(&mid, None, Some(50)).await {
                    Ok(pg) => {
                        let q: Vec<UiTrack> = pg.items.iter().map(|r| api_track_to_ui(&r.track, &c2)).collect();
                        if q.is_empty() {
                            t2("В миксе пока нет треков".into());
                        } else {
                            e2.play_queue(q, 0, "mix", Some(&mid));
                        }
                    }
                    Err(e) => t2(e.user_message()),
                }
            });
        });
    }
    {
        let w = ui.as_weak();
        let st = app_state.clone();
        let c = core.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        ui.on_shift_shelf(move |which, delta| {
            let Some(u) = w.upgrade() else { return };
            let dk = u.global::<Theme>().get_is_dark();
            if which == "recent" {
                let all = st.home_all_recent.lock().expect("hr").clone();
                if all.len() <= 5 {
                    return;
                }
                let max_off = all.len().saturating_sub(5);
                let mut off = st.home_recent_offset.lock().expect("hro");
                *off = ((*off as isize) + (delta as isize * 5)).clamp(0, max_off as isize) as usize;
                let slice: Vec<ShelfSeed> = all.into_iter().skip(*off).take(5).collect();
                u.set_recent_releases(to_model(shelf_seeds_to_slint(&slice, dk)));
                hydrate_shelf_covers(w.clone(), &rt, &c, &cc, &slice, ShelfListTarget::Recent);
            } else {
                let all = st.home_all_chart.lock().expect("hc").clone();
                if all.len() <= 5 {
                    return;
                }
                let max_off = all.len().saturating_sub(5);
                let mut off = st.home_chart_offset.lock().expect("hco");
                *off = ((*off as isize) + (delta as isize * 5)).clamp(0, max_off as isize) as usize;
                let slice: Vec<ShelfSeed> = all.into_iter().skip(*off).take(5).collect();
                u.set_chart_items(to_model(shelf_seeds_to_slint(&slice, dk)));
                hydrate_shelf_covers(w.clone(), &rt, &c, &cc, &slice, ShelfListTarget::Chart);
            }
        });
    }

    // ---- Search: two-wave search, suggestions, history, filters, artist open ----
    let exec_search = {
        let c = core.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let eng = engine.clone();
        Arc::new(move |query_s: String, filter_s: String| {
            let q = query_s.trim().to_string();
            if q.is_empty() {
                if let Some(u) = w.upgrade() {
                    u.set_search_suggestions(to_model(Vec::new()));
                    u.set_search_tracks(to_model(Vec::new()));
                    u.set_search_shelves(to_model(Vec::new()));
                    u.set_search_loading(false);
                    u.set_search_refining(false);
                    u.set_search_error(SharedString::default());
                    if let Some(sb) = c.cache.read::<CachedSearchBootstrap>(ContentCache::SEARCH_BOOTSTRAP) {
                        let dk = u.global::<Theme>().get_is_dark();
                        *st.search_artists.lock().expect("sa") = sb.artists.clone();
                        u.set_search_artists(to_model(artist_seeds_to_slint(&sb.artists, dk)));
                        hydrate_artist_covers(w.clone(), &rt, &c, &cc, &sb.artists);
                    }
                }
                return;
            }
            if c.api.session().is_none() {
                let q_low = q.to_lowercase();
                let trs: Vec<UiTrack> = st
                    .home_tracks
                    .lock()
                    .expect("ht")
                    .iter()
                    .filter(|t| t.title.to_lowercase().contains(&q_low) || t.artist.to_lowercase().contains(&q_low))
                    .cloned()
                    .collect();
                *st.search_tracks.lock().expect("st") = trs.clone();
                if let Some(u) = w.upgrade() {
                    let dk = u.global::<Theme>().get_is_dark();
                    u.set_search_tracks(to_model(ui_tracks_to_slint(&trs, None, &c, dk)));
                }
                return;
            }
            if let Some(u) = w.upgrade() {
                u.set_search_loading(true);
                u.set_search_error(SharedString::default());
            }
            let c2 = c.clone();
            let st2 = st.clone();
            let cc2 = cc.clone();
            let rt2 = rt.clone();
            let w2 = w.clone();
            let eng2 = eng.clone();
            let api_type = match filter_s.as_str() {
                "tracks" | "track" => "tracks",
                "albums" | "album" => "albums",
                "artists" | "artist" => "artists",
                "playlists" | "playlist" => "playlists",
                _ => "all",
            }
            .to_string();
            rt.spawn(async move {
                let apply_resp = |resp: kwhale_core::SearchResponse, refining: bool| {
                    let mut shelves: Vec<ShelfSeed> = Vec::new();
                    if matches!(api_type.as_str(), "all" | "albums") {
                        shelves.extend(resp.albums.iter().map(album_to_shelf));
                    }
                    if matches!(api_type.as_str(), "all" | "playlists") {
                        shelves.extend(resp.playlists.iter().map(playlist_to_shelf));
                    }
                    let artists: Vec<ArtistSeed> = if matches!(api_type.as_str(), "all" | "artists") {
                        resp.artists.iter().map(artist_to_seed).collect()
                    } else {
                        Vec::new()
                    };
                    let tracks_ui: Vec<UiTrack> = if matches!(api_type.as_str(), "all" | "tracks") {
                        resp.tracks.iter().map(|t| api_track_to_ui(t, &c2)).collect()
                    } else {
                        Vec::new()
                    };
                    let shelves_vis: Vec<ShelfSeed> = shelves.into_iter().take(5).collect();
                    let artists_vis: Vec<ArtistSeed> = artists.into_iter().take(5).collect();
                    *st2.search_shelves.lock().expect("ss") = shelves_vis.clone();
                    *st2.search_artists.lock().expect("sa") = artists_vis.clone();
                    *st2.search_tracks.lock().expect("str") = tracks_ui.clone();
                    let aid = eng2.state().current().map(|t| t.remote_id.clone());
                    let w3 = w2.clone();
                    let c3 = c2.clone();
                    let cc3 = cc2.clone();
                    let rt3 = rt2.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(u) = w3.upgrade() else { return };
                        let dk = u.global::<Theme>().get_is_dark();
                        u.set_search_loading(false);
                        u.set_search_refining(refining);
                        u.set_search_shelves(to_model(shelf_seeds_to_slint(&shelves_vis, dk)));
                        u.set_search_artists(to_model(artist_seeds_to_slint(&artists_vis, dk)));
                        u.set_search_tracks(to_model(ui_tracks_to_slint(&tracks_ui, aid.as_deref(), &c3, dk)));
                        hydrate_shelf_covers(w3.clone(), &rt3, &c3, &cc3, &shelves_vis, ShelfListTarget::Search);
                        hydrate_artist_covers(w3.clone(), &rt3, &c3, &cc3, &artists_vis);
                        hydrate_track_covers(w3, &rt3, &c3, &cc3, &tracks_ui, TrackListTarget::Search);
                    });
                };

                match c2.api.search(&q, &api_type, Some(30), false).await {
                    Ok(quick) => {
                        let is_partial = quick.partial;
                        apply_resp(quick, is_partial);
                        if is_partial {
                            if let Ok(full) = c2.api.search(&q, &api_type, Some(30), true).await {
                                apply_resp(full, false);
                            } else {
                                let w3 = w2.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(u) = w3.upgrade() {
                                        u.set_search_refining(false);
                                    }
                                });
                            }
                        }
                    }
                    Err(err) => {
                        let msg = err.user_message();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w2.upgrade() {
                                u.set_search_loading(false);
                                u.set_search_refining(false);
                                u.set_search_error(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        })
    };

    {
        let st = app_state.clone();
        let c = core.clone();
        let w = ui.as_weak();
        let rt = rt_handle.clone();
        let es = exec_search.clone();
        ui.on_search_query_edited(move |raw| {
            let seq = {
                let mut s = st.search_debounce_seq.lock().expect("seq");
                *s += 1;
                *s
            };
            let q = raw.to_string();
            if q.trim().is_empty() {
                es(String::new(), "all".into());
                return;
            }
            if c.api.session().is_none() {
                return;
            }
            let st2 = st.clone();
            let c2 = c.clone();
            let w2 = w.clone();
            let es2 = es.clone();
            rt.spawn(async move {
                tokio::time::sleep(Duration::from_millis(240)).await;
                if *st2.search_debounce_seq.lock().expect("seq") != seq {
                    return;
                }
                let q_trim = q.trim().to_string();
                if q_trim.chars().count() >= 2 {
                    if let Ok(sg) = c2.api.suggestions(&q_trim).await {
                        let list: Vec<SharedString> = sg
                            .items
                            .into_iter()
                            .filter(|x| !x.text.eq_ignore_ascii_case(&q_trim))
                            .take(6)
                            .map(|x| SharedString::from(x.text))
                            .collect();
                        let w3 = w2.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w3.upgrade() {
                                u.set_search_suggestions(to_model(list));
                            }
                        });
                    }
                }
                let filt = w2
                    .upgrade()
                    .map(|u| u.get_search_filter().to_string())
                    .unwrap_or_else(|| "all".into());
                es2(q_trim, filt);
            });
        });
    }
    {
        let es = exec_search.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        ui.on_search_submitted(move |q, filter| {
            push_nav(&st, "search");
            if let Some(u) = w.upgrade() {
                u.set_active_nav("search".into());
            }
            es(q.to_string(), filter.to_string());
        });
    }
    {
        let es = exec_search.clone();
        let w = ui.as_weak();
        ui.on_select_search_filter(move |filter| {
            let q = w.upgrade().map(|u| u.get_search_query().to_string()).unwrap_or_default();
            es(q, filter.to_string());
        });
    }
    {
        let es = exec_search.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        ui.on_apply_search_chip(move |chip_text| {
            push_nav(&st, "search");
            let filt = w
                .upgrade()
                .map(|u| {
                    u.set_search_query(chip_text.clone());
                    u.set_active_nav("search".into());
                    u.get_search_filter().to_string()
                })
                .unwrap_or_else(|| "all".into());
            es(chip_text.to_string(), filt);
        });
    }
    {
        let c = core.clone();
        let w = ui.as_weak();
        let rt = rt_handle.clone();
        ui.on_clear_search_history(move || {
            let c2 = c.clone();
            let w2 = w.clone();
            rt.spawn(async move {
                if c2.api.clear_search_history().await.is_ok() {
                    c2.cache.invalidate_search_bootstrap();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(u) = w2.upgrade() {
                            u.set_search_history(to_model(Vec::new()));
                        }
                    });
                }
            });
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        ui.on_open_artist(move |artist_id| {
            open_collection_view("artist".into(), artist_id.to_string(), w.clone(), c.clone(), eng.clone(), st.clone(), cc.clone(), rt.clone());
        });
    }

    // ---- Library: tab switch, sort, filter, pagination, shuffle, favorites, create/rename playlist ----
    {
        let w = ui.as_weak();
        let st = app_state.clone();
        let c = core.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        ui.on_select_library_tab(move |tab| {
            let Some(u) = w.upgrade() else { return };
            let dk = u.global::<Theme>().get_is_dark();
            let pls = st.library_playlists.lock().expect("lpl").clone();
            let als = st.library_albums.lock().expect("lal").clone();
            let cols = build_library_shelves(&tab, &pls, &als);
            apply_library_shelves(&u, w.clone(), &rt, &c, &cc, cols, dk);
        });
    }
    let reload_library_tracks = {
        let c = core.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let eng = engine.clone();
        Arc::new(move |sort: String, query: String| {
            if c.api.session().is_none() {
                return;
            }
            let c2 = c.clone();
            let st2 = st.clone();
            let cc2 = cc.clone();
            let rt2 = rt.clone();
            let w2 = w.clone();
            let eng2 = eng.clone();
            rt.spawn(async move {
                let q_opt = (!query.trim().is_empty()).then_some(query.trim().to_string());
                if let Ok(pg) = c2.api.library_tracks(None, Some(100), &sort, q_opt.as_deref()).await {
                    let trs: Vec<UiTrack> = pg.items.iter().map(|t| api_track_to_ui(t, &c2)).collect();
                    *st2.library_tracks.lock().expect("lt") = trs.clone();
                    *st2.library_next_cursor.lock().expect("lnc") = pg.next_cursor.clone();
                    let has_more = pg.next_cursor.is_some();
                    let aid = eng2.state().current().map(|t| t.remote_id.clone());
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(u) = w2.upgrade() {
                            let dk = u.global::<Theme>().get_is_dark();
                            u.set_library_has_more(has_more);
                            u.set_library_tracks(to_model(ui_tracks_to_slint(&trs, aid.as_deref(), &c2, dk)));
                            hydrate_track_covers(w2, &rt2, &c2, &cc2, &trs, TrackListTarget::Library);
                        }
                    });
                }
            });
        })
    };
    {
        let rlt = reload_library_tracks.clone();
        let w = ui.as_weak();
        ui.on_select_library_sort(move |sort| {
            let q = w.upgrade().map(|u| u.get_library_filter_query().to_string()).unwrap_or_default();
            rlt(sort.to_string(), q);
        });
    }
    {
        let rlt = reload_library_tracks.clone();
        let w = ui.as_weak();
        ui.on_library_filter_changed(move |q| {
            let sort = w.upgrade().map(|u| u.get_library_sort().to_string()).unwrap_or_else(|| "default".into());
            rlt(sort, q.to_string());
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let eng = engine.clone();
        ui.on_load_more_library(move || {
            let Some(cursor) = st.library_next_cursor.lock().expect("lnc").clone() else { return };
            let sort = w.upgrade().map(|u| u.get_library_sort().to_string()).unwrap_or_else(|| "default".into());
            let q = w.upgrade().map(|u| u.get_library_filter_query().to_string()).unwrap_or_default();
            let c2 = c.clone();
            let st2 = st.clone();
            let cc2 = cc.clone();
            let rt2 = rt.clone();
            let w2 = w.clone();
            let eng2 = eng.clone();
            rt.spawn(async move {
                let q_opt = (!q.trim().is_empty()).then_some(q.trim().to_string());
                if let Ok(pg) = c2.api.library_tracks(Some(&cursor), Some(100), &sort, q_opt.as_deref()).await {
                    let mut all = st2.library_tracks.lock().expect("lt").clone();
                    for item in pg.items {
                        let u_t = api_track_to_ui(&item, &c2);
                        if !all.iter().any(|x| x.remote_id == u_t.remote_id) {
                            all.push(u_t);
                        }
                    }
                    *st2.library_tracks.lock().expect("lt") = all.clone();
                    *st2.library_next_cursor.lock().expect("lnc") = pg.next_cursor.clone();
                    let has_more = pg.next_cursor.is_some();
                    let aid = eng2.state().current().map(|t| t.remote_id.clone());
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(u) = w2.upgrade() {
                            let dk = u.global::<Theme>().get_is_dark();
                            u.set_library_has_more(has_more);
                            u.set_library_tracks(to_model(ui_tracks_to_slint(&all, aid.as_deref(), &c2, dk)));
                            hydrate_track_covers(w2, &rt2, &c2, &cc2, &all, TrackListTarget::Library);
                        }
                    });
                }
            });
        });
    }
    {
        let eng = engine.clone();
        let st = app_state.clone();
        ui.on_shuffle_library(move || {
            let trs = st.library_tracks.lock().expect("lt").clone();
            if !trs.is_empty() {
                eng.play_queue(shuffle_tracks(trs), 0, "library", None);
            }
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        ui.on_open_favorites(move || {
            open_collection_view("favorites".into(), "favorites".into(), w.clone(), c.clone(), eng.clone(), st.clone(), cc.clone(), rt.clone());
        });
    }
    {
        let w = ui.as_weak();
        ui.on_open_create_playlist(move || {
            if let Some(u) = w.upgrade() {
                u.set_playlist_modal_edit(false);
                u.set_playlist_modal_title(SharedString::default());
                u.set_playlist_modal_desc(SharedString::default());
                u.set_playlist_modal_public(false);
                u.set_playlist_modal_open(true);
            }
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_submit_playlist_modal(move |title_raw, desc_raw, is_public| {
            let title = title_raw.trim().to_string();
            if title.is_empty() {
                t_fn("Введите название плейлиста".into());
                return;
            }
            if c.api.session().is_none() {
                t_fn("Войдите в аккаунт для сохранения плейлиста".into());
                return;
            }
            let is_edit = w.upgrade().is_some_and(|u| u.get_playlist_modal_edit());
            let c2 = c.clone();
            let eng2 = eng.clone();
            let st2 = st.clone();
            let cc2 = cc.clone();
            let rt2 = rt.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            let desc = (!desc_raw.trim().is_empty()).then_some(desc_raw.trim().to_string());
            rt.spawn(async move {
                if is_edit {
                    let cid = st2.collection_id.lock().expect("cid").clone();
                    match c2
                        .api
                        .update_playlist(
                            &cid,
                            &PlaylistUpdate {
                                title: Some(title.clone()),
                                description: desc,
                                visibility: None,
                            },
                        )
                        .await
                    {
                        Ok(_) => {
                            c2.cache.invalidate_library();
                            let w3 = w2.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(u) = w3.upgrade() {
                                    u.set_playlist_modal_open(false);
                                    u.set_collection_title(SharedString::from(title));
                                }
                            });
                            load_all_backend_data(w2, c2, eng2, st2, cc2, rt2, true);
                            t2("Плейлист обновлён".into());
                        }
                        Err(e) => t2(e.user_message()),
                    }
                } else {
                    match c2
                        .api
                        .create_playlist(&PlaylistCreate {
                            title,
                            description: desc,
                            visibility: if is_public { "public".into() } else { "private".into() },
                        })
                        .await
                    {
                        Ok(_) => {
                            c2.cache.invalidate_library();
                            let w3 = w2.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(u) = w3.upgrade() {
                                    u.set_playlist_modal_open(false);
                                }
                            });
                            load_all_backend_data(w2, c2, eng2, st2, cc2, rt2, true);
                            t2("Плейлист создан".into());
                        }
                        Err(e) => t2(e.user_message()),
                    }
                }
            });
        });
    }

    // ---- History & Downloads ----
    {
        let c = core.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let eng = engine.clone();
        ui.on_load_more_history(move || {
            let Some(cursor) = st.history_next_cursor.lock().expect("hc").clone() else { return };
            let c2 = c.clone();
            let st2 = st.clone();
            let cc2 = cc.clone();
            let rt2 = rt.clone();
            let w2 = w.clone();
            let eng2 = eng.clone();
            rt.spawn(async move {
                if let Ok(pg) = c2.api.history(Some(&cursor), Some(50)).await {
                    let mut all = st2.history_tracks.lock().expect("hist").clone();
                    for h in pg.items {
                        all.push(api_track_to_ui(&h.track, &c2));
                    }
                    *st2.history_tracks.lock().expect("hist") = all.clone();
                    *st2.history_next_cursor.lock().expect("hc") = pg.next_cursor.clone();
                    let aid = eng2.state().current().map(|t| t.remote_id.clone());
                    let has_more = pg.next_cursor.is_some();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(u) = w2.upgrade() {
                            let dk = u.global::<Theme>().get_is_dark();
                            u.set_history_has_more(has_more);
                            u.set_history_tracks(to_model(ui_tracks_to_slint(&all, aid.as_deref(), &c2, dk)));
                            hydrate_track_covers(w2, &rt2, &c2, &cc2, &all, TrackListTarget::History);
                        }
                    });
                }
            });
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        let rt = rt_handle.clone();
        let t_fn = toast.clone();
        ui.on_clear_history(move || {
            let c2 = c.clone();
            let st2 = st.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            rt.spawn(async move {
                match c2.api.clear_history().await {
                    Ok(()) => {
                        *st2.history_tracks.lock().expect("hist") = Vec::new();
                        *st2.history_next_cursor.lock().expect("hc") = None;
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w2.upgrade() {
                                u.set_history_tracks(to_model(Vec::new()));
                                u.set_history_has_more(false);
                            }
                        });
                        t2("История очищена".into());
                    }
                    Err(e) => t2(e.user_message()),
                }
            });
        });
    }
    {
        let eng = engine.clone();
        let st = app_state.clone();
        ui.on_play_history_track(move |idx| {
            let q = st.history_tracks.lock().expect("hist").clone();
            if idx >= 0 && (idx as usize) < q.len() {
                eng.play_queue(q, idx as usize, "home", None);
            }
        });
    }
    {
        let eng = engine.clone();
        let c = core.clone();
        ui.on_play_all_downloads(move || {
            let q: Vec<UiTrack> = c
                .downloads
                .snapshot()
                .downloaded
                .into_iter()
                .filter(|d| d.exists())
                .map(|d| d.to_ui_track())
                .collect();
            if !q.is_empty() {
                eng.play_queue(q, 0, "library", None);
            }
        });
    }
    {
        let c = core.clone();
        let t_fn = toast.clone();
        ui.on_clear_all_downloads(move || {
            c.downloads.clear_all();
            t_fn("Все загрузки удалены".into());
        });
    }
    {
        let c = core.clone();
        let t_fn = toast.clone();
        ui.on_remove_download(move |id| {
            c.downloads.remove(&id);
            t_fn("Удалено с устройства".into());
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        ui.on_retry_download(move |id| {
            if let Some(found) = st
                .download_tracks
                .lock()
                .expect("dt")
                .iter()
                .find(|t| t.remote_id == id.as_str())
                .cloned()
            {
                c.downloads.enqueue(found, true);
            }
        });
    }
    {
        let eng = engine.clone();
        let c = core.clone();
        ui.on_play_download_track(move |idx| {
            let q: Vec<UiTrack> = c
                .downloads
                .snapshot()
                .downloaded
                .into_iter()
                .filter(|d| d.exists())
                .map(|d| d.to_ui_track())
                .collect();
            if idx >= 0 && (idx as usize) < q.len() {
                eng.play_queue(q, idx as usize, "library", None);
            }
        });
    }

    // ---- Collection Actions ----
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        ui.on_open_shelf_item(move |kind, id| {
            open_collection_view(kind.to_string(), id.to_string(), w.clone(), c.clone(), eng.clone(), st.clone(), cc.clone(), rt.clone());
        });
    }
    {
        let eng = engine.clone();
        let st = app_state.clone();
        ui.on_play_collection(move || {
            let q = st.collection_tracks.lock().expect("ct").clone();
            if !q.is_empty() {
                let k = st.collection_kind.lock().expect("ck").clone();
                let cid = st.collection_id.lock().expect("cid").clone();
                let surf = if matches!(k.as_str(), "album" | "playlist" | "artist" | "mix") {
                    k.as_str()
                } else {
                    "library"
                };
                let src_id = (k == "mix" && !cid.is_empty()).then_some(cid.as_str());
                eng.play_queue(q, 0, surf, src_id);
            }
        });
    }
    {
        let eng = engine.clone();
        let st = app_state.clone();
        ui.on_shuffle_collection(move || {
            let q = shuffle_tracks(st.collection_tracks.lock().expect("ct").clone());
            if !q.is_empty() {
                let k = st.collection_kind.lock().expect("ck").clone();
                let cid = st.collection_id.lock().expect("cid").clone();
                let surf = if matches!(k.as_str(), "album" | "playlist" | "artist" | "mix") {
                    k.as_str()
                } else {
                    "library"
                };
                let src_id = (k == "mix" && !cid.is_empty()).then_some(cid.as_str());
                eng.play_queue(q, 0, surf, src_id);
            }
        });
    }
    {
        let eng = engine.clone();
        let st = app_state.clone();
        ui.on_play_collection_track(move |idx| {
            let q = st.collection_tracks.lock().expect("ct").clone();
            if idx >= 0 && (idx as usize) < q.len() {
                let k = st.collection_kind.lock().expect("ck").clone();
                let cid = st.collection_id.lock().expect("cid").clone();
                let surf = if matches!(k.as_str(), "album" | "playlist" | "artist" | "mix") {
                    k.as_str()
                } else {
                    "library"
                };
                let src_id = (k == "mix" && !cid.is_empty()).then_some(cid.as_str());
                eng.play_queue(q, idx as usize, surf, src_id);
            }
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_toggle_save_collection(move || {
            let k = st.collection_kind.lock().expect("ck").clone();
            let id = st.collection_id.lock().expect("cid").clone();
            let cur_saved = *st.collection_saved.lock().expect("cs");
            let c2 = c.clone();
            let st2 = st.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            rt.spawn(async move {
                let res = match k.as_str() {
                    "album" => if cur_saved { c2.api.unlike_album(&id).await } else { c2.api.like_album(&id).await },
                    "artist" => if cur_saved { c2.api.unlike_artist(&id).await } else { c2.api.like_artist(&id).await },
                    "playlist" => if cur_saved { c2.api.unfollow_playlist(&id).await } else { c2.api.follow_playlist(&id).await },
                    _ => return,
                };
                match res {
                    Ok(()) => {
                        let next = !cur_saved;
                        *st2.collection_saved.lock().expect("cs") = next;
                        c2.cache.invalidate_library();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w2.upgrade() {
                                u.set_collection_saved(next);
                            }
                        });
                        t2(if next { "Сохранено в библиотеку".into() } else { "Убрано из библиотеки".into() });
                    }
                    Err(e) => t2(e.user_message()),
                }
            });
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let t_fn = toast.clone();
        ui.on_download_all_collection(move || {
            let q = st.collection_tracks.lock().expect("ct").clone();
            let n = c.downloads.enqueue_all(q);
            t_fn(if n > 0 {
                format!("Добавлено в загрузки: {n}")
            } else {
                "Все треки уже скачаны".into()
            });
        });
    }
    {
        let w = ui.as_weak();
        ui.on_open_rename_collection(move || {
            if let Some(u) = w.upgrade() {
                u.set_playlist_modal_edit(true);
                u.set_playlist_modal_title(u.get_collection_title());
                u.set_playlist_modal_desc(SharedString::default());
                u.set_playlist_modal_open(true);
            }
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_delete_collection(move || {
            let id = st.collection_id.lock().expect("cid").clone();
            let c2 = c.clone();
            let eng2 = eng.clone();
            let st2 = st.clone();
            let cc2 = cc.clone();
            let rt2 = rt.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            rt.spawn(async move {
                match c2.api.delete_playlist(&id).await {
                    Ok(()) => {
                        c2.cache.invalidate_library();
                        let w3 = w2.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w3.upgrade() {
                                u.set_active_nav("library".into());
                            }
                        });
                        load_all_backend_data(w2, c2, eng2, st2, cc2, rt2, true);
                        t2("Плейлист удалён".into());
                    }
                    Err(e) => t2(e.user_message()),
                }
            });
        });
    }

    // ---- Track Playback & Like & Context Menu ----
    let track_list_for_source = |st: &AppRuntimeState, eng: &AudioEngine, src: &str| -> Vec<UiTrack> {
        match src {
            "home" => st.home_tracks.lock().expect("ht").clone(),
            "search" => st.search_tracks.lock().expect("st").clone(),
            "library" => st.library_tracks.lock().expect("lt").clone(),
            "history" => st.history_tracks.lock().expect("hi").clone(),
            "collection" => st.collection_tracks.lock().expect("ct").clone(),
            _ => eng.state().queue,
        }
    };
    {
        let eng = engine.clone();
        let st = app_state.clone();
        ui.on_play_track(move |src, idx| {
            let q = track_list_for_source(&st, &eng, &src);
            if idx >= 0 && (idx as usize) < q.len() {
                let surf = if matches!(src.as_str(), "home" | "search" | "library" | "collection") {
                    if src == "collection" { "album" } else { src.as_str() }
                } else {
                    "home"
                };
                eng.play_queue(q, idx as usize, surf, None);
            }
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_toggle_like_track(move |src, idx| {
            let list = track_list_for_source(&st, &eng, &src);
            let Some(tr) = list.get(idx as usize).cloned() else { return };
            if c.api.session().is_some() {
                let c2 = c.clone();
                let e2 = eng.clone();
                let t2 = t_fn.clone();
                let surf = src.to_string();
                rt.spawn(async move {
                    match c2.reactions.toggle_like(&tr, &surf).await {
                        Ok(liked) => e2.update_track_liked(&tr.remote_id, liked),
                        Err(e) => t2(e),
                    }
                });
            } else if let Some(u) = w.upgrade() {
                let dk = u.global::<Theme>().get_is_dark();
                let mut ht = st.home_tracks.lock().expect("ht");
                if let Some(slot) = ht.get_mut(idx as usize) {
                    slot.is_liked = !slot.is_liked;
                    u.set_tracks(to_model(ui_tracks_to_slint(&ht, None, &c, dk)));
                }
            }
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let rt = rt_handle.clone();
        let t_fn = toast.clone();
        ui.on_toggle_like_current(move || {
            let Some(cur) = eng.state().current().cloned() else { return };
            let c2 = c.clone();
            let e2 = eng.clone();
            let t2 = t_fn.clone();
            rt.spawn(async move {
                match c2.reactions.toggle_like(&cur, "player").await {
                    Ok(liked) => e2.update_track_liked(&cur.remote_id, liked),
                    Err(e) => t2(e),
                }
            });
        });
    }

    // Track Menu open & actions
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        ui.on_open_track_menu(move |src, idx| {
            let tr = if src == "player" {
                eng.state().current().cloned()
            } else {
                track_list_for_source(&st, &eng, &src).get(idx as usize).cloned()
            };
            let Some(tr) = tr else { return };
            let in_edit_pl = src == "collection"
                && *st.collection_kind.lock().expect("ck") == "playlist"
                && *st.collection_can_edit.lock().expect("cce");
            *st.menu_context.lock().expect("mc") = Some(TrackMenuContext {
                track: tr.clone(),
                source: src.to_string(),
            });
            if let Some(u) = w.upgrade() {
                let dk = u.global::<Theme>().get_is_dark();
                let info = track_menu_info_from_track(&tr, &c, dk, in_edit_pl);
                u.set_track_menu(info);
                if let Some(cid) = tr.cover_image_id.clone() {
                    let w2 = w.clone();
                    let expected_tid = tr.remote_id.clone();
                    spawn_fetch_cover(&rt, c.clone(), cc.clone(), cid, "md", 192, 0.0, move |img, _| {
                        if let Some(u2) = w2.upgrade() {
                            let mut cur = u2.get_track_menu();
                            if cur.track_id == expected_tid {
                                cur.cover = img;
                                u2.set_track_menu(cur);
                            }
                        }
                    });
                }
            }
        });
    }
    {
        let w = ui.as_weak();
        ui.on_close_track_menu(move || {
            if let Some(u) = w.upgrade() {
                let mut m = u.get_track_menu();
                m.visible = false;
                m.playlist_picker_open = false;
                u.set_track_menu(m);
            }
        });
    }
    {
        let eng = engine.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_track_menu_play_next(move || {
            if let Some(ctx) = st.menu_context.lock().expect("mc").clone() {
                eng.play_next(ctx.track, &ctx.source);
                t_fn("Включим следующим".into());
            }
            if let Some(u) = w.upgrade() {
                let mut m = u.get_track_menu();
                m.visible = false;
                u.set_track_menu(m);
            }
        });
    }
    {
        let eng = engine.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_track_menu_add_queue(move || {
            if let Some(ctx) = st.menu_context.lock().expect("mc").clone() {
                eng.add_queue(vec![ctx.track], &ctx.source);
                t_fn("Добавлено в очередь".into());
            }
            if let Some(u) = w.upgrade() {
                let mut m = u.get_track_menu();
                m.visible = false;
                u.set_track_menu(m);
            }
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_track_menu_toggle_like(move || {
            let Some(ctx) = st.menu_context.lock().expect("mc").clone() else { return };
            let c2 = c.clone();
            let e2 = eng.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            rt.spawn(async move {
                match c2.reactions.toggle_like(&ctx.track, &ctx.source).await {
                    Ok(liked) => {
                        e2.update_track_liked(&ctx.track.remote_id, liked);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w2.upgrade() {
                                let mut m = u.get_track_menu();
                                m.liked = liked;
                                if liked { m.disliked = false; }
                                u.set_track_menu(m);
                            }
                        });
                    }
                    Err(e) => t2(e),
                }
            });
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_track_menu_toggle_dislike(move || {
            let Some(ctx) = st.menu_context.lock().expect("mc").clone() else { return };
            let c2 = c.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            rt.spawn(async move {
                match c2.reactions.toggle_dislike(&ctx.track, &ctx.source).await {
                    Ok(disliked) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w2.upgrade() {
                                let mut m = u.get_track_menu();
                                m.disliked = disliked;
                                if disliked { m.liked = false; }
                                u.set_track_menu(m);
                            }
                        });
                        t2(if disliked { "Трек скрыт из рекомендаций".into() } else { "Дизлайк убран".into() });
                    }
                    Err(e) => t2(e),
                }
            });
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_track_menu_toggle_download(move || {
            let Some(ctx) = st.menu_context.lock().expect("mc").clone() else { return };
            if c.downloads.is_downloaded(&ctx.track.remote_id) {
                c.downloads.remove(&ctx.track.remote_id);
                t_fn("Удалено из офлайн-загрузок".into());
            } else {
                c.downloads.enqueue(ctx.track, false);
                t_fn("Добавлено в загрузки".into());
            }
            if let Some(u) = w.upgrade() {
                let mut m = u.get_track_menu();
                m.visible = false;
                u.set_track_menu(m);
            }
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        ui.on_track_menu_open_album(move || {
            if let Some(ctx) = st.menu_context.lock().expect("mc").clone() {
                if let Some(aid) = ctx.track.album_remote_id {
                    if let Some(u) = w.upgrade() {
                        let mut m = u.get_track_menu();
                        m.visible = false;
                        u.set_track_menu(m);
                        u.set_player_open(false);
                    }
                    open_collection_view("album".into(), aid, w.clone(), c.clone(), eng.clone(), st.clone(), cc.clone(), rt.clone());
                }
            }
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let cc = cover_cache.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        ui.on_track_menu_open_artist(move || {
            if let Some(ctx) = st.menu_context.lock().expect("mc").clone() {
                if let Some(aid) = ctx.track.artist_remote_id {
                    if let Some(u) = w.upgrade() {
                        let mut m = u.get_track_menu();
                        m.visible = false;
                        u.set_track_menu(m);
                        u.set_player_open(false);
                    }
                    open_collection_view("artist".into(), aid, w.clone(), c.clone(), eng.clone(), st.clone(), cc.clone(), rt.clone());
                }
            }
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let rt = rt_handle.clone();
        let t_fn = toast.clone();
        ui.on_track_menu_share(move || {
            let Some(ctx) = st.menu_context.lock().expect("mc").clone() else { return };
            let c2 = c.clone();
            let t2 = t_fn.clone();
            rt.spawn(async move {
                match c2.api.create_share(ShareKind::Track, &ctx.track.remote_id).await {
                    Ok(link) => {
                        let url = link.share_url;
                        let copied = arboard::Clipboard::new()
                            .and_then(|mut cb| cb.set_text(url.clone()))
                            .is_ok();
                        t2(if copied {
                            format!("Ссылка скопирована: {url}")
                        } else {
                            url
                        });
                    }
                    Err(e) => t2(e.user_message()),
                }
            });
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_track_menu_load_info(move || {
            let Some(ctx) = st.menu_context.lock().expect("mc").clone() else { return };
            let c2 = c.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            rt.spawn(async move {
                match c2.api.track_info(&ctx.track.remote_id).await {
                    Ok(info) => {
                        let q = info.quality;
                        let codec = q.as_ref().and_then(|x| x.codec.clone()).unwrap_or_else(|| "audio".into());
                        let br = q.as_ref().and_then(|x| x.bitrate_kbps).map(|b| format!("{b} kbps")).unwrap_or_default();
                        let sr = q.as_ref().and_then(|x| x.sample_rate).map(|s| format!("{s} Hz")).unwrap_or_default();
                        let lossless = if q.as_ref().is_some_and(|x| x.lossless) { "Lossless" } else { "Lossy" };
                        let src_line: Vec<String> = info.sources.iter().map(|s| s.connector.clone()).collect();
                        let summary = format!(
                            "Качество: {} {} {} {} | Источники: {}",
                            lossless,
                            codec.to_uppercase(),
                            br,
                            sr,
                            if src_line.is_empty() { "сервер".into() } else { src_line.join(", ") }
                        );
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w2.upgrade() {
                                let mut m = u.get_track_menu();
                                m.details_text = SharedString::from(summary);
                                u.set_track_menu(m);
                            }
                        });
                    }
                    Err(e) => t2(e.user_message()),
                }
            });
        });
    }
    {
        let c = core.clone();
        let eng = engine.clone();
        let st = app_state.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_track_menu_report_censorship(move || {
            let Some(ctx) = st.menu_context.lock().expect("mc").clone() else { return };
            let pos = Some(eng.state().position_ms);
            let c2 = c.clone();
            let t2 = t_fn.clone();
            let w2 = w.clone();
            rt.spawn(async move {
                match c2.reactions.report_censorship_one_click(&ctx.track, pos).await {
                    Ok(msg) => t2(msg.to_string()),
                    Err(e) => t2(e),
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(u) = w2.upgrade() {
                        let mut m = u.get_track_menu();
                        m.visible = false;
                        u.set_track_menu(m);
                    }
                });
            });
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_track_menu_add_to_playlist(move |pl_id| {
            let Some(ctx) = st.menu_context.lock().expect("mc").clone() else { return };
            let c2 = c.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            let pid = pl_id.to_string();
            rt.spawn(async move {
                match c2.api.add_playlist_tracks(&pid, vec![ctx.track.remote_id], None).await {
                    Ok(_) => {
                        c2.cache.invalidate_library();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w2.upgrade() {
                                let mut m = u.get_track_menu();
                                m.visible = false;
                                m.playlist_picker_open = false;
                                u.set_track_menu(m);
                            }
                        });
                        t2("Добавлено в плейлист".into());
                    }
                    Err(e) => t2(e.user_message()),
                }
            });
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let rt = rt_handle.clone();
        let w = ui.as_weak();
        let t_fn = toast.clone();
        ui.on_track_menu_remove_from_playlist(move || {
            let Some(ctx) = st.menu_context.lock().expect("mc").clone() else { return };
            let pid = st.collection_id.lock().expect("cid").clone();
            let c2 = c.clone();
            let st2 = st.clone();
            let w2 = w.clone();
            let t2 = t_fn.clone();
            let tid = ctx.track.remote_id.clone();
            rt.spawn(async move {
                match c2.api.remove_playlist_track(&pid, &tid).await {
                    Ok(()) => {
                        c2.cache.invalidate_library();
                        let mut list = st2.collection_tracks.lock().expect("ct").clone();
                        list.retain(|x| x.remote_id != tid);
                        *st2.collection_tracks.lock().expect("ct") = list.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = w2.upgrade() {
                                let dk = u.global::<Theme>().get_is_dark();
                                let mut m = u.get_track_menu();
                                m.visible = false;
                                u.set_track_menu(m);
                                u.set_collection_tracks(to_model(ui_tracks_to_slint(&list, None, &c2, dk)));
                            }
                        });
                        t2("Убрано из плейлиста".into());
                    }
                    Err(e) => t2(e.user_message()),
                }
            });
        });
    }

    // ---- Theme, Platform & AI Settings ----
    {
        let c = core.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        let sd = system_dark.clone();
        let sa = system_accent.clone();
        let t_fn = toast.clone();
        ui.on_set_platform(move |pid| {
            let style = PlatformStyle::from_str_lossy(&pid);
            c.settings.update(|s| s.platform_style = style);
            if let Some(u) = w.upgrade() {
                let d = *sd.lock().expect("sd");
                let a = *sa.lock().expect("sa");
                apply_theme_to_ui(&u, &c, &st, d, a, None, None, None);
            }
            t_fn(format!("Профиль интерфейса: {}", style.label()));
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        let sd = system_dark.clone();
        let sa = system_accent.clone();
        ui.on_set_theme_mode(move |mode| {
            let tm = ThemeMode::from_str_lossy(&mode);
            let snap = c.settings.snapshot();
            c.update_theme(tm, snap.accent_id, snap.follow_system_accent);
            if let Some(u) = w.upgrade() {
                let d = *sd.lock().expect("sd");
                let a = *sa.lock().expect("sa");
                apply_theme_to_ui(&u, &c, &st, d, a, None, None, None);
            }
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        let sd = system_dark.clone();
        let sa = system_accent.clone();
        ui.on_set_follow_system_accent(move |follow| {
            let snap = c.settings.snapshot();
            c.update_theme(snap.theme_mode, snap.accent_id, follow);
            if let Some(u) = w.upgrade() {
                let d = *sd.lock().expect("sd");
                let a = *sa.lock().expect("sa");
                apply_theme_to_ui(&u, &c, &st, d, a, None, None, None);
            }
        });
    }
    {
        let c = core.clone();
        let st = app_state.clone();
        let w = ui.as_weak();
        let sd = system_dark.clone();
        let sa = system_accent.clone();
        ui.on_select_accent(move |aid| {
            let snap = c.settings.snapshot();
            c.update_theme(snap.theme_mode, aid, false);
            if let Some(u) = w.upgrade() {
                let d = *sd.lock().expect("sd");
                let a = *sa.lock().expect("sa");
                apply_theme_to_ui(&u, &c, &st, d, a, None, None, None);
            }
        });
    }
    {
        let c = core.clone();
        let t_fn = toast.clone();
        ui.on_save_ai_settings(move |ep, model, key| {
            c.settings.update(|s| {
                s.llm_endpoint = ep.to_string();
                s.llm_model = model.to_string();
                s.llm_api_key = key.to_string();
            });
            t_fn("Настройки AI сохранены".into());
        });
    }
    {
        let c = core.clone();
        let t_fn = toast.clone();
        ui.on_set_close_to_tray(move |enabled| {
            c.settings.update(|s| s.close_to_tray = enabled);
            t_fn(if enabled {
                "При закрытии окно будет сворачиваться в трей".into()
            } else {
                "При закрытии приложение будет завершаться".into()
            });
        });
    }
}
