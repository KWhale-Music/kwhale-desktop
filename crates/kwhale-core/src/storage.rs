use crate::error::CoreResult;
use crate::models::Session;
use crate::server_url::ServerUrl;
use crate::theme::{ThemeMode, UiBackend};
use directories::ProjectDirs;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Self {
        if let Some(proj) = ProjectDirs::from("org.dueattendant149", "KWhale", "kwhale") {
            let paths = Self {
                config_dir: proj.config_dir().to_path_buf(),
                data_dir: proj.data_dir().to_path_buf(),
                cache_dir: proj.cache_dir().to_path_buf(),
            };
            paths.ensure_dirs();
            return paths;
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let paths = Self {
            config_dir: home.join(".config/kwhale"),
            data_dir: home.join(".local/share/kwhale"),
            cache_dir: home.join(".cache/kwhale"),
        };
        paths.ensure_dirs();
        paths
    }

    pub fn for_base(base: &Path) -> Self {
        let paths = Self {
            config_dir: base.join("config"),
            data_dir: base.join("data"),
            cache_dir: base.join("cache"),
        };
        paths.ensure_dirs();
        paths
    }

    pub fn ensure_dirs(&self) {
        let _ = fs::create_dir_all(&self.config_dir);
        let _ = fs::create_dir_all(&self.data_dir);
        let _ = fs::create_dir_all(&self.cache_dir);
        let _ = fs::create_dir_all(self.downloads_dir());
        let _ = fs::create_dir_all(self.ui_cache_dir());
        let _ = fs::create_dir_all(self.image_cache_dir());
        let _ = fs::create_dir_all(self.playback_cache_dir());
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    pub fn session_file(&self) -> PathBuf {
        self.data_dir.join("session.json")
    }

    pub fn pending_events_file(&self) -> PathBuf {
        self.data_dir.join("pending_events.json")
    }

    pub fn downloads_index_file(&self) -> PathBuf {
        self.data_dir.join("downloads.json")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.data_dir.join("downloads")
    }

    pub fn ui_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("ui-cache")
    }

    pub fn image_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("images")
    }

    pub fn playback_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("playback-cache")
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", Uuid::new_v4()));
    write_private(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Writes a file with owner-only (0600) permissions so secrets (tokens, API keys)
/// are never world-readable, even for an instant during the atomic rename.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_api_url")]
    pub api_base_url: String,
    #[serde(default)]
    pub allow_insecure_http: bool,
    #[serde(default)]
    pub theme_mode: ThemeMode,
    #[serde(default = "default_accent_id")]
    pub accent_id: i32,
    #[serde(default)]
    pub follow_system_accent: bool,
    #[serde(default)]
    pub ui_backend: UiBackend,
    #[serde(default)]
    pub llm_api_key: String,
    #[serde(default)]
    pub llm_endpoint: String,
    #[serde(default)]
    pub llm_model: String,
    #[serde(default = "default_true")]
    pub prefetch_enabled: bool,
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default)]
    pub muted: bool,
    #[serde(default = "default_device_id")]
    pub device_id: String,
    #[serde(default = "default_window_width")]
    pub window_width: i32,
    #[serde(default = "default_window_height")]
    pub window_height: i32,
}

fn default_api_url() -> String {
    ServerUrl::DEFAULT_PROD.to_string()
}

fn default_accent_id() -> i32 {
    2
}

fn default_true() -> bool {
    true
}

fn default_volume() -> f64 {
    1.0
}

fn default_device_id() -> String {
    format!("desktop-{}", Uuid::new_v4())
}

fn default_window_width() -> i32 {
    1200
}

fn default_window_height() -> i32 {
    780
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            api_base_url: default_api_url(),
            allow_insecure_http: false,
            theme_mode: ThemeMode::Dark,
            accent_id: default_accent_id(),
            follow_system_accent: false,
            ui_backend: UiBackend::Auto,
            llm_api_key: String::new(),
            llm_endpoint: String::new(),
            llm_model: String::new(),
            prefetch_enabled: true,
            volume: 1.0,
            muted: false,
            device_id: default_device_id(),
            window_width: default_window_width(),
            window_height: default_window_height(),
        }
    }
}

#[derive(Clone)]
pub struct SettingsStore {
    file: PathBuf,
    inner: Arc<RwLock<AppSettings>>,
}

