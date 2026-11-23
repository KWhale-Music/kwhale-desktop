use crate::network::KwhaleApi;
use crate::playback_state::UiTrack;
use crate::storage::{write_atomic, AppPaths};
use chrono::{Local, Utc};
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const MAX_PENDING: usize = 500;
const BATCH_SIZE: usize = 100;
const VALID_SURFACES: &[&str] = &[
    "home", "search", "library", "mix", "playlist", "artist", "album", "player",
];

#[derive(Debug, Default)]
struct ActivePlayback {
    playback_id: Option<String>,
    started_at: Option<String>,
    track_duration_ms: Option<u64>,
}

/// Buffers recommendation signals, persists pending batches to disk, and flushes to `POST /v1/events`.
#[derive(Clone)]
pub struct EventTracker {
    api: KwhaleApi,
    disk_file: PathBuf,
    session_id: String,
    pending: Arc<Mutex<VecDeque<crate::models::Event>>>,
    active: Arc<Mutex<ActivePlayback>>,
    flushing: Arc<AtomicBool>,
}

impl EventTracker {
    pub fn new(api: KwhaleApi, paths: &AppPaths) -> Self {
        let disk_file = paths.pending_events_file();
        let initial = fs::read_to_string(&disk_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<crate::models::Event>>(&s).ok())
            .map(VecDeque::from)
            .unwrap_or_default();

        let tracker = Self {
            api,
            disk_file,
            session_id: Uuid::new_v4().to_string(),
            pending: Arc::new(Mutex::new(initial)),
            active: Arc::new(Mutex::new(ActivePlayback::default())),
            flushing: Arc::new(AtomicBool::new(false)),
        };
        tracker.flush();
        tracker
    }

    pub fn play(&self, track: &UiTrack, surface: &str, mix_id: Option<&str>) {
        let pb_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let dur_ms = (track.duration_sec as u64) * 1000;
        {
            let mut active = self.active.lock().expect("active playback lock");
            active.playback_id = Some(pb_id);
            active.started_at = Some(now);
            active.track_duration_ms = Some(dur_ms);
        }
        self.push_event(
            Some(track),
            "play",
            surface,
            Some(0),
            false,
            None,
            None,
            mix_id,
            None,
        );
    }

    pub fn pause(&self, track: &UiTrack, surface: &str, position_ms: u64, mix_id: Option<&str>) {
        self.push_event(
            Some(track),
            "pause",
            surface,
            Some(position_ms),
            false,
            Some(position_ms),
            None,
            mix_id,
            None,
        );
    }

    pub fn resume(&self, track: &UiTrack, surface: &str, position_ms: u64, mix_id: Option<&str>) {
        self.push_event(
            Some(track),
            "resume",
            surface,
            Some(position_ms),
            false,
            None,
            None,
            mix_id,
            None,
        );
    }

    pub fn seek(&self, track: &UiTrack, surface: &str, position_ms: u64, mix_id: Option<&str>) {
        self.push_event(
            Some(track),
            "seek",
            surface,
            Some(position_ms),
            false,
            None,
            None,
            mix_id,
            None,
        );
    }

    pub fn skip(
        &self,
        track: &UiTrack,
        surface: &str,
        position_ms: u64,
        listened_ms: u64,
        mix_id: Option<&str>,
    ) {
        self.push_event(
            Some(track),
            "skip",
            surface,
            Some(position_ms),
            true,
            Some(listened_ms),
            None,
            mix_id,
            None,
        );
    }

    pub fn complete(
        &self,
        track: &UiTrack,
        surface: &str,
        duration_ms: Option<u64>,
        listened_ms: Option<u64>,
        mix_id: Option<&str>,
    ) {
        let total = duration_ms
            .filter(|&d| d > 0)
            .or_else(|| self.active.lock().expect("active").track_duration_ms)
            .unwrap_or((track.duration_sec as u64) * 1000);
        let listened = listened_ms.filter(|&l| l > 0).unwrap_or(total);
        self.push_event(
            Some(track),
            "complete",
            surface,
            Some(total),
            true,
            Some(listened),
            None,
            mix_id,
            None,
        );
    }

