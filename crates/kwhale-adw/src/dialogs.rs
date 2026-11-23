use crate::style::{make_cover_picture, spawn_async};
use gtk4::gio;
use gtk4::prelude::*;
use kwhale_audio::AudioEngine;
use kwhale_core::{
    format_bytes, format_duration_ms, CoreResult, KwhaleCore, Playlist, PlaylistCreate,
    PlaylistUpdate, ShareKind, ShareLink, Track, TrackInfo, UiTrack,
};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
pub struct DialogCallbacks {
    pub show_toast: Rc<dyn Fn(&str)>,
    pub open_artist: Rc<dyn Fn(String)>,
    pub open_album: Rc<dyn Fn(String)>,
    pub open_playlist: Rc<dyn Fn(String)>,
    pub open_lyrics: Rc<dyn Fn()>,
    pub reload_library: Rc<dyn Fn()>,
}

pub fn show_track_actions_dialog(
    parent: &impl IsA<gtk4::Widget>,
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    rt: tokio::runtime::Handle,
    track: UiTrack,
    surface: String,
    playlist_context_id: Option<String>,
    cbs: DialogCallbacks,
) {
    let dialog = adw::Dialog::builder()
        .title(&track.title)
        .content_width(420)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    // Header row with cover + title + artist
    let hdr = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let pic = make_cover_picture(
        &core,
        &rt,
        track.cover_image_id.as_deref(),
        &track.title,
        56,
        "sm",
    );
    let meta_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    meta_box.set_valign(gtk4::Align::Center);
    let title_lbl = gtk4::Label::builder()
        .label(&track.title)
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .css_classes(["heading"])
        .build();
    let sub_lbl = gtk4::Label::builder()
        .label(format!(
            "{} • {}",
            track.artist,
            format_duration_ms((track.duration_sec as u64) * 1000)
        ))
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .css_classes(["kw-muted"])
        .build();
    meta_box.append(&title_lbl);
    meta_box.append(&sub_lbl);
    hdr.append(&pic);
    hdr.append(&meta_box);
    vbox.append(&hdr);

    let list = gtk4::ListBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();

    let add_action = |list: &gtk4::ListBox,
                      icon: &str,
                      title: &str,
                      subtitle: Option<&str>,
                      dlg: &adw::Dialog,
                      on_click: Box<dyn Fn()>| {
        let row = adw::ActionRow::builder()
            .title(title)
            .activatable(true)
            .build();
        if let Some(sub) = subtitle {
            row.set_subtitle(sub);
        }
        let img = gtk4::Image::from_icon_name(icon);
        row.add_prefix(&img);
        let dlg_weak = dlg.downgrade();
        row.connect_activated(move |_| {
            if let Some(d) = dlg_weak.upgrade() {
                d.close();
            }
            on_click();
        });
        list.append(&row);
    };

    // 1. Play Now
    {
        let eng = engine.clone();
        let tr = track.clone();
        let surf = surface.clone();
        add_action(
            &list,
            "media-playback-start-symbolic",
            "Воспроизвести сейчас",
            None,
            &dialog,
            Box::new(move || {
                eng.play_queue(vec![tr.clone()], 0, &surf, None);
            }),
        );
    }

    // 2. Play Next
    {
        let eng = engine.clone();
        let tr = track.clone();
        let surf = surface.clone();
        let toast = cbs.show_toast.clone();
        add_action(
            &list,
            "media-skip-forward-symbolic",
            "Воспроизвести следующим",
            None,
            &dialog,
            Box::new(move || {
                eng.play_next(tr.clone(), &surf);
                toast("Будет воспроизведено следующим");
            }),
        );
    }

    // 3. Add to Queue
    {
        let eng = engine.clone();
        let tr = track.clone();
        let surf = surface.clone();
        let toast = cbs.show_toast.clone();
        add_action(
            &list,
            "view-list-symbolic",
            "Добавить в очередь",
            None,
            &dialog,
            Box::new(move || {
                eng.add_queue(vec![tr.clone()], &surf);
                toast("Добавлено в очередь");
            }),
        );
    }

    // 4. Like / Unlike
    {
        let is_liked = core.reactions.is_liked(&track.remote_id, track.is_liked);
        let label = if is_liked {
            "Убрать из Любимых треков"
        } else {
            "Добавить в Любимые треки"
        };
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let tr = track.clone();
        let surf = surface.clone();
        let toast = cbs.show_toast.clone();
        let reload = cbs.reload_library.clone();
        add_action(
            &list,
            if is_liked {
                "emblem-favorite-symbolic"
            } else {
                "non-starred-symbolic"
            },
            label,
            None,
            &dialog,
            Box::new(move || {
                let c2 = c.clone();
                let c_after = c.clone();
                let tr2 = tr.clone();
                let tr_id = tr.remote_id.clone();
                let s2 = surf.clone();
                let eng2 = eng.clone();
                let toast2 = toast.clone();
                let reload2 = reload.clone();
                spawn_async(
                    &rt_h,
                    async move { c2.reactions.toggle_like(&tr2, &s2).await },
                    move |res| match res {
                        Ok(liked) => {
                            eng2.update_track_liked(&tr_id, liked);
                            c_after.cache.invalidate_library();
                            reload2();
                            toast2(if liked {
                                "Добавлено в Любимые треки"
                            } else {
                                "Удалено из Любимых треков"
                            });
                        }
                        Err(e) => toast2(&e),
                    },
                );
            }),
        );
    }

    // 5. Dislike / Undislike
    {
        let is_disliked = core
            .reactions
            .is_disliked(&track.remote_id, track.is_disliked);
        let label = if is_disliked {
            "Снять отметку «Не нравится»"
        } else {
            "Не нравится (скрывать в рекомендациях)"
        };
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let tr = track.clone();
        let surf = surface.clone();
        let toast = cbs.show_toast.clone();
        add_action(
            &list,
            "action-unavailable-symbolic",
            label,
            None,
            &dialog,
            Box::new(move || {
                let c2 = c.clone();
                let tr2 = tr.clone();
                let tr_id = tr.remote_id.clone();
                let s2 = surf.clone();
                let eng2 = eng.clone();
                let toast2 = toast.clone();
                spawn_async(
                    &rt_h,
                    async move { c2.reactions.toggle_dislike(&tr2, &s2).await },
                    move |res| match res {
                        Ok(disliked) => {
                            if disliked {
                                if eng2
                                    .state()
                                    .current()
                                    .map(|cur| cur.remote_id == tr_id)
                                    .unwrap_or(false)
                                {
                                    eng2.next();
                                }
                                toast2("Трек отмечен «Не нравится»");
                            } else {
                                toast2("Отметка «Не нравится» снята");
                            }
                        }
                        Err(e) => toast2(&e),
                    },
                );
            }),
        );
    }

    // 6. Add to Playlist
    {
        let parent_widget = parent.upcast_ref::<gtk4::Widget>().clone();
        let c = core.clone();
        let rt_h = rt.clone();
        let tr = track.clone();
        let cbs2 = cbs.clone();
        add_action(
            &list,
            "folder-music-symbolic",
            "Добавить в плейлист…",
            None,
            &dialog,
            Box::new(move || {
                show_add_to_playlist_dialog(
                    &parent_widget,
                    c.clone(),
                    rt_h.clone(),
                    tr.clone(),
                    cbs2.clone(),
                );
            }),
        );
    }

    // 6b. Remove from current playlist if inside user playlist
    if let Some(pl_id) = playlist_context_id {
        let c = core.clone();
        let rt_h = rt.clone();
        let tr_id = track.remote_id.clone();
        let toast = cbs.show_toast.clone();
        let open_pl = cbs.open_playlist.clone();
        let reload = cbs.reload_library.clone();
        add_action(
            &list,
            "user-trash-symbolic",
            "Удалить из этого плейлиста",
            None,
            &dialog,
            Box::new(move || {
                let c2 = c.clone();
                let c_after = c.clone();
                let pid = pl_id.clone();
                let pid_after = pl_id.clone();
                let tid = tr_id.clone();
                let toast2 = toast.clone();
                let open_pl2 = open_pl.clone();
                let reload2 = reload.clone();
                spawn_async(
                    &rt_h,
                    async move { c2.api.remove_playlist_track(&pid, &tid).await },
                    move |res| match res {
                        Ok(()) => {
                            c_after.cache.invalidate_collection("playlist", &pid_after);
                            c_after.cache.invalidate_library();
                            reload2();
                            open_pl2(pid_after);
                            toast2("Трек удалён из плейлиста");
                        }
                        Err(e) => toast2(&e.user_message()),
                    },
                );
            }),
        );
    }

    // 7. Start Track Radio / Similar Tracks
    {
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let tr = track.clone();
        let toast = cbs.show_toast.clone();
        add_action(
            &list,
            "media-playlist-shuffle-symbolic",
            "Волна по треку (похожие)",
            None,
            &dialog,
            Box::new(move || {
                let c2 = c.clone();
                let c_after = c.clone();
                let tid = tr.remote_id.clone();
                let seed = tr.clone();
                let eng2 = eng.clone();
                let toast2 = toast.clone();
                spawn_async(
                    &rt_h,
                    async move { c2.api.similar_tracks(&tid, Some(25)).await },
                    move |res| match res {
                        Ok(page) => {
                            let mut queue = vec![seed];
                            for t in page.items {
                                let cached = c_after.downloads.is_downloaded(&t.id);
                                queue.push(t.to_ui(cached));
                            }
                            eng2.play_queue(queue, 0, "mix", None);
                            toast2("Запущена волна похожих треков");
                        }
                        Err(e) => toast2(&e.user_message()),
                    },
                );
            }),
        );
    }

    // 8. Offline Download / Remove Download
    {
        let is_dl = core.downloads.is_downloaded(&track.remote_id);
        let c = core.clone();
        let tr = track.clone();
        let toast = cbs.show_toast.clone();
        add_action(
            &list,
            if is_dl {
                "edit-delete-symbolic"
            } else {
                "folder-download-symbolic"
            },
            if is_dl {
                "Удалить офлайн-копию"
            } else {
                "Скачать для офлайн-прослушивания"
            },
            None,
            &dialog,
            Box::new(move || {
                if is_dl {
                    c.downloads.remove(&tr.remote_id);
                    toast("Офлайн-копия удалена");
                } else {
                    c.downloads
                        .set_duration_probe(Arc::new(kwhale_audio::measure_duration_sec));
                    c.downloads.enqueue(tr.clone(), false);
                    toast("Загрузка трека запущена");
                }
            }),
        );
    }

    // 9. Go to Artist / Album
    if let Some(aid) = track.artist_remote_id.clone() {
        let open_artist = cbs.open_artist.clone();
        let artist_name = track.artist.clone();
        add_action(
            &list,
            "avatar-default-symbolic",
            "Перейти к артисту",
            Some(&artist_name),
            &dialog,
            Box::new(move || open_artist(aid.clone())),
        );
    }
    if let Some(alid) = track.album_remote_id.clone() {
        let open_album = cbs.open_album.clone();
        let album_title = track.album.clone().unwrap_or_default();
        add_action(
            &list,
            "media-optical-symbolic",
            "Перейти к альбому",
            Some(&album_title),
            &dialog,
            Box::new(move || open_album(alid.clone())),
        );
    }

    // 10. Lyrics
    {
        let open_lyrics = cbs.open_lyrics.clone();
        add_action(
            &list,
            "format-justify-center-symbolic",
            "Открыть текст песни",
            None,
            &dialog,
            Box::new(move || open_lyrics()),
        );
    }

    // 11. Share
    {
        let parent_widget = parent.upcast_ref::<gtk4::Widget>().clone();
        let c = core.clone();
        let rt_h = rt.clone();
        let tid = track.remote_id.clone();
        let t_title = track.title.clone();
        let toast = cbs.show_toast.clone();
        add_action(
            &list,
            "emblem-shared-symbolic",
            "Поделиться треком…",
            None,
            &dialog,
            Box::new(move || {
                show_share_dialog(
                    &parent_widget,
                    c.clone(),
                    rt_h.clone(),
                    ShareKind::Track,
                    &tid,
                    &t_title,
                    toast.clone(),
                );
            }),
        );
    }

    // 12. Censorship report / cancel
    {
        let reported = core
            .reactions
            .is_censorship_reported(&track.remote_id, track.is_censorship_reported);
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let tr = track.clone();
        let toast = cbs.show_toast.clone();
        add_action(
            &list,
            "dialog-warning-symbolic",
            if reported {
                "Отменить жалобу на цензуру"
            } else {
                "Пожаловаться на цензуру (в 1 клик)"
            },
            None,
            &dialog,
            Box::new(move || {
                let c2 = c.clone();
                let tr2 = tr.clone();
                let pos = Some(eng.state().position_ms);
                let toast2 = toast.clone();
                if reported {
                    spawn_async(
                        &rt_h,
                        async move { c2.reactions.cancel_censorship_report(&tr2).await },
                        move |res| match res {
                            Ok(msg) => toast2(msg),
                            Err(e) => toast2(&e),
                        },
                    );
                } else {
                    spawn_async(
                        &rt_h,
                        async move { c2.reactions.report_censorship_one_click(&tr2, pos).await },
                        move |res| match res {
                            Ok(msg) => toast2(msg),
                            Err(e) => toast2(&e),
                        },
                    );
                }
            }),
        );
    }

    // 13. Technical Track Info
    {
        let parent_widget = parent.upcast_ref::<gtk4::Widget>().clone();
        let c = core.clone();
        let rt_h = rt.clone();
        let tr = track.clone();
        add_action(
            &list,
            "dialog-information-symbolic",
            "Сведения о треке и качестве",
            None,
            &dialog,
            Box::new(move || {
                show_track_info_dialog(&parent_widget, c.clone(), rt_h.clone(), &tr);
            }),
        );
    }

    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .min_content_height(380)
        .max_content_height(520)
        .child(&list)
        .build();
    vbox.append(&scroller);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&vbox));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

