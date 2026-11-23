use futures_util::StreamExt;
use kwhale_core::{sanitize_key, KwhaleCore};
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE};
use std::fs;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use uuid::Uuid;

const PREFETCH_LOOKAHEAD: usize = 5;
const PREFETCH_MAX_BYTES: u64 = 80 * 1024 * 1024;

/// Localhost-only HTTP/1.1 proxy so GStreamer `playbin3` can stream authenticated
/// `/v1/tracks/{id}/stream` bytes with `Range` seeking and `401` single-flight refresh,
/// while atomically warming `PlaybackCache` on complete 200/206 fetches.
pub struct StreamProxy {
    port: u16,
    nonce: String,
}

impl StreamProxy {
    pub async fn start(core: Arc<KwhaleCore>) -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let nonce = Uuid::new_v4().to_string();
        let expected_nonce = nonce.clone();

        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let c = core.clone();
                let n = expected_nonce.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(stream, c, &n).await {
                        debug!("stream proxy connection closed: {err}");
                    }
                });
            }
        });

        Ok(Self { port, nonce })
    }

    pub fn url_for(&self, track_id: &str) -> String {
        format!(
            "http://127.0.0.1:{}/track/{}/{}",
            self.port, self.nonce, track_id
        )
    }
}

async fn handle_connection(
    stream: TcpStream,
    core: Arc<KwhaleCore>,
    expected_nonce: &str,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut request_line = String::new();
    if buf_reader.read_line(&mut request_line).await? == 0 {
        return Ok(());
    }

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        writer.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n").await?;
        return Ok(());
    }
    let method = parts[0];
    let path = parts[1];

    let mut range_header: Option<String> = None;
    loop {
        let mut line = String::new();
        let n = buf_reader.read_line(&mut line).await?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("range") {
                range_header = Some(v.trim().to_string());
            }
        }
    }

    if method != "GET" && method != "HEAD" {
        writer
            .write_all(b"HTTP/1.1 405 Method Not Allowed\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    }

    let prefix = format!("/track/{expected_nonce}/");
    let Some(track_id) = path.strip_prefix(&prefix) else {
        writer
            .write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    };
    let track_id = track_id.split('?').next().unwrap_or(track_id);

    // Serve from PlaybackCache if complete on disk.
    if let Some(cached) = core.playback_cache.get_cached_path(track_id) {
        return serve_local_file(&mut writer, &cached, method, range_header.as_deref()).await;
    }

    let resp = match core
        .api
        .open_stream(track_id, range_header.as_deref())
        .await
    {
        Ok(r) => r,
        Err(err) => {
            warn!("upstream stream open failed for {track_id}: {err}");
            let code = err.status_code().unwrap_or(502);
            let msg = format!("HTTP/1.1 {code} Bad Gateway\r\nConnection: close\r\n\r\n");
            writer.write_all(msg.as_bytes()).await?;
            return Ok(());
        }
    };

    let status = resp.status().as_u16();
    let reason = resp.status().canonical_reason().unwrap_or("OK");
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("audio/mpeg")
        .to_string();
    let content_len = resp
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());
    let content_range = resp
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned);

    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n"
    );
    if let Some(len) = content_len {
        head.push_str(&format!("Content-Length: {len}\r\n"));
    }
    if let Some(cr) = &content_range {
        head.push_str(&format!("Content-Range: {cr}\r\n"));
    }
    head.push_str("\r\n");
    writer.write_all(head.as_bytes()).await?;

    if method == "HEAD" {
        return Ok(());
    }

    // Only cache when upstream gives the full body (200 or 206 covering bytes 0-(total-1)/total).
    let full_expected_len = should_cache_response(status, content_len, content_range.as_deref());
    let tmp_path = full_expected_len.map(|_| {
        core.paths
            .playback_cache_dir()
            .join(format!("stream_{}_{}.part", sanitize_key(track_id), Uuid::new_v4()))
    });
    let mut cache_file = match &tmp_path {
        Some(p) => tokio::fs::File::create(p).await.ok(),
        None => None,
    };

    let mut written: u64 = 0;
    let mut stream = resp.bytes_stream();
    let mut clean_eof = true;
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                if let Some(f) = cache_file.as_mut() {
                    let _ = f.write_all(&bytes).await;
                }
                if writer.write_all(&bytes).await.is_err() {
                    clean_eof = false;
                    break;
                }
                written += bytes.len() as u64;
            }
            Err(_) => {
                clean_eof = false;
                break;
            }
        }
    }
    let _ = writer.flush().await;

    if let (Some(tmp), Some(expected)) = (tmp_path, full_expected_len) {
        if let Some(mut f) = cache_file {
            let _ = f.flush().await;
            drop(f);
        }
        if clean_eof && written == expected && written > 0 {
            let _ = core.playback_cache.commit_temp_file(track_id, &tmp);
        } else {
            let _ = fs::remove_file(&tmp);
        }
    }
    Ok(())
}

