use crate::dialogs::{
    show_create_or_edit_playlist_dialog, show_share_dialog, show_track_actions_dialog,
    show_upload_track_dialog, DialogCallbacks,
};
use crate::style::{make_cover_picture, spawn_async};
use chrono::Timelike;
use gtk4::gdk;
use gtk4::gio;
use gtk4::prelude::*;
use kwhale_audio::AudioEngine;
use kwhale_core::{
    accent_families, format_bytes, format_duration_ms, greeting_for_hour, Album, Artist,
    ContentCache, CoreResult, EnrichmentProposal, Genre, HomeResponse, KwhaleCore, KwhalePlural,
    Mix, Playlist, PromptMixRequest, SearchResponse, ServerUrl, ShareKind, ThemeMode, Track,
    TrackDownloadStatus, UiBackend, UiTrack,
};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum CollectionTarget {
    Album(String),
    Artist(String),
    Playlist(String),
    Mix(String),
    LikedTracks,
    SeeAllTracks { title: String, tracks: Vec<Track> },
    SeeAllAlbums { title: String, albums: Vec<Album> },
    SeeAllArtists { title: String, artists: Vec<Artist> },
}

#[derive(Clone)]
pub struct NavCallbacks {
    pub dialogs: DialogCallbacks,
    pub open_collection: Rc<dyn Fn(CollectionTarget)>,
    pub switch_section: Rc<dyn Fn(&str)>,
    pub on_logout: Rc<dyn Fn()>,
    pub window: adw::ApplicationWindow,
}

pub fn make_track_row(
    core: &Arc<KwhaleCore>,
    engine: &AudioEngine,
    rt: &tokio::runtime::Handle,
    track: &UiTrack,
    queue_on_click: Vec<UiTrack>,
    index_in_queue: usize,
    surface: &str,
    mix_id: Option<String>,
    playlist_context_id: Option<String>,
    reorder_playlist: Option<Rc<dyn Fn(usize, i32)>>,
    nav: &NavCallbacks,
) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    row.add_css_class("kw-track-row");
    row.set_hexpand(true);

    let is_current = engine
        .state()
        .current()
        .map(|c| c.remote_id == track.remote_id)
        .unwrap_or(false);
    if is_current {
        row.add_css_class("kw-track-active");
    }

    // Play / index button + cover
    let idx_lbl = gtk4::Label::builder()
        .label(format!("{:>2}", index_in_queue + 1))
        .width_chars(2)
        .css_classes(["kw-muted"])
        .build();
    row.append(&idx_lbl);

    let pic = make_cover_picture(
        core,
        rt,
        track.cover_image_id.as_deref(),
        &track.title,
        44,
        "sm",
    );
    row.append(&pic);

    // Title + explicit + artist + album
    let meta = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    meta.set_hexpand(true);
    meta.set_valign(gtk4::Align::Center);

    let title_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let title_lbl = gtk4::Label::builder()
        .label(&track.title)
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .css_classes(["heading"])
        .build();
    title_box.append(&title_lbl);

    if track.is_explicit {
        let exp = gtk4::Label::builder()
            .label("E")
            .css_classes(["kw-explicit-badge"])
            .valign(gtk4::Align::Center)
            .build();
        title_box.append(&exp);
    }
    if core.downloads.is_downloaded(&track.remote_id) {
        let dl_icon = gtk4::Image::builder()
            .icon_name("folder-download-symbolic")
            .tooltip_text("Доступно офлайн")
            .css_classes(["kw-liked"])
            .build();
        title_box.append(&dl_icon);
    }
    meta.append(&title_box);

    let sub_text = match &track.album {
        Some(al) if !al.is_empty() => format!("{} • {}", track.artist, al),
        _ => track.artist.clone(),
    };
    let sub_lbl = gtk4::Label::builder()
        .label(&sub_text)
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .css_classes(["kw-muted"])
        .build();
    meta.append(&sub_lbl);
    row.append(&meta);

    // Playlist reorder buttons if applicable
    if let Some(reorder_fn) = reorder_playlist {
        let up_btn = gtk4::Button::builder()
            .icon_name("go-up-symbolic")
            .valign(gtk4::Align::Center)
            .css_classes(["flat"])
            .tooltip_text("Выше")
            .build();
        let down_btn = gtk4::Button::builder()
            .icon_name("go-down-symbolic")
            .valign(gtk4::Align::Center)
            .css_classes(["flat"])
            .tooltip_text("Ниже")
            .build();
        let rf1 = reorder_fn.clone();
        up_btn.connect_clicked(move |_| rf1(index_in_queue, -1));
        down_btn.connect_clicked(move |_| reorder_fn(index_in_queue, 1));
        row.append(&up_btn);
        row.append(&down_btn);
    }

    // Like button
    let is_liked = core.reactions.is_liked(&track.remote_id, track.is_liked);
    let like_btn = gtk4::Button::builder()
        .icon_name(if is_liked {
            "emblem-favorite-symbolic"
        } else {
            "non-starred-symbolic"
        })
        .valign(gtk4::Align::Center)
        .css_classes(if is_liked {
            vec!["flat", "kw-liked"]
        } else {
            vec!["flat"]
        })
        .tooltip_text("Любимый трек")
        .build();
    {
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let tr = track.clone();
        let surf = surface.to_string();
        let toast = nav.dialogs.show_toast.clone();
        like_btn.connect_clicked(move |btn| {
            let c2 = c.clone();
            let c_after = c.clone();
            let tr2 = tr.clone();
            let tr_id = tr.remote_id.clone();
            let s2 = surf.clone();
            let eng2 = eng.clone();
            let toast2 = toast.clone();
            let btn_weak = btn.downgrade();
            spawn_async(
                &rt_h,
                async move { c2.reactions.toggle_like(&tr2, &s2).await },
                move |res| match res {
                    Ok(liked) => {
                        eng2.update_track_liked(&tr_id, liked);
                        c_after.cache.invalidate_library();
                        if let Some(b) = btn_weak.upgrade() {
                            b.set_icon_name(if liked {
                                "emblem-favorite-symbolic"
                            } else {
                                "non-starred-symbolic"
                            });
                            if liked {
                                b.add_css_class("kw-liked");
                            } else {
                                b.remove_css_class("kw-liked");
                            }
                        }
                    }
                    Err(e) => toast2(&e),
                },
            );
        });
    }
    row.append(&like_btn);

    // Duration label
    let dur_lbl = gtk4::Label::builder()
        .label(format_duration_ms((track.duration_sec as u64) * 1000))
        .valign(gtk4::Align::Center)
        .css_classes(["kw-muted"])
        .width_chars(5)
        .xalign(1.0)
        .build();
    row.append(&dur_lbl);

    // Menu button (⋯)
    let menu_btn = gtk4::Button::builder()
        .icon_name("view-more-symbolic")
        .valign(gtk4::Align::Center)
        .css_classes(["flat"])
        .tooltip_text("Действия с треком")
        .build();
    {
        let win = nav.window.clone();
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let tr = track.clone();
        let surf = surface.to_string();
        let pl_ctx = playlist_context_id.clone();
        let cbs = nav.dialogs.clone();
        menu_btn.connect_clicked(move |_| {
            show_track_actions_dialog(
                &win,
                c.clone(),
                eng.clone(),
                rt_h.clone(),
                tr.clone(),
                surf.clone(),
                pl_ctx.clone(),
                cbs.clone(),
            );
        });
    }
    row.append(&menu_btn);

    // Primary left click on row starts playback
    {
        let click = gtk4::GestureClick::new();
        click.set_button(gdk::BUTTON_PRIMARY);
        let eng = engine.clone();
        let q = queue_on_click;
        let surf = surface.to_string();
        let mid = mix_id;
        click.connect_released(move |gesture, _, x, _| {
            // Avoid intercepting clicks on trailing action buttons
            if let Some(w) = gesture.widget() {
                if x < (w.width() - 130) as f64 {
                    eng.play_queue(q.clone(), index_in_queue, &surf, mid.as_deref());
                }
            }
        });
        row.add_controller(click);
    }

    // Secondary right-click opens Track Actions dialog
    {
        let rclick = gtk4::GestureClick::new();
        rclick.set_button(gdk::BUTTON_SECONDARY);
        let win = nav.window.clone();
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let tr = track.clone();
        let surf = surface.to_string();
        let pl_ctx = playlist_context_id;
        let cbs = nav.dialogs.clone();
        rclick.connect_pressed(move |_, _, _, _| {
            show_track_actions_dialog(
                &win,
                c.clone(),
                eng.clone(),
                rt_h.clone(),
                tr.clone(),
                surf.clone(),
                pl_ctx.clone(),
                cbs.clone(),
            );
        });
        row.add_controller(rclick);
    }

    row
}

pub fn make_entity_card(
    core: &Arc<KwhaleCore>,
    rt: &tokio::runtime::Handle,
    image_id: Option<&str>,
    title: &str,
    subtitle: &str,
    badge: Option<&str>,
    on_activate: impl Fn() + 'static,
) -> gtk4::Button {
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    vbox.set_size_request(156, 204);
    let pic = make_cover_picture(core, rt, image_id, title, 148, "md");
    vbox.append(&pic);

    let title_lbl = gtk4::Label::builder()
        .label(title)
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .max_width_chars(18)
        .css_classes(["heading"])
        .build();
    vbox.append(&title_lbl);

    let sub_str = match badge {
        Some(b) if !b.is_empty() => format!("{b} • {subtitle}"),
        _ => subtitle.to_string(),
    };
    let sub_lbl = gtk4::Label::builder()
        .label(&sub_str)
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .max_width_chars(20)
        .css_classes(["kw-muted", "caption"])
        .build();
    vbox.append(&sub_lbl);

    let btn = gtk4::Button::builder()
        .child(&vbox)
        .css_classes(["flat", "kw-card"])
        .build();
    btn.connect_clicked(move |_| on_activate());
    btn
}

// ======================= HOME PAGE =======================