pub fn show_track_info_dialog(
    parent: &impl IsA<gtk4::Widget>,
    core: Arc<KwhaleCore>,
    rt: tokio::runtime::Handle,
    track: &UiTrack,
) {
    let dialog = adw::Dialog::builder()
        .title("Сведения о треке")
        .content_width(440)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let status_lbl = gtk4::Label::builder()
        .label("Загрузка технической информации…")
        .xalign(0.0)
        .css_classes(["kw-muted"])
        .build();
    vbox.append(&status_lbl);

    let group = adw::PreferencesGroup::builder()
        .title(&track.title)
        .description(&track.artist)
        .build();
    vbox.append(&group);

    let sources_group = adw::PreferencesGroup::builder()
        .title("Источники и коннекторы")
        .build();
    vbox.append(&sources_group);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&vbox));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));

    let tid = track.remote_id.clone();
    spawn_async(
        &rt,
        async move { core.api.track_info(&tid).await },
        move |res: CoreResult<TrackInfo>| match res {
            Ok(info) => {
                status_lbl.set_visible(false);
                let add_row = |g: &adw::PreferencesGroup, k: &str, v: &str| {
                    let r = adw::ActionRow::builder().title(k).subtitle(v).build();
                    g.add(&r);
                };
                add_row(&group, "ID трека", &info.track.id);
                if let Some(isrc) = &info.isrc {
                    add_row(&group, "ISRC", isrc);
                }
                if let Some(q) = &info.quality {
                    let codec = q.codec.clone().unwrap_or_else(|| "Неизвестно".into());
                    let lossless = if q.lossless { "Lossless" } else { "Lossy" };
                    add_row(&group, "Кодек", &format!("{codec} ({lossless}, tier {})", q.tier));
                    if let Some(br) = q.bitrate_kbps {
                        add_row(&group, "Битрейт", &format!("{br} кбит/с"));
                    }
                    if let Some(sr) = q.sample_rate {
                        let bd = q
                            .bit_depth
                            .map(|b| format!(" / {b}-bit"))
                            .unwrap_or_default();
                        add_row(&group, "Частота дискретизации", &format!("{sr} Гц{bd}"));
                    }
                    if let Some(ch) = q.channels {
                        add_row(&group, "Каналы", &ch.to_string());
                    }
                    if let Some(b) = q.bytes {
                        add_row(&group, "Размер файла", &format_bytes(b));
                    }
                    if let Some(ms) = q.measured_duration_ms {
                        add_row(&group, "Измеренная длительность", &format_duration_ms(ms));
                    }
                }
                if info.sources.is_empty() {
                    let r = adw::ActionRow::builder()
                        .title("Внутренний каталог сервера")
                        .build();
                    sources_group.add(&r);
                } else {
                    for s in &info.sources {
                        let st = if s.available {
                            "Доступен"
                        } else {
                            "Недоступен"
                        };
                        let tier = s
                            .quality_tier
                            .map(|t| format!(" • tier {t}"))
                            .unwrap_or_default();
                        let r = adw::ActionRow::builder()
                            .title(&s.connector)
                            .subtitle(format!("{st}{tier}"))
                            .build();
                        sources_group.add(&r);
                    }
                }
            }
            Err(e) => {
                status_lbl.set_label(&format!("Ошибка: {}", e.user_message()));
            }
        },
    );
}

