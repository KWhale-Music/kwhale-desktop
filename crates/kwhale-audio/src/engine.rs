use crate::cache_proxy::{QueuePrefetcher, StreamProxy};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_pbutils as gst_pbutils;
use kwhale_core::{KwhaleCore, PlaybackState, RepeatMode, UiTrack};
use rand::seq::SliceRandom;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tracing::{error, info, warn};

#[derive(Debug, Default)]
struct TelemetryState {
    accumulated_listened_ms: u64,
    active_play_started: Option<Instant>,
    completed_for_idx: Option<usize>,
    finalized_for_idx: Option<usize>,
    last_index: usize,
    retried_error_key: Option<String>,
}

impl TelemetryState {
    fn sync_clock(&mut self, is_playing: bool) {
        if let Some(started) = self.active_play_started.take() {
            self.accumulated_listened_ms += started.elapsed().as_millis() as u64;
        }
        if is_playing {
            self.active_play_started = Some(Instant::now());
        }
    }

    fn current_listened_ms(&self) -> u64 {
        let active = self
            .active_play_started
            .map(|s| s.elapsed().as_millis() as u64)
            .unwrap_or(0);
        self.accumulated_listened_ms + active
    }

    fn reset(&mut self, idx: usize, is_playing: bool) {
        self.accumulated_listened_ms = 0;
        self.active_play_started = if is_playing { Some(Instant::now()) } else { None };
        self.completed_for_idx = None;
        self.finalized_for_idx = None;
        self.last_index = idx;
    }
}

#[derive(Clone)]
pub struct AudioEngine {
    core: Arc<KwhaleCore>,
    playbin: gst::Element,
    proxy: Arc<StreamProxy>,
    prefetcher: QueuePrefetcher,
    state: Arc<RwLock<PlaybackState>>,
    unshuffled_queue: Arc<RwLock<Option<Vec<UiTrack>>>>,
    telemetry: Arc<Mutex<TelemetryState>>,
    tx: watch::Sender<PlaybackState>,
}

impl AudioEngine {
    pub async fn start(core: Arc<KwhaleCore>) -> anyhow::Result<Self> {
        gst::init()?;
        let playbin = gst::ElementFactory::make("playbin3")
            .name("kwhale-playbin")
            .build()
            .or_else(|_| gst::ElementFactory::make("playbin").name("kwhale-playbin").build())?;

        let proxy = Arc::new(StreamProxy::start(core.clone()).await?);
        let prefetcher = QueuePrefetcher::new(core.clone());

        let snap = core.settings.snapshot();
        playbin.set_property("volume", snap.volume);
        playbin.set_property("mute", snap.muted);

        let initial = PlaybackState {
            volume: snap.volume,
            muted: snap.muted,
            ..Default::default()
        };
        let (tx, _) = watch::channel(initial.clone());

        // Wire GStreamer Discoverer as duration probe for DownloadManager.
        core.downloads.set_duration_probe(Arc::new(|path: &Path| {
            measure_duration_sec(path)
        }));

        let engine = Self {
            core,
            playbin,
            proxy,
            prefetcher,
            state: Arc::new(RwLock::new(initial)),
            unshuffled_queue: Arc::new(RwLock::new(None)),
            telemetry: Arc::new(Mutex::new(TelemetryState::default())),
            tx,
        };

        engine.spawn_bus_and_ticker();
        Ok(engine)
    }

    pub fn subscribe(&self) -> watch::Receiver<PlaybackState> {
        self.tx.subscribe()
    }

    pub fn state(&self) -> PlaybackState {
        self.state.read().expect("playback state").clone()
    }

