use std::borrow::Cow;

use tauri::http::{Request, Response, StatusCode};
use tauri::{Manager, Runtime, UriSchemeContext};

use crate::state::AppState;

/// shot:// 协议：前端用 http://shot.localhost/<key> 读取内存中的截图数据
pub fn protocol<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    req: Request<Vec<u8>>,
) -> Response<Cow<'static, [u8]>> {
    let key = req.uri().path().trim_start_matches('/').to_string();
    let state = ctx.app_handle().state::<AppState>();
    match state.get_blob(&key) {
        Some(blob) => Response::builder()
            .header("Content-Type", blob.mime)
            .header("Access-Control-Allow-Origin", "*")
            .header("Cache-Control", "no-store")
            .body(Cow::Owned(blob.data.as_ref().clone()))
            .unwrap(),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Access-Control-Allow-Origin", "*")
            .body(Cow::Borrowed(&[][..]))
            .unwrap(),
    }
}