pub fn show_share_dialog(
    parent: &impl IsA<gtk4::Widget>,
    core: Arc<KwhaleCore>,
    rt: tokio::runtime::Handle,
    kind: ShareKind,
    entity_id: &str,
    entity_title: &str,
    show_toast: Rc<dyn Fn(&str)>,
) {
    let dialog = adw::Dialog::builder()
        .title(format!("Поделиться: {entity_title}"))
        .content_width(440)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let status_lbl = gtk4::Label::builder()
        .label("Создание публичной ссылки…")
        .xalign(0.0)
        .css_classes(["kw-muted"])
        .build();
    vbox.append(&status_lbl);

    let group = adw::PreferencesGroup::builder()
        .title("Ссылки KWhale")
        .build();
    vbox.append(&group);

    let platforms_group = adw::PreferencesGroup::builder()
        .title("Внешние платформы")
        .build();
    vbox.append(&platforms_group);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&vbox));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));

    let id_owned = entity_id.to_string();
    spawn_async(
        &rt,
        async move { core.api.create_share(kind, &id_owned).await },
        move |res: CoreResult<ShareLink>| match res {
            Ok(link) => {
                status_lbl.set_label(&format!(
                    "Действует {} дн. • Переходов: {}",
                    link.ttl_days, link.clicks_total
                ));
                let add_copy_row =
                    |g: &adw::PreferencesGroup, title: &str, url: String, toast: Rc<dyn Fn(&str)>| {
                        let r = adw::ActionRow::builder()
                            .title(title)
                            .subtitle(&url)
                            .activatable(true)
                            .build();
                        let btn = gtk4::Button::builder()
                            .icon_name("edit-copy-symbolic")
                            .valign(gtk4::Align::Center)
                            .css_classes(["flat"])
                            .tooltip_text("Копировать")
                            .build();
                        let u2 = url.clone();
                        let t2 = toast.clone();
                        btn.connect_clicked(move |b| {
                            b.clipboard().set_text(&u2);
                            t2("Ссылка скопирована в буфер обмена");
                        });
                        r.add_suffix(&btn);
                        r.connect_activated(move |row| {
                            row.clipboard().set_text(&url);
                            toast("Ссылка скопирована в буфер обмена");
                        });
                        g.add(&r);
                    };

                add_copy_row(
                    &group,
                    "Веб-карточка (Share URL)",
                    link.share_url.clone(),
                    show_toast.clone(),
                );
                add_copy_row(
                    &group,
                    "Deep Link (kwhale://)",
                    link.deep_link.clone(),
                    show_toast.clone(),
                );

                for p in link.platforms {
                    add_copy_row(
                        &platforms_group,
                        &p.label,
                        p.url,
                        show_toast.clone(),
                    );
                }
            }
            Err(e) => {
                status_lbl.set_label(&format!("Не удалось создать ссылку: {}", e.user_message()));
            }
        },
    );
}