    pub fn play_queue(
        &self,
        tracks: Vec<UiTrack>,
        start_index: usize,
        surface: &str,
        mix_id: Option<&str>,
    ) {
        if tracks.is_empty() {
            return;
        }
        self.finalize_current_track(false);
        let idx = start_index.min(tracks.len().saturating_sub(1));
        let shuffle = self.state().shuffle;
        let final_queue = if shuffle && tracks.len() > 1 {
            *self.unshuffled_queue.write().expect("unshuffled") = Some(tracks.clone());
            let mut rng = rand::rng();
            let chosen = tracks[idx].clone();
            let mut rest: Vec<UiTrack> = tracks
                .into_iter()
                .enumerate()
                .filter(|(i, _)| *i != idx)
                .map(|(_, t)| t)
                .collect();
            rest.shuffle(&mut rng);
            let mut out = Vec::with_capacity(rest.len() + 1);
            out.push(chosen);
            out.extend(rest);
            out
        } else {
            *self.unshuffled_queue.write().expect("unshuffled") = None;
            tracks
        };
        let final_idx = if shuffle { 0 } else { idx };

        {
            let mut st = self.state.write().expect("state");
            st.queue = final_queue;
            st.index = final_idx;
            st.surface = surface.to_string();
            st.mix_id = mix_id.map(ToOwned::to_owned);
            st.error = None;
        }
        self.load_and_play_index(final_idx, true);
    }

    pub fn play_next(&self, track: UiTrack, surface: &str) {
        let mut st = self.state.write().expect("state");
        if st.queue.is_empty() {
            st.queue.push(track);
            st.index = 0;
            st.surface = surface.to_string();
            st.mix_id = None;
            drop(st);
            self.load_and_play_index(0, true);
            return;
        }
        let insert_at = (st.index + 1).min(st.queue.len());
        st.queue.insert(insert_at, track);
        drop(st);
        self.schedule_prefetch();
        self.publish();
    }

    pub fn add_queue(&self, tracks: Vec<UiTrack>, surface: &str) {
        if tracks.is_empty() {
            return;
        }
        let mut st = self.state.write().expect("state");
        let was_empty = st.queue.is_empty();
        st.queue.extend(tracks);
        if was_empty {
            st.index = 0;
            st.surface = surface.to_string();
            drop(st);
            self.load_and_play_index(0, true);
        } else {
            drop(st);
            self.schedule_prefetch();
            self.publish();
        }
    }