impl SettingsStore {
    pub fn load(paths: &AppPaths) -> Self {
        let file = paths.settings_file();
        let mut settings = fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
            .unwrap_or_default();
        settings.api_base_url = ServerUrl::normalize(&settings.api_base_url, Some(ServerUrl::DEFAULT_PROD))
            .unwrap_or_else(|| ServerUrl::DEFAULT_PROD.to_string());
        if settings.device_id.trim().is_empty() {
            settings.device_id = default_device_id();
        }
        let store = Self {
            file,
            inner: Arc::new(RwLock::new(settings)),
        };
        store.persist();
        store
    }

    pub fn snapshot(&self) -> AppSettings {
        self.inner.read().expect("settings lock").clone()
    }

    pub fn update<F: FnOnce(&mut AppSettings)>(&self, f: F) -> AppSettings {
        let snap = {
            let mut guard = self.inner.write().expect("settings lock");
            f(&mut guard);
            guard.api_base_url = ServerUrl::normalize(&guard.api_base_url, Some(ServerUrl::DEFAULT_PROD))
                .unwrap_or_else(|| ServerUrl::DEFAULT_PROD.to_string());
            guard.llm_endpoint = guard.llm_endpoint.trim().trim_end_matches('/').to_string();
            guard.llm_api_key = guard.llm_api_key.trim().to_string();
            guard.llm_model = guard.llm_model.trim().to_string();
            guard.volume = guard.volume.clamp(0.0, 1.0);
            guard.clone()
        };
        self.persist();
        snap
    }

    fn persist(&self) {
        let snap = self.snapshot();
        if let Ok(json) = serde_json::to_vec_pretty(&snap) {
            let _ = write_atomic(&self.file, &json);
        }
    }
}

/// Canonical 0600 file token store (mirrors EncryptedTokenStore reliability fix from 0.1.11)
/// with optional best-effort SecretService mirror via `oo7`.
#[derive(Clone)]
pub struct TokenStore {
    file: PathBuf,
    cache: Arc<RwLock<Option<Session>>>,
    io_lock: Arc<Mutex<()>>,
}

impl TokenStore {
    pub fn new(paths: &AppPaths) -> Self {
        let file = paths.session_file();
        let initial = fs::read_to_string(&file)
            .ok()
            .and_then(|raw| serde_json::from_str::<Session>(&raw).ok());
        Self {
            file,
            cache: Arc::new(RwLock::new(initial)),
            io_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn in_memory(initial: Option<Session>) -> Self {
        let tmp = std::env::temp_dir().join(format!("kwhale-mem-{}.json", Uuid::new_v4()));
        Self {
            file: tmp,
            cache: Arc::new(RwLock::new(initial)),
            io_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn load(&self) -> Option<Session> {
        self.cache.read().expect("token lock").clone()
    }

    pub fn save(&self, session: Session) {
        let _guard = self.io_lock.lock().expect("token io lock");
        *self.cache.write().expect("token lock") = Some(session.clone());
        if let Ok(json) = serde_json::to_vec(&session) {
            let _ = write_atomic(&self.file, &json);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&self.file, fs::Permissions::from_mode(0o600));
            }
        }
        let s_clone = session;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = save_keyring_best_effort(&s_clone).await;
            });
        }
    }

    pub fn clear(&self) {
        let _guard = self.io_lock.lock().expect("token io lock");
        *self.cache.write().expect("token lock") = None;
        let _ = fs::remove_file(&self.file);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = clear_keyring_best_effort().await;
            });
        }
    }

    /// Best-effort recovery from SecretService if local file is missing.
    pub async fn try_recover_from_keyring(&self) -> Option<Session> {
        if let Some(existing) = self.load() {
            return Some(existing);
        }
        if let Ok(Some(session)) = load_keyring_best_effort().await {
            self.save(session.clone());
            return Some(session);
        }
        None
    }
}

async fn save_keyring_best_effort(session: &Session) -> anyhow::Result<()> {
    let keyring = oo7::Keyring::new().await?;
    let json = serde_json::to_string(session)?;
    keyring
        .create_item(
            "KWhale Session",
            &[("app", "org.dueattendant149.Kwhale"), ("type", "session")],
            json,
            true,
        )
        .await?;
    Ok(())
}

async fn load_keyring_best_effort() -> anyhow::Result<Option<Session>> {
    let keyring = oo7::Keyring::new().await?;
    let items = keyring
        .search_items(&[("app", "org.dueattendant149.Kwhale"), ("type", "session")])
        .await?;
    if let Some(item) = items.first() {
        let secret = item.secret().await?;
        let text = String::from_utf8(secret.to_vec())?;
        let session = serde_json::from_str::<Session>(&text)?;
        return Ok(Some(session));
    }
    Ok(None)
}

