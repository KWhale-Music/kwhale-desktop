#[cfg(target_os = "linux")]
mod linux {
    use crate::engine::AudioEngine;
    use kwhale_core::{KwhaleCore, PlaybackState, RepeatMode};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tracing::warn;
    use zbus::{fdo, interface, ObjectPath, Variant};

struct MprisRoot {
    raise_cb: Arc<dyn Fn() + Send + Sync>,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl MprisRoot {
    async fn raise(&self) {
        (self.raise_cb)();
    }

    async fn quit(&self) {}

    #[zbus(property)]
    async fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn identity(&self) -> String {
        "KWhale".to_string()
    }

    #[zbus(property)]
    async fn desktop_entry(&self) -> String {
        "org.dueattendant149.Kwhale".to_string()
    }

    #[zbus(property)]
    async fn supported_uri_schemes(&self) -> Vec<String> {
        vec!["kwhale".to_string()]
    }

    #[zbus(property)]
    async fn supported_mime_types(&self) -> Vec<String> {
        vec![
            "audio/mpeg".into(),
            "audio/flac".into(),
            "audio/mp4".into(),
            "audio/ogg".into(),
            "audio/opus".into(),
        ]
    }
}

struct MprisPlayer {
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl MprisPlayer {
    async fn next(&self) {
        self.engine.next();
    }

    async fn previous(&self) {
        self.engine.previous();
    }

    async fn pause(&self) {
        self.engine.pause();
    }

    async fn play_pause(&self) {
        self.engine.toggle();
    }

    async fn stop(&self) {
        self.engine.stop();
    }

    async fn play(&self) {
        self.engine.play();
    }

    async fn seek(&self, offset_us: i64) {
        let pos_ms = self.engine.state().position_ms as i64;
        let next = (pos_ms + offset_us / 1000).max(0) as u64;
        self.engine.seek_ms(next);
    }

    async fn set_position(&self, _track_id: ObjectPath<'_>, position_us: i64) {
        if position_us >= 0 {
            self.engine.seek_ms((position_us / 1000) as u64);
        }
    }

    #[zbus(property)]
    async fn playback_status(&self) -> String {
        let st = self.engine.state();
        if st.queue.is_empty() {
            "Stopped".to_string()
        } else if st.is_playing {
            "Playing".to_string()
        } else {
            "Paused".to_string()
        }
    }

    #[zbus(property)]
    async fn loop_status(&self) -> String {
        match self.engine.state().repeat_mode {
            RepeatMode::Off => "None".to_string(),
            RepeatMode::One => "Track".to_string(),
            RepeatMode::All => "Playlist".to_string(),
        }
    }

    #[zbus(property)]
    async fn set_loop_status(&self, status: String) -> fdo::Result<()> {
        let mode = match status.as_str() {
            "Track" => RepeatMode::One,
            "Playlist" => RepeatMode::All,
            _ => RepeatMode::Off,
        };
        self.engine.set_repeat_mode(mode);
        Ok(())
    }

    #[zbus(property)]
    async fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    async fn shuffle(&self) -> bool {
        self.engine.state().shuffle
    }

    #[zbus(property)]
    async fn set_shuffle(&self, value: bool) -> fdo::Result<()> {
        if self.engine.state().shuffle != value {
            self.engine.toggle_shuffle();
        }
        Ok(())
    }

    #[zbus(property)]
    async fn metadata(&self) -> HashMap<String, Variant<'static>> {
        build_metadata(&self.core, &self.engine.state())
    }

    #[zbus(property)]
    async fn volume(&self) -> f64 {
        let st = self.engine.state();
        if st.muted { 0.0 } else { st.volume }
    }

    #[zbus(property)]
    async fn set_volume(&self, vol: f64) -> fdo::Result<()> {
        self.engine.set_muted(false);
        self.engine.set_volume(vol);
        Ok(())
    }

    #[zbus(property)]
    async fn position(&self) -> i64 {
        (self.engine.state().position_ms as i64) * 1000
    }

    #[zbus(property)]
    async fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    async fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    async fn can_go_next(&self) -> bool {
        !self.engine.state().queue.is_empty()
    }

    #[zbus(property)]
    async fn can_go_previous(&self) -> bool {
        !self.engine.state().queue.is_empty()
    }

    #[zbus(property)]
    async fn can_play(&self) -> bool {
        !self.engine.state().queue.is_empty()
    }

    #[zbus(property)]
    async fn can_pause(&self) -> bool {
        !self.engine.state().queue.is_empty()
    }

    #[zbus(property)]
    async fn can_seek(&self) -> bool {
        self.engine.state().duration_ms > 0
    }