    pub fn toggle(&self) {
        let st = self.state();
        if st.queue.is_empty() {
            return;
        }
        if st.is_playing {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn play(&self) {
        let st = self.state();
        if st.queue.is_empty() {
            return;
        }
        let _ = self.playbin.set_state(gst::State::Playing);
        {
            let mut tel = self.telemetry.lock().expect("tel");
            tel.sync_clock(true);
        }
        if let Some(t) = st.current() {
            self.core.events.resume(t, &st.surface, st.position_ms, st.mix_id.as_deref());
        }
        {
            let mut w = self.state.write().expect("state");
            w.is_playing = true;
        }
        self.publish();
    }

    pub fn pause(&self) {
        let st = self.state();
        let _ = self.playbin.set_state(gst::State::Paused);
        {
            let mut tel = self.telemetry.lock().expect("tel");
            tel.sync_clock(false);
        }
        if let Some(t) = st.current() {
            self.core.events.pause(t, &st.surface, st.position_ms, st.mix_id.as_deref());
        }
        {
            let mut w = self.state.write().expect("state");
            w.is_playing = false;
        }
        self.publish();
    }

    pub fn stop(&self) {
        self.finalize_current_track(false);
        let _ = self.playbin.set_state(gst::State::Null);
        *self.unshuffled_queue.write().expect("unshuffled") = None;
        {
            let mut w = self.state.write().expect("state");
            w.queue.clear();
            w.index = 0;
            w.is_playing = false;
            w.is_buffering = false;
            w.position_ms = 0;
            w.duration_ms = 0;
            w.error = None;
        }
        self.publish();
    }

    pub fn jump_to(&self, index: usize) {
        let len = self.state.read().expect("state").queue.len();
        if index >= len {
            return;
        }
        self.finalize_current_track(false);
        self.load_and_play_index(index, true);
    }

    pub fn next(&self) {
        let st = self.state();
        if st.queue.is_empty() {
            return;
        }
        let next_idx = if st.index + 1 < st.queue.len() {
            Some(st.index + 1)
        } else if st.repeat_mode == RepeatMode::All {
            Some(0)
        } else {
            None
        };
        if let Some(idx) = next_idx {
            self.finalize_current_track(false);
            self.load_and_play_index(idx, true);
        } else {
            self.pause();
            self.seek_ms(0);
        }
    }

    pub fn previous(&self) {
        let st = self.state();
        if st.queue.is_empty() {
            return;
        }
        if st.position_ms > 3_000 {
            self.seek_ms(0);
            return;
        }
        let prev_idx = if st.index > 0 {
            Some(st.index - 1)
        } else if st.repeat_mode == RepeatMode::All {
            Some(st.queue.len().saturating_sub(1))
        } else {
            Some(0)
        };
        if let Some(idx) = prev_idx {
            if idx != st.index {
                self.finalize_current_track(false);
                self.load_and_play_index(idx, true);
            } else {
                self.seek_ms(0);
            }
        }
    }

    pub fn seek_ms(&self, position_ms: u64) {
        let pos = gst::ClockTime::from_mseconds(position_ms);
        let _ = self.playbin.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            pos,
        );
        let st = self.state();
        if let Some(t) = st.current() {
            self.core.events.seek(t, &st.surface, position_ms, st.mix_id.as_deref());
        }
        {
            let mut w = self.state.write().expect("state");
            w.position_ms = position_ms;
        }
        self.publish();
    }

    pub fn set_volume(&self, volume: f64) {
        let v = volume.clamp(0.0, 1.0);
        self.playbin.set_property("volume", v);
        self.core.settings.update(|s| {
            s.volume = v;
        });
        {
            let mut w = self.state.write().expect("state");
            w.volume = v;
        }
        self.publish();
    }

    pub fn set_muted(&self, muted: bool) {
        self.playbin.set_property("mute", muted);
        self.core.settings.update(|s| {
            s.muted = muted;
        });
        {
            let mut w = self.state.write().expect("state");
            w.muted = muted;
        }
        self.publish();
    }

    pub fn toggle_shuffle(&self) {
        let mut st = self.state.write().expect("state");
        let next_shuffle = !st.shuffle;
        st.shuffle = next_shuffle;
        if st.queue.len() > 1 {
            if next_shuffle {
                *self.unshuffled_queue.write().expect("unshuffled") = Some(st.queue.clone());
                let idx = st.index;
                let current = st.queue.remove(idx);
                let mut rng = rand::rng();
                st.queue.shuffle(&mut rng);
                st.queue.insert(0, current);
                st.index = 0;
            } else if let Some(orig) = self.unshuffled_queue.write().expect("unshuffled").take() {
                let current_id = st.current().map(|t| t.remote_id.clone());
                let new_idx = current_id
                    .and_then(|id| orig.iter().position(|t| t.remote_id == id))
                    .unwrap_or(0);
                st.queue = orig;
                st.index = new_idx;
            }
        }
        drop(st);
        self.schedule_prefetch();
        self.publish();
    }

    pub fn cycle_repeat(&self) {
        let mut st = self.state.write().expect("state");
        st.repeat_mode = st.repeat_mode.cycle();
        drop(st);
        self.publish();
    }

    pub fn set_repeat_mode(&self, mode: RepeatMode) {
        let mut st = self.state.write().expect("state");
        st.repeat_mode = mode;
        drop(st);
        self.publish();
    }

    pub fn move_in_queue(&self, from: usize, to: usize) {
        let mut st = self.state.write().expect("state");
        if from >= st.queue.len() || to >= st.queue.len() || from == to {
            return;
        }
        let current_idx = st.index;
        let item = st.queue.remove(from);
        st.queue.insert(to, item);
        st.index = if current_idx == from {
            to
        } else if from < current_idx && to >= current_idx {
            current_idx.saturating_sub(1)
        } else if from > current_idx && to <= current_idx {
            current_idx + 1
        } else {
            current_idx
        };
        drop(st);
        self.schedule_prefetch();
        self.publish();
    }

    pub fn remove_from_queue(&self, index: usize) {
        let mut st = self.state.write().expect("state");
        if index >= st.queue.len() || index == st.index {
            return;
        }
        st.queue.remove(index);
        if index < st.index {
            st.index = st.index.saturating_sub(1);
        }
        drop(st);
        self.schedule_prefetch();
        self.publish();
    }

    pub fn update_track_liked(&self, remote_id: &str, is_liked: bool) {
        let mut st = self.state.write().expect("state");
        for t in &mut st.queue {
            if t.remote_id == remote_id {
                t.is_liked = is_liked;
            }
        }
        drop(st);
        self.publish();
    }

    fn uri_for_track(&self, track: &UiTrack) -> String {
        if let Some(local) = self.core.downloads.local_file_for(&track.remote_id) {
            return format!("file://{}", local.to_string_lossy());
        }
        if let Some(cached) = self.core.playback_cache.get_cached_path(&track.remote_id) {
            return format!("file://{}", cached.to_string_lossy());
        }
        self.proxy.url_for(&track.remote_id)
    }

    fn load_and_play_index(&self, index: usize, auto_play: bool) {
        let (track, surface, mix_id) = {
            let mut st = self.state.write().expect("state");
            if index >= st.queue.len() {
                return;
            }
            st.index = index;
            st.position_ms = 0;
            st.duration_ms = (st.queue[index].duration_sec as u64) * 1000;
            st.is_playing = auto_play;
            st.is_buffering = true;
            st.error = None;
            (st.queue[index].clone(), st.surface.clone(), st.mix_id.clone())
        };

        let uri = self.uri_for_track(&track);
        info!("loading track {} uri={}", track.title, uri);
        let _ = self.playbin.set_state(gst::State::Null);
        self.playbin.set_property("uri", &uri);
        let target_state = if auto_play {
            gst::State::Playing
        } else {
            gst::State::Paused
        };
        let _ = self.playbin.set_state(target_state);

        {
            let mut tel = self.telemetry.lock().expect("tel");
            tel.reset(index, auto_play);
            tel.retried_error_key = None;
        }
        self.core
            .events
            .play(&track, &surface, mix_id.as_deref());
        self.schedule_prefetch();
        self.publish();
    }

    fn schedule_prefetch(&self) {
        let st = self.state();
        if st.queue.is_empty() || st.index + 1 >= st.queue.len() {
            self.prefetcher.schedule(Vec::new());
            return;
        }
        let ids: Vec<String> = st.queue[st.index + 1..]
            .iter()
            .take(5)
            .map(|t| t.remote_id.clone())
            .collect();
        self.prefetcher.schedule(ids);
    }

    fn finalize_current_track(&self, ended_naturally: bool) {
        let st = self.state();
        let idx = st.index;
        let Some(track) = st.queue.get(idx) else {
            return;
        };
        let mut tel = self.telemetry.lock().expect("tel");
        if tel.finalized_for_idx == Some(idx) {
            return;
        }
        tel.finalized_for_idx = Some(idx);
        tel.sync_clock(false);
        let listened = tel.current_listened_ms();
        let dur_ms = if st.duration_ms > 0 {
            st.duration_ms
        } else {
            (track.duration_sec as u64) * 1000
        };
        let threshold_ms = if dur_ms > 0 {
            ((dur_ms as f64 * 0.80) as u64).clamp(15_000, 180_000)
        } else {
            30_000
        };
        if ended_naturally || listened >= threshold_ms {
            if tel.completed_for_idx != Some(idx) {
                tel.completed_for_idx = Some(idx);
                let l = if ended_naturally && listened == 0 {
                    dur_ms
                } else {
                    listened
                };
                self.core.events.complete(
                    track,
                    &st.surface,
                    (dur_ms > 0).then_some(dur_ms),
                    Some(l),
                    st.mix_id.as_deref(),
                );
            }
        } else {
            self.core.events.skip(
                track,
                &st.surface,
                st.position_ms,
                listened,
                st.mix_id.as_deref(),
            );
        }
    }

    fn spawn_bus_and_ticker(&self) {
        let engine = self.clone();
        let bus = self.playbin.bus().expect("playbin bus");
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(250));
            loop {
                interval.tick().await;

                while let Some(msg) = bus.pop() {
                    use gst::MessageView;
                    match msg.view() {
                        MessageView::Eos(_) => {
                            engine.handle_eos();
                        }
                        MessageView::Buffering(b) => {
                            let pct = b.percent();
                            let mut st = engine.state.write().expect("state");
                            st.is_buffering = pct < 100;
                        }
                        MessageView::StateChanged(sc) => {
                            if sc.src().map(|s| *s == engine.playbin).unwrap_or(false) {
                                let is_playing = sc.current() == gst::State::Playing;
                                let is_ready = matches!(
                                    sc.current(),
                                    gst::State::Playing | gst::State::Paused
                                );
                                let mut st = engine.state.write().expect("state");
                                st.is_playing = is_playing;
                                if is_ready {
                                    st.is_buffering = false;
                                }
                            }
                        }
                        MessageView::Error(err) => {
                            let msg_txt = err.error().to_string();
                            warn!("gstreamer error: {msg_txt}");
                            engine.handle_player_error(msg_txt);
                        }
                        _ => {}
                    }
                }

                // Update position / duration
                if !engine.state.read().expect("state").queue.is_empty() {
                    let pos = engine
                        .playbin
                        .query_position::<gst::ClockTime>()
                        .map(|t| t.mseconds())
                        .unwrap_or(0);
                    let dur = engine
                        .playbin
                        .query_duration::<gst::ClockTime>()
                        .map(|t| t.mseconds())
                        .unwrap_or(0);
                    let mut st = engine.state.write().expect("state");
                    let changed = st.position_ms != pos || (dur > 0 && st.duration_ms != dur);
                    st.position_ms = pos;
                    if dur > 0 {
                        st.duration_ms = dur;
                    }
                    drop(st);
                    if changed {
                        engine.publish();
                    }
                }
            }
        });
    }

    fn handle_eos(&self) {
        let st = self.state();
        if st.queue.is_empty() {
            return;
        }
        self.finalize_current_track(true);
        match st.repeat_mode {
            RepeatMode::One => {
                if let Some(t) = st.current() {
                    self.core
                        .events
                        .repeat(t, &st.surface, 0, st.mix_id.as_deref());
                }
                self.load_and_play_index(st.index, true);
            }
            RepeatMode::All => {
                let next_idx = (st.index + 1) % st.queue.len();
                self.load_and_play_index(next_idx, true);
            }
            RepeatMode::Off => {
                if st.index + 1 < st.queue.len() {
                    self.load_and_play_index(st.index + 1, true);
                } else {
                    let _ = self.playbin.set_state(gst::State::Paused);
                    let _ = self.playbin.seek_simple(
                        gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                        gst::ClockTime::ZERO,
                    );
                    let mut w = self.state.write().expect("state");
                    w.is_playing = false;
                    w.position_ms = 0;
                    drop(w);
                    self.publish();
                }
            }
        }
    }

    fn handle_player_error(&self, err_msg: String) {
        let st = self.state();
        let Some(track) = st.current().cloned() else {
            return;
        };
        let key = track.remote_id.clone();
        let should_retry = {
            let mut tel = self.telemetry.lock().expect("tel");
            if tel.retried_error_key.as_deref() != Some(&key) {
                tel.retried_error_key = Some(key);
                true
            } else {
                false
            }
        };
        if should_retry {
            let saved_pos = st.position_ms;
            let uri = self.uri_for_track(&track);
            let _ = self.playbin.set_state(gst::State::Null);
            self.playbin.set_property("uri", &uri);
            let _ = self.playbin.set_state(gst::State::Playing);
            if saved_pos > 0 {
                let _ = self.playbin.seek_simple(
                    gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                    gst::ClockTime::from_mseconds(saved_pos),
                );
            }
            return;
        }

        error!("unrecoverable playback error for {}: {err_msg}", track.title);
        let _ = self.playbin.set_state(gst::State::Null);
        {
            let mut w = self.state.write().expect("state");
            w.is_playing = false;
            w.is_buffering = false;
            w.error = Some(format!("Не удалось воспроизвести: {err_msg}"));
        }
        self.publish();
    }

    fn publish(&self) {
        let st = self.state();
        let _ = self.tx.send(st);
    }
}

pub fn measure_duration_sec(path: &Path) -> Option<u32> {
    let discoverer = gst_pbutils::Discoverer::new(gst::ClockTime::from_seconds(5)).ok()?;
    let uri = format!("file://{}", path.to_string_lossy());
    let info = discoverer.discover_uri(&uri).ok()?;
    info.duration().map(|d| d.seconds() as u32)
}