async fn serve_local_file(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    path: &std::path::Path,
    method: &str,
    range_header: Option<&str>,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut file = tokio::fs::File::open(path).await?;
    let total = file.metadata().await?.len();

    let (status, start, end) = if let Some((s, e)) = range_header.and_then(|r| parse_range(r, total)) {
        (206u16, s, e)
    } else {
        (200u16, 0, total.saturating_sub(1))
    };

    if total == 0 || start >= total || start > end {
        let resp = format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nConnection: close\r\n\r\n");
        writer.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let len = end - start + 1;
    let mut head = if status == 206 {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Type: audio/mpeg\r\nAccept-Ranges: bytes\r\nContent-Length: {len}\r\nContent-Range: bytes {start}-{end}/{total}\r\nConnection: close\r\n\r\n"
        )
    } else {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nAccept-Ranges: bytes\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
        )
    };
    writer.write_all(head.as_bytes()).await?;
    if method == "HEAD" {
        return Ok(());
    }

    file.seek(std::io::SeekFrom::Start(start)).await?;
    let mut remaining = len;
    let mut buf = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let to_read = (remaining as usize).min(buf.len());
        let n = file.read(&mut buf[..to_read]).await?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await?;
        remaining -= n as u64;
    }
    let _ = writer.flush().await;
    let _ = &mut head;
    Ok(())
}

fn parse_range(raw: &str, total: u64) -> Option<(u64, u64)> {
    let spec = raw.trim().strip_prefix("bytes=")?;
    let (s_str, e_str) = spec.split_once('-')?;
    if total == 0 {
        return None;
    }
    if s_str.is_empty() {
        let suffix: u64 = e_str.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        let start = total.saturating_sub(suffix);
        return Some((start, total - 1));
    }
    let start: u64 = s_str.parse().ok()?;
    let end = if e_str.is_empty() {
        total - 1
    } else {
        e_str.parse::<u64>().ok()?.min(total - 1)
    };
    if start <= end {
        Some((start, end))
    } else {
        None
    }
}

fn should_cache_response(
    status: u16,
    content_length: Option<u64>,
    content_range: Option<&str>,
) -> Option<u64> {
    if status == 200 {
        return content_length.filter(|&l| l > 0 && l <= PREFETCH_MAX_BYTES);
    }
    if status == 206 {
        let cr = content_range?.trim().strip_prefix("bytes ")?;
        let (range_part, total_part) = cr.split_once('/')?;
        let total: u64 = total_part.parse().ok()?;
        let (start_s, end_s) = range_part.split_once('-')?;
        let start: u64 = start_s.parse().ok()?;
        let end: u64 = end_s.parse().ok()?;
        if start == 0 && end + 1 == total && total <= PREFETCH_MAX_BYTES {
            return Some(total);
        }
    }
    None
}