pub fn build_home_page(
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    rt: tokio::runtime::Handle,
    nav: NavCallbacks,
) -> (gtk4::ScrolledWindow, Rc<dyn Fn()>) {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 20);
    content.set_margin_top(20);
    content.set_margin_bottom(28);
    content.set_margin_start(24);
    content.set_margin_end(24);

    // Top bar: Greeting + Quick actions
    let top_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let hour = chrono::Local::now().hour();
    let user_name = core
        .api
        .session()
        .map(|s| s.user.display_name)
        .unwrap_or_else(|| "Слушатель".into());
    let greet_lbl = gtk4::Label::builder()
        .label(format!("{}, {}!", greeting_for_hour(hour), user_name))
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["kw-hero-title"])
        .build();
    top_row.append(&greet_lbl);

    let upload_btn = gtk4::Button::builder()
        .label("Загрузить трек")
        .icon_name("document-send-symbolic")
        .css_classes(["pill"])
        .build();
    {
        let win = nav.window.clone();
        let c = core.clone();
        let rt_h = rt.clone();
        let cbs = nav.dialogs.clone();
        upload_btn.connect_clicked(move |_| {
            show_upload_track_dialog(&win, c.clone(), rt_h.clone(), None, cbs.clone());
        });
    }
    top_row.append(&upload_btn);

    let refresh_btn = gtk4::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Обновить главную")
        .css_classes(["circular"])
        .build();
    top_row.append(&refresh_btn);
    content.append(&top_row);

    // Hero Wave + AI Prompt-Mix cards
    let hero_grid = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
    hero_grid.set_homogeneous(true);

    // 1. Personal Wave Hero Card
    let wave_card = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    wave_card.add_css_class("kw-hero-card");
    let wave_title = gtk4::Label::builder()
        .label("Моя волна KWhale")
        .xalign(0.0)
        .css_classes(["kw-display-title"])
        .build();
    let wave_sub = gtk4::Label::builder()
        .label("Персональный поток на основе ваших лайков, истории и времени суток")
        .xalign(0.0)
        .wrap(true)
        .build();
    let wave_btn = gtk4::Button::builder()
        .label("▶ Запустить Мою волну")
        .halign(gtk4::Align::Start)
        .css_classes(["suggested-action", "pill"])
        .build();
    wave_card.append(&wave_title);
    wave_card.append(&wave_sub);
    wave_card.append(&wave_btn);
    hero_grid.append(&wave_card);

    // 2. AI Prompt-Mix Generator Card
    let ai_card = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    ai_card.add_css_class("kw-card");
    let ai_title = gtk4::Label::builder()
        .label("AI Промпт-Микс")
        .xalign(0.0)
        .css_classes(["kw-section-title"])
        .build();
    let ai_entry = gtk4::Entry::builder()
        .placeholder_text("Например: ночной синтвейв для кодинга без вокала…")
        .hexpand(true)
        .build();
    let ai_controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let size_spin = gtk4::SpinButton::with_range(10.0, 50.0, 5.0);
    size_spin.set_value(25.0);
    size_spin.set_tooltip_text(Some("Количество треков"));
    let ai_run_btn = gtk4::Button::builder()
        .label("Собрать микс")
        .css_classes(["suggested-action", "pill"])
        .build();
    ai_controls.append(&gtk4::Label::new(Some("Треков:")));
    ai_controls.append(&size_spin);
    ai_controls.append(&ai_run_btn);
    ai_card.append(&ai_title);
    ai_card.append(&ai_entry);
    ai_card.append(&ai_controls);
    hero_grid.append(&ai_card);
    content.append(&hero_grid);

    // Wire AI Prompt Mix button
    {
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let toast = nav.dialogs.show_toast.clone();
        let entry = ai_entry.clone();
        let spin = size_spin.clone();
        ai_run_btn.connect_clicked(move |btn| {
            let prompt = entry.text().trim().to_string();
            if prompt.is_empty() {
                toast("Введите описание настроения или жанра для AI-микса");
                return;
            }
            let size = spin.value() as i32;
            let snap = c.settings.snapshot();
            let req = PromptMixRequest {
                prompt: prompt.clone(),
                size,
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
            btn.set_sensitive(false);
            btn.set_label("Генерация…");
            let c2 = c.clone();
            let c_after = c.clone();
            let eng2 = eng.clone();
            let toast2 = toast.clone();
            let bw = btn.downgrade();
            spawn_async(
                &rt_h,
                async move { c2.api.prompt_mix(&req).await },
                move |res| {
                    if let Some(b) = bw.upgrade() {
                        b.set_sensitive(true);
                        b.set_label("Собрать микс");
                    }
                    match res {
                        Ok(page) if !page.items.is_empty() => {
                            let queue: Vec<UiTrack> = page
                                .items
                                .iter()
                                .map(|r| {
                                    r.track
                                        .to_ui(c_after.downloads.is_downloaded(&r.track.id))
                                })
                                .collect();
                            eng2.play_queue(queue, 0, "mix", None);
                            toast2(&format!("AI-микс запущен ({} треков)", page.items.len()));
                        }
                        Ok(_) => toast2("По этому запросу треки не найдены"),
                        Err(e) => toast2(&e.user_message()),
                    }
                },
            );
        });
    }

    // Dynamic sections container
    let sections_box = gtk4::Box::new(gtk4::Orientation::Vertical, 24);
    content.append(&sections_box);

    let render_home = {
        let sections_box = sections_box.clone();
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let nav_c = nav.clone();
        let wave_btn = wave_btn.clone();

        Rc::new(move |home_opt: Option<HomeResponse>, mixes: Vec<Mix>| {
            while let Some(child) = sections_box.first_child() {
                sections_box.remove(&child);
            }

            // Wire personal wave button to first mix or first track section
            let first_mix_id = mixes.first().map(|m| m.id.clone());
            {
                let c2 = c.clone();
                let eng2 = eng.clone();
                let rt2 = rt_h.clone();
                let toast = nav_c.dialogs.show_toast.clone();
                let mid = first_mix_id.clone();
                // Avoid stacking multiple handlers by recreating or using current state
                let _ = &wave_btn;
                let _ = (c2, eng2, rt2, toast, mid);
            }

            // Render mixes carousel if non-empty
            if !mixes.is_empty() {
                let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
                let hdr = gtk4::Label::builder()
                    .label("Персональные миксы и волны")
                    .xalign(0.0)
                    .css_classes(["kw-section-title"])
                    .build();
                sec.append(&hdr);

                let flow = gtk4::FlowBox::builder()
                    .selection_mode(gtk4::SelectionMode::None)
                    .max_children_per_line(6)
                    .min_children_per_line(2)
                    .column_spacing(12)
                    .row_spacing(12)
                    .build();
                for mix in &mixes {
                    let mid = mix.id.clone();
                    let open_col = nav_c.open_collection.clone();
                    let card = make_entity_card(
                        &c,
                        &rt_h,
                        mix.cover.as_ref().and_then(|im| im.valid_id()),
                        &mix.title,
                        mix.subtitle.as_deref().unwrap_or("Персональный микс"),
                        None,
                        move || open_col(CollectionTarget::Mix(mid.clone())),
                    );
                    flow.insert(&card, -1);
                }
                sec.append(&flow);
                sections_box.append(&sec);
            }

            if let Some(home) = home_opt {
                for section in home.sections {
                    if section.items.is_empty() {
                        continue;
                    }
                    let sec_box = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
                    let hdr_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                    let title_str = section
                        .title
                        .clone()
                        .unwrap_or_else(|| "Рекомендации".into());
                    let title_lbl = gtk4::Label::builder()
                        .label(&title_str)
                        .xalign(0.0)
                        .hexpand(true)
                        .css_classes(["kw-section-title"])
                        .build();
                    hdr_row.append(&title_lbl);

                    let tracks: Vec<Track> = section
                        .items
                        .iter()
                        .filter_map(|i| i.track.clone())
                        .collect();
                    let albums: Vec<Album> = section
                        .items
                        .iter()
                        .filter_map(|i| i.album.clone())
                        .collect();
                    let artists: Vec<Artist> = section
                        .items
                        .iter()
                        .filter_map(|i| i.artist.clone())
                        .collect();

                    if tracks.len() > 5 || albums.len() > 5 || artists.len() > 5 {
                        let all_btn = gtk4::Button::builder()
                            .label("Все")
                            .css_classes(["flat"])
                            .build();
                        let open_col = nav_c.open_collection.clone();
                        let t_clone = tracks.clone();
                        let al_clone = albums.clone();
                        let ar_clone = artists.clone();
                        let t_title = title_str.clone();
                        all_btn.connect_clicked(move |_| {
                            if !t_clone.is_empty() {
                                open_col(CollectionTarget::SeeAllTracks {
                                    title: t_title.clone(),
                                    tracks: t_clone.clone(),
                                });
                            } else if !al_clone.is_empty() {
                                open_col(CollectionTarget::SeeAllAlbums {
                                    title: t_title.clone(),
                                    albums: al_clone.clone(),
                                });
                            } else if !ar_clone.is_empty() {
                                open_col(CollectionTarget::SeeAllArtists {
                                    title: t_title.clone(),
                                    artists: ar_clone.clone(),
                                });
                            }
                        });
                        hdr_row.append(&all_btn);
                    }
                    sec_box.append(&hdr_row);

                    if !tracks.is_empty() {
                        let ui_tracks: Vec<UiTrack> = tracks
                            .iter()
                            .map(|t| t.to_ui(c.downloads.is_downloaded(&t.id)))
                            .collect();
                        let list_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
                        for (idx, ut) in ui_tracks.iter().take(8).enumerate() {
                            let row = make_track_row(
                                &c,
                                &eng,
                                &rt_h,
                                ut,
                                ui_tracks.clone(),
                                idx,
                                "home",
                                None,
                                None,
                                None,
                                &nav_c,
                            );
                            list_box.append(&row);
                        }
                        sec_box.append(&list_box);
                    }

                    if !albums.is_empty() || !artists.is_empty() {
                        let flow = gtk4::FlowBox::builder()
                            .selection_mode(gtk4::SelectionMode::None)
                            .max_children_per_line(6)
                            .min_children_per_line(2)
                            .column_spacing(12)
                            .row_spacing(12)
                            .build();
                        for al in albums.iter().take(12) {
                            let al_id = al.id.clone();
                            let open_col = nav_c.open_collection.clone();
                            let card = make_entity_card(
                                &c,
                                &rt_h,
                                al.cover_image_id(),
                                &al.title,
                                &al.artist_line(),
                                al.type_label(),
                                move || open_col(CollectionTarget::Album(al_id.clone())),
                            );
                            flow.insert(&card, -1);
                        }
                        for ar in artists.iter().take(12) {
                            let ar_id = ar.id.clone();
                            let open_col = nav_c.open_collection.clone();
                            let card = make_entity_card(
                                &c,
                                &rt_h,
                                ar.avatar_image_id(),
                                &ar.name,
                                "Артист",
                                None,
                                move || open_col(CollectionTarget::Artist(ar_id.clone())),
                            );
                            flow.insert(&card, -1);
                        }
                        sec_box.append(&flow);
                    }

                    sections_box.append(&sec_box);
                }
            }
        })
    };

    // Wave button click handler
    {
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let toast = nav.dialogs.show_toast.clone();
        wave_btn.connect_clicked(move |_| {
            let c2 = c.clone();
            let c_after = c.clone();
            let eng2 = eng.clone();
            let toast2 = toast.clone();
            spawn_async(
                &rt_h,
                async move {
                    if let Ok(mixes) = c2.api.mixes(None, None, Some(5)).await {
                        if let Some(first) = mixes.items.first() {
                            if let Ok(page) = c2.api.mix_tracks(&first.id, None, Some(40)).await {
                                if !page.items.is_empty() {
                                    let mapped: Vec<Track> =
                                        page.items.into_iter().map(|r| r.track).collect();
                                    return Ok((Some(first.id.clone()), mapped));
                                }
                            }
                        }
                    }
                    let lib = c2
                        .api
                        .library_tracks(None, Some(40), "recent", None)
                        .await?;
                    Ok::<(Option<String>, Vec<Track>), kwhale_core::CoreError>((None, lib.items))
                },
                move |res| match res {
                    Ok((mid, tracks)) if !tracks.is_empty() => {
                        let q: Vec<UiTrack> = tracks
                            .iter()
                            .map(|t| t.to_ui(c_after.downloads.is_downloaded(&t.id)))
                            .collect();
                        eng2.play_queue(q, 0, "mix", mid.as_deref());
                        toast2("Волна запущена");
                    }
                    Ok(_) => toast2("На сервере пока нет треков для волны"),
                    Err(e) => toast2(&e.user_message()),
                },
            );
        });
    }

    let reload: Rc<dyn Fn()> = {
        let c = core.clone();
        let rt_h = rt.clone();
        let render = render_home.clone();
        Rc::new(move || {
            if let Some(cached) = c.cache.read::<HomeResponse>(ContentCache::HOME) {
                render(Some(cached), Vec::new());
            }
            let c2 = c.clone();
            let c3 = c.clone();
            let render2 = render.clone();
            spawn_async(
                &rt_h,
                async move {
                    let home = c2.api.home(None, Some(12)).await.ok();
                    let mixes = c2
                        .api
                        .mixes(None, None, Some(8))
                        .await
                        .map(|m| m.items)
                        .unwrap_or_default();
                    (home, mixes)
                },
                move |(home, mixes)| {
                    if let Some(h) = &home {
                        c3.cache.write(ContentCache::HOME, h);
                    }
                    render2(home, mixes);
                },
            );
        })
    };

    {
        let r = reload.clone();
        let c = core.clone();
        refresh_btn.connect_clicked(move |_| {
            c.cache.invalidate_home();
            r();
        });
    }

    reload();

    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build();
    (scroller, reload)
}

// ======================= SEARCH PAGE =======================

