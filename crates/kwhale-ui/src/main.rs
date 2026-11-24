mod handlers;
mod images;
mod platform;
mod state;
mod tray;

slint::include_modules!();

use chrono::Timelike;
use handlers::{apply_theme_to_ui, bind_all_callbacks, load_all_backend_data, ToastFn};
use images::{
    extract_widget_palette, make_hero_backdrop_from_cover, make_hero_backdrop_image,
    make_placeholder_image, make_player_backdrop_image, make_widget_backdrop_image,
    spawn_fetch_cover, spawn_fetch_cover_decoded, CoverMemoryCache,
};
use kwhale_audio::{spawn_mpris, spawn_single_instance_dbus, AudioEngine};
use kwhale_core::{
    format_duration_player_ms, greeting_for_hour, image_id_or_null, KwhaleCore, KwhaleCoverPalette,
    PlatformStyle, RepeatMode, Rgb, ThemeMode,
};
use platform::{detect_system_appearance, spawn_appearance_watcher};
use slint::winit_030::WinitWindowAccessor;
use slint::{Color, ComponentHandle, Model, SharedString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tray::{spawn_tray, TrayCommand};

use state::{
    active_lyric_index, apply_demo_seed, downloads_to_slint, hydrate_track_covers,
    initials_for_name, lyrics_to_slint, to_model, ui_tracks_to_slint, AppRuntimeState,
    TrackListTarget,
};

#[derive(Default)]
struct CliOverrides {
    screenshot: Option<PathBuf>,
    platform: Option<PlatformStyle>,
    theme: Option<ThemeMode>,
    screen: Option<String>,
    accent: Option<i32>,
    demo_playing: bool,
    player_open: bool,
    widget_only: bool,
}

fn parse_cli_overrides() -> CliOverrides {
    let mut out = CliOverrides::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(p) = arg.strip_prefix("--screenshot=") {
            out.screenshot = Some(PathBuf::from(p));
        } else if arg == "--screenshot" {
            if let Some(p) = args.next() {
                out.screenshot = Some(PathBuf::from(p));
            }
        } else if let Some(p) = arg.strip_prefix("--platform=") {
            out.platform = Some(PlatformStyle::from_str_lossy(p));
        } else if let Some(t) = arg.strip_prefix("--theme=") {
            out.theme = Some(ThemeMode::from_str_lossy(t));
        } else if let Some(s) = arg.strip_prefix("--screen=") {
            out.screen = Some(s.to_string());
        } else if let Some(a) = arg.strip_prefix("--accent=") {
            if let Ok(id) = a.parse::<i32>() {
                out.accent = Some(id);
            }
        } else if arg == "--demo-playing" {
            out.demo_playing = true;
        } else if arg == "--player" {
            out.player_open = true;
        } else if arg == "--widget" {
            out.widget_only = true;
        }
    }
    out
}

fn sync_track_models<F>(ui: &MainWindow, mut update_fn: F)
where
    F: FnMut(&mut TrackItem) -> bool,
{
    let models = [
        ui.get_tracks(),
        ui.get_search_tracks(),
        ui.get_library_tracks(),
        ui.get_history_tracks(),
        ui.get_collection_tracks(),
        ui.get_queue_items(),
    ];
    for model in models {
        let count = model.row_count();
        for i in 0..count {
            if let Some(mut item) = model.row_data(i) {
                if update_fn(&mut item) {
                    model.set_row_data(i, item);
                }
            }
        }
    }
}

