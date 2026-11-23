use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use kwhale_core::{
    accent_by_id, KwhaleCore, KwhalePalette, ThemeMode, ALL_ACCENTS,
};
use libadwaita as adw;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

thread_local! {
    static TEXTURE_CACHE: RefCell<HashMap<String, gdk::Texture>> = RefCell::new(HashMap::new());
}

pub fn apply_theme_css(
    provider: &gtk4::CssProvider,
    mode: ThemeMode,
    accent_id: i32,
    follow_system_accent: bool,
) {
    let style_mgr = adw::StyleManager::default();
    match mode {
        ThemeMode::Light => style_mgr.set_color_scheme(adw::ColorScheme::ForceLight),
        ThemeMode::Dark | ThemeMode::Amoled => {
            style_mgr.set_color_scheme(adw::ColorScheme::ForceDark)
        }
        ThemeMode::System => style_mgr.set_color_scheme(adw::ColorScheme::Default),
    }

    let system_is_dark = style_mgr.is_dark();
    let accent = if follow_system_accent {
        ALL_ACCENTS
            .iter()
            .find(|a| a.id == 106)
            .copied()
            .unwrap_or_else(|| accent_by_id(accent_id))
    } else {
        accent_by_id(accent_id)
    };
    let pal = KwhalePalette::build(mode, accent.rgb(), system_is_dark);

    let primary = pal.primary.to_hex();
    let on_primary = pal.on_primary.to_hex();
    let primary_container = pal.primary_container.to_hex();
    let on_primary_container = pal.on_primary_container.to_hex();
    let secondary_container = pal.secondary_container.to_hex();
    let _on_secondary_container = pal.on_secondary_container.to_hex();
    let surface = pal.surface.to_hex();
    let surface_low = pal.surface_container_low.to_hex();
    let surface_container = pal.surface_container.to_hex();
    let surface_high = pal.surface_container_high.to_hex();
    let surface_highest = pal.surface_container_highest.to_hex();
    let on_surface = pal.on_surface.to_hex();
    let on_surface_variant = pal.on_surface_variant.to_hex();
    let outline_variant = pal.outline_variant.to_hex();
    let error = pal.error.to_hex();
    let primary_glow = pal.primary.to_rgba_css(0.18);
    let dock_bg = if pal.is_amoled {
        "#050505".to_string()
    } else {
        surface_low.clone()
    };

    let css = format!(
        r#"
        @define-color accent_bg_color {primary};
        @define-color accent_fg_color {on_primary};
        @define-color accent_color {primary};
        @define-color window_bg_color {surface};
        @define-color window_fg_color {on_surface};
        @define-color view_bg_color {surface};
        @define-color view_fg_color {on_surface};
        @define-color headerbar_bg_color {surface_low};
        @define-color headerbar_fg_color {on_surface};
        @define-color sidebar_bg_color {surface_low};
        @define-color sidebar_fg_color {on_surface};
        @define-color card_bg_color {surface_container};
        @define-color card_fg_color {on_surface};
        @define-color popover_bg_color {surface_high};
        @define-color popover_fg_color {on_surface};
        @define-color dialog_bg_color {surface_container};
        @define-color dialog_fg_color {on_surface};

        window, .background {{
            background-color: {surface};
            color: {on_surface};
            font-family: "VK Sans Text", "Inter", system-ui, sans-serif;
        }}

        .kw-display-title {{
            font-family: "VK Sans Display", "VK Sans Text", system-ui, sans-serif;
            font-weight: 700;
            font-size: 22px;
        }}

        .kw-hero-title {{
            font-family: "VK Sans Display", "VK Sans Text", system-ui, sans-serif;
            font-weight: 800;
            font-size: 28px;
        }}

        .kw-section-title {{
            font-family: "VK Sans Display", "VK Sans Text", system-ui, sans-serif;
            font-weight: 700;
            font-size: 18px;
        }}

        .kw-sidebar {{
            background-color: {surface_low};
            border-right: 1px solid {outline_variant};
        }}

        .kw-inspector {{
            background-color: {surface_low};
            border-left: 1px solid {outline_variant};
        }}

        .kw-dock {{
            background-color: {dock_bg};
            border-top: 1px solid {outline_variant};
            padding: 8px 16px;
        }}

        .kw-card {{
            background-color: {surface_container};
            border-radius: 14px;
            padding: 12px;
            border: 1px solid alpha({outline_variant}, 0.45);
        }}

        .kw-card:hover {{
            background-color: {surface_high};
        }}

        .kw-hero-card {{
            background-image: linear-gradient(135deg, {primary_container}, {secondary_container});
            color: {on_primary_container};
            border-radius: 18px;
            padding: 20px;
            border: 1px solid alpha({primary}, 0.35);
        }}

        .kw-track-row {{
            border-radius: 10px;
            padding: 6px 10px;
        }}

        .kw-track-row:hover {{
            background-color: {surface_container};
        }}

        .kw-track-active {{
            background-color: {primary_glow};
            border-left: 3px solid {primary};
        }}

        .kw-chip {{
            border-radius: 999px;
            padding: 4px 12px;
            background-color: {surface_high};
            color: {on_surface};
            font-weight: 600;
        }}

        .kw-chip-active {{
            background-color: {primary};
            color: {on_primary};
        }}

        .kw-explicit-badge {{
            background-color: {surface_highest};
            color: {on_surface_variant};
            border-radius: 4px;
            padding: 1px 5px;
            font-size: 10px;
            font-weight: 700;
        }}

        .kw-liked {{
            color: {primary};
        }}

        .kw-error-text {{
            color: {error};
        }}

        .kw-muted {{
            color: {on_surface_variant};
        }}

        .kw-lyric-active {{
            font-family: "VK Sans Display", "VK Sans Text", sans-serif;
            font-size: 20px;
            font-weight: 800;
            color: {primary};
            padding: 6px 8px;
            border-radius: 8px;
            background-color: {primary_glow};
        }}

        .kw-lyric-line {{
            font-size: 15px;
            font-weight: 500;
            color: {on_surface_variant};
            padding: 5px 8px;
            border-radius: 8px;
        }}

        .kw-lyric-line:hover {{
            background-color: {surface_container};
            color: {on_surface};
        }}

        .kw-accent-swatch {{
            border-radius: 999px;
            min-width: 34px;
            min-height: 34px;
            border: 2px solid transparent;
        }}

        .kw-accent-swatch-active {{
            border: 3px solid {on_surface};
        }}

        .kw-now-playing-overlay {{
            background-image: linear-gradient(160deg, {primary_container}, {surface});
            color: {on_surface};
        }}

        .kw-cover-frame {{
            border-radius: 10px;
        }}
        "#
    );

    provider.load_from_string(&css);
}