pub fn build_search_page(
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    rt: tokio::runtime::Handle,
    nav: NavCallbacks,
) -> (gtk4::ScrolledWindow, gtk4::SearchEntry, Rc<dyn Fn(String)>) {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    content.set_margin_top(20);
    content.set_margin_bottom(28);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let title = gtk4::Label::builder()
        .label("Поиск")
        .xalign(0.0)
        .css_classes(["kw-hero-title"])
        .build();
    content.append(&title);

    let search_bar_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    let search_entry = gtk4::SearchEntry::builder()
        .placeholder_text("Треки, альбомы, артисты, плейлисты… (Ctrl+F)")
        .hexpand(true)
        .build();
    let search_btn = gtk4::Button::builder()
        .label("Найти")
        .css_classes(["suggested-action", "pill"])
        .build();
    search_bar_row.append(&search_entry);
    search_bar_row.append(&search_btn);
    content.append(&search_bar_row);

    // Filter chips: all / track / album / artist / playlist
    let selected_type = Rc::new(RefCell::new("all".to_string()));
    let chips_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let filter_defs = [
        ("all", "Все"),
        ("track", "Треки"),
        ("album", "Альбомы"),
        ("artist", "Артисты"),
        ("playlist", "Плейлисты"),
    ];
    let mut chip_btns: Vec<(String, gtk4::Button)> = Vec::new();
    for (k, label) in filter_defs {
        let b = gtk4::Button::builder()
            .label(label)
            .css_classes(if k == "all" {
                vec!["kw-chip", "kw-chip-active"]
            } else {
                vec!["kw-chip"]
            })
            .build();
        chips_row.append(&b);
        chip_btns.push((k.to_string(), b));
    }
    content.append(&chips_row);

    let wave_status_lbl = gtk4::Label::builder()
        .xalign(0.0)
        .visible(false)
        .css_classes(["kw-muted"])
        .build();
    content.append(&wave_status_lbl);

    let body_box = gtk4::Box::new(gtk4::Orientation::Vertical, 20);
    content.append(&body_box);

    // Bootstrap / Browse view when query is empty
    let populate_bootstrap: Rc<dyn Fn()> = {
        let body_box = body_box.clone();
        let c = core.clone();
        let rt_h = rt.clone();
        let nav_c = nav.clone();
        let search_entry = search_entry.clone();
        Rc::new(move || {
            let c2 = c.clone();
            let c_after = c.clone();
            let rt2 = rt_h.clone();
            let body2 = body_box.clone();
            let nav2 = nav_c.clone();
            let se2 = search_entry.clone();
            spawn_async(
                &rt_h,
                async move {
                    let hist = c2
                        .api
                        .search_history()
                        .await
                        .map(|h| h.items)
                        .unwrap_or_default();
                    let genres = c2.api.genres().await.map(|g| g.items).unwrap_or_default();
                    let artists = c2
                        .api
                        .top_artists(Some(12))
                        .await
                        .map(|a| a.items)
                        .unwrap_or_default();
                    (hist, genres, artists)
                },
                move |(hist, genres, artists): (Vec<String>, Vec<Genre>, Vec<Artist>)| {
                    while let Some(ch) = body2.first_child() {
                        body2.remove(&ch);
                    }
                    if !hist.is_empty() {
                        let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
                        let hdr_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                        let lbl = gtk4::Label::builder()
                            .label("История поиска")
                            .xalign(0.0)
                            .hexpand(true)
                            .css_classes(["kw-section-title"])
                            .build();
                        let clear_btn = gtk4::Button::builder()
                            .label("Очистить")
                            .css_classes(["flat"])
                            .build();
                        hdr_row.append(&lbl);
                        hdr_row.append(&clear_btn);
                        sec.append(&hdr_row);

                        let flow = gtk4::FlowBox::builder()
                            .selection_mode(gtk4::SelectionMode::None)
                            .max_children_per_line(8)
                            .column_spacing(8)
                            .row_spacing(8)
                            .build();
                        for q in hist {
                            let chip = gtk4::Button::builder()
                                .label(&q)
                                .css_classes(["kw-chip"])
                                .build();
                            let se3 = se2.clone();
                            chip.connect_clicked(move |_| {
                                se3.set_text(&q);
                                se3.emit_activate();
                            });
                            flow.insert(&chip, -1);
                        }
                        sec.append(&flow);
                        body2.append(&sec);

                        let c3 = c_after.clone();
                        let rt3 = rt2.clone();
                        let sec_weak = sec.downgrade();
                        clear_btn.connect_clicked(move |_| {
                            let c4 = c3.clone();
                            let sw = sec_weak.clone();
                            spawn_async(
                                &rt3,
                                async move { c4.api.clear_search_history().await },
                                move |_| {
                                    if let Some(s) = sw.upgrade() {
                                        s.set_visible(false);
                                    }
                                },
                            );
                        });
                    }

                    if !artists.is_empty() {
                        let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
                        sec.append(
                            &gtk4::Label::builder()
                                .label("Популярные артисты")
                                .xalign(0.0)
                                .css_classes(["kw-section-title"])
                                .build(),
                        );
                        let flow = gtk4::FlowBox::builder()
                            .selection_mode(gtk4::SelectionMode::None)
                            .max_children_per_line(6)
                            .column_spacing(12)
                            .row_spacing(12)
                            .build();
                        for ar in artists {
                            let aid = ar.id.clone();
                            let open_col = nav2.open_collection.clone();
                            let card = make_entity_card(
                                &c_after,
                                &rt2,
                                ar.avatar_image_id(),
                                &ar.name,
                                "Артист",
                                None,
                                move || open_col(CollectionTarget::Artist(aid.clone())),
                            );
                            flow.insert(&card, -1);
                        }
                        sec.append(&flow);
                        body2.append(&sec);
                    }

                    if !genres.is_empty() {
                        let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
                        sec.append(
                            &gtk4::Label::builder()
                                .label("Жанры и настроения")
                                .xalign(0.0)
                                .css_classes(["kw-section-title"])
                                .build(),
                        );
                        let flow = gtk4::FlowBox::builder()
                            .selection_mode(gtk4::SelectionMode::None)
                            .max_children_per_line(6)
                            .column_spacing(10)
                            .row_spacing(10)
                            .build();
                        for g in genres {
                            let gname = g.name.clone();
                            let btn = gtk4::Button::builder()
                                .label(&g.name)
                                .css_classes(["kw-card"])
                                .build();
                            let se3 = se2.clone();
                            btn.connect_clicked(move |_| {
                                se3.set_text(&gname);
                                se3.emit_activate();
                            });
                            flow.insert(&btn, -1);
                        }
                        sec.append(&flow);
                        body2.append(&sec);
                    }
                },
            );
        })
    };

    let render_results: Rc<dyn Fn(SearchResponse)> = {
        let body_box = body_box.clone();
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let nav_c = nav.clone();
        Rc::new(move |resp: SearchResponse| {
            while let Some(ch) = body_box.first_child() {
                body_box.remove(&ch);
            }

            if resp.tracks.is_empty()
                && resp.albums.is_empty()
                && resp.artists.is_empty()
                && resp.playlists.is_empty()
            {
                let empty = adw::StatusPage::builder()
                    .icon_name("system-search-symbolic")
                    .title("Ничего не найдено")
                    .description("Попробуйте изменить запрос или фильтр")
                    .build();
                body_box.append(&empty);
                return;
            }

            if !resp.tracks.is_empty() {
                let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
                sec.append(
                    &gtk4::Label::builder()
                        .label(format!("Треки ({})", resp.tracks.len()))
                        .xalign(0.0)
                        .css_classes(["kw-section-title"])
                        .build(),
                );
                let ui_tracks: Vec<UiTrack> = resp
                    .tracks
                    .iter()
                    .map(|t| t.to_ui(c.downloads.is_downloaded(&t.id)))
                    .collect();
                for (idx, ut) in ui_tracks.iter().enumerate() {
                    let row = make_track_row(
                        &c,
                        &eng,
                        &rt_h,
                        ut,
                        ui_tracks.clone(),
                        idx,
                        "search",
                        None,
                        None,
                        None,
                        &nav_c,
                    );
                    sec.append(&row);
                }
                body_box.append(&sec);
            }

            if !resp.albums.is_empty() {
                let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
                sec.append(
                    &gtk4::Label::builder()
                        .label("Альбомы")
                        .xalign(0.0)
                        .css_classes(["kw-section-title"])
                        .build(),
                );
                let flow = gtk4::FlowBox::builder()
                    .selection_mode(gtk4::SelectionMode::None)
                    .max_children_per_line(6)
                    .column_spacing(12)
                    .row_spacing(12)
                    .build();
                for al in resp.albums {
                    let id = al.id.clone();
                    let open_col = nav_c.open_collection.clone();
                    let card = make_entity_card(
                        &c,
                        &rt_h,
                        al.cover_image_id(),
                        &al.title,
                        &al.artist_line(),
                        al.type_label(),
                        move || open_col(CollectionTarget::Album(id.clone())),
                    );
                    flow.insert(&card, -1);
                }
                sec.append(&flow);
                body_box.append(&sec);
            }

            if !resp.artists.is_empty() {
                let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
                sec.append(
                    &gtk4::Label::builder()
                        .label("Артисты")
                        .xalign(0.0)
                        .css_classes(["kw-section-title"])
                        .build(),
                );
                let flow = gtk4::FlowBox::builder()
                    .selection_mode(gtk4::SelectionMode::None)
                    .max_children_per_line(6)
                    .column_spacing(12)
                    .row_spacing(12)
                    .build();
                for ar in resp.artists {
                    let id = ar.id.clone();
                    let open_col = nav_c.open_collection.clone();
                    let card = make_entity_card(
                        &c,
                        &rt_h,
                        ar.avatar_image_id(),
                        &ar.name,
                        "Артист",
                        None,
                        move || open_col(CollectionTarget::Artist(id.clone())),
                    );
                    flow.insert(&card, -1);
                }
                sec.append(&flow);
                body_box.append(&sec);
            }

            if !resp.playlists.is_empty() {
                let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
                sec.append(
                    &gtk4::Label::builder()
                        .label("Плейлисты")
                        .xalign(0.0)
                        .css_classes(["kw-section-title"])
                        .build(),
                );
                let flow = gtk4::FlowBox::builder()
                    .selection_mode(gtk4::SelectionMode::None)
                    .max_children_per_line(6)
                    .column_spacing(12)
                    .row_spacing(12)
                    .build();
                for pl in resp.playlists {
                    let id = pl.id.clone();
                    let open_col = nav_c.open_collection.clone();
                    let card = make_entity_card(
                        &c,
                        &rt_h,
                        pl.cover_image_id(),
                        &pl.title,
                        &KwhalePlural::tracks(pl.track_count),
                        None,
                        move || open_col(CollectionTarget::Playlist(id.clone())),
                    );
                    flow.insert(&card, -1);
                }
                sec.append(&flow);
                body_box.append(&sec);
            }
        })
    };

    let run_search: Rc<dyn Fn(String)> = {
        let c = core.clone();
        let rt_h = rt.clone();
        let sel_type = selected_type.clone();
        let wave_lbl = wave_status_lbl.clone();
        let render = render_results.clone();
        let boot = populate_bootstrap.clone();
        let se = search_entry.clone();

        Rc::new(move |query: String| {
            let q = query.trim().to_string();
            if se.text().as_str() != q {
                se.set_text(&q);
            }
            if q.is_empty() {
                wave_lbl.set_visible(false);
                boot();
                return;
            }
            let stype = sel_type.borrow().clone();
            wave_lbl.set_label("Поиск в локальном каталоге и внешних источниках…");
            wave_lbl.set_visible(true);

            // Wave 1: fast local catalog (fast=true)
            let c1 = c.clone();
            let c2 = c.clone();
            let rt2 = rt_h.clone();
            let q1 = q.clone();
            let q2 = q.clone();
            let st1 = stype.clone();
            let st2 = stype;
            let render1 = render.clone();
            let render2 = render.clone();
            let wlbl1 = wave_lbl.clone();
            let wlbl2 = wave_lbl.clone();

            spawn_async(
                &rt_h,
                async move { c1.api.search(&q1, &st1, Some(30), false).await },
                move |res1| {
                    if let Ok(r1) = res1 {
                        let is_partial = r1.partial;
                        render1(r1);
                        if is_partial {
                            wlbl1.set_label("Догружаем результаты из внешних коннекторов…");
                        } else {
                            wlbl1.set_visible(false);
                        }
                    }
                    // Wave 2: full aggregated search (wait=true)
                    let c3 = c2.clone();
                    let q3 = q2.clone();
                    let st3 = st2.clone();
                    let render3 = render2.clone();
                    let wlbl3 = wlbl2.clone();
                    spawn_async(
                        &rt2,
                        async move { c3.api.search(&q3, &st3, Some(30), true).await },
                        move |res2| {
                            wlbl3.set_visible(false);
                            if let Ok(r2) = res2 {
                                render3(r2);
                            }
                        },
                    );
                },
            );
        })
    };

    // Wire filter chips
    for (k, btn) in &chip_btns {
        let k_owned = k.clone();
        let all_btns = chip_btns.clone();
        let sel = selected_type.clone();
        let se = search_entry.clone();
        let run = run_search.clone();
        btn.connect_clicked(move |_| {
            *sel.borrow_mut() = k_owned.clone();
            for (other_k, other_b) in &all_btns {
                if other_k == &k_owned {
                    other_b.add_css_class("kw-chip-active");
                } else {
                    other_b.remove_css_class("kw-chip-active");
                }
            }
            let cur_q = se.text().to_string();
            if !cur_q.trim().is_empty() {
                run(cur_q);
            }
        });
    }

    {
        let run = run_search.clone();
        search_entry.connect_activate(move |e| {
            run(e.text().to_string());
        });
    }
    {
        let run = run_search.clone();
        let se = search_entry.clone();
        search_btn.connect_clicked(move |_| {
            run(se.text().to_string());
        });
    }

    populate_bootstrap();

    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build();
    (scroller, search_entry, run_search)
}

// ======================= LIBRARY PAGE =======================