pub fn show_shared_link_sheet(
    parent: &impl IsA<gtk4::Widget>,
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    rt: tokio::runtime::Handle,
    share_id: &str,
    cbs: DialogCallbacks,
) {
    let sid = share_id.to_string();
    let parent_widget = parent.upcast_ref::<gtk4::Widget>().clone();
    let c2 = core.clone();
    let rt2 = rt.clone();
    spawn_async(
        &rt,
        async move { core.api.share_details(&sid).await },
        move |res| match res {
            Ok(link) => match link.kind {
                ShareKind::Album => (cbs.open_album)(link.entity_id),
                ShareKind::Artist => (cbs.open_artist)(link.entity_id),
                ShareKind::Track => {
                    let eid = link.entity_id.clone();
                    let c3 = c2.clone();
                    let eng2 = engine.clone();
                    let toast = cbs.show_toast.clone();
                    let _ = &parent_widget;
                    spawn_async(
                        &rt2,
                        async move { c3.api.track(&eid).await },
                        move |tr_res| match tr_res {
                            Ok(t) => {
                                let ui = t.to_ui(c2.downloads.is_downloaded(&t.id));
                                eng2.play_queue(vec![ui], 0, "player", None);
                                toast(&format!("Воспроизводится общий трек: {}", t.title));
                            }
                            Err(e) => toast(&e.user_message()),
                        },
                    );
                }
            },
            Err(e) => (cbs.show_toast)(&format!("Ссылка недоступна: {}", e.user_message())),
        },
    );
}

