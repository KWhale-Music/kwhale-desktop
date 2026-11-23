#![allow(clippy::collapsible_if, clippy::too_many_arguments, clippy::type_complexity)]

mod dialogs;
mod shell;
mod style;
mod views;

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use kwhale_audio::{spawn_mpris, spawn_single_instance_dbus, AudioEngine};
use kwhale_core::{DeepLink, KwhaleCore};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

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
    let _rt_guard = rt.enter();
    let rt_handle = rt.handle().clone();

    let core = KwhaleCore::init(env!("CARGO_PKG_VERSION"))?;
    // Try best-effort keyring recovery if session file is missing
    if core.api.session().is_none() {
        let _ = rt.block_on(core.token_store.try_recover_from_keyring());
    }
    let engine = rt.block_on(AudioEngine::start(core.clone()))?;

    let initial_links: Vec<DeepLink> = std::env::args()
        .skip(1)
        .filter_map(|arg| DeepLink::parse(&arg))
        .collect();

    let (dbus_tx, dbus_rx) = std::sync::mpsc::channel::<Option<DeepLink>>();
    {
        let tx_raise = dbus_tx.clone();
        let tx_uri = dbus_tx.clone();
        spawn_mpris(
            core.clone(),
            engine.clone(),
            Arc::new(move || {
                let _ = tx_raise.send(None);
            }),
        );
        spawn_single_instance_dbus(
            Arc::new({
                let tx = dbus_tx;
                move || {
                    let _ = tx.send(None);
                }
            }),
            Arc::new(move |uri: String| {
                if let Some(dl) = DeepLink::parse(&uri) {
                    let _ = tx_uri.send(Some(dl));
                }
            }),
        );
    }

    let app = adw::Application::builder()
        .application_id("org.dueattendant149.Kwhale")
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let win_slot: Rc<RefCell<Option<shell::AdwWindowHandle>>> = Rc::new(RefCell::new(None));

    {
        let core = core.clone();
        let engine = engine.clone();
        let rt_handle = rt_handle.clone();
        let initial_links = initial_links.clone();
        let win_slot = win_slot.clone();
        app.connect_activate(move |app| {
            if let Some(existing) = win_slot.borrow().as_ref() {
                existing.window.present();
                return;
            }
            let handle = shell::build_main_window(
                app,
                core.clone(),
                engine.clone(),
                rt_handle.clone(),
                initial_links.clone(),
            );
            handle.window.present();
            *win_slot.borrow_mut() = Some(handle);
        });
    }

    {
        let core = core.clone();
        let engine = engine.clone();
        let rt_handle = rt_handle.clone();
        let win_slot = win_slot.clone();
        app.connect_open(move |app, files, _| {
            let mut links = Vec::new();
            for f in files {
                let uri = f.uri();
                if let Some(dl) = DeepLink::parse(uri.as_str()) {
                    links.push(dl);
                }
            }
            if let Some(existing) = win_slot.borrow().as_ref() {
                for l in links {
                    (existing.handle_deep_link)(l);
                }
                existing.window.present();
                return;
            }
            let handle = shell::build_main_window(
                app,
                core.clone(),
                engine.clone(),
                rt_handle.clone(),
                links,
            );
            handle.window.present();
            *win_slot.borrow_mut() = Some(handle);
        });
    }

    {
        let win_slot = win_slot.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            while let Ok(ev) = dbus_rx.try_recv() {
                if let Some(h) = win_slot.borrow().as_ref() {
                    if let Some(dl) = ev {
                        (h.handle_deep_link)(dl);
                    }
                    h.window.present();
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // Pass only program name to GApplication so custom args don't get rejected
    let argv0 = std::env::args()
        .next()
        .unwrap_or_else(|| "kwhale-adw".to_string());
    app.run_with_args(&[argv0]);
    Ok(())
}
