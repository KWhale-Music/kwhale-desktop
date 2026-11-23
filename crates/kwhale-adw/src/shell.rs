use crate::dialogs::{
    show_create_or_edit_playlist_dialog, show_shared_link_sheet, show_track_actions_dialog,
    show_track_info_dialog, show_upload_track_dialog, DialogCallbacks,
};
use crate::style::{apply_theme_css, make_cover_picture, set_placeholder_on_picture, spawn_async};
use crate::views::{
    build_appearance_page, build_collection_page, build_downloads_page, build_history_page,
    build_home_page, build_library_page, build_profile_page, build_search_page,
    build_settings_page, CollectionTarget, NavCallbacks,
};
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use kwhale_audio::AudioEngine;
use kwhale_core::{
    format_duration_player_ms, DeepLink, KwhaleCore, Lyrics, PlaybackState, Playlist, RepeatMode,
    ServerUrl,
};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

pub struct AdwWindowHandle {
    pub window: adw::ApplicationWindow,
    pub handle_deep_link: Rc<dyn Fn(DeepLink)>,
}

pub fn build_main_window(
    app: &adw::Application,
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    rt: tokio::runtime::Handle,
    initial_links: Vec<DeepLink>,
) -> AdwWindowHandle {
    let snap = core.settings.snapshot();

    // Global CSS Provider for dynamic KWhale theme (129 accents + Light/Dark/AMOLED/System)
    let css_provider = gtk4::CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &css_provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    apply_theme_css(
        &css_provider,
        snap.theme_mode,
        snap.accent_id,
        snap.follow_system_accent,
    );

    // Subscribe to live theme changes
    {
        let mut theme_rx = core.subscribe_theme();
        let provider = css_provider.clone();
        let (tx, rx) = std::sync::mpsc::channel::<(kwhale_core::ThemeMode, i32, bool)>();
        rt.spawn(async move {
            while theme_rx.changed().await.is_ok() {
                let val = *theme_rx.borrow();
                if tx.send(val).is_err() {
                    break;
                }
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
            while let Ok((mode, aid, sys)) = rx.try_recv() {
                apply_theme_css(&provider, mode, aid, sys);
            }
            glib::ControlFlow::Continue
        });
    }

    let toast_overlay = adw::ToastOverlay::new();
    let root_stack = gtk4::Stack::builder()
        .transition_type(gtk4::StackTransitionType::Crossfade)
        .vexpand(true)
        .hexpand(true)
        .build();
    toast_overlay.set_child(Some(&root_stack));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("KWhale")
        .default_width(snap.window_width.max(980))
        .default_height(snap.window_height.max(660))
        .content(&toast_overlay)
        .build();

    // Persist window dimensions on close
    {
        let c = core.clone();
        window.connect_close_request(move |win| {
            let w = win.width();
            let h = win.height();
            if w >= 640 && h >= 480 {
                c.settings.update(|s| {
                    s.window_width = w;
                    s.window_height = h;
                });
            }
            glib::Propagation::Proceed
        });
    }

    let show_toast: Rc<dyn Fn(&str)> = {
        let to = toast_overlay.clone();
        Rc::new(move |msg: &str| {
            let t = adw::Toast::new(msg);
            t.set_timeout(3);
            to.add_toast(t);
        })
    };

    // Auth view vs Main workspace stack
    let auth_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    let main_overlay = gtk4::Overlay::new();
    root_stack.add_named(&auth_box, Some("auth"));
    root_stack.add_named(&main_overlay, Some("workspace"));

    let populate_workspace: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let deep_link_handler: Rc<RefCell<Option<Rc<dyn Fn(DeepLink)>>>> =
        Rc::new(RefCell::new(None));

    // Build Auth screen
    build_auth_view(
        &auth_box,
        core.clone(),
        rt.clone(),
        show_toast.clone(),
        {
            let rs = root_stack.clone();
            let pw = populate_workspace.clone();
            Rc::new(move || {
                if let Some(f) = pw.borrow().as_ref() {
                    f();
                }
                rs.set_visible_child_name("workspace");
            })
        },
    );

    let on_logout: Rc<dyn Fn()> = {
        let rs = root_stack.clone();
        let eng = engine.clone();
        let toast = show_toast.clone();
        Rc::new(move || {
            eng.stop();
            rs.set_visible_child_name("auth");
            toast("Вы вышли из аккаунта");
        })
    };

    // Build Main Desktop 3-Zone Workspace inside `main_overlay`
    let build_ws: Rc<dyn Fn()> = {
        let main_overlay = main_overlay.clone();
        let window = window.clone();
        let core = core.clone();
        let engine = engine.clone();
        let rt = rt.clone();
        let show_toast = show_toast.clone();
        let on_logout = on_logout.clone();
        let dl_slot = deep_link_handler.clone();

        Rc::new(move || {
            let shell = populate_desktop_workspace(
                &main_overlay,
                &window,
                core.clone(),
                engine.clone(),
                rt.clone(),
                show_toast.clone(),
                on_logout.clone(),
            );
            *dl_slot.borrow_mut() = Some(shell);
        })
    };
    *populate_workspace.borrow_mut() = Some(build_ws.clone());

    if core.api.session().is_some() {
        build_ws();
        root_stack.set_visible_child_name("workspace");
    } else {
        root_stack.set_visible_child_name("auth");
    }

    let handle_deep_link: Rc<dyn Fn(DeepLink)> = {
        let dl_slot = deep_link_handler;
        Rc::new(move |link: DeepLink| {
            if let Some(h) = dl_slot.borrow().as_ref() {
                h(link);
            }
        })
    };

    for link in initial_links {
        handle_deep_link(link);
    }

    AdwWindowHandle {
        window,
        handle_deep_link,
    }
}

fn build_auth_view(
    container: &gtk4::Box,
    core: Arc<KwhaleCore>,
    rt: tokio::runtime::Handle,
    show_toast: Rc<dyn Fn(&str)>,
    on_authenticated: Rc<dyn Fn()>,
) {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    let clamp = adw::Clamp::builder()
        .maximum_size(460)
        .margin_top(32)
        .margin_bottom(32)
        .margin_start(20)
        .margin_end(20)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 18);
    vbox.set_valign(gtk4::Align::Center);

    let title = gtk4::Label::builder()
        .label("KWhale Music")
        .css_classes(["kw-hero-title"])
        .build();
    let sub = gtk4::Label::builder()
        .label("Войдите в свой self-hosted сервер или зарегистрируйтесь по инвайт-токену")
        .wrap(true)
        .justify(gtk4::Justification::Center)
        .css_classes(["kw-muted"])
        .build();
    vbox.append(&title);
    vbox.append(&sub);

    let snap = core.settings.snapshot();
    let srv_group = adw::PreferencesGroup::builder()
        .title("Подключение к серверу")
        .build();
    let url_row = adw::EntryRow::builder()
        .title("Адрес сервера (https://...)")
        .text(&snap.api_base_url)
        .build();
    let http_switch = adw::SwitchRow::builder()
        .title("Разрешить HTTP вне локальной сети")
        .active(snap.allow_insecure_http)
        .build();
    srv_group.add(&url_row);
    srv_group.add(&http_switch);
    vbox.append(&srv_group);

    let mode_reg = Rc::new(Cell::new(false));
    let mode_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    mode_row.set_halign(gtk4::Align::Center);
    let login_tab = gtk4::Button::builder()
        .label("Вход")
        .css_classes(["kw-chip", "kw-chip-active"])
        .build();
    let reg_tab = gtk4::Button::builder()
        .label("Регистрация по инвайту")
        .css_classes(["kw-chip"])
        .build();
    mode_row.append(&login_tab);
    mode_row.append(&reg_tab);
    vbox.append(&mode_row);

    let creds_group = adw::PreferencesGroup::new();
    let name_row = adw::EntryRow::builder()
        .title("Имя пользователя")
        .visible(false)
        .build();
    let email_row = adw::EntryRow::builder().title("Email").build();
    let pass_row = adw::PasswordEntryRow::builder()
        .title("Пароль (не менее 8 символов)")
        .build();
    let invite_row = adw::EntryRow::builder()
        .title("Инвайт-токен")
        .visible(false)
        .build();
    creds_group.add(&name_row);
    creds_group.add(&email_row);
    creds_group.add(&pass_row);
    creds_group.add(&invite_row);
    vbox.append(&creds_group);

    let err_lbl = gtk4::Label::builder()
        .visible(false)
        .wrap(true)
        .css_classes(["kw-error-text"])
        .build();
    vbox.append(&err_lbl);

    let submit_btn = gtk4::Button::builder()
        .label("Войти в KWhale")
        .css_classes(["suggested-action", "pill"])
        .build();
    vbox.append(&submit_btn);

    {
        let mr = mode_reg.clone();
        let nr = name_row.clone();
        let ir = invite_row.clone();
        let sb = submit_btn.clone();
        let lt = login_tab.clone();
        let rt_btn = reg_tab.clone();
        login_tab.connect_clicked(move |_| {
            mr.set(false);
            nr.set_visible(false);
            ir.set_visible(false);
            sb.set_label("Войти в KWhale");
            lt.add_css_class("kw-chip-active");
            rt_btn.remove_css_class("kw-chip-active");
        });
    }
    {
        let mr = mode_reg.clone();
        let nr = name_row.clone();
        let ir = invite_row.clone();
        let sb = submit_btn.clone();
        let lt = login_tab;
        let rt_btn = reg_tab.clone();
        reg_tab.connect_clicked(move |_| {
            mr.set(true);
            nr.set_visible(true);
            ir.set_visible(true);
            sb.set_label("Зарегистрироваться");
            rt_btn.add_css_class("kw-chip-active");
            lt.remove_css_class("kw-chip-active");
        });
    }

    submit_btn.connect_clicked(move |btn| {
        err_lbl.set_visible(false);
        let raw_url = url_row.text().trim().to_string();
        let Some(norm_url) = ServerUrl::normalize(&raw_url, None) else {
            err_lbl.set_label("Введите корректный адрес сервера");
            err_lbl.set_visible(true);
            return;
        };
        if let Err(e) = core.reconfigure_api(&norm_url, http_switch.is_active()) {
            err_lbl.set_label(&e.user_message());
            err_lbl.set_visible(true);
            return;
        }
        let email = email_row.text().trim().to_string();
        let password = pass_row.text().to_string();
        if !email.contains('@') || password.len() < 8 {
            err_lbl.set_label("Введите корректный Email и пароль не короче 8 символов");
            err_lbl.set_visible(true);
            return;
        }
        let is_register = mode_reg.get();
        let display_name = name_row.text().trim().to_string();
        let invite_token = invite_row.text().trim().to_string();
        if is_register && (display_name.is_empty() || invite_token.is_empty()) {
            err_lbl.set_label("Для регистрации укажите имя и инвайт-токен");
            err_lbl.set_visible(true);
            return;
        }

        btn.set_sensitive(false);
        let c2 = core.clone();
        let c3 = core.clone();
        let err2 = err_lbl.clone();
        let bw = btn.downgrade();
        let on_auth = on_authenticated.clone();
        let toast = show_toast.clone();
        spawn_async(
            &rt,
            async move {
                if is_register {
                    c2.api
                        .register(&email, &password, &display_name, &invite_token)
                        .await
                } else {
                    c2.api.login(&email, &password).await
                }
            },
            move |res| {
                if let Some(b) = bw.upgrade() {
                    b.set_sensitive(true);
                }
                match res {
                    Ok(sess) => {
                        c3.notify_auth_user(Some(sess.user.clone()));
                        toast(&format!("Добро пожаловать, {}!", sess.user.display_name));
                        on_auth();
                    }
                    Err(e) => {
                        err2.set_label(&e.user_message());
                        err2.set_visible(true);
                    }
                }
            },
        );
    });

    clamp.set_child(Some(&vbox));
    let scroller = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    toolbar.set_content(Some(&scroller));
    container.append(&toolbar);
}

