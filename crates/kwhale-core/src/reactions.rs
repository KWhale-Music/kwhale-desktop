use crate::events::EventTracker;
use crate::network::KwhaleApi;
use crate::playback_state::UiTrack;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tokio::sync::watch;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReactionOverrides {
    pub liked: HashMap<String, bool>,
    pub disliked: HashMap<String, bool>,
    pub censorship: HashMap<String, bool>,
    pub censorship_in_flight: HashSet<String>,
}

#[derive(Clone)]
pub struct TrackReactionStore {
    api: KwhaleApi,
    events: EventTracker,
    state: Arc<RwLock<ReactionOverrides>>,
    tx: watch::Sender<ReactionOverrides>,
}

impl TrackReactionStore {
    pub fn new(api: KwhaleApi, events: EventTracker) -> Self {
        let initial = ReactionOverrides::default();
        let (tx, _) = watch::channel(initial.clone());
        Self {
            api,
            events,
            state: Arc::new(RwLock::new(initial)),
            tx,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<ReactionOverrides> {
        self.tx.subscribe()
    }

    pub fn snapshot(&self) -> ReactionOverrides {
        self.state.read().expect("reaction lock").clone()
    }

    pub fn is_liked(&self, track_id: &str, server_liked: bool) -> bool {
        self.state
            .read()
            .expect("reaction lock")
            .liked
            .get(track_id)
            .copied()
            .unwrap_or(server_liked)
    }

    pub fn is_disliked(&self, track_id: &str, server_disliked: bool) -> bool {
        self.state
            .read()
            .expect("reaction lock")
            .disliked
            .get(track_id)
            .copied()
            .unwrap_or(server_disliked)
    }

    pub fn is_censorship_reported(&self, track_id: &str, server_reported: bool) -> bool {
        self.state
            .read()
            .expect("reaction lock")
            .censorship
            .get(track_id)
            .copied()
            .unwrap_or(server_reported)
    }

    fn mutate<F: FnOnce(&mut ReactionOverrides)>(&self, f: F) {
        let next = {
            let mut guard = self.state.write().expect("reaction lock");
            f(&mut guard);
            guard.clone()
        };
        let _ = self.tx.send(next);
    }

    pub async fn toggle_like(&self, track: &UiTrack, surface: &str) -> Result<bool, String> {
        let remote_id = track.remote_id.clone();
        if remote_id.trim().is_empty() {
            return Err("Локальный трек нельзя добавить в избранное сервера".into());
        }
        let (prev_liked_entry, prev_liked, prev_disliked_entry) = {
            let st = self.state.read().expect("reaction lock");
            let pl = st.liked.get(&remote_id).copied();
            let pd = st.disliked.get(&remote_id).copied();
            (pl, pl.unwrap_or(track.is_liked), pd)
        };
        let next_liked = !prev_liked;
        self.mutate(|st| {
            st.liked.insert(remote_id.clone(), next_liked);
            if next_liked {
                st.disliked.insert(remote_id.clone(), false);
            }
        });

        let res = if next_liked {
            self.api.like(&remote_id).await
        } else {
            self.api.unlike(&remote_id).await
        };

        match res {
            Ok(()) => {
                if next_liked {
                    self.events.like(track, surface, None);
                } else {
                    self.events.unlike(track, surface, None);
                }
                Ok(next_liked)
            }
            Err(err) => {
                self.mutate(|st| {
                    match prev_liked_entry {
                        Some(v) => {
                            st.liked.insert(remote_id.clone(), v);
                        }
                        None => {
                            st.liked.remove(&remote_id);
                        }
                    }
                    if next_liked {
                        match prev_disliked_entry {
                            Some(v) => {
                                st.disliked.insert(remote_id.clone(), v);
                            }
                            None => {
                                st.disliked.remove(&remote_id);
                            }
                        }
                    }
                });
                Err(err.to_string())
            }
        }
    }

    pub async fn toggle_dislike(&self, track: &UiTrack, surface: &str) -> Result<bool, String> {
        let remote_id = track.remote_id.clone();
        if remote_id.trim().is_empty() {
            return Err("Локальный трек нельзя скрыть на сервере".into());
        }
        let (prev_disliked_entry, prev_disliked, prev_liked_entry) = {
            let st = self.state.read().expect("reaction lock");
            let pd = st.disliked.get(&remote_id).copied();
            let pl = st.liked.get(&remote_id).copied();
            (pd, pd.unwrap_or(track.is_disliked), pl)
        };
        let next_disliked = !prev_disliked;
        self.mutate(|st| {
            st.disliked.insert(remote_id.clone(), next_disliked);
            if next_disliked {
                st.liked.insert(remote_id.clone(), false);
            }
        });

        let res = if next_disliked {
            self.api.dislike(&remote_id).await
        } else {
            self.api.undislike(&remote_id).await
        };

        match res {
            Ok(()) => {
                if next_disliked {
                    self.events.dislike(track, surface, None);
                } else {
                    self.events.undislike(track, surface, None);
                }
                Ok(next_disliked)
            }
            Err(err) => {
                self.mutate(|st| {
                    match prev_disliked_entry {
                        Some(v) => {
                            st.disliked.insert(remote_id.clone(), v);
                        }
                        None => {
                            st.disliked.remove(&remote_id);
                        }
                    }
                    if next_disliked {
                        match prev_liked_entry {
                            Some(v) => {
                                st.liked.insert(remote_id.clone(), v);
                            }
                            None => {
                                st.liked.remove(&remote_id);
                            }
                        }
                    }
                });
                Err(err.to_string())
            }
        }
    }

    pub async fn report_censorship_one_click(
        &self,
        track: &UiTrack,
        position_ms: Option<u64>,
    ) -> Result<&'static str, String> {
        let remote_id = track.remote_id.clone();
        if self.is_censorship_reported(&remote_id, track.is_censorship_reported) {
            return Ok("Трек уже на пересмотре");
        }
        let prev_entry = {
            let st = self.state.read().expect("reaction lock");
            if st.censorship_in_flight.contains(&remote_id) {
                return Ok("Заявка уже отправляется");
            }
            st.censorship.get(&remote_id).copied()
        };

        self.mutate(|st| {
            st.censorship_in_flight.insert(remote_id.clone());
            st.censorship.insert(remote_id.clone(), true);
        });

        let res = self
            .api
            .report_censorship(&remote_id, "censored", None, position_ms)
            .await;

        self.mutate(|st| {
            st.censorship_in_flight.remove(&remote_id);
        });

        match res {
            Ok(_) => Ok("Отправлено на пересмотр в админку"),
            Err(err) => {
                self.mutate(|st| match prev_entry {
                    Some(v) => {
                        st.censorship.insert(remote_id.clone(), v);
                    }
                    None => {
                        st.censorship.remove(&remote_id);
                    }
                });
                if err.is_not_found() {
                    Err("На сервере ещё не подключён эндпоинт цензуры".into())
                } else {
                    Err(err.to_string())
                }
            }
        }
    }

    pub async fn cancel_censorship_report(&self, track: &UiTrack) -> Result<&'static str, String> {
        let remote_id = track.remote_id.clone();
        let prev_entry = self
            .state
            .read()
            .expect("reaction lock")
            .censorship
            .get(&remote_id)
            .copied();
        self.mutate(|st| {
            st.censorship.insert(remote_id.clone(), false);
        });
        match self.api.cancel_censorship_report(&remote_id).await {
            Ok(()) => Ok("Отправка на пересмотр отменена"),
            Err(err) => {
                self.mutate(|st| match prev_entry {
                    Some(v) => {
                        st.censorship.insert(remote_id.clone(), v);
                    }
                    None => {
                        st.censorship.remove(&remote_id);
                    }
                });
                Err(err.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Session, User};
    use crate::network::ApiConfig;
    use crate::storage::{AppPaths, TokenStore};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_track(id: &str) -> UiTrack {
        UiTrack {
            id: UiTrack::stable_id_for(id),
            remote_id: id.to_string(),
            title: "Song".into(),
            artist: "Artist".into(),
            artists: vec!["Artist".into()],
            artist_remote_id: None,
            album: None,
            album_remote_id: None,
            duration_sec: 180,
            genre: None,
            year: None,
            is_explicit: false,
            stream_path: format!("/tracks/{id}/stream"),
            cover_image_id: None,
            has_lyrics: false,
            is_liked: false,
            is_disliked: true,
            is_offline_cached: false,
            is_censorship_reported: false,
        }
    }

    #[tokio::test]
    async fn optimistic_like_clears_dislike_and_rolls_back_on_error() {
        let server = MockServer::start().await;
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = AppPaths::for_base(dir.path());
        let token_store = TokenStore::in_memory(Some(Session {
            access_token: "tok".into(),
            refresh_token: "ref".into(),
            expires_at_epoch_sec: 9_999_999_999,
            user: User {
                id: "u1".into(),
                display_name: "U".into(),
                email: None,
                avatar: None,
                created_at: None,
            },
        }));
        let cfg = ApiConfig::new(&server.uri(), "0.1.0", "dev-1", true).expect("cfg");
        let api = KwhaleApi::new(cfg, token_store).expect("api");
        let events = EventTracker::new(api.clone(), &paths);
        let store = TrackReactionStore::new(api, events);

        Mock::given(method("PUT"))
            .and(path("/v1/tracks/t1/like"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let t1 = sample_track("t1");
        let res = store.toggle_like(&t1, "home").await;
        assert_eq!(res, Ok(true));
        assert!(store.is_liked("t1", false));
        assert!(!store.is_disliked("t1", true));

        // Failure on t2 rolls back optimistic like
        Mock::given(method("PUT"))
            .and(path("/v1/tracks/t2/like"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let t2 = sample_track("t2");
        assert!(store.toggle_like(&t2, "home").await.is_err());
        assert!(!store.is_liked("t2", false));
    }
}
