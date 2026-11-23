use url::Url;

/// Normalizes user-entered server addresses into a canonical `.../v1` base URL.
///
/// Examples:
/// - `music.example.org` -> `https://music.example.org/v1`
/// - `https://music.example.org/` -> `https://music.example.org/v1`
/// - `http://192.168.1.229:19001` -> `http://192.168.1.229:19001/v1`
pub struct ServerUrl;

impl ServerUrl {
    pub const DEFAULT_PROD: &'static str = "https://music.dueattendant149.org/v1";

    pub const PRESETS: &'static [&'static str] = &[
        "https://music.dueattendant149.org/v1",
        "https://test.dueattendant149.org/v1",
        "http://192.168.1.229:19001/v1",
        "http://127.0.0.1:19001/v1",
    ];

    pub fn normalize(raw: &str, fallback: Option<&str>) -> Option<String> {
        let mut value = raw.trim().to_string();
        if value.is_empty() {
            return fallback.map(|s| s.to_string());
        }

        if value.contains("://")
            && !value.starts_with("http://")
            && !value.starts_with("https://")
        {
            return None;
        }

        if !value.starts_with("http://") && !value.starts_with("https://") {
            value = format!("https://{value}");
        }

        let parsed = Url::parse(&value).ok()?;
        let host = parsed.host_str()?.trim();
        if host.is_empty() {
            return None;
        }
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return None;
        }

        let trimmed = value.trim_end_matches('/');
        if trimmed.ends_with("/v1") {
            Some(trimmed.to_string())
        } else {
            Some(format!("{trimmed}/v1"))
        }
    }

    pub fn is_http(url: &str) -> bool {
        url.starts_with("http://")
    }

    /// Returns true when the host is loopback or an RFC1918/local LAN host where plain HTTP is normal.
    pub fn is_local_or_lan(url: &str) -> bool {
        let Ok(parsed) = Url::parse(url) else {
            return false;
        };
        let Some(host) = parsed.host_str() else {
            return false;
        };
        let lower = host.to_ascii_lowercase();
        if lower == "localhost"
            || lower == "127.0.0.1"
            || lower == "::1"
            || lower == "[::1]"
            || lower.ends_with(".local")
            || lower.ends_with(".lan")
        {
            return true;
        }
        if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
            return match ip {
                std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
                std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local(),
            };
        }
        false
    }

    pub fn extract_host(url: &str) -> Option<String> {
        Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::ServerUrl;

    #[test]
    fn normalizes_bare_domain_and_trailing_slash() {
        assert_eq!(
            ServerUrl::normalize("music.example.org", None).as_deref(),
            Some("https://music.example.org/v1")
        );
        assert_eq!(
            ServerUrl::normalize("https://music.example.org/", None).as_deref(),
            Some("https://music.example.org/v1")
        );
        assert_eq!(
            ServerUrl::normalize("http://192.168.1.229:19001/v1/", None).as_deref(),
            Some("http://192.168.1.229:19001/v1")
        );
    }

    #[test]
    fn rejects_non_http_schemes_and_empty_hosts() {
        assert_eq!(ServerUrl::normalize("ftp://example.org", None), None);
        assert_eq!(ServerUrl::normalize("https://", None), None);
        assert_eq!(
            ServerUrl::normalize("   ", Some(ServerUrl::DEFAULT_PROD)).as_deref(),
            Some(ServerUrl::DEFAULT_PROD)
        );
    }

    #[test]
    fn identifies_lan_vs_public_hosts() {
        assert!(ServerUrl::is_local_or_lan("http://127.0.0.1:19001/v1"));
        assert!(ServerUrl::is_local_or_lan("http://192.168.1.229:19001/v1"));
        assert!(ServerUrl::is_local_or_lan("http://localhost:19001/v1"));
        assert!(!ServerUrl::is_local_or_lan("http://music.dueattendant149.org/v1"));
    }
}