pub fn build_library_page(
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    rt: tokio::runtime::Handle,
    nav: NavCallbacks,
    sidebar_playlists_cb: Rc<dyn Fn(Vec<Playlist>)>,
) -> (gtk4::ScrolledWindow, Rc<dyn Fn()>) {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 18);
    content.set_margin_top(20);
    content.set_margin_bottom(28);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let top_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let title = gtk4::Label::builder()
        .label("Библиотека")
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["kw-hero-title"])
        .build();
    top_row.append(&title);

    let new_pl_btn = gtk4::Button::builder()
        .label("+ Плейлист")
        .css_classes(["pill"])
        .build();
    {
        let win = nav.window.clone();
        let c = core.clone();
        let rt_h = rt.clone();
        let cbs = nav.dialogs.clone();
        new_pl_btn.connect_clicked(move |_| {
            show_create_or_edit_playlist_dialog(&win, c.clone(), rt_h.clone(), None, None, cbs.clone());
        });
    }
    top_row.append(&new_pl_btn);

    let upload_btn = gtk4::Button::builder()
        .label("Загрузить трек")
        .css_classes(["pill"])
        .build();
    {
        let win = nav.window.clone();
        let c = core.clone();
        let rt_h = rt.clone();
        let cbs = nav.dialogs.clone();
        upload_btn.connect_clicked(move |_| {
            show_upload_track_dialog(&win, c.clone(), rt_h.clone(), None, cbs.clone());
        });
    }
    top_row.append(&upload_btn);
    content.append(&top_row);

    // Pinned quick cards: Любимые треки & Загруженные
    let pinned_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
    pinned_row.set_homogeneous(true);

    let liked_card = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    liked_card.add_css_class("kw-hero-card");
    let liked_title = gtk4::Label::builder()
        .label("♥ Любимые треки")
        .xalign(0.0)
        .css_classes(["kw-display-title"])
        .build();
    let liked_count_lbl = gtk4::Label::builder()
        .label("Ваша коллекция избранного")
        .xalign(0.0)
        .build();
    let liked_open_btn = gtk4::Button::builder()
        .label("Открыть Любимые треки")
        .halign(gtk4::Align::Start)
        .css_classes(["suggested-action", "pill"])
        .build();
    {
        let open_col = nav.open_collection.clone();
        liked_open_btn.connect_clicked(move |_| {
            open_col(CollectionTarget::LikedTracks);
        });
    }
    liked_card.append(&liked_title);
    liked_card.append(&liked_count_lbl);
    liked_card.append(&liked_open_btn);
    pinned_row.append(&liked_card);

    let dl_card = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    dl_card.add_css_class("kw-card");
    let dl_title = gtk4::Label::builder()
        .label("⬇ Загруженные офлайн")
        .xalign(0.0)
        .css_classes(["kw-section-title"])
        .build();
    let dl_sub = gtk4::Label::builder()
        .label("Доступны без подключения к интернету")
        .xalign(0.0)
        .css_classes(["kw-muted"])
        .build();
    let dl_open_btn = gtk4::Button::builder()
        .label("Открыть загрузки")
        .halign(gtk4::Align::Start)
        .css_classes(["pill"])
        .build();
    {
        let sw = nav.switch_section.clone();
        dl_open_btn.connect_clicked(move |_| sw("downloads"));
    }
    dl_card.append(&dl_title);
    dl_card.append(&dl_sub);
    dl_card.append(&dl_open_btn);
    pinned_row.append(&dl_card);
    content.append(&pinned_row);

    // Filter tabs + in-library filter entry
    let filter_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let active_tab = Rc::new(RefCell::new("all".to_string()));
    let tabs = [
        ("all", "Все"),
        ("playlists", "Плейлисты"),
        ("albums", "Альбомы"),
        ("tracks", "Треки"),
    ];
    let mut tab_btns: Vec<(String, gtk4::Button)> = Vec::new();
    for (k, lbl) in tabs {
        let b = gtk4::Button::builder()
            .label(lbl)
            .css_classes(if k == "all" {
                vec!["kw-chip", "kw-chip-active"]
            } else {
                vec!["kw-chip"]
            })
            .build();
        filter_row.append(&b);
        tab_btns.push((k.to_string(), b));
    }
    let filter_entry = gtk4::SearchEntry::builder()
        .placeholder_text("Фильтр по библиотеке…")
        .hexpand(true)
        .build();
    filter_row.append(&filter_entry);
    content.append(&filter_row);

    let body_box = gtk4::Box::new(gtk4::Orientation::Vertical, 20);
    content.append(&body_box);

    let snapshot_state: Rc<RefCell<(Vec<Track>, Vec<Playlist>, Vec<Album>)>> =
        Rc::new(RefCell::new((Vec::new(), Vec::new(), Vec::new())));

    let render_lib: Rc<dyn Fn()> = {
        let body_box = body_box.clone();
        let state = snapshot_state.clone();
        let tab = active_tab.clone();
        let fe = filter_entry.clone();
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let nav_c = nav.clone();
        let liked_count_lbl = liked_count_lbl.clone();

        Rc::new(move || {
            while let Some(ch) = body_box.first_child() {
                body_box.remove(&ch);
            }
            let (tracks, playlists, albums) = state.borrow().clone();
            liked_count_lbl.set_label(&KwhalePlural::tracks(tracks.len() as i32));
            let cur_tab = tab.borrow().clone();
            let q = fe.text().trim().to_lowercase();

            if (cur_tab == "all" || cur_tab == "playlists") && !playlists.is_empty() {
                let filtered: Vec<Playlist> = playlists
                    .into_iter()
                    .filter(|p| q.is_empty() || p.title.to_lowercase().contains(&q))
                    .collect();
                if !filtered.is_empty() {
                    let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
                    sec.append(
                        &gtk4::Label::builder()
                            .label(format!("Плейлисты ({})", filtered.len()))
                            .xalign(0.0)
                            .css_classes(["kw-section-title"])
                            .build(),
                    );
                    let flow = gtk4::FlowBox::builder()
                        .selection_mode(gtk4::SelectionMode::None)
                        .max_children_per_line(6)
                        .column_spacing(12)
                        .row_spacing(12)
                        .build();
                    for pl in filtered {
                        let pid = pl.id.clone();
                        let open_col = nav_c.open_collection.clone();
                        let card = make_entity_card(
                            &c,
                            &rt_h,
                            pl.cover_image_id(),
                            &pl.title,
                            &KwhalePlural::tracks(pl.track_count),
                            None,
                            move || open_col(CollectionTarget::Playlist(pid.clone())),
                        );
                        flow.insert(&card, -1);
                    }
                    sec.append(&flow);
                    body_box.append(&sec);
                }
            }

            if (cur_tab == "all" || cur_tab == "albums") && !albums.is_empty() {
                let filtered: Vec<Album> = albums
                    .into_iter()
                    .filter(|a| {
                        q.is_empty()
                            || a.title.to_lowercase().contains(&q)
                            || a.artist_line().to_lowercase().contains(&q)
                    })
                    .collect();
                if !filtered.is_empty() {
                    let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
                    sec.append(
                        &gtk4::Label::builder()
                            .label(format!("Альбомы ({})", filtered.len()))
                            .xalign(0.0)
                            .css_classes(["kw-section-title"])
                            .build(),
                    );
                    let flow = gtk4::FlowBox::builder()
                        .selection_mode(gtk4::SelectionMode::None)
                        .max_children_per_line(6)
                        .column_spacing(12)
                        .row_spacing(12)
                        .build();
                    for al in filtered {
                        let aid = al.id.clone();
                        let open_col = nav_c.open_collection.clone();
                        let card = make_entity_card(
                            &c,
                            &rt_h,
                            al.cover_image_id(),
                            &al.title,
                            &al.artist_line(),
                            al.type_label(),
                            move || open_col(CollectionTarget::Album(aid.clone())),
                        );
                        flow.insert(&card, -1);
                    }
                    sec.append(&flow);
                    body_box.append(&sec);
                }
            }

            if (cur_tab == "all" || cur_tab == "tracks") && !tracks.is_empty() {
                let filtered: Vec<UiTrack> = tracks
                    .iter()
                    .filter(|t| {
                        q.is_empty()
                            || t.title.to_lowercase().contains(&q)
                            || t.artist_line().to_lowercase().contains(&q)
                    })
                    .map(|t| t.to_ui(c.downloads.is_downloaded(&t.id)))
                    .collect();
                if !filtered.is_empty() {
                    let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
                    sec.append(
                        &gtk4::Label::builder()
                            .label(format!("Любимые треки ({})", filtered.len()))
                            .xalign(0.0)
                            .css_classes(["kw-section-title"])
                            .build(),
                    );
                    for (idx, ut) in filtered.iter().enumerate() {
                        let row = make_track_row(
                            &c,
                            &eng,
                            &rt_h,
                            ut,
                            filtered.clone(),
                            idx,
                            "library",
                            None,
                            None,
                            None,
                            &nav_c,
                        );
                        sec.append(&row);
                    }
                    body_box.append(&sec);
                }
            }
        })
    };

    for (k, btn) in &tab_btns {
        let k_owned = k.clone();
        let all_btns = tab_btns.clone();
        let tab = active_tab.clone();
        let render = render_lib.clone();
        btn.connect_clicked(move |_| {
            *tab.borrow_mut() = k_owned.clone();
            for (ok, ob) in &all_btns {
                if ok == &k_owned {
                    ob.add_css_class("kw-chip-active");
                } else {
                    ob.remove_css_class("kw-chip-active");
                }
            }
            render();
        });
    }
    {
        let render = render_lib.clone();
        filter_entry.connect_search_changed(move |_| render());
    }

    let reload: Rc<dyn Fn()> = {
        let c = core.clone();
        let rt_h = rt.clone();
        let state = snapshot_state.clone();
        let render = render_lib.clone();
        let sidebar_cb = sidebar_playlists_cb;
        Rc::new(move || {
            let c2 = c.clone();
            let st2 = state.clone();
            let render2 = render.clone();
            let sb2 = sidebar_cb.clone();
            spawn_async(
                &rt_h,
                async move {
                    let tracks = c2
                        .api
                        .library_tracks(None, Some(100), "recent", None)
                        .await
                        .map(|p| p.items)
                        .unwrap_or_default();
                    let playlists = c2
                        .api
                        .library_playlists(None, Some(100))
                        .await
                        .map(|p| p.items)
                        .unwrap_or_default();
                    let albums = c2
                        .api
                        .library_albums(None, Some(100))
                        .await
                        .map(|p| p.items)
                        .unwrap_or_default();
                    (tracks, playlists, albums)
                },
                move |(tracks, playlists, albums)| {
                    sb2(playlists.clone());
                    *st2.borrow_mut() = (tracks, playlists, albums);
                    render2();
                },
            );
        })
    };

    reload();

    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build();
    (scroller, reload)
}

// ======================= COLLECTION / DETAIL PAGE =======================

