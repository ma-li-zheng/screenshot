use std::borrow::Cow;
use std::io::{Read, Seek, SeekFrom};

use tauri::http::{Request, Response, StatusCode};
use tauri::{Manager, Runtime, UriSchemeContext};

use crate::state::AppState;

fn not_found() -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Access-Control-Allow-Origin", "*")
        .body(Cow::Borrowed(&[][..]))
        .unwrap()
}

/// shot:// 协议：前端用 http://shot.localhost/<key> 读取内存中的截图数据，
/// 或用 http://shot.localhost/file/<id> 读取本地文件（录屏结果，支持 Range 以便拖动进度）
pub fn protocol<R: Runtime>(ctx: UriSchemeContext<'_, R>, req: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let key = req.uri().path().trim_start_matches('/').to_string();
    let state = ctx.app_handle().state::<AppState>();
    if let Some(id) = key.strip_prefix("file/") {
        let path = state.files.lock().unwrap().get(id).cloned();
        return match path {
            Some(p) => serve_file(&p, req.headers().get("range").and_then(|v| v.to_str().ok())).unwrap_or_else(|_| not_found()),
            None => not_found(),
        };
    }
    match state.get_blob(&key) {
        Some(blob) => Response::builder()
            .header("Content-Type", blob.mime)
            .header("Access-Control-Allow-Origin", "*")
            .header("Cache-Control", "no-store")
            .body(Cow::Owned(blob.data.as_ref().clone()))
            .unwrap(),
        None => not_found(),
    }
}

/// 解析 "bytes=a-b"，返回闭区间
fn parse_range(range: &str, len: u64) -> Option<(u64, u64)> {
    let spec = range.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (len.saturating_sub(n), len.checked_sub(1)?)
        }
        (a, "") => (a.parse().ok()?, len.checked_sub(1)?),
        (a, b) => (a.parse().ok()?, b.parse::<u64>().ok()?.min(len.checked_sub(1)?)),
    };
    (start <= end && end < len).then_some((start, end))
}

fn serve_file(path: &std::path::Path, range: Option<&str>) -> std::io::Result<Response<Cow<'static, [u8]>>> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let mime = match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("mp4") => "video/mp4",
        Some("gif") => "image/gif",
        _ => "application/octet-stream",
    };
    let builder = Response::builder()
        .header("Content-Type", mime)
        .header("Accept-Ranges", "bytes")
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store");
    // 每次最多返回 4 MB，播放器会继续按需请求
    const CHUNK: u64 = 4 * 1024 * 1024;
    match range.and_then(|r| parse_range(r, len)) {
        Some((start, end)) => {
            let end = end.min(start + CHUNK - 1);
            let mut buf = vec![0u8; (end - start + 1) as usize];
            f.seek(SeekFrom::Start(start))?;
            f.read_exact(&mut buf)?;
            Ok(builder
                .status(StatusCode::PARTIAL_CONTENT)
                .header("Content-Range", format!("bytes {start}-{end}/{len}"))
                .body(Cow::Owned(buf))
                .unwrap())
        }
        None => {
            let mut buf = Vec::with_capacity(len as usize);
            f.read_to_end(&mut buf)?;
            Ok(builder.body(Cow::Owned(buf)).unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_range;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=900-", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=500-5000", 1000), Some((500, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None);
    }
}