fn populate_desktop_workspace(
    main_overlay: &gtk4::Overlay,
    window: &adw::ApplicationWindow,
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    rt: tokio::runtime::Handle,
    show_toast: Rc<dyn Fn(&str)>,
    on_logout: Rc<dyn Fn()>,
) -> Rc<dyn Fn(DeepLink)> {
    let outer_vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    outer_vbox.set_vexpand(true);
    outer_vbox.set_hexpand(true);

    // Top main 3-column horizontal area: [Left Sidebar] | [Center Stack] | [Right Inspector]
    let middle_hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    middle_hbox.set_vexpand(true);
    middle_hbox.set_hexpand(true);
    outer_vbox.append(&middle_hbox);

    // 1. Left Persistent Sidebar (250px)
    let sidebar = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    sidebar.set_size_request(250, -1);
    sidebar.add_css_class("kw-sidebar");

    let sb_header = adw::HeaderBar::builder()
        .show_end_title_buttons(false)
        .title_widget(&gtk4::Label::builder().label("KWhale").css_classes(["heading"]).build())
        .build();
    let back_btn = gtk4::Button::builder()
        .icon_name("go-previous-symbolic")
        .tooltip_text("Назад")
        .visible(false)
        .build();
    sb_header.pack_start(&back_btn);
    sidebar.append(&sb_header);

    let nav_list = gtk4::ListBox::builder()
        .selection_mode(gtk4::SelectionMode::Single)
        .css_classes(["navigation-sidebar"])
        .build();

    let nav_items = [
        ("home", "user-home-symbolic", "Главная"),
        ("search", "system-search-symbolic", "Поиск"),
        ("library", "folder-music-symbolic", "Библиотека"),
        ("downloads", "folder-download-symbolic", "Загрузки"),
        ("history", "document-open-recent-symbolic", "История"),
        ("profile", "avatar-default-symbolic", "Профиль"),
        ("appearance", "applications-graphics-symbolic", "Оформление"),
        ("settings", "emblem-system-symbolic", "Настройки"),
    ];
    for (id, icon, title) in nav_items {
        let row = gtk4::ListBoxRow::new();
        row.set_widget_name(id);
        let h = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
        h.set_margin_top(6);
        h.set_margin_bottom(6);
        h.set_margin_start(10);
        h.set_margin_end(10);
        h.append(&gtk4::Image::from_icon_name(icon));
        h.append(&gtk4::Label::builder().label(title).xalign(0.0).build());
        row.set_child(Some(&h));
        nav_list.append(&row);
    }
    sidebar.append(&nav_list);

    // Sidebar Playlists header + dynamic list
    let pl_hdr = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    pl_hdr.set_margin_start(14);
    pl_hdr.set_margin_end(10);
    pl_hdr.set_margin_top(8);
    pl_hdr.append(
        &gtk4::Label::builder()
            .label("МОИ ПЛЕЙЛИСТЫ")
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["caption", "kw-muted"])
            .build(),
    );
    let sb_new_pl_btn = gtk4::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text("Создать плейлист")
        .css_classes(["flat", "circular"])
        .build();
    pl_hdr.append(&sb_new_pl_btn);
    sidebar.append(&pl_hdr);

    let sidebar_playlists_box = gtk4::ListBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .css_classes(["navigation-sidebar"])
        .build();
    let sb_pl_scroll = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&sidebar_playlists_box)
        .build();
    sidebar.append(&sb_pl_scroll);
    middle_hbox.append(&sidebar);

    // 2. Center Content Area (ToolbarView + Stack + Detail Container)
    let center_toolbar = adw::ToolbarView::new();
    center_toolbar.set_hexpand(true);
    center_toolbar.set_vexpand(true);
    let center_header = adw::HeaderBar::new();
    center_toolbar.add_top_bar(&center_header);

    let center_stack = gtk4::Stack::builder()
        .transition_type(gtk4::StackTransitionType::Crossfade)
        .vexpand(true)
        .hexpand(true)
        .build();
    let detail_slot = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    center_stack.add_named(&detail_slot, Some("detail"));
    center_toolbar.set_content(Some(&center_stack));
    middle_hbox.append(&center_toolbar);

    // 3. Right Collapsible Inspector Panel (Queue + Synchronized Karaoke Lyrics)
    let inspector_revealer = gtk4::Revealer::builder()
        .transition_type(gtk4::RevealerTransitionType::SlideLeft)
        .reveal_child(false)
        .build();
    let inspector_box = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    inspector_box.set_size_request(340, -1);
    inspector_box.add_css_class("kw-inspector");

    let insp_tabs = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    insp_tabs.set_margin_top(10);
    insp_tabs.set_margin_start(12);
    insp_tabs.set_margin_end(12);
    let queue_tab_btn = gtk4::Button::builder()
        .label("Очередь")
        .hexpand(true)
        .css_classes(["kw-chip", "kw-chip-active"])
        .build();
    let lyrics_tab_btn = gtk4::Button::builder()
        .label("Текст песни")
        .hexpand(true)
        .css_classes(["kw-chip"])
        .build();
    let close_insp_btn = gtk4::Button::builder()
        .icon_name("window-close-symbolic")
        .css_classes(["flat", "circular"])
        .build();
    insp_tabs.append(&queue_tab_btn);
    insp_tabs.append(&lyrics_tab_btn);
    insp_tabs.append(&close_insp_btn);
    inspector_box.append(&insp_tabs);

    let inspector_stack = gtk4::Stack::builder()
        .transition_type(gtk4::StackTransitionType::Crossfade)
        .vexpand(true)
        .build();
    let queue_list_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    queue_list_box.set_margin_start(10);
    queue_list_box.set_margin_end(10);
    queue_list_box.set_margin_bottom(12);
    let queue_scroll = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&queue_list_box)
        .build();

    let lyrics_list_box = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    lyrics_list_box.set_margin_start(12);
    lyrics_list_box.set_margin_end(12);
    lyrics_list_box.set_margin_bottom(16);
    let lyrics_scroll = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&lyrics_list_box)
        .build();

    inspector_stack.add_named(&queue_scroll, Some("queue"));
    inspector_stack.add_named(&lyrics_scroll, Some("lyrics"));
    inspector_box.append(&inspector_stack);
    inspector_revealer.set_child(Some(&inspector_box));
    middle_hbox.append(&inspector_revealer);

    {
        let istack = inspector_stack.clone();
        let qb = queue_tab_btn.clone();
        let lb = lyrics_tab_btn.clone();
        queue_tab_btn.connect_clicked(move |_| {
            istack.set_visible_child_name("queue");
            qb.add_css_class("kw-chip-active");
            lb.remove_css_class("kw-chip-active");
        });
    }
    {
        let istack = inspector_stack.clone();
        let qb = queue_tab_btn.clone();
        let lb = lyrics_tab_btn.clone();
        lyrics_tab_btn.connect_clicked(move |_| {
            istack.set_visible_child_name("lyrics");
            lb.add_css_class("kw-chip-active");
            qb.remove_css_class("kw-chip-active");
        });
    }
    {
        let rev = inspector_revealer.clone();
        close_insp_btn.connect_clicked(move |_| rev.set_reveal_child(false));
    }

    let toggle_inspector_tab: Rc<dyn Fn(&str)> = {
        let rev = inspector_revealer.clone();
        let istack = inspector_stack.clone();
        let qb = queue_tab_btn;
        let lb = lyrics_tab_btn;
        Rc::new(move |tab: &str| {
            let currently_open = rev.reveals_child();
            let current_tab = istack
                .visible_child_name()
                .map(|s| s.to_string())
                .unwrap_or_default();
            if currently_open && current_tab == tab {
                rev.set_reveal_child(false);
            } else {
                istack.set_visible_child_name(tab);
                if tab == "lyrics" {
                    lb.add_css_class("kw-chip-active");
                    qb.remove_css_class("kw-chip-active");
                } else {
                    qb.add_css_class("kw-chip-active");
                    lb.remove_css_class("kw-chip-active");
                }
                rev.set_reveal_child(true);
            }
        })
    };

    // Navigation callbacks wiring
    let last_primary_section = Rc::new(RefCell::new("home".to_string()));
    let reload_library_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let open_collection_slot: Rc<RefCell<Option<Rc<dyn Fn(CollectionTarget)>>>> =
        Rc::new(RefCell::new(None));

    let switch_section: Rc<dyn Fn(&str)> = {
        let cs = center_stack.clone();
        let bb = back_btn.clone();
        let lps = last_primary_section.clone();
        Rc::new(move |sec: &str| {
            *lps.borrow_mut() = sec.to_string();
            bb.set_visible(false);
            cs.set_visible_child_name(sec);
        })
    };

    {
        let cs = center_stack.clone();
        let lps = last_primary_section.clone();
        back_btn.connect_clicked(move |btn| {
            btn.set_visible(false);
            let prev = lps.borrow().clone();
            cs.set_visible_child_name(&prev);
        });
    }

    let dialog_cbs = DialogCallbacks {
        show_toast: show_toast.clone(),
        open_artist: {
            let oc = open_collection_slot.clone();
            Rc::new(move |id| {
                if let Some(f) = oc.borrow().as_ref() {
                    f(CollectionTarget::Artist(id));
                }
            })
        },
        open_album: {
            let oc = open_collection_slot.clone();
            Rc::new(move |id| {
                if let Some(f) = oc.borrow().as_ref() {
                    f(CollectionTarget::Album(id));
                }
            })
        },
        open_playlist: {
            let oc = open_collection_slot.clone();
            Rc::new(move |id| {
                if let Some(f) = oc.borrow().as_ref() {
                    f(CollectionTarget::Playlist(id));
                }
            })
        },
        open_lyrics: {
            let ti = toggle_inspector_tab.clone();
            Rc::new(move || ti("lyrics"))
        },
        reload_library: {
            let rl = reload_library_slot.clone();
            Rc::new(move || {
                if let Some(f) = rl.borrow().as_ref() {
                    f();
                }
            })
        },
    };

    let nav_cbs = NavCallbacks {
        dialogs: dialog_cbs.clone(),
        open_collection: {
            let oc = open_collection_slot.clone();
            Rc::new(move |target| {
                if let Some(f) = oc.borrow().as_ref() {
                    f(target);
                }
            })
        },
        switch_section: switch_section.clone(),
        on_logout,
        window: window.clone(),
    };

    let open_collection: Rc<dyn Fn(CollectionTarget)> = {
        let detail_slot = detail_slot.clone();
        let cs = center_stack.clone();
        let bb = back_btn.clone();
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let nav_c = nav_cbs.clone();
        Rc::new(move |target: CollectionTarget| {
            while let Some(ch) = detail_slot.first_child() {
                detail_slot.remove(&ch);
            }
            let page =
                build_collection_page(c.clone(), eng.clone(), rt_h.clone(), target, nav_c.clone());
            detail_slot.append(&page);
            bb.set_visible(true);
            cs.set_visible_child_name("detail");
        })
    };
    *open_collection_slot.borrow_mut() = Some(open_collection.clone());

    // Populate sidebar playlists callback
    let update_sidebar_playlists: Rc<dyn Fn(Vec<Playlist>)> = {
        let sb_list = sidebar_playlists_box.clone();
        let open_col = open_collection.clone();
        Rc::new(move |playlists: Vec<Playlist>| {
            while let Some(ch) = sb_list.first_child() {
                sb_list.remove(&ch);
            }
            for pl in playlists {
                let btn = gtk4::Button::builder()
                    .css_classes(["flat"])
                    .build();
                let h = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                h.append(&gtk4::Image::from_icon_name("playlist-symbolic"));
                h.append(
                    &gtk4::Label::builder()
                        .label(&pl.title)
                        .xalign(0.0)
                        .ellipsize(gtk4::pango::EllipsizeMode::End)
                        .hexpand(true)
                        .build(),
                );
                btn.set_child(Some(&h));
                let pid = pl.id.clone();
                let oc = open_col.clone();
                btn.connect_clicked(move |_| oc(CollectionTarget::Playlist(pid.clone())));
                sb_list.append(&btn);
            }
        })
    };

    {
        let win = window.clone();
        let c = core.clone();
        let rt_h = rt.clone();
        let dcbs = dialog_cbs.clone();
        sb_new_pl_btn.connect_clicked(move |_| {
            show_create_or_edit_playlist_dialog(&win, c.clone(), rt_h.clone(), None, None, dcbs.clone());
        });
    }

    // Build all primary pages into `center_stack`
    let (home_page, _reload_home) =
        build_home_page(core.clone(), engine.clone(), rt.clone(), nav_cbs.clone());
    let (search_page, search_entry, _run_search) =
        build_search_page(core.clone(), engine.clone(), rt.clone(), nav_cbs.clone());
    let (library_page, reload_lib) = build_library_page(
        core.clone(),
        engine.clone(),
        rt.clone(),
        nav_cbs.clone(),
        update_sidebar_playlists,
    );
    *reload_library_slot.borrow_mut() = Some(reload_lib.clone());
    let (downloads_page, reload_dl) =
        build_downloads_page(core.clone(), engine.clone(), rt.clone(), nav_cbs.clone());
    let (history_page, reload_hist) =
        build_history_page(core.clone(), engine.clone(), rt.clone(), nav_cbs.clone());
    let (profile_page, reload_prof) =
        build_profile_page(core.clone(), rt.clone(), nav_cbs.clone());
    let appearance_page = build_appearance_page(core.clone());
    let settings_page = build_settings_page(core.clone(), rt.clone(), nav_cbs.clone());

    center_stack.add_named(&home_page, Some("home"));
    center_stack.add_named(&search_page, Some("search"));
    center_stack.add_named(&library_page, Some("library"));
    center_stack.add_named(&downloads_page, Some("downloads"));
    center_stack.add_named(&history_page, Some("history"));
    center_stack.add_named(&profile_page, Some("profile"));
    center_stack.add_named(&appearance_page, Some("appearance"));
    center_stack.add_named(&settings_page, Some("settings"));
    center_stack.set_visible_child_name("home");

    {
        let sw = switch_section.clone();
        let rl = reload_lib;
        let rdl = reload_dl.clone();
        let rh = reload_hist;
        let rp = reload_prof;
        nav_list.connect_row_selected(move |_, row_opt| {
            if let Some(row) = row_opt {
                let name = row.widget_name();
                sw(name.as_str());
                match name.as_str() {
                    "library" => rl(),
                    "downloads" => rdl(),
                    "history" => rh(),
                    "profile" => rp(),
                    _ => {}
                }
            }
        });
    }

    // Fullscreen / Focus Now Playing overlay
    let now_playing_revealer = gtk4::Revealer::builder()
        .transition_type(gtk4::RevealerTransitionType::Crossfade)
        .reveal_child(false)
        .vexpand(true)
        .hexpand(true)
        .can_target(false)
        .build();

    let np_box = gtk4::Box::new(gtk4::Orientation::Vertical, 20);
    np_box.add_css_class("kw-now-playing-overlay");
    np_box.set_margin_top(24);
    np_box.set_margin_bottom(24);
    np_box.set_margin_start(32);
    np_box.set_margin_end(32);

    let np_top = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    let np_close_btn = gtk4::Button::builder()
        .label("Свернуть плеер (F11)")
        .icon_name("go-down-symbolic")
        .css_classes(["pill"])
        .build();
    np_top.append(&np_close_btn);
    np_box.append(&np_top);

    let np_split = gtk4::Box::new(gtk4::Orientation::Horizontal, 32);
    np_split.set_vexpand(true);
    np_split.set_homogeneous(true);

    let np_left = gtk4::Box::new(gtk4::Orientation::Vertical, 14);
    np_left.set_valign(gtk4::Align::Center);
    np_left.set_halign(gtk4::Align::Center);
    let np_cover = make_cover_picture(&core, &rt, None, "KWhale", 320, "lg");
    let np_title = gtk4::Label::builder()
        .label("Нет активного трека")
        .css_classes(["kw-hero-title"])
        .wrap(true)
        .justify(gtk4::Justification::Center)
        .build();
    let np_artist = gtk4::Label::builder()
        .label("Выберите трек для воспроизведения")
        .css_classes(["kw-display-title", "kw-muted"])
        .build();
    np_left.append(&np_cover);
    np_left.append(&np_title);
    np_left.append(&np_artist);
    np_split.append(&np_left);

    let np_lyrics_box = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    let np_lyrics_scroll = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&np_lyrics_box)
        .build();
    np_split.append(&np_lyrics_scroll);
    np_box.append(&np_split);
    now_playing_revealer.set_child(Some(&np_box));

    let toggle_now_playing: Rc<dyn Fn()> = {
        let rev = now_playing_revealer.clone();
        Rc::new(move || {
            let next = !rev.reveals_child();
            rev.set_can_target(next);
            rev.set_reveal_child(next);
        })
    };
    {
        let tnp = toggle_now_playing.clone();
        np_close_btn.connect_clicked(move |_| tnp());
    }

    // 4. Bottom Persistent Desktop PlayerDockBar (84px)
    let dock = build_player_dock(
        window,
        &core,
        &engine,
        &rt,
        &nav_cbs,
        toggle_inspector_tab.clone(),
        toggle_now_playing.clone(),
        &queue_list_box,
        &lyrics_list_box,
        &np_cover,
        &np_title,
        &np_artist,
        &np_lyrics_box,
    );
    outer_vbox.append(&dock);

    main_overlay.set_child(Some(&outer_vbox));
    main_overlay.add_overlay(&now_playing_revealer);

    // Drag-and-Drop Audio File Upload onto the window
    {
        let drop_target =
            gtk4::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        let win = window.clone();
        let c = core.clone();
        let rt_h = rt.clone();
        let dcbs = dialog_cbs.clone();
        drop_target.connect_drop(move |_, val, _, _| {
            if let Ok(flist) = val.get::<gdk::FileList>() {
                for f in flist.files() {
                    if let Some(p) = f.path() {
                        if is_supported_audio_file(&p) {
                            show_upload_track_dialog(
                                &win,
                                c.clone(),
                                rt_h.clone(),
                                Some(p),
                                dcbs.clone(),
                            );
                            return true;
                        }
                    }
                }
            }
            false
        });
        window.add_controller(drop_target);
    }

    // Global Desktop Keyboard Shortcuts
    {
        let key_ctrl = gtk4::EventControllerKey::new();
        let eng = engine.clone();
        let win = window.clone();
        let se = search_entry;
        let sw = switch_section.clone();
        let ti = toggle_inspector_tab;
        let tnp = toggle_now_playing;

        key_ctrl.connect_key_pressed(move |_, keyval, _, state| {
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);

            // Check if focus is inside an editable text entry
            let focus_is_entry = GtkWindowExt::focus(&win)
                .map(|w| {
                    w.is::<gtk4::Text>()
                        || w.is::<gtk4::Entry>()
                        || w.is::<gtk4::SearchEntry>()
                        || w.is::<gtk4::PasswordEntry>()
                })
                .unwrap_or(false);

            if ctrl && (keyval == gdk::Key::f || keyval == gdk::Key::k) {
                sw("search");
                se.grab_focus();
                return glib::Propagation::Stop;
            }
            if ctrl && keyval == gdk::Key::l {
                ti("lyrics");
                return glib::Propagation::Stop;
            }
            if ctrl && keyval == gdk::Key::q {
                ti("queue");
                return glib::Propagation::Stop;
            }
            if keyval == gdk::Key::F11 {
                tnp();
                return glib::Propagation::Stop;
            }
            if ctrl && keyval == gdk::Key::Left {
                eng.previous();
                return glib::Propagation::Stop;
            }
            if ctrl && keyval == gdk::Key::Right {
                eng.next();
                return glib::Propagation::Stop;
            }
            if ctrl && keyval == gdk::Key::Up {
                let st = eng.state();
                eng.set_volume(st.volume + 0.05);
                return glib::Propagation::Stop;
            }
            if ctrl && keyval == gdk::Key::Down {
                let st = eng.state();
                eng.set_volume(st.volume - 0.05);
                return glib::Propagation::Stop;
            }
            if !focus_is_entry {
                if keyval == gdk::Key::space {
                    eng.toggle();
                    return glib::Propagation::Stop;
                }
                if shift && keyval == gdk::Key::Left {
                    let pos = eng.state().position_ms;
                    eng.seek_ms(pos.saturating_sub(5000));
                    return glib::Propagation::Stop;
                }
                if shift && keyval == gdk::Key::Right {
                    let pos = eng.state().position_ms;
                    eng.seek_ms(pos + 5000);
                    return glib::Propagation::Stop;
                }
                if keyval == gdk::Key::m || keyval == gdk::Key::M {
                    let muted = eng.state().muted;
                    eng.set_muted(!muted);
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        window.add_controller(key_ctrl);
    }

    // Deep-link router
    let win_dl = window.clone();
    let c_dl = core;
    let eng_dl = engine;
    let rt_dl = rt;
    let oc_dl = open_collection;
    let dcbs_dl = dialog_cbs;
    Rc::new(move |link: DeepLink| match link {
        DeepLink::Album(id) => oc_dl(CollectionTarget::Album(id)),
        DeepLink::Artist(id) => oc_dl(CollectionTarget::Artist(id)),
        DeepLink::Playlist(id) => oc_dl(CollectionTarget::Playlist(id)),
        DeepLink::Share(sid) => {
            show_shared_link_sheet(
                &win_dl,
                c_dl.clone(),
                eng_dl.clone(),
                rt_dl.clone(),
                &sid,
                dcbs_dl.clone(),
            );
        }
        DeepLink::Track(tid) => {
            let c2 = c_dl.clone();
            let c3 = c_dl.clone();
            let eng2 = eng_dl.clone();
            let toast = dcbs_dl.show_toast.clone();
            spawn_async(
                &rt_dl,
                async move { c2.api.track(&tid).await },
                move |res| match res {
                    Ok(t) => {
                        let ui = t.to_ui(c3.downloads.is_downloaded(&t.id));
                        eng2.play_queue(vec![ui], 0, "player", None);
                    }
                    Err(e) => toast(&e.user_message()),
                },
            );
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn build_player_dock(
    window: &adw::ApplicationWindow,
    core: &Arc<KwhaleCore>,
    engine: &AudioEngine,
    rt: &tokio::runtime::Handle,
    nav: &NavCallbacks,
    toggle_inspector_tab: Rc<dyn Fn(&str)>,
    toggle_now_playing: Rc<dyn Fn()>,
    queue_list_box: &gtk4::Box,
    lyrics_list_box: &gtk4::Box,
    np_cover: &gtk4::Picture,
    np_title: &gtk4::Label,
    np_artist: &gtk4::Label,
    np_lyrics_box: &gtk4::Box,
) -> gtk4::Box {
    let dock = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
    dock.add_css_class("kw-dock");
    dock.set_size_request(-1, 84);

    // LEFT ZONE: Cover (click opens Fullscreen Now Playing) + Title + Artist + Like + Dislike + Censorship + Menu
    let left_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    left_box.set_size_request(300, -1);

    let dock_cover = make_cover_picture(core, rt, None, "KWhale", 56, "sm");
    let cover_btn = gtk4::Button::builder()
        .child(&dock_cover)
        .tooltip_text("Полноэкранный плеер (F11)")
        .css_classes(["flat"])
        .build();
    {
        let tnp = toggle_now_playing.clone();
        cover_btn.connect_clicked(move |_| tnp());
    }
    left_box.append(&cover_btn);

    let meta_vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    meta_vbox.set_valign(gtk4::Align::Center);
    meta_vbox.set_hexpand(true);
    let dock_title = gtk4::Label::builder()
        .label("Ничего не играет")
        .xalign(0.0)
        .ellipsize(gtk4::pango::EllipsizeMode::End)
        .max_width_chars(24)
        .css_classes(["heading"])
        .build();
    let dock_artist_btn = gtk4::Button::builder()
        .label("Выберите трек")
        .halign(gtk4::Align::Start)
        .css_classes(["flat", "kw-muted"])
        .build();
    {
        let eng = engine.clone();
        let open_artist = nav.dialogs.open_artist.clone();
        dock_artist_btn.connect_clicked(move |_| {
            if let Some(aid) = eng.state().current().and_then(|t| t.artist_remote_id.clone()) {
                open_artist(aid);
            }
        });
    }
    meta_vbox.append(&dock_title);
    meta_vbox.append(&dock_artist_btn);
    left_box.append(&meta_vbox);

    let dock_like_btn = gtk4::Button::builder()
        .icon_name("non-starred-symbolic")
        .valign(gtk4::Align::Center)
        .tooltip_text("Любимый трек")
        .css_classes(["flat"])
        .build();
    {
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let toast = nav.dialogs.show_toast.clone();
        dock_like_btn.connect_clicked(move |_| {
            let Some(cur) = eng.state().current().cloned() else {
                return;
            };
            let c2 = c.clone();
            let eng2 = eng.clone();
            let toast2 = toast.clone();
            spawn_async(
                &rt_h,
                async move { c2.reactions.toggle_like(&cur, "player").await },
                move |res| match res {
                    Ok(liked) => {
                        if let Some(id) = eng2.state().current().map(|t| t.remote_id.clone()) {
                            eng2.update_track_liked(&id, liked);
                        }
                    }
                    Err(e) => toast2(&e),
                },
            );
        });
    }
    left_box.append(&dock_like_btn);

    let dock_flag_btn = gtk4::Button::builder()
        .icon_name("dialog-warning-symbolic")
        .valign(gtk4::Align::Center)
        .tooltip_text("Пожаловаться на цензуру в 1 клик")
        .css_classes(["flat"])
        .build();
    {
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let toast = nav.dialogs.show_toast.clone();
        dock_flag_btn.connect_clicked(move |_| {
            let st = eng.state();
            let Some(cur) = st.current().cloned() else {
                return;
            };
            let pos = Some(st.position_ms);
            let c2 = c.clone();
            let toast2 = toast.clone();
            spawn_async(
                &rt_h,
                async move { c2.reactions.report_censorship_one_click(&cur, pos).await },
                move |res| match res {
                    Ok(msg) => toast2(msg),
                    Err(e) => toast2(&e),
                },
            );
        });
    }
    left_box.append(&dock_flag_btn);

    let dock_menu_btn = gtk4::Button::builder()
        .icon_name("view-more-symbolic")
        .valign(gtk4::Align::Center)
        .tooltip_text("Меню трека")
        .css_classes(["flat"])
        .build();
    {
        let win = window.clone();
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        let cbs = nav.dialogs.clone();
        dock_menu_btn.connect_clicked(move |_| {
            if let Some(cur) = eng.state().current().cloned() {
                show_track_actions_dialog(
                    &win,
                    c.clone(),
                    eng.clone(),
                    rt_h.clone(),
                    cur,
                    "player".into(),
                    None,
                    cbs.clone(),
                );
            }
        });
    }
    left_box.append(&dock_menu_btn);
    dock.append(&left_box);

    // CENTER ZONE: Transport Buttons + Seek Scale
    let center_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    center_box.set_hexpand(true);
    center_box.set_valign(gtk4::Align::Center);

    let transport_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    transport_row.set_halign(gtk4::Align::Center);

    let shuffle_btn = gtk4::Button::builder()
        .icon_name("media-playlist-shuffle-symbolic")
        .tooltip_text("Перемешать")
        .css_classes(["flat", "circular"])
        .build();
    let prev_btn = gtk4::Button::builder()
        .icon_name("media-skip-backward-symbolic")
        .tooltip_text("Предыдущий (Ctrl+Left)")
        .css_classes(["flat", "circular"])
        .build();
    let play_btn = gtk4::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .tooltip_text("Воспроизведение / Пауза (Space)")
        .css_classes(["suggested-action", "circular"])
        .build();
    let next_btn = gtk4::Button::builder()
        .icon_name("media-skip-forward-symbolic")
        .tooltip_text("Следующий (Ctrl+Right)")
        .css_classes(["flat", "circular"])
        .build();
    let repeat_btn = gtk4::Button::builder()
        .icon_name("media-playlist-repeat-symbolic")
        .tooltip_text("Повтор")
        .css_classes(["flat", "circular"])
        .build();

    {
        let eng = engine.clone();
        shuffle_btn.connect_clicked(move |_| eng.toggle_shuffle());
    }
    {
        let eng = engine.clone();
        prev_btn.connect_clicked(move |_| eng.previous());
    }
    {
        let eng = engine.clone();
        play_btn.connect_clicked(move |_| eng.toggle());
    }
    {
        let eng = engine.clone();
        next_btn.connect_clicked(move |_| eng.next());
    }
    {
        let eng = engine.clone();
        repeat_btn.connect_clicked(move |_| eng.cycle_repeat());
    }

    transport_row.append(&shuffle_btn);
    transport_row.append(&prev_btn);
    transport_row.append(&play_btn);
    transport_row.append(&next_btn);
    transport_row.append(&repeat_btn);
    center_box.append(&transport_row);

    let seek_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let pos_lbl = gtk4::Label::builder()
        .label("00:00")
        .width_chars(5)
        .css_classes(["caption", "kw-muted"])
        .build();
    let seek_scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 1000.0, 1.0);
    seek_scale.set_draw_value(false);
    seek_scale.set_hexpand(true);
    let dur_lbl = gtk4::Label::builder()
        .label("00:00")
        .width_chars(5)
        .css_classes(["caption", "kw-muted"])
        .build();
    seek_row.append(&pos_lbl);
    seek_row.append(&seek_scale);
    seek_row.append(&dur_lbl);
    center_box.append(&seek_row);

    let updating_seek = Rc::new(Cell::new(false));
    {
        let eng = engine.clone();
        let guard = updating_seek.clone();
        seek_scale.connect_change_value(move |_, _, val| {
            if !guard.get() {
                let dur = eng.state().duration_ms;
                if dur > 0 {
                    let target_ms = ((val / 1000.0).clamp(0.0, 1.0) * dur as f64) as u64;
                    eng.seek_ms(target_ms);
                }
            }
            glib::Propagation::Proceed
        });
    }
    dock.append(&center_box);

    // RIGHT ZONE: Info + Lyrics + Queue + Mute + Volume Slider + Fullscreen
    let right_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    right_box.set_valign(gtk4::Align::Center);

    let info_btn = gtk4::Button::builder()
        .icon_name("dialog-information-symbolic")
        .tooltip_text("Качество и источники трека")
        .css_classes(["flat", "circular"])
        .build();
    {
        let win = window.clone();
        let c = core.clone();
        let eng = engine.clone();
        let rt_h = rt.clone();
        info_btn.connect_clicked(move |_| {
            if let Some(cur) = eng.state().current().cloned() {
                show_track_info_dialog(&win, c.clone(), rt_h.clone(), &cur);
            }
        });
    }

    let lyrics_btn = gtk4::Button::builder()
        .icon_name("format-justify-center-symbolic")
        .tooltip_text("Текст песни (Ctrl+L)")
        .css_classes(["flat", "circular"])
        .build();
    {
        let ti = toggle_inspector_tab.clone();
        lyrics_btn.connect_clicked(move |_| ti("lyrics"));
    }

    let queue_btn = gtk4::Button::builder()
        .icon_name("view-list-symbolic")
        .tooltip_text("Очередь воспроизведения (Ctrl+Q)")
        .css_classes(["flat", "circular"])
        .build();
    {
        let ti = toggle_inspector_tab;
        queue_btn.connect_clicked(move |_| ti("queue"));
    }

    let mute_btn = gtk4::Button::builder()
        .icon_name("audio-volume-high-symbolic")
        .tooltip_text("Звук (M)")
        .css_classes(["flat", "circular"])
        .build();
    {
        let eng = engine.clone();
        mute_btn.connect_clicked(move |_| {
            let m = eng.state().muted;
            eng.set_muted(!m);
        });
    }

    let vol_scale = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 1.0, 0.02);
    vol_scale.set_draw_value(false);
    vol_scale.set_size_request(100, -1);
    vol_scale.set_value(core.settings.snapshot().volume);
    let updating_vol = Rc::new(Cell::new(false));
    {
        let eng = engine.clone();
        let guard = updating_vol.clone();
        vol_scale.connect_value_changed(move |sc| {
            if !guard.get() {
                eng.set_muted(false);
                eng.set_volume(sc.value());
            }
        });
    }

    let expand_btn = gtk4::Button::builder()
        .icon_name("view-fullscreen-symbolic")
        .tooltip_text("Полноэкранный плеер (F11)")
        .css_classes(["flat", "circular"])
        .build();
    {
        let tnp = toggle_now_playing;
        expand_btn.connect_clicked(move |_| tnp());
    }

    right_box.append(&info_btn);
    right_box.append(&lyrics_btn);
    right_box.append(&queue_btn);
    right_box.append(&mute_btn);
    right_box.append(&vol_scale);
    right_box.append(&expand_btn);
    dock.append(&right_box);

    // Subscribe to PlaybackState and update Dock, Queue, Lyrics & Fullscreen Now Playing
    let last_track_id: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let last_queue_sig: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let active_lyrics: Rc<RefCell<Option<Lyrics>>> = Rc::new(RefCell::new(None));
    let lyric_line_widgets: Rc<RefCell<Vec<(u64, gtk4::Widget, gtk4::Widget)>>> =
        Rc::new(RefCell::new(Vec::new()));

    let mut pb_rx = engine.subscribe();
    let (tx, rx) = std::sync::mpsc::channel::<PlaybackState>();
    rt.spawn(async move {
        let _ = tx.send(pb_rx.borrow().clone());
        while pb_rx.changed().await.is_ok() {
            let st = pb_rx.borrow().clone();
            if tx.send(st).is_err() {
                break;
            }
        }
    });

    let c_loop = core.clone();
    let eng_loop = engine.clone();
    let rt_loop = rt.clone();
    let q_box = queue_list_box.clone();
    let l_box = lyrics_list_box.clone();
    let np_c = np_cover.clone();
    let np_t = np_title.clone();
    let np_a = np_artist.clone();
    let np_lbox = np_lyrics_box.clone();

    glib::timeout_add_local(std::time::Duration::from_millis(90), move || {
        let mut latest: Option<PlaybackState> = None;
        while let Ok(st) = rx.try_recv() {
            latest = Some(st);
        }
        let Some(st) = latest else {
            return glib::ControlFlow::Continue;
        };

        // Play/Pause icon
        play_btn.set_icon_name(if st.is_playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });

        // Shuffle / Repeat highlight
        if st.shuffle {
            shuffle_btn.add_css_class("kw-liked");
        } else {
            shuffle_btn.remove_css_class("kw-liked");
        }
        match st.repeat_mode {
            RepeatMode::Off => {
                repeat_btn.set_icon_name("media-playlist-repeat-symbolic");
                repeat_btn.remove_css_class("kw-liked");
            }
            RepeatMode::All => {
                repeat_btn.set_icon_name("media-playlist-repeat-symbolic");
                repeat_btn.add_css_class("kw-liked");
            }
            RepeatMode::One => {
                repeat_btn.set_icon_name("media-playlist-repeat-song-symbolic");
                repeat_btn.add_css_class("kw-liked");
            }
        }

        // Seek & duration
        pos_lbl.set_label(&format_duration_player_ms(st.position_ms));
        dur_lbl.set_label(&format_duration_player_ms(st.duration_ms));
        updating_seek.set(true);
        seek_scale.set_value((st.progress() * 1000.0) as f64);
        updating_seek.set(false);

        // Volume & mute
        updating_vol.set(true);
        vol_scale.set_value(if st.muted { 0.0 } else { st.volume });
        updating_vol.set(false);
        mute_btn.set_icon_name(if st.muted || st.volume <= 0.01 {
            "audio-volume-muted-symbolic"
        } else if st.volume < 0.5 {
            "audio-volume-medium-symbolic"
        } else {
            "audio-volume-high-symbolic"
        });

        // Track change check
        let cur_id = st.current().map(|t| t.remote_id.clone());
        if let Some(cur) = st.current() {
            let liked = c_loop.reactions.is_liked(&cur.remote_id, cur.is_liked);
            dock_like_btn.set_icon_name(if liked {
                "emblem-favorite-symbolic"
            } else {
                "non-starred-symbolic"
            });
            if liked {
                dock_like_btn.add_css_class("kw-liked");
            } else {
                dock_like_btn.remove_css_class("kw-liked");
            }
        }

        if cur_id != *last_track_id.borrow() {
            *last_track_id.borrow_mut() = cur_id.clone();
            if let Some(cur) = st.current() {
                dock_title.set_label(&cur.title);
                dock_artist_btn.set_label(&cur.artist);
                np_t.set_label(&cur.title);
                np_a.set_label(&cur.artist);

                // Refresh cover art in dock and fullscreen Now Playing
                set_placeholder_on_picture(&dock_cover, &c_loop, &cur.title, 64);
                set_placeholder_on_picture(&np_c, &c_loop, &cur.title, 256);
                if let Some(img_id) = cur.cover_image_id.clone() {
                    let c_img = c_loop.clone();
                    let dc = dock_cover.downgrade();
                    let npc = np_c.downgrade();
                    spawn_async(
                        &rt_loop,
                        async move { c_img.get_or_fetch_image(&img_id, "lg").await },
                        move |res| {
                            if let Ok((_, bytes)) = res {
                                if let Ok(tex) =
                                    gdk::Texture::from_bytes(&glib::Bytes::from(&bytes))
                                {
                                    if let Some(p) = dc.upgrade() {
                                        p.set_paintable(Some(&tex));
                                    }
                                    if let Some(p) = npc.upgrade() {
                                        p.set_paintable(Some(&tex));
                                    }
                                }
                            }
                        },
                    );
                }

                // Fetch lyrics for new track
                let tid = cur.remote_id.clone();
                let c_lyr = c_loop.clone();
                let eng_lyr = eng_loop.clone();
                let l_box2 = l_box.clone();
                let np_lbox2 = np_lbox.clone();
                let active_lyr2 = active_lyrics.clone();
                let line_widgets2 = lyric_line_widgets.clone();
                spawn_async(
                    &rt_loop,
                    async move { c_lyr.api.lyrics(&tid).await },
                    move |res| {
                        while let Some(ch) = l_box2.first_child() {
                            l_box2.remove(&ch);
                        }
                        while let Some(ch) = np_lbox2.first_child() {
                            np_lbox2.remove(&ch);
                        }
                        line_widgets2.borrow_mut().clear();

                        match res {
                            Ok(lyr) if !lyr.synced.is_empty() => {
                                for line in &lyr.synced {
                                    let start_ms = line.start_ms;
                                    let text = if line.text.trim().is_empty() {
                                        "♪"
                                    } else {
                                        line.text.as_str()
                                    };

                                    let btn1 = gtk4::Button::builder()
                                        .label(text)
                                        .halign(gtk4::Align::Fill)
                                        .css_classes(["flat", "kw-lyric-line"])
                                        .build();
                                    let eng1 = eng_lyr.clone();
                                    btn1.connect_clicked(move |_| eng1.seek_ms(start_ms));
                                    l_box2.append(&btn1);

                                    let btn2 = gtk4::Button::builder()
                                        .label(text)
                                        .halign(gtk4::Align::Fill)
                                        .css_classes(["flat", "kw-lyric-line"])
                                        .build();
                                    let eng2 = eng_lyr.clone();
                                    btn2.connect_clicked(move |_| eng2.seek_ms(start_ms));
                                    np_lbox2.append(&btn2);

                                    line_widgets2.borrow_mut().push((
                                        start_ms,
                                        btn1.upcast(),
                                        btn2.upcast(),
                                    ));
                                }
                                *active_lyr2.borrow_mut() = Some(lyr);
                            }
                            Ok(lyr) if lyr.plain.as_deref().map(|s| !s.is_empty()).unwrap_or(false) => {
                                let plain = lyr.plain.clone().unwrap_or_default();
                                l_box2.append(
                                    &gtk4::Label::builder()
                                        .label(&plain)
                                        .wrap(true)
                                        .xalign(0.0)
                                        .selectable(true)
                                        .build(),
                                );
                                np_lbox2.append(
                                    &gtk4::Label::builder()
                                        .label(&plain)
                                        .wrap(true)
                                        .xalign(0.0)
                                        .build(),
                                );
                                *active_lyr2.borrow_mut() = Some(lyr);
                            }
                            _ => {
                                l_box2.append(
                                    &gtk4::Label::builder()
                                        .label("Текст песни недоступен")
                                        .css_classes(["kw-muted"])
                                        .margin_top(24)
                                        .build(),
                                );
                                np_lbox2.append(
                                    &gtk4::Label::builder()
                                        .label("Текст песни недоступен")
                                        .css_classes(["kw-muted"])
                                        .margin_top(24)
                                        .build(),
                                );
                                *active_lyr2.borrow_mut() = None;
                            }
                        }
                    },
                );
            }
        }

        // Update active karaoke line highlight
        {
            let widgets = lyric_line_widgets.borrow();
            if !widgets.is_empty() {
                let pos = st.position_ms;
                let active_idx = widgets
                    .iter()
                    .rposition(|(ms, _, _)| *ms <= pos)
                    .unwrap_or(0);
                for (i, (_, w1, w2)) in widgets.iter().enumerate() {
                    if i == active_idx {
                        w1.add_css_class("kw-lyric-active");
                        w1.remove_css_class("kw-lyric-line");
                        w2.add_css_class("kw-lyric-active");
                        w2.remove_css_class("kw-lyric-line");
                    } else {
                        w1.remove_css_class("kw-lyric-active");
                        w1.add_css_class("kw-lyric-line");
                        w2.remove_css_class("kw-lyric-active");
                        w2.add_css_class("kw-lyric-line");
                    }
                }
            }
        }

        // Rebuild Queue inspector only when queue or index changes
        let q_sig = format!(
            "{}:{}",
            st.index,
            st.queue
                .iter()
                .map(|t| t.remote_id.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
        if q_sig != *last_queue_sig.borrow() {
            *last_queue_sig.borrow_mut() = q_sig;
            while let Some(ch) = q_box.first_child() {
                q_box.remove(&ch);
            }
            for (idx, tr) in st.queue.iter().enumerate() {
                let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                row.add_css_class("kw-track-row");
                if idx == st.index {
                    row.add_css_class("kw-track-active");
                }
                let jump_btn = gtk4::Button::builder()
                    .hexpand(true)
                    .css_classes(["flat"])
                    .build();
                let v = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
                v.append(
                    &gtk4::Label::builder()
                        .label(&tr.title)
                        .xalign(0.0)
                        .ellipsize(gtk4::pango::EllipsizeMode::End)
                        .css_classes(["heading"])
                        .build(),
                );
                v.append(
                    &gtk4::Label::builder()
                        .label(&tr.artist)
                        .xalign(0.0)
                        .ellipsize(gtk4::pango::EllipsizeMode::End)
                        .css_classes(["caption", "kw-muted"])
                        .build(),
                );
                jump_btn.set_child(Some(&v));
                {
                    let eng = eng_loop.clone();
                    jump_btn.connect_clicked(move |_| eng.jump_to(idx));
                }

                let up_btn = gtk4::Button::builder()
                    .icon_name("go-up-symbolic")
                    .css_classes(["flat"])
                    .build();
                {
                    let eng = eng_loop.clone();
                    up_btn.connect_clicked(move |_| {
                        if idx > 0 {
                            eng.move_in_queue(idx, idx - 1);
                        }
                    });
                }

                let rm_btn = gtk4::Button::builder()
                    .icon_name("window-close-symbolic")
                    .css_classes(["flat"])
                    .build();
                {
                    let eng = eng_loop.clone();
                    rm_btn.connect_clicked(move |_| eng.remove_from_queue(idx));
                }

                row.append(&jump_btn);
                row.append(&up_btn);
                row.append(&rm_btn);
                q_box.append(&row);
            }
        }

        glib::ControlFlow::Continue
    });

    dock
}

fn is_supported_audio_file(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "mp3" | "flac" | "m4a" | "ogg" | "opus" | "wav" | "aac"
    )
}