pub fn show_add_to_playlist_dialog(
    parent: &impl IsA<gtk4::Widget>,
    core: Arc<KwhaleCore>,
    rt: tokio::runtime::Handle,
    track: UiTrack,
    cbs: DialogCallbacks,
) {
    let dialog = adw::Dialog::builder()
        .title("Добавить в плейлист")
        .content_width(420)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let new_btn = gtk4::Button::builder()
        .label("+ Создать новый плейлист")
        .css_classes(["suggested-action", "pill"])
        .build();
    vbox.append(&new_btn);

    let list = gtk4::ListBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .min_content_height(260)
        .child(&list)
        .build();
    vbox.append(&scroller);

    {
        let parent_w = parent.upcast_ref::<gtk4::Widget>().clone();
        let dlg_weak = dialog.downgrade();
        let c = core.clone();
        let rt_h = rt.clone();
        let tr = track.clone();
        let cbs2 = cbs.clone();
        new_btn.connect_clicked(move |_| {
            if let Some(d) = dlg_weak.upgrade() {
                d.close();
            }
            show_create_or_edit_playlist_dialog(
                &parent_w,
                c.clone(),
                rt_h.clone(),
                None,
                Some(tr.remote_id.clone()),
                cbs2.clone(),
            );
        });
    }

    let c2 = core.clone();
    let rt2 = rt.clone();
    let dlg_weak = dialog.downgrade();
    let tid = track.remote_id.clone();
    let toast = cbs.show_toast.clone();
    let reload = cbs.reload_library.clone();
    spawn_async(
        &rt,
        async move { c2.api.library_playlists(None, Some(100)).await },
        move |res| match res {
            Ok(page) => {
                for pl in page.items {
                    let row = adw::ActionRow::builder()
                        .title(&pl.title)
                        .subtitle(format!("{} треков", pl.track_count))
                        .activatable(true)
                        .build();
                    row.add_prefix(&gtk4::Image::from_icon_name("playlist-symbolic"));
                    let pid = pl.id.clone();
                    let ptitle = pl.title.clone();
                    let c3 = core.clone();
                    let rt3 = rt2.clone();
                    let tid2 = tid.clone();
                    let toast2 = toast.clone();
                    let reload2 = reload.clone();
                    let dw = dlg_weak.clone();
                    row.connect_activated(move |_| {
                        if let Some(d) = dw.upgrade() {
                            d.close();
                        }
                        let c4 = c3.clone();
                        let c_after = c3.clone();
                        let pid2 = pid.clone();
                        let pid_after = pid.clone();
                        let tid3 = tid2.clone();
                        let ptitle2 = ptitle.clone();
                        let toast3 = toast2.clone();
                        let reload3 = reload2.clone();
                        spawn_async(
                            &rt3,
                            async move {
                                c4.api
                                    .add_playlist_tracks(&pid2, vec![tid3], None)
                                    .await
                            },
                            move |add_res| match add_res {
                                Ok(_) => {
                                    c_after.cache.invalidate_collection("playlist", &pid_after);
                                    c_after.cache.invalidate_library();
                                    reload3();
                                    toast3(&format!("Добавлено в «{ptitle2}»"));
                                }
                                Err(e) => toast3(&e.user_message()),
                            },
                        );
                    });
                    list.append(&row);
                }
            }
            Err(e) => toast(&e.user_message()),
        },
    );

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&vbox));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