    pub fn repeat(&self, track: &UiTrack, surface: &str, position_ms: u64, mix_id: Option<&str>) {
        self.push_event(
            Some(track),
            "repeat",
            surface,
            Some(position_ms),
            false,
            None,
            None,
            mix_id,
            None,
        );
    }

    pub fn like(&self, track: &UiTrack, surface: &str, mix_id: Option<&str>) {
        self.push_event(Some(track), "like", surface, None, false, None, None, mix_id, None);
    }

    pub fn unlike(&self, track: &UiTrack, surface: &str, mix_id: Option<&str>) {
        self.push_event(Some(track), "unlike", surface, None, false, None, None, mix_id, None);
    }

    pub fn dislike(&self, track: &UiTrack, surface: &str, mix_id: Option<&str>) {
        self.push_event(Some(track), "dislike", surface, None, false, None, None, mix_id, None);
    }

    pub fn undislike(&self, track: &UiTrack, surface: &str, mix_id: Option<&str>) {
        self.push_event(Some(track), "undislike", surface, None, false, None, None, mix_id, None);
    }

    pub fn playlist_add(&self, track: &UiTrack, playlist_id: Option<&str>, surface: &str) {
        self.push_event(
            Some(track),
            "add_to_playlist",
            surface,
            None,
            false,
            None,
            playlist_id,
            None,
            None,
        );
    }

    pub fn share(&self, track: Option<&UiTrack>, surface: &str) {
        self.push_event(track, "share", surface, None, false, None, None, None, None);
    }

    pub fn impression(
        &self,
        track: Option<&UiTrack>,
        surface: &str,
        mix_id: Option<&str>,
        position: Option<i32>,
    ) {
        self.push_event(
            track,
            "impression",
            surface,
            None,
            false,
            None,
            None,
            mix_id,
            position,
        );
    }