pub fn build_collection_page(
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    rt: tokio::runtime::Handle,
    target: CollectionTarget,
    nav: NavCallbacks,
) -> gtk4::ScrolledWindow {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 18);
    content.set_margin_top(20);
    content.set_margin_bottom(28);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build();

    let status_lbl = gtk4::Label::builder()
        .label("Загрузка…")
        .xalign(0.0)
        .css_classes(["kw-muted"])
        .build();
    content.append(&status_lbl);

    match target {
        CollectionTarget::LikedTracks => {
            let c = core.clone();
            let c2 = core.clone();
            let eng = engine.clone();
            let rt_h = rt.clone();
            let nav_c = nav.clone();
            let cont = content.clone();
            spawn_async(
                &rt,
                async move { c.api.library_tracks(None, Some(200), "recent", None).await },
                move |res| {
                    while let Some(ch) = cont.first_child() {
                        cont.remove(&ch);
                    }
                    match res {
                        Ok(page) => {
                            let ui_tracks: Vec<UiTrack> = page
                                .items
                                .iter()
                                .map(|t| t.to_ui(c2.downloads.is_downloaded(&t.id)))
                                .collect();
                            render_collection_header(
                                &cont,
                                &c2,
                                &eng,
                                &rt_h,
                                None,
                                "Любимые треки",
                                "Ваша библиотека избранного",
                                &KwhalePlural::tracks(ui_tracks.len() as i32),
                                ui_tracks.clone(),
                                "library",
                                None,
                                None,
                            );
                            for (idx, ut) in ui_tracks.iter().enumerate() {
                                let row = make_track_row(
                                    &c2,
                                    &eng,
                                    &rt_h,
                                    ut,
                                    ui_tracks.clone(),
                                    idx,
                                    "library",
                                    None,
                                    None,
                                    None,
                                    &nav_c,
                                );
                                cont.append(&row);
                            }
                        }
                        Err(e) => {
                            cont.append(&gtk4::Label::new(Some(&e.user_message())));
                        }
                    }
                },
            );
        }
        CollectionTarget::SeeAllTracks { title, tracks } => {
            while let Some(ch) = content.first_child() {
                content.remove(&ch);
            }
            let ui_tracks: Vec<UiTrack> = tracks
                .iter()
                .map(|t| t.to_ui(core.downloads.is_downloaded(&t.id)))
                .collect();
            render_collection_header(
                &content,
                &core,
                &engine,
                &rt,
                ui_tracks.first().and_then(|t| t.cover_image_id.as_deref()),
                &title,
                "Подборка треков",
                &KwhalePlural::tracks(ui_tracks.len() as i32),
                ui_tracks.clone(),
                "home",
                None,
                None,
            );
            for (idx, ut) in ui_tracks.iter().enumerate() {
                let row = make_track_row(
                    &core,
                    &engine,
                    &rt,
                    ut,
                    ui_tracks.clone(),
                    idx,
                    "home",
                    None,
                    None,
                    None,
                    &nav,
                );
                content.append(&row);
            }
        }
        CollectionTarget::SeeAllAlbums { title, albums } => {
            while let Some(ch) = content.first_child() {
                content.remove(&ch);
            }
            content.append(
                &gtk4::Label::builder()
                    .label(&title)
                    .xalign(0.0)
                    .css_classes(["kw-hero-title"])
                    .build(),
            );
            let flow = gtk4::FlowBox::builder()
                .selection_mode(gtk4::SelectionMode::None)
                .max_children_per_line(6)
                .column_spacing(12)
                .row_spacing(12)
                .build();
            for al in albums {
                let id = al.id.clone();
                let open_col = nav.open_collection.clone();
                let card = make_entity_card(
                    &core,
                    &rt,
                    al.cover_image_id(),
                    &al.title,
                    &al.artist_line(),
                    al.type_label(),
                    move || open_col(CollectionTarget::Album(id.clone())),
                );
                flow.insert(&card, -1);
            }
            content.append(&flow);
        }
        CollectionTarget::SeeAllArtists { title, artists } => {
            while let Some(ch) = content.first_child() {
                content.remove(&ch);
            }
            content.append(
                &gtk4::Label::builder()
                    .label(&title)
                    .xalign(0.0)
                    .css_classes(["kw-hero-title"])
                    .build(),
            );
            let flow = gtk4::FlowBox::builder()
                .selection_mode(gtk4::SelectionMode::None)
                .max_children_per_line(6)
                .column_spacing(12)
                .row_spacing(12)
                .build();
            for ar in artists {
                let id = ar.id.clone();
                let open_col = nav.open_collection.clone();
                let card = make_entity_card(
                    &core,
                    &rt,
                    ar.avatar_image_id(),
                    &ar.name,
                    "Артист",
                    None,
                    move || open_col(CollectionTarget::Artist(id.clone())),
                );
                flow.insert(&card, -1);
            }
            content.append(&flow);
        }
        CollectionTarget::Album(album_id) => {
            let c = core.clone();
            let c2 = core.clone();
            let eng = engine.clone();
            let rt_h = rt.clone();
            let nav_c = nav.clone();
            let cont = content.clone();
            spawn_async(
                &rt,
                async move {
                    let album = c.api.album(&album_id).await?;
                    let tracks = if album.tracks.is_empty() {
                        c.api
                            .album_tracks(&album_id, None, Some(100))
                            .await
                            .map(|p| p.items)
                            .unwrap_or_default()
                    } else {
                        album.tracks.clone()
                    };
                    Ok::<(Album, Vec<Track>), kwhale_core::CoreError>((album, tracks))
                },
                move |res| {
                    while let Some(ch) = cont.first_child() {
                        cont.remove(&ch);
                    }
                    match res {
                        Ok((album, tracks)) => {
                            let ui_tracks: Vec<UiTrack> = tracks
                                .iter()
                                .map(|t| t.to_ui(c2.downloads.is_downloaded(&t.id)))
                                .collect();
                            let meta = format!(
                                "{}{} • {}",
                                album.type_label().unwrap_or("Альбом"),
                                album.year.map(|y| format!(" • {y}")).unwrap_or_default(),
                                KwhalePlural::tracks(ui_tracks.len() as i32)
                            );
                            let extra_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);

                            // Favorite album toggle
                            let is_fav = Rc::new(RefCell::new(album.is_favorite));
                            let fav_btn = gtk4::Button::builder()
                                .label(if album.is_favorite {
                                    "♥ В библиотеке"
                                } else {
                                    "♡ В библиотеку"
                                })
                                .css_classes(["pill"])
                                .build();
                            {
                                let c3 = c2.clone();
                                let rt3 = rt_h.clone();
                                let aid = album.id.clone();
                                let fav_state = is_fav.clone();
                                let toast = nav_c.dialogs.show_toast.clone();
                                fav_btn.connect_clicked(move |btn| {
                                    let cur = *fav_state.borrow();
                                    let next = !cur;
                                    *fav_state.borrow_mut() = next;
                                    btn.set_label(if next {
                                        "♥ В библиотеке"
                                    } else {
                                        "♡ В библиотеку"
                                    });
                                    let c4 = c3.clone();
                                    let aid2 = aid.clone();
                                    let toast2 = toast.clone();
                                    spawn_async(
                                        &rt3,
                                        async move {
                                            if next {
                                                c4.api.like_album(&aid2).await
                                            } else {
                                                c4.api.unlike_album(&aid2).await
                                            }
                                        },
                                        move |r| {
                                            if let Err(e) = r {
                                                toast2(&e.user_message());
                                            }
                                        },
                                    );
                                });
                            }
                            extra_bar.append(&fav_btn);

                            // Like all album tracks button
                            let like_all_btn = gtk4::Button::builder()
                                .label("Лайкнуть все треки")
                                .css_classes(["pill"])
                                .build();
                            {
                                let c3 = c2.clone();
                                let rt3 = rt_h.clone();
                                let aid = album.id.clone();
                                let toast = nav_c.dialogs.show_toast.clone();
                                like_all_btn.connect_clicked(move |_| {
                                    let c4 = c3.clone();
                                    let aid2 = aid.clone();
                                    let toast2 = toast.clone();
                                    spawn_async(
                                        &rt3,
                                        async move { c4.api.like_album_tracks(&aid2).await },
                                        move |r| match r {
                                            Ok(res) => toast2(&format!(
                                                "Добавлено в Любимые: {} треков",
                                                res.liked + res.already_liked
                                            )),
                                            Err(e) => toast2(&e.user_message()),
                                        },
                                    );
                                });
                            }
                            extra_bar.append(&like_all_btn);

                            // Share album button
                            let share_btn = gtk4::Button::builder()
                                .icon_name("emblem-shared-symbolic")
                                .tooltip_text("Поделиться альбомом")
                                .css_classes(["circular"])
                                .build();
                            {
                                let win = nav_c.window.clone();
                                let c3 = c2.clone();
                                let rt3 = rt_h.clone();
                                let aid = album.id.clone();
                                let atitle = album.title.clone();
                                let toast = nav_c.dialogs.show_toast.clone();
                                share_btn.connect_clicked(move |_| {
                                    show_share_dialog(
                                        &win,
                                        c3.clone(),
                                        rt3.clone(),
                                        ShareKind::Album,
                                        &aid,
                                        &atitle,
                                        toast.clone(),
                                    );
                                });
                            }
                            extra_bar.append(&share_btn);

                            render_collection_header(
                                &cont,
                                &c2,
                                &eng,
                                &rt_h,
                                album.cover_image_id(),
                                &album.title,
                                &album.artist_line(),
                                &meta,
                                ui_tracks.clone(),
                                "album",
                                None,
                                Some(extra_bar),
                            );

                            for (idx, ut) in ui_tracks.iter().enumerate() {
                                let row = make_track_row(
                                    &c2,
                                    &eng,
                                    &rt_h,
                                    ut,
                                    ui_tracks.clone(),
                                    idx,
                                    "album",
                                    None,
                                    None,
                                    None,
                                    &nav_c,
                                );
                                cont.append(&row);
                            }
                        }
                        Err(e) => {
                            cont.append(&gtk4::Label::new(Some(&e.user_message())));
                        }
                    }
                },
            );
        }
        CollectionTarget::Playlist(playlist_id) => {
            let c = core.clone();
            let c2 = core.clone();
            let eng = engine.clone();
            let rt_h = rt.clone();
            let nav_c = nav.clone();
            let cont = content.clone();
            spawn_async(
                &rt,
                async move {
                    let pl = c.api.playlist(&playlist_id).await?;
                    let tracks = if pl.tracks.is_empty() {
                        c.api
                            .playlist_tracks(&playlist_id, None, Some(200))
                            .await
                            .map(|p| p.items)
                            .unwrap_or_default()
                    } else {
                        pl.tracks.clone()
                    };
                    Ok::<(Playlist, Vec<Track>), kwhale_core::CoreError>((pl, tracks))
                },
                move |res| {
                    while let Some(ch) = cont.first_child() {
                        cont.remove(&ch);
                    }
                    match res {
                        Ok((pl, tracks)) => {
                            let ui_tracks: Vec<UiTrack> = tracks
                                .iter()
                                .map(|t| t.to_ui(c2.downloads.is_downloaded(&t.id)))
                                .collect();
                            let owner_name = pl
                                .owner
                                .as_ref()
                                .map(|o| o.display_name.as_str())
                                .unwrap_or("KWhale");
                            let meta = format!(
                                "{} • {}",
                                if pl.visibility == "public" {
                                    "Публичный"
                                } else {
                                    "Приватный"
                                },
                                KwhalePlural::tracks(ui_tracks.len() as i32)
                            );

                            let extra_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                            let can_edit = pl.permissions.can_edit
                                || c2
                                    .api
                                    .session()
                                    .and_then(|s| pl.owner.as_ref().map(|o| o.id == s.user.id))
                                    .unwrap_or(false);

                            // Like all playlist tracks
                            let like_all_btn = gtk4::Button::builder()
                                .label("Лайкнуть все")
                                .css_classes(["pill"])
                                .build();
                            {
                                let c3 = c2.clone();
                                let rt3 = rt_h.clone();
                                let pid = pl.id.clone();
                                let toast = nav_c.dialogs.show_toast.clone();
                                like_all_btn.connect_clicked(move |_| {
                                    let c4 = c3.clone();
                                    let pid2 = pid.clone();
                                    let toast2 = toast.clone();
                                    spawn_async(
                                        &rt3,
                                        async move { c4.api.like_playlist_tracks(&pid2).await },
                                        move |r| match r {
                                            Ok(res) => toast2(&format!(
                                                "В Любимые добавлено: {} треков",
                                                res.liked + res.already_liked
                                            )),
                                            Err(e) => toast2(&e.user_message()),
                                        },
                                    );
                                });
                            }
                            extra_bar.append(&like_all_btn);

                            if can_edit {
                                // Edit metadata
                                let edit_btn = gtk4::Button::builder()
                                    .label("Изменить")
                                    .css_classes(["pill"])
                                    .build();
                                {
                                    let win = nav_c.window.clone();
                                    let c3 = c2.clone();
                                    let rt3 = rt_h.clone();
                                    let pl_clone = pl.clone();
                                    let cbs = nav_c.dialogs.clone();
                                    edit_btn.connect_clicked(move |_| {
                                        show_create_or_edit_playlist_dialog(
                                            &win,
                                            c3.clone(),
                                            rt3.clone(),
                                            Some(pl_clone.clone()),
                                            None,
                                            cbs.clone(),
                                        );
                                    });
                                }
                                extra_bar.append(&edit_btn);

                                // Upload cover via Portal FileDialog
                                let cover_btn = gtk4::Button::builder()
                                    .label("Обложка…")
                                    .css_classes(["pill"])
                                    .build();
                                {
                                    let win = nav_c.window.clone();
                                    let c3 = c2.clone();
                                    let rt3 = rt_h.clone();
                                    let pid = pl.id.clone();
                                    let toast = nav_c.dialogs.show_toast.clone();
                                    let open_col = nav_c.open_collection.clone();
                                    let reload_lib = nav_c.dialogs.reload_library.clone();
                                    cover_btn.connect_clicked(move |_| {
                                        let filter = gtk4::FileFilter::new();
                                        filter.set_name(Some("Изображения (PNG, JPEG, WebP)"));
                                        for pat in ["*.png", "*.jpg", "*.jpeg", "*.webp"] {
                                            filter.add_pattern(pat);
                                        }
                                        let filters = gio::ListStore::new::<gtk4::FileFilter>();
                                        filters.append(&filter);
                                        let fd = gtk4::FileDialog::builder()
                                            .title("Выберите обложку плейлиста")
                                            .filters(&filters)
                                            .build();
                                        let c4 = c3.clone();
                                        let rt4 = rt3.clone();
                                        let pid2 = pid.clone();
                                        let toast2 = toast.clone();
                                        let open_col2 = open_col.clone();
                                        let reload2 = reload_lib.clone();
                                        fd.open(Some(&win), gio::Cancellable::NONE, move |res| {
                                            if let Ok(file) = res {
                                                if let Some(path) = file.path() {
                                                    let c5 = c4.clone();
                                                    let c6 = c4.clone();
                                                    let pid3 = pid2.clone();
                                                    let pid4 = pid2.clone();
                                                    let toast3 = toast2.clone();
                                                    let open_col3 = open_col2.clone();
                                                    let reload3 = reload2.clone();
                                                    spawn_async(
                                                        &rt4,
                                                        async move {
                                                            c5.api
                                                                .upload_playlist_cover(&pid3, &path)
                                                                .await
                                                        },
                                                        move |up_res| match up_res {
                                                            Ok(_) => {
                                                                c6.cache.invalidate_collection(
                                                                    "playlist", &pid4,
                                                                );
                                                                c6.cache.invalidate_library();
                                                                reload3();
                                                                open_col3(
                                                                    CollectionTarget::Playlist(
                                                                        pid4,
                                                                    ),
                                                                );
                                                                toast3("Обложка плейлиста обновлена");
                                                            }
                                                            Err(e) => toast3(&e.user_message()),
                                                        },
                                                    );
                                                }
                                            }
                                        });
                                    });
                                }
                                extra_bar.append(&cover_btn);

                                // Delete playlist
                                let del_btn = gtk4::Button::builder()
                                    .icon_name("user-trash-symbolic")
                                    .tooltip_text("Удалить плейлист")
                                    .css_classes(["destructive-action", "circular"])
                                    .build();
                                {
                                    let c3 = c2.clone();
                                    let rt3 = rt_h.clone();
                                    let pid = pl.id.clone();
                                    let toast = nav_c.dialogs.show_toast.clone();
                                    let reload_lib = nav_c.dialogs.reload_library.clone();
                                    let sw = nav_c.switch_section.clone();
                                    del_btn.connect_clicked(move |_| {
                                        let c4 = c3.clone();
                                        let c5 = c3.clone();
                                        let pid2 = pid.clone();
                                        let toast2 = toast.clone();
                                        let reload2 = reload_lib.clone();
                                        let sw2 = sw.clone();
                                        spawn_async(
                                            &rt3,
                                            async move { c4.api.delete_playlist(&pid2).await },
                                            move |r| match r {
                                                Ok(()) => {
                                                    c5.cache.invalidate_library();
                                                    reload2();
                                                    sw2("library");
                                                    toast2("Плейлист удалён");
                                                }
                                                Err(e) => toast2(&e.user_message()),
                                            },
                                        );
                                    });
                                }
                                extra_bar.append(&del_btn);
                            }

                            render_collection_header(
                                &cont,
                                &c2,
                                &eng,
                                &rt_h,
                                pl.cover_image_id(),
                                &pl.title,
                                pl.description.as_deref().unwrap_or(owner_name),
                                &meta,
                                ui_tracks.clone(),
                                "playlist",
                                None,
                                Some(extra_bar),
                            );

                            let reorder_cb: Option<Rc<dyn Fn(usize, i32)>> = if can_edit {
                                let c3 = c2.clone();
                                let rt3 = rt_h.clone();
                                let pid = pl.id.clone();
                                let ids: Vec<String> =
                                    ui_tracks.iter().map(|t| t.remote_id.clone()).collect();
                                let open_col = nav_c.open_collection.clone();
                                let toast = nav_c.dialogs.show_toast.clone();
                                Some(Rc::new(move |idx: usize, delta: i32| {
                                    let target_idx = idx as i32 + delta;
                                    if target_idx < 0 || (target_idx as usize) >= ids.len() {
                                        return;
                                    }
                                    let mut new_ids = ids.clone();
                                    new_ids.swap(idx, target_idx as usize);
                                    let c4 = c3.clone();
                                    let pid2 = pid.clone();
                                    let pid3 = pid.clone();
                                    let open_col2 = open_col.clone();
                                    let toast2 = toast.clone();
                                    spawn_async(
                                        &rt3,
                                        async move {
                                            c4.api.reorder_playlist_tracks(&pid2, new_ids).await
                                        },
                                        move |r| match r {
                                            Ok(_) => {
                                                open_col2(CollectionTarget::Playlist(pid3));
                                            }
                                            Err(e) => toast2(&e.user_message()),
                                        },
                                    );
                                }))
                            } else {
                                None
                            };

                            for (idx, ut) in ui_tracks.iter().enumerate() {
                                let row = make_track_row(
                                    &c2,
                                    &eng,
                                    &rt_h,
                                    ut,
                                    ui_tracks.clone(),
                                    idx,
                                    "playlist",
                                    None,
                                    if can_edit {
                                        Some(pl.id.clone())
                                    } else {
                                        None
                                    },
                                    reorder_cb.clone(),
                                    &nav_c,
                                );
                                cont.append(&row);
                            }
                        }
                        Err(e) => {
                            cont.append(&gtk4::Label::new(Some(&e.user_message())));
                        }
                    }
                },
            );
        }
        CollectionTarget::Mix(mix_id) => {
            let c = core.clone();
            let c2 = core.clone();
            let eng = engine.clone();
            let rt_h = rt.clone();
            let nav_c = nav.clone();
            let cont = content.clone();
            spawn_async(
                &rt,
                async move { c.api.mix_tracks(&mix_id, None, Some(50)).await.map(|p| (mix_id, p)) },
                move |res| {
                    while let Some(ch) = cont.first_child() {
                        cont.remove(&ch);
                    }
                    match res {
                        Ok((mid, page)) => {
                            let ui_tracks: Vec<UiTrack> = page
                                .items
                                .iter()
                                .map(|r| r.track.to_ui(c2.downloads.is_downloaded(&r.track.id)))
                                .collect();
                            render_collection_header(
                                &cont,
                                &c2,
                                &eng,
                                &rt_h,
                                ui_tracks.first().and_then(|t| t.cover_image_id.as_deref()),
                                "Персональный микс",
                                "Поток рекомендаций KWhale",
                                &KwhalePlural::tracks(ui_tracks.len() as i32),
                                ui_tracks.clone(),
                                "mix",
                                Some(mid.clone()),
                                None,
                            );
                            for (idx, ut) in ui_tracks.iter().enumerate() {
                                let row = make_track_row(
                                    &c2,
                                    &eng,
                                    &rt_h,
                                    ut,
                                    ui_tracks.clone(),
                                    idx,
                                    "mix",
                                    Some(mid.clone()),
                                    None,
                                    None,
                                    &nav_c,
                                );
                                cont.append(&row);
                            }
                        }
                        Err(e) => {
                            cont.append(&gtk4::Label::new(Some(&e.user_message())));
                        }
                    }
                },
            );
        }
        CollectionTarget::Artist(artist_id) => {
            let c = core.clone();
            let c2 = core.clone();
            let eng = engine.clone();
            let rt_h = rt.clone();
            let nav_c = nav.clone();
            let cont = content.clone();
            spawn_async(
                &rt,
                async move {
                    let artist = c.api.artist(&artist_id).await?;
                    let top_tracks = if artist.top_tracks.is_empty() {
                        c.api
                            .artist_top_tracks(&artist_id, Some(20))
                            .await
                            .map(|p| p.items)
                            .unwrap_or_default()
                    } else {
                        artist.top_tracks.clone()
                    };
                    let albums = if artist.albums.is_empty() {
                        c.api
                            .artist_albums(&artist_id, None, Some(60))
                            .await
                            .map(|p| p.items)
                            .unwrap_or_default()
                    } else {
                        artist.albums.clone()
                    };
                    Ok::<(Artist, Vec<Track>, Vec<Album>), kwhale_core::CoreError>((
                        artist, top_tracks, albums,
                    ))
                },
                move |res| {
                    while let Some(ch) = cont.first_child() {
                        cont.remove(&ch);
                    }
                    match res {
                        Ok((artist, top_tracks, albums)) => {
                            let ui_tracks: Vec<UiTrack> = top_tracks
                                .iter()
                                .map(|t| t.to_ui(c2.downloads.is_downloaded(&t.id)))
                                .collect();
                            let meta = format!(
                                "{} • {}",
                                KwhalePlural::albums(albums.len() as i32),
                                KwhalePlural::tracks(ui_tracks.len() as i32)
                            );

                            let extra_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);

                            // Favorite artist button
                            let is_fav = Rc::new(RefCell::new(artist.is_favorite));
                            let fav_btn = gtk4::Button::builder()
                                .label(if artist.is_favorite {
                                    "♥ Подписан"
                                } else {
                                    "♡ Подписаться"
                                })
                                .css_classes(["pill"])
                                .build();
                            {
                                let c3 = c2.clone();
                                let rt3 = rt_h.clone();
                                let aid = artist.id.clone();
                                let st = is_fav.clone();
                                let toast = nav_c.dialogs.show_toast.clone();
                                fav_btn.connect_clicked(move |btn| {
                                    let next = !*st.borrow();
                                    *st.borrow_mut() = next;
                                    btn.set_label(if next {
                                        "♥ Подписан"
                                    } else {
                                        "♡ Подписаться"
                                    });
                                    let c4 = c3.clone();
                                    let aid2 = aid.clone();
                                    let toast2 = toast.clone();
                                    spawn_async(
                                        &rt3,
                                        async move {
                                            if next {
                                                c4.api.like_artist(&aid2).await
                                            } else {
                                                c4.api.unlike_artist(&aid2).await
                                            }
                                        },
                                        move |r| {
                                            if let Err(e) = r {
                                                toast2(&e.user_message());
                                            }
                                        },
                                    );
                                });
                            }
                            extra_bar.append(&fav_btn);

                            // Sync discography button
                            let sync_btn = gtk4::Button::builder()
                                .label("Синхронизировать дискографию")
                                .icon_name("view-refresh-symbolic")
                                .css_classes(["pill"])
                                .build();
                            {
                                let c3 = c2.clone();
                                let rt3 = rt_h.clone();
                                let aid = artist.id.clone();
                                let toast = nav_c.dialogs.show_toast.clone();
                                let open_col = nav_c.open_collection.clone();
                                sync_btn.connect_clicked(move |btn| {
                                    btn.set_sensitive(false);
                                    btn.set_label("Синхронизация…");
                                    let c4 = c3.clone();
                                    let aid2 = aid.clone();
                                    let aid3 = aid.clone();
                                    let toast2 = toast.clone();
                                    let open_col2 = open_col.clone();
                                    spawn_async(
                                        &rt3,
                                        async move { c4.api.sync_artist(&aid2).await },
                                        move |r| match r {
                                            Ok(st) => {
                                                toast2(&format!(
                                                    "Синхронизация дискографии: {} (альбомов: {}, треков: {})",
                                                    st.status, st.album_count, st.track_count
                                                ));
                                                open_col2(CollectionTarget::Artist(aid3));
                                            }
                                            Err(e) => toast2(&e.user_message()),
                                        },
                                    );
                                });
                            }
                            extra_bar.append(&sync_btn);

                            // Share artist button
                            let share_btn = gtk4::Button::builder()
                                .icon_name("emblem-shared-symbolic")
                                .tooltip_text("Поделиться артистом")
                                .css_classes(["circular"])
                                .build();
                            {
                                let win = nav_c.window.clone();
                                let c3 = c2.clone();
                                let rt3 = rt_h.clone();
                                let aid = artist.id.clone();
                                let aname = artist.name.clone();
                                let toast = nav_c.dialogs.show_toast.clone();
                                share_btn.connect_clicked(move |_| {
                                    show_share_dialog(
                                        &win,
                                        c3.clone(),
                                        rt3.clone(),
                                        ShareKind::Artist,
                                        &aid,
                                        &aname,
                                        toast.clone(),
                                    );
                                });
                            }
                            extra_bar.append(&share_btn);

                            render_collection_header(
                                &cont,
                                &c2,
                                &eng,
                                &rt_h,
                                artist.avatar_image_id(),
                                &artist.name,
                                artist.bio.as_deref().unwrap_or("Артист KWhale"),
                                &meta,
                                ui_tracks.clone(),
                                "artist",
                                None,
                                Some(extra_bar),
                            );

                            // Top Tracks section with "Все треки" button
                            if !ui_tracks.is_empty() {
                                let hdr_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                                hdr_row.append(
                                    &gtk4::Label::builder()
                                        .label("Популярные треки")
                                        .xalign(0.0)
                                        .hexpand(true)
                                        .css_classes(["kw-section-title"])
                                        .build(),
                                );
                                let all_tracks_btn = gtk4::Button::builder()
                                    .label("Все треки артиста")
                                    .css_classes(["flat"])
                                    .build();
                                {
                                    let c3 = c2.clone();
                                    let rt3 = rt_h.clone();
                                    let aid = artist.id.clone();
                                    let aname = artist.name.clone();
                                    let open_col = nav_c.open_collection.clone();
                                    all_tracks_btn.connect_clicked(move |_| {
                                        let c4 = c3.clone();
                                        let aid2 = aid.clone();
                                        let aname2 = aname.clone();
                                        let open_col2 = open_col.clone();
                                        spawn_async(
                                            &rt3,
                                            async move {
                                                c4.api
                                                    .artist_tracks(&aid2, None, Some(150))
                                                    .await
                                            },
                                            move |r| {
                                                if let Ok(p) = r {
                                                    open_col2(CollectionTarget::SeeAllTracks {
                                                        title: format!("Все треки: {aname2}"),
                                                        tracks: p.items,
                                                    });
                                                }
                                            },
                                        );
                                    });
                                }
                                hdr_row.append(&all_tracks_btn);
                                cont.append(&hdr_row);

                                for (idx, ut) in ui_tracks.iter().take(10).enumerate() {
                                    let row = make_track_row(
                                        &c2,
                                        &eng,
                                        &rt_h,
                                        ut,
                                        ui_tracks.clone(),
                                        idx,
                                        "artist",
                                        None,
                                        None,
                                        None,
                                        &nav_c,
                                    );
                                    cont.append(&row);
                                }
                            }

                            // Discography with filter: Все / Альбомы / Синглы и EP
                            if !albums.is_empty() {
                                let disc_sec = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
                                let disc_hdr = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                                disc_hdr.append(
                                    &gtk4::Label::builder()
                                        .label("Дискография")
                                        .xalign(0.0)
                                        .hexpand(true)
                                        .css_classes(["kw-section-title"])
                                        .build(),
                                );

                                let filter_all = gtk4::Button::builder()
                                    .label("Все")
                                    .css_classes(["kw-chip", "kw-chip-active"])
                                    .build();
                                let filter_full = gtk4::Button::builder()
                                    .label("Альбомы")
                                    .css_classes(["kw-chip"])
                                    .build();
                                let filter_singles = gtk4::Button::builder()
                                    .label("Синглы и EP")
                                    .css_classes(["kw-chip"])
                                    .build();
                                disc_hdr.append(&filter_all);
                                disc_hdr.append(&filter_full);
                                disc_hdr.append(&filter_singles);
                                disc_sec.append(&disc_hdr);

                                let flow = gtk4::FlowBox::builder()
                                    .selection_mode(gtk4::SelectionMode::None)
                                    .max_children_per_line(6)
                                    .column_spacing(12)
                                    .row_spacing(12)
                                    .build();
                                disc_sec.append(&flow);
                                cont.append(&disc_sec);

                                let fill_disc: Rc<dyn Fn(&str)> = {
                                    let flow = flow.clone();
                                    let albums_all = albums;
                                    let c3 = c2.clone();
                                    let rt3 = rt_h.clone();
                                    let open_col = nav_c.open_collection.clone();
                                    Rc::new(move |mode: &str| {
                                        while let Some(ch) = flow.first_child() {
                                            flow.remove(&ch);
                                        }
                                        for al in &albums_all {
                                            let include = match mode {
                                                "albums" => !al.is_single_like(),
                                                "singles" => al.is_single_like(),
                                                _ => true,
                                            };
                                            if !include {
                                                continue;
                                            }
                                            let id = al.id.clone();
                                            let open_col2 = open_col.clone();
                                            let sub = al
                                                .year
                                                .map(|y| y.to_string())
                                                .unwrap_or_else(|| al.artist_line());
                                            let card = make_entity_card(
                                                &c3,
                                                &rt3,
                                                al.cover_image_id(),
                                                &al.title,
                                                &sub,
                                                al.type_label(),
                                                move || open_col2(CollectionTarget::Album(id.clone())),
                                            );
                                            flow.insert(&card, -1);
                                        }
                                    })
                                };

                                fill_disc("all");
                                {
                                    let fd = fill_disc.clone();
                                    let b1 = filter_all.clone();
                                    let b2 = filter_full.clone();
                                    let b3 = filter_singles.clone();
                                    filter_all.connect_clicked(move |_| {
                                        b1.add_css_class("kw-chip-active");
                                        b2.remove_css_class("kw-chip-active");
                                        b3.remove_css_class("kw-chip-active");
                                        fd("all");
                                    });
                                }
                                {
                                    let fd = fill_disc.clone();
                                    let b1 = filter_all.clone();
                                    let b2 = filter_full.clone();
                                    let b3 = filter_singles.clone();
                                    filter_full.connect_clicked(move |_| {
                                        b2.add_css_class("kw-chip-active");
                                        b1.remove_css_class("kw-chip-active");
                                        b3.remove_css_class("kw-chip-active");
                                        fd("albums");
                                    });
                                }
                                {
                                    let fd = fill_disc;
                                    let b1 = filter_all;
                                    let b2 = filter_full;
                                    let b3 = filter_singles.clone();
                                    filter_singles.connect_clicked(move |_| {
                                        b3.add_css_class("kw-chip-active");
                                        b1.remove_css_class("kw-chip-active");
                                        b2.remove_css_class("kw-chip-active");
                                        fd("singles");
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            cont.append(&gtk4::Label::new(Some(&e.user_message())));
                        }
                    }
                },
            );
        }
    }

    scroller
}

#[allow(clippy::too_many_arguments)]
fn render_collection_header(
    container: &gtk4::Box,
    core: &Arc<KwhaleCore>,
    engine: &AudioEngine,
    rt: &tokio::runtime::Handle,
    cover_id: Option<&str>,
    title: &str,
    subtitle: &str,
    meta: &str,
    tracks: Vec<UiTrack>,
    surface: &str,
    mix_id: Option<String>,
    extra_buttons: Option<gtk4::Box>,
) {
    let hdr = gtk4::Box::new(gtk4::Orientation::Horizontal, 20);
    hdr.add_css_class("kw-card");

    let pic = make_cover_picture(core, rt, cover_id, title, 160, "lg");
    hdr.append(&pic);

    let info = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    info.set_valign(gtk4::Align::Center);
    info.set_hexpand(true);

    let title_lbl = gtk4::Label::builder()
        .label(title)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["kw-hero-title"])
        .build();
    let sub_lbl = gtk4::Label::builder()
        .label(subtitle)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(70)
        .css_classes(["kw-muted"])
        .build();
    let meta_lbl = gtk4::Label::builder()
        .label(meta)
        .xalign(0.0)
        .css_classes(["caption", "kw-muted"])
        .build();

    let btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    let play_btn = gtk4::Button::builder()
        .label("▶ Слушать")
        .css_classes(["suggested-action", "pill"])
        .build();
    let shuffle_btn = gtk4::Button::builder()
        .label("Перемешать")
        .icon_name("media-playlist-shuffle-symbolic")
        .css_classes(["pill"])
        .build();

    {
        let eng = engine.clone();
        let q = tracks.clone();
        let surf = surface.to_string();
        let mid = mix_id.clone();
        play_btn.connect_clicked(move |_| {
            if !q.is_empty() {
                eng.play_queue(q.clone(), 0, &surf, mid.as_deref());
            }
        });
    }
    {
        let eng = engine.clone();
        let q = tracks;
        let surf = surface.to_string();
        let mid = mix_id;
        shuffle_btn.connect_clicked(move |_| {
            if !q.is_empty() {
                eng.play_queue(q.clone(), 0, &surf, mid.as_deref());
                if !eng.state().shuffle {
                    eng.toggle_shuffle();
                }
            }
        });
    }

    btn_row.append(&play_btn);
    btn_row.append(&shuffle_btn);
    if let Some(extra) = extra_buttons {
        btn_row.append(&extra);
    }

    info.append(&title_lbl);
    info.append(&sub_lbl);
    info.append(&meta_lbl);
    info.append(&btn_row);
    hdr.append(&info);
    container.append(&hdr);
}