    #[zbus(property)]
    async fn can_control(&self) -> bool {
        true
    }
}

fn build_metadata(core: &KwhaleCore, st: &PlaybackState) -> HashMap<String, Variant<'static>> {
    let mut map: HashMap<String, Variant<'static>> = HashMap::new();
    let Some(track) = st.current() else {
        return map;
    };
    let safe_id: String = track
        .remote_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let track_obj = format!("/org/dueattendant149/Kwhale/track/{safe_id}");
    if let Ok(p) = ObjectPath::try_from(track_obj) {
        map.insert("mpris:trackid".into(), Variant::from(p.into_owned()));
    }
    let dur_us = (st.duration_ms.max((track.duration_sec as u64) * 1000) as i64) * 1000;
    map.insert("mpris:length".into(), Variant::from(dur_us));
    map.insert("xesam:title".into(), Variant::from(track.title.clone()));
    map.insert("xesam:artist".into(), Variant::from(track.artists.clone()));
    if let Some(album) = &track.album {
        map.insert("xesam:album".into(), Variant::from(album.clone()));
    }
    if let Some(img_id) = &track.cover_image_id {
        let cached = core.image_cache.path_for(img_id, "md");
        if cached.is_file() {
            map.insert(
                "mpris:artUrl".into(),
                Variant::from(kwhale_core::file_uri(&cached)),
            );
        }
    }
    map
}

pub fn spawn_mpris(
    core: Arc<KwhaleCore>,
    engine: AudioEngine,
    raise_cb: Arc<dyn Fn() + Send + Sync>,
) {
    tokio::spawn(async move {
        let root = MprisRoot { raise_cb };
        let player = MprisPlayer {
            core: core.clone(),
            engine: engine.clone(),
        };
        let conn = match zbus::connection::Builder::session()
            .name("org.mpris.MediaPlayer2.kwhale")
            .serve_at("/org/mpris/MediaPlayer2", root)
            .serve_at("/org/mpris/MediaPlayer2", player)
            .build()
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!("MPRIS2 D-Bus connect failed: {e}");
                return;
            }
        };

        let mut rx = engine.subscribe();
        let mut last_track_id: Option<String> = None;
        let mut last_playing = false;
        let mut last_shuffle = false;
        let mut last_repeat = RepeatMode::Off;

        while rx.changed().await.is_ok() {
            let st = rx.borrow().clone();
            let cur_id = st.current().map(|t| t.remote_id.clone());
            if let Some(img_id) = st.current().and_then(|t| t.cover_image_id.clone()) {
                if !core.image_cache.path_for(&img_id, "md").is_file() {
                    let _ = core.get_or_fetch_image(&img_id, "md").await;
                }
            }
            if cur_id != last_track_id
                || st.is_playing != last_playing
                || st.shuffle != last_shuffle
                || st.repeat_mode != last_repeat
            {
                last_track_id = cur_id;
                last_playing = st.is_playing;
                last_shuffle = st.shuffle;
                last_repeat = st.repeat_mode;

                if let Ok(iface) = conn
                    .object_server()
                    .interface::<_, MprisPlayer>("/org/mpris/MediaPlayer2")
                    .await
                {
                    let emitter = iface.signal_emitter();
                    let _ = iface.get().await.playback_status_changed(emitter).await;
                    let _ = iface.get().await.metadata_changed(emitter).await;
                    let _ = iface.get().await.shuffle_changed(emitter).await;
                    let _ = iface.get().await.loop_status_changed(emitter).await;
                    let _ = iface.get().await.can_go_next_changed(emitter).await;
                    let _ = iface.get().await.can_go_previous_changed(emitter).await;
                    let _ = iface.get().await.can_play_changed(emitter).await;
                    let _ = iface.get().await.can_pause_changed(emitter).await;
                }
            }
        }
    });
}

struct AppControlIface {
    raise_cb: Arc<dyn Fn() + Send + Sync>,
    open_uri_cb: Arc<dyn Fn(String) + Send + Sync>,
}

#[interface(name = "org.dueattendant149.Kwhale.Control")]
impl AppControlIface {
    async fn raise(&self) {
        (self.raise_cb)();
    }

    async fn open_uri(&self, uri: String) {
        (self.open_uri_cb)(uri);
        (self.raise_cb)();
    }
}

/// Registers `org.dueattendant149.Kwhale` on the session bus so `kwhale-launcher`
/// can forward `kwhale://` deep links and single-instance activation to either
/// `kwhale-adw` or `kwhale-qt`.
pub fn spawn_single_instance_dbus(
    raise_cb: Arc<dyn Fn() + Send + Sync>,
    open_uri_cb: Arc<dyn Fn(String) + Send + Sync>,
) {
    tokio::spawn(async move {
        let iface = AppControlIface {
            raise_cb,
            open_uri_cb,
        };
        match zbus::connection::Builder::session()
            .name("org.dueattendant149.Kwhale")
            .serve_at("/org/dueattendant149/Kwhale", iface)
            .build()
            .await
        {
            Ok(_conn) => {
                // Keep connection alive for the lifetime of the process.
                std::future::pending::<()>().await;
            }
            Err(e) => {
                warn!("Single-instance D-Bus registration skipped: {e}");
            }
        }
    });
}
}

#[cfg(target_os = "linux")]
pub use linux::{spawn_mpris, spawn_single_instance_dbus};

#[cfg(not(target_os = "linux"))]
pub fn spawn_mpris(
    _core: std::sync::Arc<kwhale_core::KwhaleCore>,
    _engine: crate::engine::AudioEngine,
    _raise_cb: std::sync::Arc<dyn Fn() + Send + Sync>,
) {
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_single_instance_dbus(
    _raise_cb: std::sync::Arc<dyn Fn() + Send + Sync>,
    _open_uri_cb: std::sync::Arc<dyn Fn(String) + Send + Sync>,
) {
}
