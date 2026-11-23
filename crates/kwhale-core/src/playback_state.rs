use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiTrack {
    pub id: i64,
    pub remote_id: String,
    pub title: String,
    pub artist: String,
    pub artists: Vec<String>,
    pub artist_remote_id: Option<String>,
    pub album: Option<String>,
    pub album_remote_id: Option<String>,
    pub duration_sec: u32,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub is_explicit: bool,
    pub stream_path: String,
    pub cover_image_id: Option<String>,
    pub has_lyrics: bool,
    pub is_liked: bool,
    pub is_disliked: bool,
    pub is_offline_cached: bool,
    pub is_censorship_reported: bool,
}

impl UiTrack {
    pub fn stable_id_for(remote_id: &str) -> i64 {
        if let Ok(v) = remote_id.parse::<i64>() {
            return v;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        remote_id.hash(&mut h);
        h.finish() as i64
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    pub queue: Vec<UiTrack>,
    pub index: usize,
    pub is_playing: bool,
    pub is_buffering: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
    pub volume: f64,
    pub muted: bool,
    pub surface: String,
    pub mix_id: Option<String>,
    pub error: Option<String>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            queue: Vec::new(),
            index: 0,
            is_playing: false,
            is_buffering: false,
            position_ms: 0,
            duration_ms: 0,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
            volume: 1.0,
            muted: false,
            surface: "player".to_string(),
            mix_id: None,
            error: None,
        }
    }
}

impl PlaybackState {
    pub fn current(&self) -> Option<&UiTrack> {
        self.queue.get(self.index)
    }

    pub fn progress(&self) -> f32 {
        if self.duration_ms == 0 {
            0.0
        } else {
            (self.position_ms as f32 / self.duration_ms as f32).clamp(0.0, 1.0)
        }
    }

    pub fn repeat(&self) -> bool {
        self.repeat_mode != RepeatMode::Off
    }
}
