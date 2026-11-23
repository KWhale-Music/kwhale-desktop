use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeepLink {
    Track(String),
    Album(String),
    Artist(String),
    Playlist(String),
    Share(String),
}

impl DeepLink {
    /// Parses `kwhale://track/{id}`, `kwhale://album/{id}`, `kwhale://artist/{id}`, `kwhale://playlist/{id}`, `kwhale://share/{id}`.
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let parsed = Url::parse(trimmed).ok()?;
        if !parsed.scheme().eq_ignore_ascii_case("kwhale") {
            return None;
        }
        let host = parsed.host_str()?.to_ascii_lowercase();
        let first_seg = parsed
            .path_segments()?
            .map(str::trim)
            .find(|s| !s.is_empty())?
            .to_string();

        match host.as_str() {
            "track" => Some(Self::Track(first_seg)),
            "album" => Some(Self::Album(first_seg)),
            "artist" => Some(Self::Artist(first_seg)),
            "playlist" => Some(Self::Playlist(first_seg)),
            "share" => Some(Self::Share(first_seg)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DeepLink;

    #[test]
    fn parses_valid_kwhale_uris() {
        assert_eq!(
            DeepLink::parse("kwhale://track/42"),
            Some(DeepLink::Track("42".into()))
        );
        assert_eq!(
            DeepLink::parse("kwhale://album/alb-1?foo=bar"),
            Some(DeepLink::Album("alb-1".into()))
        );
        assert_eq!(
            DeepLink::parse("kwhale://artist/1170"),
            Some(DeepLink::Artist("1170".into()))
        );
        assert_eq!(
            DeepLink::parse("kwhale://share/AbCd123"),
            Some(DeepLink::Share("AbCd123".into()))
        );
    }

    #[test]
    fn rejects_invalid_uris() {
        assert_eq!(DeepLink::parse("https://music.example.org/track/42"), None);
        assert_eq!(DeepLink::parse("kwhale://track/"), None);
        assert_eq!(DeepLink::parse("kwhale://unknown/42"), None);
    }
}