/// Lookahead prefetcher warming the next 5 tracks of the queue into `PlaybackCache`.
#[derive(Clone)]
pub struct QueuePrefetcher {
    core: Arc<KwhaleCore>,
    active: Arc<Mutex<Option<(String, CancellationToken)>>>,
}

impl QueuePrefetcher {
    pub fn new(core: Arc<KwhaleCore>) -> Self {
        Self {
            core,
            active: Arc::new(Mutex::new(None)),
        }
    }

    pub fn schedule(&self, track_ids: Vec<String>) {
        let core = self.core.clone();
        let active = self.active.clone();
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async move {
            if !core.settings.snapshot().prefetch_enabled || track_ids.is_empty() {
                let mut g = active.lock().await;
                if let Some((_, tok)) = g.take() {
                    tok.cancel();
                }
                return;
            }
            let ids: Vec<String> = track_ids.into_iter().take(PREFETCH_LOOKAHEAD).collect();
            let key = ids.join("|");
            let token = {
                let mut g = active.lock().await;
                if let Some((prev_key, prev_tok)) = g.as_ref() {
                    if prev_key == &key {
                        return;
                    }
                    prev_tok.cancel();
                }
                let tok = CancellationToken::new();
                *g = Some((key.clone(), tok.clone()));
                tok
            };

            for id in ids {
                if token.is_cancelled() || !core.settings.snapshot().prefetch_enabled {
                    break;
                }
                if core.downloads.local_file_for(&id).is_some()
                    || core.playback_cache.get_cached_path(&id).is_some()
                {
                    continue;
                }
                let resp = tokio::select! {
                    _ = token.cancelled() => break,
                    res = core.api.open_stream(&id, None) => match res {
                        Ok(r) => r,
                        Err(_) => continue,
                    }
                };
                let expected = resp.content_length();
                if expected.map(|l| l > PREFETCH_MAX_BYTES).unwrap_or(false) {
                    continue;
                }
                let tmp = core
                    .paths
                    .playback_cache_dir()
                    .join(format!("prefetch_{}_{}.part", sanitize_key(&id), Uuid::new_v4()));
                let Ok(mut f) = tokio::fs::File::create(&tmp).await else {
                    continue;
                };
                let mut written = 0u64;
                let mut ok = true;
                let mut stream = resp.bytes_stream();
                loop {
                    tokio::select! {
                        _ = token.cancelled() => {
                            ok = false;
                            break;
                        }
                        item = stream.next() => {
                            match item {
                                Some(Ok(bytes)) => {
                                    written += bytes.len() as u64;
                                    if written > PREFETCH_MAX_BYTES || f.write_all(&bytes).await.is_err() {
                                        ok = false;
                                        break;
                                    }
                                }
                                Some(Err(_)) => {
                                    ok = false;
                                    break;
                                }
                                None => break,
                            }
                        }
                    }
                }
                let _ = f.flush().await;
                drop(f);
                if ok && written > 0 && expected.map(|e| e == written).unwrap_or(true) {
                    let _ = core.playback_cache.commit_temp_file(&id, &tmp);
                } else {
                    let _ = fs::remove_file(&tmp);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parser_handles_standard_and_suffix_ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=-200", 1000), Some((800, 999)));
        assert_eq!(parse_range("bytes=0-5000", 1000), Some((0, 999)));
        assert_eq!(parse_range("bytes=900-100", 1000), None);
        assert_eq!(parse_range("bytes=0-10", 0), None);
    }

    #[test]
    fn caches_only_full_200_or_full_206_responses() {
        assert_eq!(should_cache_response(200, Some(4096), None), Some(4096));
        assert_eq!(
            should_cache_response(206, Some(4096), Some("bytes 0-4095/4096")),
            Some(4096)
        );
        assert_eq!(
            should_cache_response(206, Some(2048), Some("bytes 2048-4095/4096")),
            None
        );
        assert_eq!(should_cache_response(404, Some(100), None), None);
    }
}
