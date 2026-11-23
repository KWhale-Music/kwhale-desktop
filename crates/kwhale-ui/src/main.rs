slint::include_modules!();

use kwhale_audio::AudioEngine;
use kwhale_core::KwhaleCore;
use slint::{Color, ModelRc, SharedString, VecModel};
use std::rc::Rc;
use std::sync::Arc;

fn to_model(v: Vec<AlbumItem>) -> ModelRc<AlbumItem> {
    ModelRc::from(Rc::new(VecModel::from(v)))
}

// Pleasant cover-color palette (stand-in for album art in the slice).
const COVER_COLORS: [(u8, u8, u8); 8] = [
    (0x6b, 0x5a, 0xa8),
    (0x0e, 0xa5, 0xa5),
    (0xc2, 0x71, 0x1c),
    (0xb4, 0x4f, 0xc0),
    (0x3b, 0x6f, 0xd4),
    (0xe1, 0x1d, 0x48),
    (0x22, 0xa0, 0x6b),
    (0x7c, 0x2d, 0x92),
];

fn cover_color(seed: &str) -> Color {
    let mut h: u32 = 0;
    for b in seed.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    let (r, g, b) = COVER_COLORS[(h as usize) % COVER_COLORS.len()];
    Color::from_rgb_u8(r, g, b)
}

fn album_item(title: &str, subtitle: &str, seed: &str) -> AlbumItem {
    AlbumItem {
        title: SharedString::from(title),
        subtitle: SharedString::from(substring_or_placeholder(subtitle)),
        cover: cover_color(seed),
    }
}

fn substring_or_placeholder(s: &str) -> &str {
    if s.is_empty() {
        "KWhale"
    } else {
        s
    }
}

// Seed data so the Home screen renders even before/without a server response.
fn seed_shelves() -> (Vec<AlbumItem>, Vec<AlbumItem>) {
    let albums = vec![
        album_item("Прости меня моя любовь", "Земфира", "a1"),
        album_item("Группа крови", "Кино", "a2"),
        album_item("Морская", "Мумий Тролль", "a3"),
        album_item("Сигнал из космоса", "Космос", "a4"),
        album_item("Синоптик", "Cream Soda", "a5"),
    ];
    let recent = vec![
        album_item("Дом с нормальными явлениями", "Сироткин", "a6"),
        album_item("Горы по колено", "Мот", "a7"),
        album_item("Пыяла", "AIGEL", "a8"),
        album_item("Розовое вино", "Федук", "a9"),
        album_item("Тёмная ночь", "Марк Бернес", "a10"),
    ];
    (albums, recent)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    let core = KwhaleCore::init(env!("CARGO_PKG_VERSION"))?;
    let engine = Arc::new(rt.block_on(AudioEngine::start(core.clone()))?);

    let ui = MainWindow::new()?;
    ui.set_platform("gnome".into());

    let (albums, recent) = seed_shelves();
    ui.set_albums(to_model(albums));
    ui.set_recent(to_model(recent));

    // ---- Mini-player controls -> audio engine ----
    {
        let engine = engine.clone();
        ui.on_toggle_play(move || engine.toggle());
    }
    {
        let engine = engine.clone();
        ui.on_next_track(move || engine.next());
    }

    // ---- Audio engine state -> mini-player ----
    {
        let engine = engine.clone();
        let weak = ui.as_weak();
        rt.spawn(async move {
            let mut rx = engine.subscribe();
            loop {
                if rx.changed().await.is_err() {
                    break;
                }
                let st = rx.borrow().clone();
                let (title, artist, playing) = match st.current() {
                    Some(t) => {
                        let sub = match &t.album {
                            Some(al) if !al.is_empty() => format!("{} · {}", t.artist, al),
                            _ => t.artist.clone(),
                        };
                        (t.title.clone(), sub, st.is_playing)
                    }
                    None => (String::new(), String::new(), st.is_playing),
                };
                let _ = slint::invoke_from_event_loop({
                    let weak = weak.clone();
                    move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_playing(playing);
                            ui.set_now_title(SharedString::from(title));
                            ui.set_now_artist(SharedString::from(artist));
                        }
                    }
                });
            }
        });
    }

    // ---- Real data: kwhale-core home() -> shelves ----
    {
        let core = core.clone();
        let weak = ui.as_weak();
        rt.spawn(async move {
            let Ok(home) = core.api.home(None, Some(30)).await else {
                return;
            };
            let mut albums: Vec<AlbumItem> = Vec::new();
            let mut recent: Vec<AlbumItem> = Vec::new();
            for section in &home.sections {
                for item in &section.items {
                    if let Some(album) = &item.album {
                        let artist = album
                            .artists
                            .first()
                            .map(|a| a.name.clone())
                            .unwrap_or_default();
                        let ai = album_item(&album.title, &artist, &album.id);
                        if albums.len() < 5 {
                            albums.push(ai);
                        } else {
                            recent.push(ai);
                        }
                    }
                }
            }
            if albums.is_empty() {
                return;
            }
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.set_albums(to_model(albums));
                    ui.set_recent(to_model(recent));
                }
            });
        });
    }

    ui.run()?;
    Ok(())
}