fn save_rgba_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    data: &[u8],
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    image::save_buffer_with_format(
        path,
        data,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = parse_cli_overrides();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _rt_guard = rt.enter();
    let rt_handle = rt.handle().clone();

    let core = KwhaleCore::init(env!("CARGO_PKG_VERSION"))?;
    if core.api.session().is_none() {
        let _ = rt.block_on(core.token_store.try_recover_from_keyring());
    }
    let engine = rt.block_on(AudioEngine::start(core.clone()))?;

    let (platform_override, theme_override, accent_override) = if cli.screenshot.is_some() {
        (cli.platform, cli.theme, cli.accent)
    } else {
        if let Some(p) = cli.platform {
            core.settings.update(|s| s.platform_style = p);
        }
        if let Some(t) = cli.theme {
            let snap = core.settings.snapshot();
            core.update_theme(
                t,
                cli.accent.unwrap_or(snap.accent_id),
                snap.follow_system_accent,
            );
        } else if let Some(aid) = cli.accent {
            let snap = core.settings.snapshot();
            core.update_theme(snap.theme_mode, aid, false);
        }
        (None, None, None)
    };

    let snap = core.settings.snapshot();
    let initial_app = rt.block_on(detect_system_appearance(
        platform_override.unwrap_or(snap.platform_style),
    ));
    let system_dark = Arc::new(Mutex::new(initial_app.system_is_dark));
    let system_accent = Arc::new(Mutex::new(initial_app.system_accent));

    let ui = MainWindow::new()?;
    let widget = PlayerWidgetWindow::new()?;
    let default_wpal = extract_widget_palette(None);
    widget.set_backdrop(make_widget_backdrop_image(default_wpal, 420, 140));
    widget.set_artist_color(Color::from_rgb_u8(
        default_wpal.artist_tint.r,
        default_wpal.artist_tint.g,
        default_wpal.artist_tint.b,
    ));

    let cover_cache = CoverMemoryCache::default();
    let is_auth_screenshot =
        cli.screenshot.is_some() && cli.screen.as_deref() == Some("auth");
    let app_state = AppRuntimeState::new(
        KwhaleCoverPalette::gradient("kwhale", true).0,
        Rgb::from_u32(0x9085FF),
        cli.demo_playing || (cli.screenshot.is_some() && !is_auth_screenshot),
    );

    // Theme + Settings fields
    let accent_now = apply_theme_to_ui(
        &ui,
        &core,
        &app_state,
        initial_app.system_is_dark,
        initial_app.system_accent,
        platform_override,
        theme_override,
        accent_override,
    );
    let dark = ui.global::<Theme>().get_is_dark();

    ui.set_greeting(SharedString::from(greeting_for_hour(chrono::Local::now().hour())));
    ui.set_server_url(SharedString::from(&snap.api_base_url));
    ui.set_auth_server_url(SharedString::from(&snap.api_base_url));
    ui.set_auth_allow_http(snap.allow_insecure_http);
    ui.set_llm_endpoint_input(SharedString::from(&snap.llm_endpoint));
    ui.set_llm_model_input(SharedString::from(&snap.llm_model));
    ui.set_llm_key_input(SharedString::from(&snap.llm_api_key));
    ui.set_demo_mode(app_state.demo_mode);

    if is_auth_screenshot {
        ui.set_demo_mode(false);
        ui.set_authenticated(false);
    } else if let Some(sess) = core.api.session() {
        ui.set_authenticated(true);
        ui.set_user_name(SharedString::from(&sess.user.display_name));
        ui.set_user_email(SharedString::from(sess.user.email.as_deref().unwrap_or_default()));
        ui.set_user_initials(SharedString::from(initials_for_name(&sess.user.display_name)));
    } else {
        ui.set_authenticated(false);
    }

    if let Some(scr) = cli.screen.as_deref() {
        if scr != "auth" {
            ui.set_active_nav(SharedString::from(scr));
        }
    }
    if cli.demo_playing {
        apply_demo_seed(&ui, &core, &app_state, dark, accent_now);
        ui.set_playing(true);
        let demo_rgba = KwhaleCoverPalette::placeholder_rgba("Вечерний город", true, 256, 0.0);
        let wpal = extract_widget_palette(Some((&demo_rgba, 256, 256)));
        widget.set_has_track(true);
        widget.set_track_title("Вечерний город".into());
        widget.set_track_artist("Молчат Дома".into());
        widget.set_cover(make_placeholder_image("Вечерний город", true, 256, 0.0));
        widget.set_backdrop(make_widget_backdrop_image(wpal, 420, 140));
        widget.set_artist_color(Color::from_rgb_u8(
            wpal.artist_tint.r,
            wpal.artist_tint.g,
            wpal.artist_tint.b,
        ));
        widget.set_playing(true);
    }
    if cli.player_open {
        ui.set_player_open(true);
    }

    let quitting = Arc::new(AtomicBool::new(false));
    let tray_handle = if cli.screenshot.is_none() {
        let (tray_tx, mut tray_rx) = tokio::sync::mpsc::unbounded_channel::<TrayCommand>();
        let handle = rt.block_on(spawn_tray(tray_tx));
        if let Some(ref th) = handle {
            let th_init = th.clone();
            let main_vis_init = !cli.widget_only;
            let widget_vis_init = cli.widget_only;
            rt_handle.spawn(async move {
                th_init
                    .update(|t| {
                        t.state.main_visible = main_vis_init;
                        t.state.widget_visible = widget_vis_init;
                    })
                    .await;
            });
        }
        let ui_w_t = ui.as_weak();
        let wid_w_t = widget.as_weak();
        let eng_t = engine.clone();
        let th_cmd = handle.clone();
        let quit_t = quitting.clone();
        rt_handle.spawn(async move {
            while let Some(cmd) = tray_rx.recv().await {
                match cmd {
                    TrayCommand::TogglePlay => eng_t.toggle(),
                    TrayCommand::PrevTrack => eng_t.previous(),
                    TrayCommand::NextTrack => eng_t.next(),
                    TrayCommand::ToggleMainWindow => {
                        let u_w = ui_w_t.clone();
                        let th2 = th_cmd.clone();
                        let rt2 = tokio::runtime::Handle::current();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(u) = u_w.upgrade() {
                                let vis = if u.window().is_visible() {
                                    let _ = u.window().hide();
                                    false
                                } else {
                                    let _ = u.show();
                                    true
                                };
                                if let Some(th) = th2 {
                                    rt2.spawn(async move {
                                        th.update(|t| t.state.main_visible = vis).await;
                                    });
                                }
                            }
                        });
                    }
                    TrayCommand::ToggleWidget => {
                        let w_w = wid_w_t.clone();
                        let th2 = th_cmd.clone();
                        let rt2 = tokio::runtime::Handle::current();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = w_w.upgrade() {
                                let vis = if w.window().is_visible() {
                                    let _ = w.window().hide();
                                    false
                                } else {
                                    let _ = w.show();
                                    true
                                };
                                if let Some(th) = th2 {
                                    rt2.spawn(async move {
                                        th.update(|t| t.state.widget_visible = vis).await;
                                    });
                                }
                            }
                        });
                    }
                    TrayCommand::Quit => {
                        quit_t.store(true, Ordering::SeqCst);
                        eng_t.stop();
                        let _ = slint::invoke_from_event_loop(move || {
                            let _ = slint::quit_event_loop();
                        });
                        break;
                    }
                }
            }
        });
        handle
    } else {
        None
    };

    // Live OS/DE appearance auto-switching (GNOME/KDE XDG Portal)
    if cli.screenshot.is_none() {
        let ui_w_a = ui.as_weak();
        let c_a = core.clone();
        let st_a = app_state.clone();
        let sd_a = system_dark.clone();
        let sa_a = system_accent.clone();
        spawn_appearance_watcher(
            &rt_handle,
            initial_app.system_is_dark,
            initial_app.system_accent,
            move |new_dark, new_acc| {
                *sd_a.lock().expect("sd") = new_dark;
                *sa_a.lock().expect("sa") = new_acc;
                let u_w = ui_w_a.clone();
                let c2 = c_a.clone();
                let st2 = st_a.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(u) = u_w.upgrade() {
                        apply_theme_to_ui(&u, &c2, &st2, new_dark, new_acc, None, None, None);
                    }
                });
            },
        );
    }

    // Wire Mini-Widget & Tray window lifecycle callbacks
    {
        let wid_w = widget.as_weak();
        let th_w = tray_handle.clone();
        let rt_w = rt_handle.clone();
        ui.on_open_widget(move || {
            if let Some(w) = wid_w.upgrade() {
                let _ = w.show();
                if let Some(th) = th_w.clone() {
                    rt_w.spawn(async move {
                        th.update(|t| t.state.widget_visible = true).await;
                    });
                }
            }
        });
    }
    {
        let ui_w = ui.as_weak();
        let th_w = tray_handle.clone();
        let rt_w = rt_handle.clone();
        ui.on_hide_to_tray(move || {
            if let Some(u) = ui_w.upgrade() {
                let _ = u.window().hide();
                if let Some(th) = th_w.clone() {
                    rt_w.spawn(async move {
                        th.update(|t| t.state.main_visible = false).await;
                    });
                }
            }
        });
    }
    {
        let ui_w = ui.as_weak();
        let c_cl = core.clone();
        let e_cl = engine.clone();
        let quit_cl = quitting.clone();
        let th_cl = tray_handle.clone();
        let rt_cl = rt_handle.clone();
        ui.window().on_close_requested(move || {
            if !quit_cl.load(Ordering::SeqCst) && c_cl.settings.snapshot().close_to_tray {
                if let Some(u) = ui_w.upgrade() {
                    let _ = u.window().hide();
                    if let Some(th) = th_cl.clone() {
                        rt_cl.spawn(async move {
                            th.update(|t| t.state.main_visible = false).await;
                        });
                    }
                }
                slint::CloseRequestResponse::KeepWindowShown
            } else {
                quit_cl.store(true, Ordering::SeqCst);
                e_cl.stop();
                let _ = slint::quit_event_loop();
                slint::CloseRequestResponse::HideWindow
            }
        });
    }
    {
        let wid_d = widget.as_weak();
        widget.on_start_drag(move || {
            if let Some(w) = wid_d.upgrade() {
                w.window().with_winit_window(|ww| {
                    let _ = ww.drag_window();
                });
            }
        });
    }
    {
        let ui_o = ui.as_weak();
        let th_o = tray_handle.clone();
        let rt_o = rt_handle.clone();
        widget.on_open_app(move || {
            if let Some(u) = ui_o.upgrade() {
                let _ = u.show();
                if let Some(th) = th_o.clone() {
                    rt_o.spawn(async move {
                        th.update(|t| t.state.main_visible = true).await;
                    });
                }
            }
        });
    }
    {
        let ui_q = ui.as_weak();
        let th_q = tray_handle.clone();
        let rt_q = rt_handle.clone();
        widget.on_open_queue(move || {
            if let Some(u) = ui_q.upgrade() {
                u.set_player_open(false);
                u.set_queue_open(true);
                u.set_right_tab("queue".into());
                let _ = u.show();
                if let Some(th) = th_q.clone() {
                    rt_q.spawn(async move {
                        th.update(|t| t.state.main_visible = true).await;
                    });
                }
            }
        });
    }
    {
        let e = engine.clone();
        widget.on_prev_track(move || e.previous());
    }
    {
        let e = engine.clone();
        widget.on_toggle_play(move || e.toggle());
    }
    {
        let e = engine.clone();
        widget.on_next_track(move || e.next());
    }
    {
        let wid_h = widget.as_weak();
        let th_h = tray_handle.clone();
        let rt_h = rt_handle.clone();
        widget.on_hide_widget(move || {
            if let Some(w) = wid_h.upgrade() {
                let _ = w.window().hide();
                if let Some(th) = th_h.clone() {
                    rt_h.spawn(async move {
                        th.update(|t| t.state.widget_visible = false).await;
                    });
                }
            }
        });
    }

    let toast: ToastFn = {
        let weak = ui.as_weak();
        let rt_h = rt_handle.clone();
        Arc::new(move |msg: String| {
            let weak_show = weak.clone();
            let msg_show = msg.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = weak_show.upgrade() {
                    u.set_toast_message(SharedString::from(msg_show));
                }
            });
            let weak_hide = weak.clone();
            rt_h.spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(u) = weak_hide.upgrade() {
                        u.set_toast_message(SharedString::default());
                    }
                });
            });
        })
    };

    // Single-instance D-Bus & MPRIS2
    {
        let weak = ui.as_weak();
        let th_r = tray_handle.clone();
        let rt_r = rt_handle.clone();
        let raise_cb = Arc::new(move || {
            let w = weak.clone();
            let th2 = th_r.clone();
            let rt2 = rt_r.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(u) = w.upgrade() {
                    let _ = u.show();
                    if let Some(th) = th2 {
                        rt2.spawn(async move {
                            th.update(|t| t.state.main_visible = true).await;
                        });
                    }
                }
            });
        });
        spawn_mpris(core.clone(), engine.clone(), raise_cb.clone());
        spawn_single_instance_dbus(raise_cb, Arc::new(|_uri| {}));
    }

    bind_all_callbacks(
        &ui,
        core.clone(),
        engine.clone(),
        app_state.clone(),
        cover_cache.clone(),
        rt_handle.clone(),
        system_dark,
        system_accent,
        toast,
    );

    // Sync AudioEngine -> UI + PlayerWidget + Tray (progress, lyrics, queue, active highlights, real covers)
    {
        let eng = engine.clone();
        let core_c = core.clone();
        let rt_h = rt_handle.clone();
        let cache_c = cover_cache.clone();
        let st_c = app_state.clone();
        let weak = ui.as_weak();
        let wid_weak = widget.as_weak();
        let th_sync = tray_handle.clone();
        rt_handle.spawn(async move {
            let mut rx = eng.subscribe();
            let mut last_cover_key: Option<String> = None;
            let mut last_lyrics_track_id: Option<String> = None;
            let mut last_lyric_active: Option<usize> = None;
            let mut last_active_track_id: Option<String> = None;
            let mut last_queue_sig: Vec<String> = Vec::new();
            let mut last_tray_sig: (Option<String>, bool) = (None, false);
            loop {
                if rx.changed().await.is_err() {
                    break;
                }
                let st = rx.borrow().clone();
                let cur_opt = st.current().cloned();
                let cur_id = cur_opt.as_ref().map(|t| t.remote_id.clone());
                let cur_img = cur_opt.as_ref().and_then(|t| t.cover_image_id.clone());
                let q_sig: Vec<String> = st.queue.iter().map(|t| t.remote_id.clone()).collect();
                let queue_changed = q_sig != last_queue_sig;
                if queue_changed {
                    last_queue_sig = q_sig;
                }
                let progress = st.progress();
                let pos_text = format_duration_player_ms(st.position_ms);
                let dur_text = format_duration_player_ms(st.duration_ms);
                let time_pair = format!("{pos_text} / {dur_text}");
                let rep = match st.repeat_mode {
                    RepeatMode::Off => 0,
                    RepeatMode::All => 1,
                    RepeatMode::One => 2,
                };

                let track_changed = last_active_track_id != cur_id;
                if track_changed {
                    last_active_track_id = cur_id.clone();
                }
                let cover_key = format!(
                    "{}:{}",
                    cur_id.as_deref().unwrap_or(""),
                    cur_img.as_deref().unwrap_or("")
                );
                let cover_changed = last_cover_key.as_deref() != Some(cover_key.as_str());
                if cover_changed {
                    last_cover_key = Some(cover_key);
                }

                if let Some(tid) = cur_id.as_deref() {
                    if last_lyrics_track_id.as_deref() != Some(tid) {
                        last_lyrics_track_id = Some(tid.to_string());
                        last_lyric_active = None;
                        if core_c.api.session().is_some() {
                            let c_l = core_c.clone();
                            let tid_s = tid.to_string();
                            let st_l = st_c.clone();
                            let w_l = weak.clone();
                            let pos_now = st.position_ms;
                            rt_h.spawn(async move {
                                let fetched: Vec<(u64, String)> = match c_l.api.lyrics(&tid_s).await {
                                    Ok(l) if !l.synced.is_empty() => l
                                        .synced
                                        .into_iter()
                                        .map(|ln| (ln.start_ms, ln.text))
                                        .collect(),
                                    Ok(l) => l
                                        .plain
                                        .unwrap_or_default()
                                        .lines()
                                        .map(str::trim)
                                        .filter(|s| !s.is_empty())
                                        .map(|s| (0, s.to_string()))
                                        .collect(),
                                    Err(_) => Vec::new(),
                                };
                                *st_l.lyrics.lock().expect("lyr") = fetched.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(u) = w_l.upgrade() {
                                        u.set_lyric_lines(to_model(lyrics_to_slint(&fetched, pos_now)));
                                    }
                                });
                            });
                        }
                    }
                }

                let cur_lines = st_c.lyrics.lock().expect("lyr").clone();
                let now_act = active_lyric_index(&cur_lines, st.position_ms);
                let lyrics_need_refresh = now_act != last_lyric_active;
                if lyrics_need_refresh {
                    last_lyric_active = now_act;
                }

                let tray_sig = (cur_id.clone(), st.is_playing);
                if tray_sig != last_tray_sig {
                    last_tray_sig = tray_sig;
                    if let Some(th) = th_sync.clone() {
                        let lbl = cur_opt
                            .as_ref()
                            .map(|c| format!("{} — {}", c.title, c.artist))
                            .unwrap_or_default();
                        let has = cur_opt.is_some();
                        let playing = st.is_playing;
                        rt_h.spawn(async move {
                            th.update(|t| {
                                t.state.now_label = lbl;
                                t.state.has_track = has;
                                t.state.is_playing = playing;
                            })
                            .await;
                        });
                    }
                }

                let c_snap = core_c.clone();
                let rt_snap = rt_h.clone();
                let cache_snap = cache_c.clone();
                let st_snap = st_c.clone();
                let w_img = weak.clone();
                let wid_img = wid_weak.clone();
                let q_snapshot = st.queue.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(u) = w_img.upgrade() else { return };
                    u.set_has_current_track(cur_opt.is_some());
                    u.set_playing(st.is_playing);
                    u.set_progress(progress);
                    u.set_shuffle(st.shuffle);
                    u.set_repeat_mode(rep);
                    u.set_volume(st.volume as f32);
                    u.set_muted(st.muted);
                    u.set_now_pos_text(SharedString::from(&pos_text));
                    u.set_now_dur_text(SharedString::from(&dur_text));
                    if lyrics_need_refresh {
                        u.set_lyric_lines(to_model(lyrics_to_slint(&cur_lines, st.position_ms)));
                    }

                    if let Some(pw) = wid_img.upgrade() {
                        pw.set_has_track(cur_opt.is_some());
                        pw.set_playing(st.is_playing);
                        if let Some(cur) = &cur_opt {
                            pw.set_track_title(SharedString::from(&cur.title));
                            pw.set_track_artist(SharedString::from(&cur.artist));
                        } else {
                            pw.set_track_title("KWhale".into());
                            pw.set_track_artist("Нет активного трека".into());
                            let wpal = extract_widget_palette(None);
                            pw.set_backdrop(make_widget_backdrop_image(wpal, 420, 140));
                            pw.set_artist_color(Color::from_rgb_u8(
                                wpal.artist_tint.r,
                                wpal.artist_tint.g,
                                wpal.artist_tint.b,
                            ));
                        }
                    }

                    let dk = u.global::<Theme>().get_is_dark();
                    if let Some(cur) = &cur_opt {
                        u.set_now_title(SharedString::from(&cur.title));
                        u.set_now_artist(SharedString::from(&cur.artist));
                        u.set_now_liked(c_snap.reactions.is_liked(&cur.remote_id, cur.is_liked));
                        u.set_hero_badge("Сейчас играет".into());
                        u.set_hero_title(SharedString::from(&cur.title));
                        u.set_hero_artist(SharedString::from(&cur.artist));
                        u.set_hero_progress(progress);
                        u.set_hero_time(SharedString::from(&time_pair));

                        if cover_changed {
                            let dom = KwhaleCoverPalette::gradient(&cur.title, dk).0;
                            *st_snap.now_dominant.lock().expect("nd") = dom;
                            *st_snap.hero_cover_decoded.lock().expect("hcd") = None;
                            let acc = *st_snap.current_accent.lock().expect("ca");
                            let ph_mini = make_placeholder_image(&cur.title, dk, 256, 0.0);
                            let ph_hd = make_placeholder_image(&cur.title, dk, 512, 0.0);
                            u.set_now_cover(ph_mini.clone());
                            u.set_now_cover_hd(ph_hd);
                            u.set_hero_cover(ph_mini);
                            u.set_has_hero_cover(true);
                            u.set_player_backdrop(make_player_backdrop_image(dom, acc, dk, 800, 450));
                            u.set_hero_backdrop(make_hero_backdrop_image(&cur.title, acc, dk, 960, 440, 0.0));
                            if let Some(pw) = wid_img.upgrade() {
                                let ph_rgba = KwhaleCoverPalette::placeholder_rgba(&cur.title, true, 256, 0.0);
                                let wpal = extract_widget_palette(Some((&ph_rgba, 256, 256)));
                                pw.set_cover(make_placeholder_image(&cur.title, true, 256, 0.0));
                                pw.set_backdrop(make_widget_backdrop_image(wpal, 420, 140));
                                pw.set_artist_color(Color::from_rgb_u8(
                                    wpal.artist_tint.r,
                                    wpal.artist_tint.g,
                                    wpal.artist_tint.b,
                                ));
                            }

                            if let Some(iid) = cur_img.as_deref().and_then(|s| image_id_or_null(Some(s))) {
                                let w2 = w_img.clone();
                                let wid2 = wid_img.clone();
                                let st2 = st_snap.clone();
                                let cur_title_snap = cur.title.clone();
                                spawn_fetch_cover_decoded(
                                    &rt_snap,
                                    c_snap.clone(),
                                    cache_snap.clone(),
                                    iid.to_string(),
                                    "lg",
                                    800,
                                    0.0,
                                    move |cov| {
                                        *st2.now_dominant.lock().expect("nd") = cov.dominant;
                                        *st2.hero_cover_decoded.lock().expect("hcd") =
                                            Some((cur_title_snap, cov.clone()));
                                        let acc = *st2.current_accent.lock().expect("ca");
                                        let mini = cov.resized_lanczos(256);
                                        if let Some(u2) = w2.upgrade() {
                                            let dk2 = u2.global::<Theme>().get_is_dark();
                                            u2.set_now_cover_hd(cov.to_slint_image());
                                            u2.set_now_cover(mini.to_slint_image());
                                            u2.set_player_backdrop(make_player_backdrop_image(
                                                cov.dominant,
                                                acc,
                                                dk2,
                                                800,
                                                450,
                                            ));
                                            if u2.get_hero_badge() == "Сейчас играет" {
                                                u2.set_hero_cover(mini.to_slint_image());
                                                u2.set_has_hero_cover(true);
                                                u2.set_hero_backdrop(make_hero_backdrop_from_cover(
                                                    &cov.rgba,
                                                    cov.width,
                                                    cov.height,
                                                    cov.dominant,
                                                    acc,
                                                    dk2,
                                                    960,
                                                    440,
                                                ));
                                            }
                                        }
                                        if let Some(pw2) = wid2.upgrade() {
                                            let wpal = extract_widget_palette(Some((
                                                mini.rgba.as_slice(),
                                                mini.width,
                                                mini.height,
                                            )));
                                            pw2.set_cover(mini.to_slint_image());
                                            pw2.set_backdrop(make_widget_backdrop_image(wpal, 420, 140));
                                            pw2.set_artist_color(Color::from_rgb_u8(
                                                wpal.artist_tint.r,
                                                wpal.artist_tint.g,
                                                wpal.artist_tint.b,
                                            ));
                                        }
                                    },
                                );
                            }
                        }
                    }

                    if queue_changed {
                        u.set_queue_items(to_model(ui_tracks_to_slint(&q_snapshot, cur_id.as_deref(), &c_snap, dk)));
                        if !q_snapshot.is_empty() {
                            hydrate_track_covers(w_img.clone(), &rt_snap, &c_snap, &cache_snap, &q_snapshot, TrackListTarget::Queue);
                        }
                    }
                    if track_changed {
                        sync_track_models(&u, |slot| {
                            let is_act = cur_id.as_deref() == Some(slot.id.as_str());
                            if slot.active != is_act {
                                slot.active = is_act;
                                true
                            } else {
                                false
                            }
                        });
                    }
                });
            }
        });
    }

    // Sync Downloads -> UI
    {
        let core_d = core.clone();
        let st_d = app_state.clone();
        let cc_d = cover_cache.clone();
        let rt_d = rt_handle.clone();
        let weak_d = ui.as_weak();
        let initial_dl = core.downloads.snapshot();
        {
            let (dl_items, dl_tracks) = downloads_to_slint(&initial_dl, dark);
            *app_state.download_tracks.lock().expect("dt") = dl_tracks.clone();
            ui.set_download_items(to_model(dl_items));
            if core.api.session().is_some() {
                for (idx, tr) in dl_tracks.iter().enumerate() {
                    if let Some(cid) = tr.cover_image_id.clone() {
                        let w3 = weak_d.clone();
                        let eid = tr.remote_id.clone();
                        spawn_fetch_cover(&rt_d, core_d.clone(), cc_d.clone(), cid, "sm", 160, 0.0, move |img, _| {
                            if let Some(u3) = w3.upgrade() {
                                let model = u3.get_download_items();
                                if let Some(mut slot) = model.row_data(idx) {
                                    if slot.id == eid {
                                        slot.cover = img;
                                        model.set_row_data(idx, slot);
                                    }
                                }
                            }
                        });
                    }
                }
            }
        }
        rt_handle.spawn(async move {
            let mut rx = core_d.downloads.subscribe();
            loop {
                if rx.changed().await.is_err() {
                    break;
                }
                let snap = rx.borrow().clone();
                let w2 = weak_d.clone();
                let st2 = st_d.clone();
                let c2 = core_d.clone();
                let cc2 = cc_d.clone();
                let rt2 = rt_d.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(u) = w2.upgrade() else { return };
                    let dk = u.global::<Theme>().get_is_dark();
                    let (items, tracks) = downloads_to_slint(&snap, dk);
                    *st2.download_tracks.lock().expect("dt") = tracks.clone();
                    u.set_download_items(to_model(items));
                    sync_track_models(&u, |slot| {
                        let dl_now = c2.downloads.is_downloaded(slot.id.as_str());
                        if slot.downloaded != dl_now {
                            slot.downloaded = dl_now;
                            true
                        } else {
                            false
                        }
                    });
                    if c2.api.session().is_some() {
                        for (idx, tr) in tracks.iter().enumerate() {
                            if let Some(cid) = tr.cover_image_id.clone() {
                                let w3 = w2.clone();
                                let eid = tr.remote_id.clone();
                                spawn_fetch_cover(&rt2, c2.clone(), cc2.clone(), cid, "sm", 160, 0.0, move |img, _| {
                                    if let Some(u3) = w3.upgrade() {
                                        let model = u3.get_download_items();
                                        if let Some(mut slot) = model.row_data(idx) {
                                            if slot.id == eid {
                                                slot.cover = img;
                                                model.set_row_data(idx, slot);
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    }
                });
            }
        });
    }

    // Sync Reaction changes across track lists
    {
        let core_r = core.clone();
        let eng_r = engine.clone();
        let w_r = ui.as_weak();
        rt_handle.spawn(async move {
            let mut rx = core_r.reactions.subscribe();
            loop {
                if rx.changed().await.is_err() {
                    break;
                }
                let w2 = w_r.clone();
                let c2 = core_r.clone();
                let e2 = eng_r.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(u) = w2.upgrade() else { return };
                    if let Some(cur) = e2.state().current() {
                        u.set_now_liked(c2.reactions.is_liked(&cur.remote_id, cur.is_liked));
                    }
                    sync_track_models(&u, |slot| {
                        let lk = c2.reactions.is_liked(slot.id.as_str(), slot.liked);
                        let dl = c2.reactions.is_disliked(slot.id.as_str(), slot.disliked);
                        if slot.liked != lk || slot.disliked != dl {
                            slot.liked = lk;
                            slot.disliked = dl;
                            true
                        } else {
                            false
                        }
                    });
                });
            }
        });
    }

    // Kick off live backend loading if logged in (and not in demo screenshot mode)
    if core.api.session().is_some() && !cli.demo_playing && !is_auth_screenshot {
        load_all_backend_data(
            ui.as_weak(),
            core.clone(),
            engine.clone(),
            app_state,
            cover_cache,
            rt_handle,
            false,
        );
    }

    // Optional headless screenshot capture mode
    if let Some(out_path) = cli.screenshot {
        if cli.widget_only {
            widget.show()?;
        } else {
            ui.show()?;
        }
        let weak = ui.as_weak();
        let wid_w = widget.as_weak();
        let widget_only = cli.widget_only;
        let snap_timer = slint::Timer::default();
        snap_timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(240),
            move || {
                let res = if widget_only {
                    wid_w.upgrade().map(|w| w.window().take_snapshot())
                } else {
                    weak.upgrade().map(|u| u.window().take_snapshot())
                };
                if let Some(snap_res) = res {
                    match snap_res {
                        Ok(buf) => {
                            if let Err(err) =
                                save_rgba_png(&out_path, buf.width(), buf.height(), buf.as_bytes())
                            {
                                eprintln!("screenshot save error: {err}");
                            }
                        }
                        Err(err) => eprintln!("take_snapshot error: {err}"),
                    }
                }
                let _ = slint::quit_event_loop();
            },
        );
        slint::run_event_loop()?;
        drop(snap_timer);
        return Ok(());
    }

    if cli.widget_only {
        widget.show()?;
    } else {
        ui.show()?;
    }
    slint::run_event_loop_until_quit()?;
    // Ensure clean flush on exit
    if let Some(th) = tray_handle {
        rt.block_on(th.shutdown());
    }
    engine.stop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_views_have_scrollable_viewports_and_respond_to_mouse_wheel() {
        i_slint_backend_testing::init_no_event_loop();
        let ui = MainWindow::new().expect("create MainWindow");
        ui.set_demo_mode(true);

        let tracks: Vec<TrackItem> = (0..18)
            .map(|i| TrackItem {
                id: SharedString::from(format!("t{i}")),
                title: SharedString::from(format!("Track {i}")),
                artist: SharedString::from("Artist"),
                album: SharedString::from("Album"),
                duration: SharedString::from("3:30"),
                cover: slint::Image::default(),
                active: i == 0,
                liked: false,
                disliked: false,
                downloaded: false,
                explicit: false,
            })
            .collect();
        ui.set_tracks(to_model(tracks.clone()));
        ui.set_queue_items(to_model(tracks));
        ui.set_active_nav("home".into());

        ui.window().set_size(slint::LogicalSize::new(1280.0, 720.0));
        // Pump timers/layout passes so preferred-heights settle
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));

        let main_vh = ui.get_debug_main_viewport_height();
        let main_vis = ui.get_debug_main_visible_height();
        assert!(
            main_vh > main_vis + 100.0,
            "main ScrollView must exceed visible height ({main_vh} vs {main_vis})"
        );

        let q_vh = ui.get_debug_queue_viewport_height();
        let q_vis = ui.get_debug_queue_visible_height();
        assert!(
            q_vh > q_vis + 50.0,
            "queue ScrollView must exceed visible height ({q_vh} vs {q_vis})"
        );

        let initial_main_y = ui.get_debug_main_viewport_y();
        let initial_q_y = ui.get_debug_queue_viewport_y();

        // Dispatch mouse wheel down events over the center view (x: 500, y: 400)
        ui.window().dispatch_event(slint::platform::WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(500.0, 400.0),
            delta_x: 0.0,
            delta_y: -180.0,
        });
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
        let after_main_y = ui.get_debug_main_viewport_y();
        assert!(
            after_main_y < initial_main_y,
            "main ScrollView did not scroll on mouse wheel ({initial_main_y} -> {after_main_y})"
        );

        // Dispatch mouse wheel down over the right queue panel (x: 1180, y: 300)
        ui.window().dispatch_event(slint::platform::WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(1180.0, 300.0),
            delta_x: 0.0,
            delta_y: -160.0,
        });
        i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_millis(50));
        let after_q_y = ui.get_debug_queue_viewport_y();
        assert!(
            after_q_y < initial_q_y,
            "queue ScrollView did not scroll on mouse wheel ({initial_q_y} -> {after_q_y})"
        );
    }
}