// ======================= DOWNLOADS PAGE =======================

pub fn build_downloads_page(
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    rt: tokio::runtime::Handle,
    nav: NavCallbacks,
) -> (gtk4::ScrolledWindow, Rc<dyn Fn()>) {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    content.set_margin_top(20);
    content.set_margin_bottom(28);
    content.set_margin_start(24);
    content.set_margin_end(24);

    content.append(
        &gtk4::Label::builder()
            .label("Загрузки (офлайн)")
            .xalign(0.0)
            .css_classes(["kw-hero-title"])
            .build(),
    );

    let body_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    content.append(&body_box);

    let render: Rc<dyn Fn()> = {
        let body = body_box.clone();
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let nav_c = nav.clone();
        Rc::new(move || {
            while let Some(ch) = body.first_child() {
                body.remove(&ch);
            }
            let snap = c.downloads.snapshot();
            if !snap.active.is_empty() {
                let grp = adw::PreferencesGroup::builder()
                    .title("Активные загрузки")
                    .build();
                for act in &snap.active {
                    let sub = match &act.status {
                        TrackDownloadStatus::Queued => "В очереди…".to_string(),
                        TrackDownloadStatus::Downloading { percent } => {
                            format!("Загрузка: {percent}%")
                        }
                        TrackDownloadStatus::Failed { message } => format!("Ошибка: {message}"),
                        _ => String::new(),
                    };
                    let row = adw::ActionRow::builder()
                        .title(&act.track.title)
                        .subtitle(format!("{} • {}", act.track.artist, sub))
                        .build();
                    let cancel_btn = gtk4::Button::builder()
                        .icon_name("process-stop-symbolic")
                        .valign(gtk4::Align::Center)
                        .css_classes(["flat"])
                        .build();
                    let c2 = c.clone();
                    let tid = act.track.remote_id.clone();
                    cancel_btn.connect_clicked(move |_| c2.downloads.cancel(&tid));
                    row.add_suffix(&cancel_btn);
                    grp.add(&row);
                }
                body.append(&grp);
            }

            if snap.downloaded.is_empty() && snap.active.is_empty() {
                body.append(
                    &adw::StatusPage::builder()
                        .icon_name("folder-download-symbolic")
                        .title("Нет загруженных треков")
                        .description(
                            "Скачивайте треки через контекстное меню для прослушивания без сети",
                        )
                        .build(),
                );
                return;
            }

            let ui_tracks: Vec<UiTrack> =
                snap.downloaded.iter().map(|d| d.to_ui_track()).collect();
            for (idx, ut) in ui_tracks.iter().enumerate() {
                let row = make_track_row(
                    &c,
                    &eng,
                    &rt_h,
                    ut,
                    ui_tracks.clone(),
                    idx,
                    "library",
                    None,
                    None,
                    None,
                    &nav_c,
                );
                body.append(&row);
            }
        })
    };

    render();

    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build();
    (scroller, render)
}