async fn clear_keyring_best_effort() -> anyhow::Result<()> {
    let keyring = oo7::Keyring::new().await?;
    keyring
        .delete(&[("app", "org.dueattendant149.Kwhale"), ("type", "session")])
        .await?;
    Ok(())
}

/// Small atomic JSON cache for UI snapshots matching Android's ContentCache.
#[derive(Clone)]
pub struct ContentCache {
    dir: PathBuf,
    memory: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl ContentCache {
    pub const HOME: &'static str = "home-v1";
    pub const LIBRARY: &'static str = "library-v1";
    pub const PROFILE: &'static str = "profile-v1";
    pub const SEARCH_BOOTSTRAP: &'static str = "search-bootstrap-v1";

    pub fn collection_key(kind: &str, id: &str) -> String {
        let safe_id = sanitize_key(id);
        format!("collection-v1-{kind}-{safe_id}")
    }

    pub fn new(paths: &AppPaths) -> Self {
        Self {
            dir: paths.ui_cache_dir(),
            memory: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        if let Some(bytes) = self.memory.read().expect("cache lock").get(key) {
            if let Ok(parsed) = serde_json::from_slice::<T>(bytes) {
                return Some(parsed);
            }
        }
        let path = self.file(key);
        let bytes = fs::read(&path).ok()?;
        let parsed = serde_json::from_slice::<T>(&bytes).ok()?;
        self.memory
            .write()
            .expect("cache lock")
            .insert(key.to_string(), bytes);
        Some(parsed)
    }

    pub fn write<T: Serialize>(&self, key: &str, value: &T) {
        if let Ok(bytes) = serde_json::to_vec(value) {
            self.memory
                .write()
                .expect("cache lock")
                .insert(key.to_string(), bytes.clone());
            let path = self.file(key);
            let _ = write_atomic(&path, &bytes);
        }
    }

    pub fn is_fresh(&self, key: &str, ttl: Duration) -> bool {
        let Ok(meta) = fs::metadata(self.file(key)) else {
            return false;
        };
        let Ok(modified) = meta.modified() else {
            return false;
        };
        SystemTime::now()
            .duration_since(modified)
            .map(|age| age < ttl)
            .unwrap_or(false)
    }

    pub fn invalidate(&self, key: &str) {
        self.memory.write().expect("cache lock").remove(key);
        let _ = fs::remove_file(self.file(key));
    }

    pub fn invalidate_home(&self) {
        self.invalidate(Self::HOME);
    }

    pub fn invalidate_library(&self) {
        self.invalidate(Self::LIBRARY);
    }

    pub fn invalidate_profile(&self) {
        self.invalidate(Self::PROFILE);
    }

    pub fn invalidate_search_bootstrap(&self) {
        self.invalidate(Self::SEARCH_BOOTSTRAP);
    }

    pub fn invalidate_collection(&self, kind: &str, id: &str) {
        self.invalidate(&Self::collection_key(kind, id));
    }

    fn file(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{}.json", sanitize_key(key)))
    }
}

/// 256 MB LRU disk cache for cover art.
#[derive(Clone)]
pub struct ImageCache {
    dir: PathBuf,
    max_bytes: u64,
    lock: Arc<Mutex<()>>,
}

impl ImageCache {
    pub const DEFAULT_MAX_BYTES: u64 = 256 * 1024 * 1024;

    pub fn new(paths: &AppPaths) -> Self {
        Self {
            dir: paths.image_cache_dir(),
            max_bytes: Self::DEFAULT_MAX_BYTES,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn path_for(&self, image_id: &str, size: &str) -> PathBuf {
        self.dir
            .join(format!("{}_{}.img", sanitize_key(image_id), sanitize_key(size)))
    }

    pub fn get(&self, image_id: &str, size: &str) -> Option<Vec<u8>> {
        let path = self.path_for(image_id, size);
        fs::read(path).ok()
    }

    pub fn put(&self, image_id: &str, size: &str, bytes: &[u8]) -> CoreResult<PathBuf> {
        let _guard = self.lock.lock().expect("img cache lock");
        let path = self.path_for(image_id, size);
        write_atomic(&path, bytes)?;
        self.evict_if_needed();
        Ok(path)
    }

    fn evict_if_needed(&self) {
        evict_dir_lru(&self.dir, self.max_bytes);
    }
}

/// 1 GB LRU disk cache for streamed audio (keyed with `v2_track_` matching Android's `v2:track:` reset).
#[derive(Clone)]
pub struct PlaybackCache {
    dir: PathBuf,
    max_bytes: u64,
    lock: Arc<Mutex<()>>,
}

impl PlaybackCache {
    pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024 * 1024;

    pub fn new(paths: &AppPaths) -> Self {
        Self {
            dir: paths.playback_cache_dir(),
            max_bytes: Self::DEFAULT_MAX_BYTES,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn max_mb(&self) -> u64 {
        self.max_bytes / (1024 * 1024)
    }

    pub fn used_bytes(&self) -> u64 {
        dir_entries_by_mtime(&self.dir)
            .into_iter()
            .map(|(_, size, _)| size)
            .sum()
    }

    pub fn used_mb(&self) -> u64 {
        self.used_bytes() / (1024 * 1024)
    }

    pub fn file_for(&self, track_id: &str) -> PathBuf {
        self.dir.join(format!("v2_track_{}.bin", sanitize_key(track_id)))
    }

    pub fn get_cached_path(&self, track_id: &str) -> Option<PathBuf> {
        let p = self.file_for(track_id);
        if p.is_file() && fs::metadata(&p).map(|m| m.len() > 0).unwrap_or(false) {
            Some(p)
        } else {
            None
        }
    }

    pub fn put_bytes(&self, track_id: &str, bytes: &[u8]) -> CoreResult<PathBuf> {
        let _guard = self.lock.lock().expect("playback cache lock");
        let p = self.file_for(track_id);
        write_atomic(&p, bytes)?;
        evict_dir_lru(&self.dir, self.max_bytes);
        Ok(p)
    }

    pub fn commit_temp_file(&self, track_id: &str, tmp: &Path) -> CoreResult<PathBuf> {
        let _guard = self.lock.lock().expect("playback cache lock");
        let p = self.file_for(track_id);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(tmp, &p)?;
        evict_dir_lru(&self.dir, self.max_bytes);
        Ok(p)
    }

    pub fn clear(&self) {
        let _guard = self.lock.lock().expect("playback cache lock");
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

pub fn sanitize_key(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn dir_entries_by_mtime(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("tmp") {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                items.push((path, meta.len(), mtime));
            }
        }
    }
    items.sort_by_key(|(_, _, t)| *t);
    items
}

fn evict_dir_lru(dir: &Path, max_bytes: u64) {
    let items = dir_entries_by_mtime(dir);
    let mut total: u64 = items.iter().map(|(_, s, _)| *s).sum();
    for (path, size, _) in items {
        if total <= max_bytes {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::User;

    #[test]
    fn write_atomic_creates_owner_only_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("secret.json");
        write_atomic(&p, b"{\"token\":\"s\"}").expect("write");
        assert!(p.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&p).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "secrets must never be world-readable");
        }
    }

    #[test]
    fn settings_content_and_playback_cache_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = AppPaths::for_base(dir.path());

        let settings = SettingsStore::load(&paths);
        let updated = settings.update(|s| {
            s.api_base_url = "https://music.example.com/".to_string();
            s.volume = 1.5;
            s.accent_id = 42;
        });
        assert_eq!(updated.api_base_url, "https://music.example.com/v1");
        assert!((updated.volume - 1.0).abs() < f64::EPSILON);
        assert_eq!(updated.accent_id, 42);

        let cache = ContentCache::new(&paths);
        cache.write(ContentCache::HOME, &vec!["a".to_string(), "b".to_string()]);
        let read_back: Option<Vec<String>> = cache.read(ContentCache::HOME);
        assert_eq!(read_back, Some(vec!["a".to_string(), "b".to_string()]));
        assert!(cache.is_fresh(ContentCache::HOME, Duration::from_secs(60)));
        cache.invalidate_home();
        assert!(cache.read::<Vec<String>>(ContentCache::HOME).is_none());

        let pb_cache = PlaybackCache::new(&paths);
        let saved = pb_cache.put_bytes("tr/123", b"audio-bytes").expect("put");
        assert!(saved.is_file());
        assert_eq!(pb_cache.get_cached_path("tr/123"), Some(saved));
        pb_cache.clear();
        assert!(pb_cache.get_cached_path("tr/123").is_none());

        let tok = TokenStore::new(&paths);
        let sess = Session {
            access_token: "acc".into(),
            refresh_token: "ref".into(),
            expires_at_epoch_sec: 9999999999,
            user: User {
                id: "u1".into(),
                display_name: "Tester".into(),
                email: Some("t@example.com".into()),
                avatar: None,
                created_at: None,
            },
        };
        tok.save(sess.clone());
        assert_eq!(tok.load(), Some(sess));
        tok.clear();
        assert!(tok.load().is_none());
    }
}