pub fn show_create_or_edit_playlist_dialog(
    parent: &impl IsA<gtk4::Widget>,
    core: Arc<KwhaleCore>,
    rt: tokio::runtime::Handle,
    existing: Option<Playlist>,
    initial_track_id: Option<String>,
    cbs: DialogCallbacks,
) {
    let is_edit = existing.is_some();
    let dialog = adw::Dialog::builder()
        .title(if is_edit {
            "Редактировать плейлист"
        } else {
            "Новый плейлист"
        })
        .content_width(420)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let group = adw::PreferencesGroup::new();
    let title_row = adw::EntryRow::builder()
        .title("Название плейлиста")
        .text(
            existing
                .as_ref()
                .map(|p| p.title.as_str())
                .unwrap_or_default(),
        )
        .build();
    let desc_row = adw::EntryRow::builder()
        .title("Описание (необязательно)")
        .text(
            existing
                .as_ref()
                .and_then(|p| p.description.as_deref())
                .unwrap_or_default(),
        )
        .build();
    let public_switch = adw::SwitchRow::builder()
        .title("Публичный плейлист")
        .active(
            existing
                .as_ref()
                .map(|p| p.visibility == "public")
                .unwrap_or(false),
        )
        .build();
    group.add(&title_row);
    group.add(&desc_row);
    group.add(&public_switch);
    vbox.append(&group);

    let save_btn = gtk4::Button::builder()
        .label(if is_edit { "Сохранить" } else { "Создать" })
        .css_classes(["suggested-action", "pill"])
        .build();
    vbox.append(&save_btn);

    let dlg_weak = dialog.downgrade();
    save_btn.connect_clicked(move |_| {
        let title = title_row.text().trim().to_string();
        if title.is_empty() {
            (cbs.show_toast)("Введите название плейлиста");
            return;
        }
        let desc = {
            let d = desc_row.text().trim().to_string();
            if d.is_empty() { None } else { Some(d) }
        };
        let vis = if public_switch.is_active() {
            "public".to_string()
        } else {
            "private".to_string()
        };
        if let Some(d) = dlg_weak.upgrade() {
            d.close();
        }
        let c = core.clone();
        let c_after = core.clone();
        let toast = cbs.show_toast.clone();
        let reload = cbs.reload_library.clone();
        let open_pl = cbs.open_playlist.clone();
        let init_tid = initial_track_id.clone();

        if let Some(ex) = existing.clone() {
            let pid = ex.id.clone();
            spawn_async(
                &rt,
                async move {
                    c.api
                        .update_playlist(
                            &pid,
                            &PlaylistUpdate {
                                title: Some(title),
                                description: desc,
                                visibility: Some(vis),
                            },
                        )
                        .await
                },
                move |res| match res {
                    Ok(pl) => {
                        c_after.cache.invalidate_collection("playlist", &pl.id);
                        c_after.cache.invalidate_library();
                        reload();
                        open_pl(pl.id);
                        toast("Плейлист обновлён");
                    }
                    Err(e) => toast(&e.user_message()),
                },
            );
        } else {
            spawn_async(
                &rt,
                async move {
                    let pl = c
                        .api
                        .create_playlist(&PlaylistCreate {
                            title,
                            description: desc,
                            visibility: vis,
                        })
                        .await?;
                    if let Some(tid) = init_tid {
                        let _ = c
                            .api
                            .add_playlist_tracks(&pl.id, vec![tid], None)
                            .await;
                    }
                    Ok::<Playlist, kwhale_core::CoreError>(pl)
                },
                move |res| match res {
                    Ok(pl) => {
                        c_after.cache.invalidate_library();
                        reload();
                        open_pl(pl.id);
                        toast("Плейлист создан");
                    }
                    Err(e) => toast(&e.user_message()),
                },
            );
        }
    });

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&vbox));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