// ======================= HISTORY PAGE =======================

pub fn build_history_page(
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    rt: tokio::runtime::Handle,
    nav: NavCallbacks,
) -> (gtk4::ScrolledWindow, Rc<dyn Fn()>) {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
    content.set_margin_top(20);
    content.set_margin_bottom(28);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let top_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    top_row.append(
        &gtk4::Label::builder()
            .label("История прослушивания")
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["kw-hero-title"])
            .build(),
    );
    let clear_btn = gtk4::Button::builder()
        .label("Очистить историю")
        .css_classes(["destructive-action", "pill"])
        .build();
    top_row.append(&clear_btn);
    content.append(&top_row);

    let body_box = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    content.append(&body_box);

    let reload: Rc<dyn Fn()> = {
        let body = body_box.clone();
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let nav_c = nav.clone();
        Rc::new(move || {
            let c2 = c.clone();
            let c3 = c.clone();
            let eng2 = eng.clone();
            let rt2 = rt_h.clone();
            let nav2 = nav_c.clone();
            let body2 = body.clone();
            spawn_async(
                &rt_h,
                async move { c2.api.history(None, Some(100)).await },
                move |res| {
                    while let Some(ch) = body2.first_child() {
                        body2.remove(&ch);
                    }
                    match res {
                        Ok(page) if !page.items.is_empty() => {
                            let ui_tracks: Vec<UiTrack> = page
                                .items
                                .iter()
                                .map(|e| e.track.to_ui(c3.downloads.is_downloaded(&e.track.id)))
                                .collect();
                            for (idx, ut) in ui_tracks.iter().enumerate() {
                                let row = make_track_row(
                                    &c3,
                                    &eng2,
                                    &rt2,
                                    ut,
                                    ui_tracks.clone(),
                                    idx,
                                    "library",
                                    None,
                                    None,
                                    None,
                                    &nav2,
                                );
                                body2.append(&row);
                            }
                        }
                        _ => {
                            body2.append(
                                &adw::StatusPage::builder()
                                    .icon_name("document-open-recent-symbolic")
                                    .title("История пуста")
                                    .build(),
                            );
                        }
                    }
                },
            );
        })
    };

    {
        let c = core.clone();
        let rt_h = rt.clone();
        let r = reload.clone();
        let toast = nav.dialogs.show_toast.clone();
        clear_btn.connect_clicked(move |_| {
            let c2 = c.clone();
            let r2 = r.clone();
            let toast2 = toast.clone();
            spawn_async(
                &rt_h,
                async move { c2.api.clear_history().await },
                move |res| match res {
                    Ok(()) => {
                        r2();
                        toast2("История очищена");
                    }
                    Err(e) => toast2(&e.user_message()),
                },
            );
        });
    }

    reload();

    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build();
    (scroller, reload)
}

// ======================= PROFILE & ENRICHMENTS PAGE =======================

pub fn build_profile_page(
    core: Arc<KwhaleCore>,
    rt: tokio::runtime::Handle,
    nav: NavCallbacks,
) -> (gtk4::ScrolledWindow, Rc<dyn Fn()>) {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 18);
    content.set_margin_top(20);
    content.set_margin_bottom(28);
    content.set_margin_start(24);
    content.set_margin_end(24);

    content.append(
        &gtk4::Label::builder()
            .label("Профиль и метаданные")
            .xalign(0.0)
            .css_classes(["kw-hero-title"])
            .build(),
    );

    let user_group = adw::PreferencesGroup::builder()
        .title("Учётная запись")
        .build();
    let name_entry = adw::EntryRow::builder()
        .title("Отображаемое имя")
        .show_apply_button(true)
        .build();
    let email_row = adw::ActionRow::builder().title("Email").build();
    user_group.add(&name_entry);
    user_group.add(&email_row);
    content.append(&user_group);

    {
        let c = core.clone();
        let rt_h = rt.clone();
        let toast = nav.dialogs.show_toast.clone();
        name_entry.connect_apply(move |entry| {
            let new_name = entry.text().trim().to_string();
            if new_name.is_empty() {
                return;
            }
            let c2 = c.clone();
            let toast2 = toast.clone();
            spawn_async(
                &rt_h,
                async move { c2.api.update_me(&new_name).await },
                move |res| match res {
                    Ok(_) => toast2("Имя профиля сохранено"),
                    Err(e) => toast2(&e.user_message()),
                },
            );
        });
    }

    let enrich_box = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    content.append(&enrich_box);

    let logout_btn = gtk4::Button::builder()
        .label("Выйти из аккаунта")
        .halign(gtk4::Align::Start)
        .css_classes(["destructive-action", "pill"])
        .build();
    {
        let c = core.clone();
        let rt_h = rt.clone();
        let on_logout = nav.on_logout.clone();
        logout_btn.connect_clicked(move |_| {
            let c2 = c.clone();
            let cb = on_logout.clone();
            spawn_async(
                &rt_h,
                async move {
                    c2.api.logout().await;
                },
                move |_| cb(),
            );
        });
    }
    content.append(&logout_btn);

    let reload: Rc<dyn Fn()> = {
        let c = core.clone();
        let rt_h = rt.clone();
        let name_entry = name_entry.clone();
        let email_row = email_row.clone();
        let enrich_box = enrich_box.clone();
        let toast = nav.dialogs.show_toast.clone();
        Rc::new(move || {
            if let Some(sess) = c.api.session() {
                name_entry.set_text(&sess.user.display_name);
                email_row.set_subtitle(sess.user.email.as_deref().unwrap_or("—"));
            }
            let c2 = c.clone();
            let c3 = c.clone();
            let rt2 = rt_h.clone();
            let eb2 = enrich_box.clone();
            let toast2 = toast.clone();
            spawn_async(
                &rt_h,
                async move { c2.api.enrichments().await.map(|p| p.items) },
                move |res: CoreResult<Vec<EnrichmentProposal>>| {
                    while let Some(ch) = eb2.first_child() {
                        eb2.remove(&ch);
                    }
                    let grp = adw::PreferencesGroup::builder()
                        .title("Предложения обогащения метаданных")
                        .description("Автоматически найденные альбомы, годы, жанры и ISRC для ваших треков")
                        .build();
                    match res {
                        Ok(items) if !items.is_empty() => {
                            for item in items {
                                let title = item
                                    .title
                                    .clone()
                                    .unwrap_or_else(|| format!("Трек #{:?}", item.track_id));
                                let proposed_summary = format!(
                                    "Альбом: {} • Год: {} • Жанр: {}",
                                    item.proposed.album.as_deref().unwrap_or("—"),
                                    item.proposed
                                        .year
                                        .map(|y| y.to_string())
                                        .unwrap_or_else(|| "—".into()),
                                    item.proposed.genre.as_deref().unwrap_or("—")
                                );
                                let row = adw::ActionRow::builder()
                                    .title(&title)
                                    .subtitle(&proposed_summary)
                                    .build();
                                let ok_btn = gtk4::Button::builder()
                                    .icon_name("emblem-ok-symbolic")
                                    .valign(gtk4::Align::Center)
                                    .css_classes(["suggested-action", "circular"])
                                    .tooltip_text("Подтвердить")
                                    .build();
                                let rej_btn = gtk4::Button::builder()
                                    .icon_name("window-close-symbolic")
                                    .valign(gtk4::Align::Center)
                                    .css_classes(["destructive-action", "circular"])
                                    .tooltip_text("Отклонить")
                                    .build();

                                let pid = item.id;
                                {
                                    let c4 = c3.clone();
                                    let rt3 = rt2.clone();
                                    let toast3 = toast2.clone();
                                    let rw = row.downgrade();
                                    ok_btn.connect_clicked(move |_| {
                                        let c5 = c4.clone();
                                        let toast4 = toast3.clone();
                                        let rw2 = rw.clone();
                                        spawn_async(
                                            &rt3,
                                            async move { c5.api.confirm_enrichment(pid).await },
                                            move |r| match r {
                                                Ok(()) => {
                                                    if let Some(row_w) = rw2.upgrade() {
                                                        row_w.set_visible(false);
                                                    }
                                                    toast4("Метаданные трека обновлены");
                                                }
                                                Err(e) => toast4(&e.user_message()),
                                            },
                                        );
                                    });
                                }
                                {
                                    let c4 = c3.clone();
                                    let rt3 = rt2.clone();
                                    let toast3 = toast2.clone();
                                    let rw = row.downgrade();
                                    rej_btn.connect_clicked(move |_| {
                                        let c5 = c4.clone();
                                        let toast4 = toast3.clone();
                                        let rw2 = rw.clone();
                                        spawn_async(
                                            &rt3,
                                            async move { c5.api.reject_enrichment(pid).await },
                                            move |r| match r {
                                                Ok(()) => {
                                                    if let Some(row_w) = rw2.upgrade() {
                                                        row_w.set_visible(false);
                                                    }
                                                    toast4("Предложение отклонено");
                                                }
                                                Err(e) => toast4(&e.user_message()),
                                            },
                                        );
                                    });
                                }
                                row.add_suffix(&ok_btn);
                                row.add_suffix(&rej_btn);
                                grp.add(&row);
                            }
                        }
                        _ => {
                            let empty = adw::ActionRow::builder()
                                .title("Нет ожидающих предложений")
                                .build();
                            grp.add(&empty);
                        }
                    }
                    eb2.append(&grp);
                },
            );
        })
    };

    reload();

    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build();
    (scroller, reload)
}