pub fn make_cover_picture(
    core: &Arc<KwhaleCore>,
    rt: &tokio::runtime::Handle,
    image_id: Option<&str>,
    seed: &str,
    size_px: i32,
    size_tier: &str,
) -> gtk4::Picture {
    let pic = gtk4::Picture::new();
    pic.set_can_shrink(true);
    pic.set_content_fit(gtk4::ContentFit::Cover);
    pic.set_size_request(size_px, size_px);
    pic.add_css_class("kw-cover-frame");

    set_placeholder_on_picture(&pic, core, seed, size_px as u32);

    let Some(valid_id) = kwhale_core::image_id_or_null(image_id) else {
        return pic;
    };

    let cache_key = format!("{valid_id}:{size_tier}");
    if let Some(tex) = TEXTURE_CACHE.with(|c| c.borrow().get(&cache_key).cloned()) {
        pic.set_paintable(Some(&tex));
        return pic;
    }

    if let Some(bytes) = core.image_cache.get(valid_id, size_tier) {
        if let Ok(tex) = gdk::Texture::from_bytes(&glib::Bytes::from(&bytes)) {
            TEXTURE_CACHE.with(|c| {
                c.borrow_mut().insert(cache_key, tex.clone());
            });
            pic.set_paintable(Some(&tex));
            return pic;
        }
    }

    let core_clone = core.clone();
    let id_owned = valid_id.to_string();
    let tier_owned = size_tier.to_string();
    let pic_weak = pic.downgrade();
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    rt.spawn(async move {
        if let Ok((_, bytes)) = core_clone.get_or_fetch_image(&id_owned, &tier_owned).await {
            let _ = tx.send(bytes);
        }
    });

    glib::timeout_add_local(std::time::Duration::from_millis(45), move || {
        match rx.try_recv() {
            Ok(bytes) => {
                if let Some(p) = pic_weak.upgrade() {
                    if let Ok(tex) = gdk::Texture::from_bytes(&glib::Bytes::from(&bytes)) {
                        TEXTURE_CACHE.with(|c| {
                            c.borrow_mut().insert(cache_key.clone(), tex.clone());
                        });
                        p.set_paintable(Some(&tex));
                    }
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });

    pic
}

pub fn set_placeholder_on_picture(
    pic: &gtk4::Picture,
    core: &KwhaleCore,
    seed: &str,
    size_px: u32,
) {
    let dark = adw::StyleManager::default().is_dark();
    let key = format!("__svg__:{seed}:{dark}:{}", size_px.clamp(48, 256));
    if let Some(tex) = TEXTURE_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        pic.set_paintable(Some(&tex));
        return;
    }
    let svg = core.placeholder_svg(seed, dark, true, size_px.clamp(64, 256));
    if let Ok(tex) = gdk::Texture::from_bytes(&glib::Bytes::from(svg.as_bytes())) {
        TEXTURE_CACHE.with(|c| {
            c.borrow_mut().insert(key, tex.clone());
        });
        pic.set_paintable(Some(&tex));
    }
}

pub fn spawn_async<T, F>(rt: &tokio::runtime::Handle, fut: F, on_done: impl FnOnce(T) + 'static)
where
    T: Send + 'static,
    F: std::future::Future<Output = T> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel::<T>();
    rt.spawn(async move {
        let res = fut.await;
        let _ = tx.send(res);
    });
    let cb = Rc::new(RefCell::new(Some(on_done)));
    glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
        match rx.try_recv() {
            Ok(val) => {
                if let Some(f) = cb.borrow_mut().take() {
                    f(val);
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}