    pub fn mix_click(&self, mix_id: &str) {
        self.push_event(
            None,
            "click",
            "home",
            None,
            false,
            None,
            None,
            Some(mix_id),
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn push_event(
        &self,
        track: Option<&UiTrack>,
        event_type: &str,
        surface: &str,
        position_ms: Option<u64>,
        ended: bool,
        listened_ms: Option<u64>,
        playlist_id: Option<&str>,
        mix_id: Option<&str>,
        clicked_position: Option<i32>,
    ) {
        if self.api.session().is_none() {
            return;
        }
        let valid_surface = normalize_surface(surface);
        let now_utc = Utc::now().to_rfc3339();
        let tz_offset_min = Local::now().offset().local_minus_utc() / 60;

        let (pb_id, started_at, dur_ms) = {
            let mut active = self.active.lock().expect("active lock");
            let pb = active.playback_id.clone();
            let sa = active.started_at.clone();
            let td = active
                .track_duration_ms
                .or_else(|| track.map(|t| (t.duration_sec as u64) * 1000));
            if ended {
                *active = ActivePlayback::default();
            }
            (pb, sa, td)
        };

        let event = crate::models::Event {
            id: Uuid::new_v4().to_string(),
            r#type: event_type.to_string(),
            track_id: track.map(|t| t.remote_id.clone()),
            playback_id: pb_id,
            context: Some(crate::models::EventContext {
                source: Some(valid_surface.to_string()),
                mix_id: mix_id.map(ToOwned::to_owned),
                playlist_id: playlist_id.map(ToOwned::to_owned),
            }),
            position_ms,
            listened_ms,
            track_duration_ms: dur_ms,
            started_at,
            ended_at: if ended { Some(now_utc.clone()) } else { None },
            timezone_offset_minutes: Some(tz_offset_min),
            surface: Some(valid_surface.to_string()),
            session_id: Some(self.session_id.clone()),
            device_id: Some(self.api.config().device_id),
            query: None,
            result_count: None,
            clicked_position,
            occurred_at: now_utc,
        };

        {
            let mut q = self.pending.lock().expect("pending lock");
            q.push_back(event);
            while q.len() > MAX_PENDING {
                q.pop_front();
            }
        }
        self.persist_best_effort();
        self.flush();
    }

    pub fn flush(&self) {
        if self.api.session().is_none() {
            return;
        }
        if self
            .flushing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            self.flushing.store(false, Ordering::SeqCst);
            return;
        };
        let tracker = self.clone();
        handle.spawn(async move {
            loop {
                let batch: Vec<crate::models::Event> = {
                    let q = tracker.pending.lock().expect("pending lock");
                    q.iter().take(BATCH_SIZE).cloned().collect()
                };
                if batch.is_empty() {
                    break;
                }
                match tracker.api.send_events(&batch).await {
                    Ok(()) => {
                        let sent: std::collections::HashSet<String> =
                            batch.iter().map(|e| e.id.clone()).collect();
                        let mut q = tracker.pending.lock().expect("pending lock");
                        retain_unsent(&mut q, &sent);
                        drop(q);
                        tracker.persist_best_effort();
                    }
                    Err(_) => break,
                }
            }
            tracker.flushing.store(false, Ordering::SeqCst);
        });
    }

    fn persist_best_effort(&self) {
        let snapshot: Vec<crate::models::Event> = self
            .pending
            .lock()
            .expect("pending lock")
            .iter()
            .cloned()
            .collect();
        if let Ok(bytes) = serde_json::to_vec(&snapshot) {
            let _ = write_atomic(&self.disk_file, &bytes);
        }
    }
}

fn normalize_surface(raw: &str) -> &'static str {
    let clean = raw.trim().to_ascii_lowercase();
    VALID_SURFACES
        .iter()
        .copied()
        .find(|&s| s == clean)
        .unwrap_or("player")
}

/// Removes the successfully-sent events from the pending queue by identity. Matching on
/// ids (rather than popping a fixed count) ensures events added or re-ordered while the
/// send was in flight are never dropped.
fn retain_unsent(
    q: &mut VecDeque<crate::models::Event>,
    sent_ids: &std::collections::HashSet<String>,
) {
    q.retain(|e| !sent_ids.contains(&e.id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_unknown_surface_to_player() {
        assert_eq!(normalize_surface("HOME"), "home");
        assert_eq!(normalize_surface("search"), "search");
        assert_eq!(normalize_surface("unknown_place"), "player");
    }

    fn ev(id: &str) -> crate::models::Event {
        crate::models::Event {
            id: id.to_string(),
            r#type: "test".to_string(),
            track_id: None,
            playback_id: None,
            context: None,
            position_ms: None,
            listened_ms: None,
            track_duration_ms: None,
            started_at: None,
            ended_at: None,
            timezone_offset_minutes: None,
            surface: None,
            session_id: None,
            device_id: None,
            query: None,
            result_count: None,
            clicked_position: None,
            occurred_at: String::new(),
        }
    }

    #[test]
    fn retain_unsent_drops_only_sent_even_after_overflow_eviction() {
        let mut q: VecDeque<crate::models::Event> = VecDeque::from([ev("A"), ev("B"), ev("C"), ev("D")]);
        // Batch was cloned as [A, B]; a concurrent overflow then evicts A from the front.
        q.pop_front();
        let sent: std::collections::HashSet<String> =
            [ev("A"), ev("B")].iter().map(|e| e.id.clone()).collect();
        retain_unsent(&mut q, &sent);
        let ids: Vec<String> = q.iter().map(|e| e.id.clone()).collect();
        // Only the sent events are removed; C and D (never sent) must survive.
        assert_eq!(ids, vec!["C".to_string(), "D".to_string()]);
    }
}
