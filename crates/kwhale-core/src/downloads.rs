use crate::network::KwhaleApi;
use crate::playback_state::UiTrack;
use crate::storage::{sanitize_key, write_atomic, AppPaths};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{watch, Semaphore};
use tokio_util::sync::CancellationToken;

const MIN_RELIABLE_DURATION_SEC: i32 = 40;
const DURATION_TOLERANCE_SEC: i32 = 30;

pub type DurationProbeFn = Arc<dyn Fn(&Path) -> Option<u32> + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedTrack {
    pub remote_id: String,
    pub stream_path: String,
    pub title: String,
    pub artist: String,
    pub file_path: String,
    #[serde(default)]
    pub cover_image_id: Option<String>,
    #[serde(default)]
    pub duration_sec: u32,
    #[serde(default = "default_mime")]
    pub mime_type: String,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub downloaded_at_ms: i64,
}

fn default_mime() -> String {
    "audio/mpeg".to_string()
}

impl DownloadedTrack {
    pub fn exists(&self) -> bool {
        Path::new(&self.file_path).is_file()
    }

    pub fn to_ui_track(&self) -> UiTrack {
        UiTrack {
            id: UiTrack::stable_id_for(&self.remote_id),
            remote_id: self.remote_id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            artists: vec![self.artist.clone()],
            artist_remote_id: None,
            album: None,
            album_remote_id: None,
            duration_sec: self.duration_sec,
            genre: None,
            year: None,
            is_explicit: false,
            stream_path: self.stream_path.clone(),
            cover_image_id: self.cover_image_id.clone(),
            has_lyrics: false,
            is_liked: false,
            is_disliked: false,
            is_offline_cached: true,
            is_censorship_reported: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackDownloadStatus {
    NotDownloaded,
    Queued,
    Downloading { percent: u8 },
    Downloaded { file_path: PathBuf },
    Failed { message: String },
}

#[derive(Debug, Clone)]
pub struct ActiveDownload {
    pub track: UiTrack,
    pub status: TrackDownloadStatus,
}

#[derive(Debug, Clone, Default)]
pub struct DownloadSnapshot {
    pub downloaded: Vec<DownloadedTrack>,
    pub active: Vec<ActiveDownload>,
}

impl DownloadSnapshot {
    pub fn status_of(&self, remote_id: &str) -> TrackDownloadStatus {
        if let Some(a) = self.active.iter().find(|a| a.track.remote_id == remote_id) {
            return a.status.clone();
        }
        if let Some(d) = self.downloaded.iter().find(|d| d.remote_id == remote_id) {
            if d.exists() {
                return TrackDownloadStatus::Downloaded {
                    file_path: PathBuf::from(&d.file_path),
                };
            }
        }
        TrackDownloadStatus::NotDownloaded
    }

    pub fn local_file_for(&self, remote_id: &str) -> Option<PathBuf> {
        self.downloaded
            .iter()
            .find(|d| d.remote_id == remote_id && d.exists())
            .map(|d| PathBuf::from(&d.file_path))
    }
}

#[derive(Clone)]
pub struct DownloadManager {
    api: KwhaleApi,
    index_file: PathBuf,
    downloads_dir: PathBuf,
    downloaded: Arc<RwLock<HashMap<String, DownloadedTrack>>>,
    active: Arc<RwLock<HashMap<String, ActiveDownload>>>,
    tokens: Arc<Mutex<HashMap<String, CancellationToken>>>,
    semaphore: Arc<Semaphore>,
    duration_probe: Arc<RwLock<Option<DurationProbeFn>>>,
    tx: watch::Sender<DownloadSnapshot>,
}

impl DownloadManager {
    pub fn new(api: KwhaleApi, paths: &AppPaths) -> Self {
        let index_file = paths.downloads_index_file();
        let downloads_dir = paths.downloads_dir();
        let _ = fs::create_dir_all(&downloads_dir);

        let initial_map: HashMap<String, DownloadedTrack> = fs::read_to_string(&index_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<DownloadedTrack>>(&s).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|d| d.exists())
            .map(|d| (d.remote_id.clone(), d))
            .collect();

        let snap = Self::build_snapshot(&initial_map, &HashMap::new());
        let (tx, _) = watch::channel(snap);

        Self {
            api,
            index_file,
            downloads_dir,
            downloaded: Arc::new(RwLock::new(initial_map)),
            active: Arc::new(RwLock::new(HashMap::new())),
            tokens: Arc::new(Mutex::new(HashMap::new())),
            semaphore: Arc::new(Semaphore::new(3)),
            duration_probe: Arc::new(RwLock::new(None)),
            tx,
        }
    }

    pub fn set_duration_probe(&self, probe: DurationProbeFn) {
        *self.duration_probe.write().expect("probe lock") = Some(probe);
    }

    pub fn subscribe(&self) -> watch::Receiver<DownloadSnapshot> {
        self.tx.subscribe()
    }

    pub fn snapshot(&self) -> DownloadSnapshot {
        self.tx.borrow().clone()
    }

    pub fn local_file_for(&self, remote_id: &str) -> Option<PathBuf> {
        self.downloaded
            .read()
            .expect("dl lock")
            .get(remote_id)
            .filter(|d| d.exists())
            .map(|d| PathBuf::from(&d.file_path))
    }

    pub fn is_downloaded(&self, remote_id: &str) -> bool {
        self.local_file_for(remote_id).is_some()
    }

    pub fn enqueue(&self, track: UiTrack, replace_existing: bool) {
        let remote_id = track.remote_id.clone();
        if remote_id.trim().is_empty() {
            return;
        }
        if !replace_existing && self.local_file_for(&remote_id).is_some() {
            return;
        }
        if !replace_existing
            && self
                .active
                .read()
                .expect("active dl")
                .get(&remote_id)
                .map(|a| {
                    matches!(
                        a.status,
                        TrackDownloadStatus::Queued | TrackDownloadStatus::Downloading { .. }
                    )
                })
                .unwrap_or(false)
        {
            return;
        }

        let token = CancellationToken::new();
        {
            let mut toks = self.tokens.lock().expect("dl tokens");
            if let Some(old) = toks.insert(remote_id.clone(), token.clone()) {
                old.cancel();
            }
        }
        {
            let mut act = self.active.write().expect("active dl");
            act.insert(
                remote_id.clone(),
                ActiveDownload {
                    track: track.clone(),
                    status: TrackDownloadStatus::Queued,
                },
            );
        }
        self.publish();

        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let mgr = self.clone();
        handle.spawn(async move {
            mgr.run_download(track, token).await;
        });
    }

    pub fn enqueue_all(&self, tracks: Vec<UiTrack>) -> usize {
        let mut count = 0;
        for t in tracks {
            if !t.remote_id.trim().is_empty() && self.local_file_for(&t.remote_id).is_none() {
                self.enqueue(t, false);
                count += 1;
            }
        }
        count
    }

    pub fn cancel(&self, remote_id: &str) {
        if let Some(tok) = self.tokens.lock().expect("dl tokens").remove(remote_id) {
            tok.cancel();
        }
        self.active.write().expect("active dl").remove(remote_id);
        self.publish();
    }

    pub fn remove(&self, remote_id: &str) {
        self.cancel(remote_id);
        let removed = self.downloaded.write().expect("dl lock").remove(remote_id);
        if let Some(entry) = removed {
            let _ = fs::remove_file(entry.file_path);
        }
        self.persist_index();
        self.publish();
    }

    pub fn clear_all(&self) {
        let ids: Vec<String> = self
            .downloaded
            .read()
            .expect("dl lock")
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.remove(&id);
        }
    }

    async fn run_download(&self, track: UiTrack, token: CancellationToken) {
        let permit = tokio::select! {
            _ = token.cancelled() => return,
            res = self.semaphore.acquire() => match res {
                Ok(p) => p,
                Err(_) => return,
            }
        };

        let remote_id = track.remote_id.clone();
        let safe_id = sanitize_key(&remote_id);
        let tmp_path = self.downloads_dir.join(format!("{safe_id}.part"));

        self.set_active_status(&remote_id, TrackDownloadStatus::Downloading { percent: 0 });

        let mut last_pct = 0u8;
        let dl_fut = self.api.download_track_to(&remote_id, &tmp_path, |got, total| {
            if let Some(t) = total.filter(|&x| x > 0) {
                let pct = ((got.saturating_mul(100)) / t).min(100) as u8;
                if pct != last_pct {
                    last_pct = pct;
                    self.set_active_status(
                        &remote_id,
                        TrackDownloadStatus::Downloading { percent: pct },
                    );
                }
            }
        });

        let outcome = tokio::select! {
            _ = token.cancelled() => {
                let _ = fs::remove_file(&tmp_path);
                return;
            }
            res = dl_fut => res,
        };
        drop(permit);

        match outcome {
            Ok(mime_opt) => {
                let ext = ext_for_mime(mime_opt.as_deref());
                let final_path = self.downloads_dir.join(format!("{safe_id}.{ext}"));
                if fs::rename(&tmp_path, &final_path).is_err() {
                    let _ = fs::remove_file(&tmp_path);
                    self.set_active_status(
                        &remote_id,
                        TrackDownloadStatus::Failed {
                            message: "Не удалось сохранить файл".into(),
                        },
                    );
                    return;
                }

                // Content verification matching DownloadWorker.kt (±30s tolerance for >40s tracks).
                let probe = self.duration_probe.read().expect("probe lock").clone();
                let measured_sec = probe.as_ref().and_then(|f| f(&final_path)).unwrap_or(0);
                if measured_sec > 0 && track.duration_sec as i32 > MIN_RELIABLE_DURATION_SEC {
                    let diff = (measured_sec as i32 - track.duration_sec as i32).abs();
                    if diff > DURATION_TOLERANCE_SEC {
                        let _ = fs::remove_file(&final_path);
                        self.set_active_status(
                            &remote_id,
                            TrackDownloadStatus::Failed {
                                message: "Несовпадение длительности файла".into(),
                            },
                        );
                        return;
                    }
                }

                let bytes = fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
                let record = DownloadedTrack {
                    remote_id: remote_id.clone(),
                    stream_path: track.stream_path.clone(),
                    title: track.title.clone(),
                    artist: track.artist.clone(),
                    file_path: final_path.to_string_lossy().to_string(),
                    cover_image_id: track.cover_image_id.clone(),
                    duration_sec: if measured_sec > 0 {
                        measured_sec
                    } else {
                        track.duration_sec
                    },
                    mime_type: mime_opt.unwrap_or_else(default_mime),
                    bytes,
                    downloaded_at_ms: chrono::Utc::now().timestamp_millis(),
                };
                self.downloaded
                    .write()
                    .expect("dl lock")
                    .insert(remote_id.clone(), record);
                self.active.write().expect("active dl").remove(&remote_id);
                self.tokens.lock().expect("dl tokens").remove(&remote_id);
                self.persist_index();
                self.publish();
            }
            Err(err) => {
                let _ = fs::remove_file(&tmp_path);
                self.set_active_status(
                    &remote_id,
                    TrackDownloadStatus::Failed {
                        message: err.to_string(),
                    },
                );
            }
        }
    }

    fn set_active_status(&self, remote_id: &str, status: TrackDownloadStatus) {
        if let Some(entry) = self.active.write().expect("active dl").get_mut(remote_id) {
            entry.status = status;
        }
        self.publish();
    }

    fn persist_index(&self) {
        let list: Vec<DownloadedTrack> = self
            .downloaded
            .read()
            .expect("dl lock")
            .values()
            .cloned()
            .collect();
        if let Ok(bytes) = serde_json::to_vec_pretty(&list) {
            let _ = write_atomic(&self.index_file, &bytes);
        }
    }

    fn build_snapshot(
        dl: &HashMap<String, DownloadedTrack>,
        act: &HashMap<String, ActiveDownload>,
    ) -> DownloadSnapshot {
        let mut downloaded: Vec<DownloadedTrack> = dl.values().cloned().collect();
        downloaded.sort_by_key(|a| std::cmp::Reverse(a.downloaded_at_ms));
        let mut active: Vec<ActiveDownload> = act.values().cloned().collect();
        active.sort_by(|a, b| a.track.title.cmp(&b.track.title));
        DownloadSnapshot { downloaded, active }
    }

    fn publish(&self) {
        let snap = Self::build_snapshot(
            &self.downloaded.read().expect("dl lock"),
            &self.active.read().expect("active dl"),
        );
        let _ = self.tx.send(snap);
    }
}

pub fn ext_for_mime(mime: Option<&str>) -> &'static str {
    let base = mime
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match base.as_str() {
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => "m4a",
        "audio/ogg" | "application/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/aac" => "aac",
        _ => "mp3",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_mime_types_to_extensions() {
        assert_eq!(ext_for_mime(Some("audio/flac; charset=binary")), "flac");
        assert_eq!(ext_for_mime(Some("audio/mp4")), "m4a");
        assert_eq!(ext_for_mime(Some("audio/ogg")), "ogg");
        assert_eq!(ext_for_mime(None), "mp3");
    }
}