pub fn show_upload_track_dialog(
    window: &(impl IsA<gtk4::Window> + IsA<gtk4::Widget>),
    core: Arc<KwhaleCore>,
    rt: tokio::runtime::Handle,
    initial_file: Option<PathBuf>,
    cbs: DialogCallbacks,
) {
    let dialog = adw::Dialog::builder()
        .title("Загрузить трек на сервер")
        .content_width(440)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let selected_path: Rc<std::cell::RefCell<Option<PathBuf>>> =
        Rc::new(std::cell::RefCell::new(initial_file.clone()));

    let file_row = adw::ActionRow::builder()
        .title("Аудиофайл")
        .subtitle(
            initial_file
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("Файл не выбран (MP3, FLAC, M4A, OGG, WAV)"),
        )
        .build();
    let pick_btn = gtk4::Button::builder()
        .label("Выбрать…")
        .valign(gtk4::Align::Center)
        .build();
    file_row.add_suffix(&pick_btn);

    let group = adw::PreferencesGroup::new();
    group.add(&file_row);

    let initial_stem = initial_file
        .as_ref()
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let title_row = adw::EntryRow::builder()
        .title("Название (необязательно)")
        .text(&initial_stem)
        .build();
    let artist_row = adw::EntryRow::builder()
        .title("Артист (необязательно)")
        .build();
    let album_row = adw::EntryRow::builder()
        .title("Альбом (необязательно)")
        .build();
    group.add(&title_row);
    group.add(&artist_row);
    group.add(&album_row);
    vbox.append(&group);

    let upload_btn = gtk4::Button::builder()
        .label("Загрузить в облако KWhale")
        .css_classes(["suggested-action", "pill"])
        .build();
    vbox.append(&upload_btn);

    {
        let win = window.upcast_ref::<gtk4::Window>().clone();
        let sp = selected_path.clone();
        let f_row = file_row.clone();
        let t_row = title_row.clone();
        pick_btn.connect_clicked(move |_| {
            let filter = gtk4::FileFilter::new();
            filter.set_name(Some("Аудиофайлы"));
            for pat in ["*.mp3", "*.flac", "*.m4a", "*.ogg", "*.opus", "*.wav", "*.aac"] {
                filter.add_pattern(pat);
            }
            let filters = gio::ListStore::new::<gtk4::FileFilter>();
            filters.append(&filter);
            let fd = gtk4::FileDialog::builder()
                .title("Выберите аудиофайл")
                .filters(&filters)
                .build();
            let sp2 = sp.clone();
            let f_row2 = f_row.clone();
            let t_row2 = t_row.clone();
            fd.open(Some(&win), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                            f_row2.set_subtitle(name);
                        }
                        if t_row2.text().trim().is_empty() {
                            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                t_row2.set_text(stem);
                            }
                        }
                        *sp2.borrow_mut() = Some(path);
                    }
                }
            });
        });
    }

    let dlg_weak = dialog.downgrade();
    upload_btn.connect_clicked(move |btn| {
        let Some(path) = selected_path.borrow().clone() else {
            (cbs.show_toast)("Сначала выберите аудиофайл");
            return;
        };
        btn.set_sensitive(false);
        btn.set_label("Загрузка…");
        let title_opt = {
            let s = title_row.text().trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        let artist_opt = {
            let s = artist_row.text().trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        let album_opt = {
            let s = album_row.text().trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        let c = core.clone();
        let c_after = core.clone();
        let toast = cbs.show_toast.clone();
        let reload = cbs.reload_library.clone();
        let dw = dlg_weak.clone();
        spawn_async(
            &rt,
            async move {
                c.api
                    .upload_track(
                        &path,
                        title_opt.as_deref(),
                        artist_opt.as_deref(),
                        album_opt.as_deref(),
                    )
                    .await
            },
            move |res: CoreResult<Track>| match res {
                Ok(t) => {
                    if let Some(d) = dw.upgrade() {
                        d.close();
                    }
                    c_after.cache.invalidate_library();
                    c_after.cache.invalidate_home();
                    c_after.cache.invalidate_profile();
                    reload();
                    toast(&format!("Трек «{}» успешно загружен", t.title));
                }
                Err(e) => {
                    toast(&format!("Ошибка загрузки: {}", e.user_message()));
                }
            },
        );
    });

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&vbox));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(window));
}