// ======================= APPEARANCE PAGE (129 ACCENTS + 4 MODES) =======================

pub fn build_appearance_page(core: Arc<KwhaleCore>) -> gtk4::ScrolledWindow {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 18);
    content.set_margin_top(20);
    content.set_margin_bottom(28);
    content.set_margin_start(24);
    content.set_margin_end(24);

    content.append(
        &gtk4::Label::builder()
            .label("Оформление и акценты (129 палитр)")
            .xalign(0.0)
            .css_classes(["kw-hero-title"])
            .build(),
    );

    let snap = core.settings.snapshot();
    let mode_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    let modes = [
        (ThemeMode::System, "Системная"),
        (ThemeMode::Dark, "Тёмная"),
        (ThemeMode::Light, "Светлая"),
        (ThemeMode::Amoled, "AMOLED (чистый чёрный)"),
    ];
    let mut mode_btns: Vec<(ThemeMode, gtk4::Button)> = Vec::new();
    for (m, label) in modes {
        let b = gtk4::Button::builder()
            .label(label)
            .css_classes(if snap.theme_mode == m {
                vec!["kw-chip", "kw-chip-active"]
            } else {
                vec!["kw-chip"]
            })
            .build();
        mode_row.append(&b);
        mode_btns.push((m, b));
    }
    content.append(&mode_row);

    let sys_accent_group = adw::PreferencesGroup::new();
    let sys_accent_switch = adw::SwitchRow::builder()
        .title("Следовать системному акценту")
        .subtitle("Использовать системный акцент рабочего стола")
        .active(snap.follow_system_accent)
        .build();
    sys_accent_group.add(&sys_accent_switch);
    content.append(&sys_accent_group);

    for (m, btn) in &mode_btns {
        let m_val = *m;
        let all = mode_btns.clone();
        let c = core.clone();
        btn.connect_clicked(move |_| {
            for (om, ob) in &all {
                if *om == m_val {
                    ob.add_css_class("kw-chip-active");
                } else {
                    ob.remove_css_class("kw-chip-active");
                }
            }
            let s = c.settings.snapshot();
            c.update_theme(m_val, s.accent_id, s.follow_system_accent);
        });
    }

    {
        let c = core.clone();
        sys_accent_switch.connect_active_notify(move |sw| {
            let s = c.settings.snapshot();
            c.update_theme(s.theme_mode, s.accent_id, sw.is_active());
        });
    }

    for family in accent_families() {
        let sec = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        sec.append(
            &gtk4::Label::builder()
                .label(format!("{} ({})", family.title, family.accents.len()))
                .xalign(0.0)
                .css_classes(["kw-section-title"])
                .build(),
        );
        let flow = gtk4::FlowBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .max_children_per_line(8)
            .column_spacing(8)
            .row_spacing(8)
            .build();

        for acc in family.accents {
            let btn = gtk4::Button::builder()
                .label(format!("● {}", acc.name))
                .tooltip_text(acc.rgb().to_hex())
                .css_classes(["kw-chip"])
                .build();
            let c = core.clone();
            let aid = acc.id;
            let sys_sw = sys_accent_switch.clone();
            btn.connect_clicked(move |_| {
                sys_sw.set_active(false);
                let s = c.settings.snapshot();
                c.update_theme(s.theme_mode, aid, false);
            });
            flow.insert(&btn, -1);
        }
        sec.append(&flow);
        content.append(&sec);
    }

    gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build()
}

// ======================= SETTINGS PAGE =======================

pub fn build_settings_page(
    core: Arc<KwhaleCore>,
    rt: tokio::runtime::Handle,
    nav: NavCallbacks,
) -> gtk4::ScrolledWindow {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 18);
    content.set_margin_top(20);
    content.set_margin_bottom(28);
    content.set_margin_start(24);
    content.set_margin_end(24);

    content.append(
        &gtk4::Label::builder()
            .label("Настройки")
            .xalign(0.0)
            .css_classes(["kw-hero-title"])
            .build(),
    );

    let snap = core.settings.snapshot();

    // 1. Server URL & Health check
    let srv_group = adw::PreferencesGroup::builder()
        .title("Сервер KWhale")
        .description("Централизованный API (/v1)")
        .build();
    let url_row = adw::EntryRow::builder()
        .title("Адрес сервера")
        .text(&snap.api_base_url)
        .show_apply_button(true)
        .build();
    let http_row = adw::SwitchRow::builder()
        .title("Разрешить незащищённый HTTP вне LAN")
        .active(snap.allow_insecure_http)
        .build();
    let check_row = adw::ActionRow::builder()
        .title("Проверка соединения")
        .subtitle("Нажмите, чтобы проверить /v1/health и /v1/config")
        .activatable(true)
        .build();
    let check_btn = gtk4::Button::builder()
        .label("Проверить")
        .valign(gtk4::Align::Center)
        .css_classes(["pill"])
        .build();
    check_row.add_suffix(&check_btn);
    srv_group.add(&url_row);
    srv_group.add(&http_row);
    srv_group.add(&check_row);
    content.append(&srv_group);

    let apply_server = {
        let c = core.clone();
        let url_r = url_row.clone();
        let http_r = http_row.clone();
        let toast = nav.dialogs.show_toast.clone();
        Rc::new(move || {
            let raw = url_r.text().trim().to_string();
            let Some(norm) = ServerUrl::normalize(&raw, None) else {
                toast("Некорректный URL сервера");
                return;
            };
            match c.reconfigure_api(&norm, http_r.is_active()) {
                Ok(()) => {
                    url_r.set_text(&norm);
                    toast("Адрес сервера сохранён");
                }
                Err(e) => toast(&e.user_message()),
            }
        })
    };
    {
        let app_srv = apply_server.clone();
        url_row.connect_apply(move |_| app_srv());
    }
    {
        let app_srv = apply_server;
        http_row.connect_active_notify(move |_| app_srv());
    }
    {
        let c = core.clone();
        let rt_h = rt.clone();
        let crow = check_row.clone();
        check_btn.connect_clicked(move |_| {
            crow.set_subtitle("Проверка…");
            let c2 = c.clone();
            let crow2 = crow.clone();
            spawn_async(
                &rt_h,
                async move {
                    let h = c2.api.health().await?;
                    let cfg = c2.api.server_config().await.ok();
                    Ok::<(String, Option<String>), kwhale_core::CoreError>((
                        h.status,
                        cfg.map(|c| c.api_version),
                    ))
                },
                move |res| match res {
                    Ok((st, ver)) => {
                        crow2.set_subtitle(&format!(
                            "Статус: {} • API: {}",
                            st,
                            ver.unwrap_or_else(|| "v1".into())
                        ));
                    }
                    Err(e) => {
                        crow2.set_subtitle(&format!("Ошибка: {}", e.user_message()));
                    }
                },
            );
        });
    }

    // 2. Native UI Backend Selector (Auto / libadwaita / Qt6)
    let ui_group = adw::PreferencesGroup::builder()
        .title("Нативный движок интерфейса")
        .description("Выберите движок при следующем запуске приложения (или через флаг --ui=adw|qt)")
        .build();
    let ui_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    ui_row.set_margin_top(8);
    ui_row.set_margin_bottom(8);
    let backends = [
        (UiBackend::Auto, "Авто (по рабочему столу)"),
        (UiBackend::Adw, "GNOME (GTK4 + libadwaita)"),
        (UiBackend::Qt, "KDE (Qt6 Quick + Kirigami)"),
    ];
    let mut ui_btns: Vec<(UiBackend, gtk4::Button)> = Vec::new();
    for (b_mode, label) in backends {
        let btn = gtk4::Button::builder()
            .label(label)
            .css_classes(if snap.ui_backend == b_mode {
                vec!["kw-chip", "kw-chip-active"]
            } else {
                vec!["kw-chip"]
            })
            .build();
        ui_row.append(&btn);
        ui_btns.push((b_mode, btn));
    }
    for (b_mode, btn) in &ui_btns {
        let bm = *b_mode;
        let all = ui_btns.clone();
        let c = core.clone();
        let toast = nav.dialogs.show_toast.clone();
        btn.connect_clicked(move |_| {
            for (obm, ob) in &all {
                if *obm == bm {
                    ob.add_css_class("kw-chip-active");
                } else {
                    ob.remove_css_class("kw-chip-active");
                }
            }
            c.settings.update(|s| s.ui_backend = bm);
            toast("Движок интерфейса сохранён (применится при следующем запуске)");
        });
    }
    ui_group.add(&ui_row);
    content.append(&ui_group);

    // 3. AI Prompt-Mix BYOK
    let llm_group = adw::PreferencesGroup::builder()
        .title("AI Промпт-Микс (BYOK OpenAI-совместимый LLM)")
        .description("Необязательные ключи для генерации промпт-миксов через собственный LLM endpoint")
        .build();
    let llm_ep = adw::EntryRow::builder()
        .title("LLM Endpoint URL")
        .text(&snap.llm_endpoint)
        .build();
    let llm_key = adw::PasswordEntryRow::builder()
        .title("LLM API Key")
        .text(&snap.llm_api_key)
        .build();
    let llm_model = adw::EntryRow::builder()
        .title("LLM Модель")
        .text(&snap.llm_model)
        .build();
    let save_llm_btn = gtk4::Button::builder()
        .label("Сохранить настройки AI")
        .halign(gtk4::Align::Start)
        .margin_top(8)
        .css_classes(["pill"])
        .build();
    {
        let c = core.clone();
        let ep = llm_ep.clone();
        let key = llm_key.clone();
        let mdl = llm_model.clone();
        let toast = nav.dialogs.show_toast.clone();
        save_llm_btn.connect_clicked(move |_| {
            c.settings.update(|s| {
                s.llm_endpoint = ep.text().to_string();
                s.llm_api_key = key.text().to_string();
                s.llm_model = mdl.text().to_string();
            });
            toast("Настройки AI сохранены");
        });
    }
    llm_group.add(&llm_ep);
    llm_group.add(&llm_key);
    llm_group.add(&llm_model);
    llm_group.add(&save_llm_btn);
    content.append(&llm_group);

    // 4. Playback cache & Prefetch
    let cache_group = adw::PreferencesGroup::builder()
        .title("Кэш воспроизведения и предзагрузка")
        .build();
    let prefetch_row = adw::SwitchRow::builder()
        .title("Предзагрузка следующих 5 треков очереди")
        .active(snap.prefetch_enabled)
        .build();
    {
        let c = core.clone();
        prefetch_row.connect_active_notify(move |sw| {
            c.settings.update(|s| s.prefetch_enabled = sw.is_active());
        });
    }
    let cache_row = adw::ActionRow::builder()
        .title("Дисковый кэш аудиопотоков (лимит 1 ГБ)")
        .subtitle(format!(
            "Занято: {}",
            format_bytes(core.playback_cache.used_bytes())
        ))
        .build();
    let clear_cache_btn = gtk4::Button::builder()
        .label("Очистить кэш")
        .valign(gtk4::Align::Center)
        .css_classes(["pill"])
        .build();
    {
        let c = core.clone();
        let cr = cache_row.clone();
        let toast = nav.dialogs.show_toast.clone();
        clear_cache_btn.connect_clicked(move |_| {
            c.playback_cache.clear();
            cr.set_subtitle(&format!(
                "Занято: {}",
                format_bytes(c.playback_cache.used_bytes())
            ));
            toast("Кэш воспроизведения очищен");
        });
    }
    cache_row.add_suffix(&clear_cache_btn);
    cache_group.add(&prefetch_row);
    cache_group.add(&cache_row);
    content.append(&cache_group);

    gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .hexpand(true)
        .child(&content)
        .build()
}
