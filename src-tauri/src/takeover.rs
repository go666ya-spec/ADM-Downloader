//! 接管请求：始终只有一个二级窗。
//!
//! - 窗口未关闭时，新接管直接在同一个窗口里**重新渲染**内容；
//! - 窗口关掉后，下一次接管重新创建。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::Notify;

use serde::Serialize;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const WIN_LABEL: &str = "takeover";
const WIN_WIDTH: f64 = 520.0;
const WIN_HEIGHT: f64 = 300.0;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingTakeover {
    pub url: String,
    pub file_name: String,
    pub save_dir: String,
    pub referrer: String,
    pub cookies: String,
    /// 扩展传来的真实 User-Agent，可为空
    pub user_agent: String,
    pub file_size: u64,
}

static PENDING: Mutex<Option<PendingTakeover>> = Mutex::new(None);

/// 前端渲染完成的握手：每次接管 +1
static READY_COUNTER: AtomicU64 = AtomicU64::new(0);
/// 置顶计时器编号：每次接管重新计时，旧定时器作废
static TOPMOST_SEQ: AtomicU64 = AtomicU64::new(0);
static READY_NOTIFY: OnceLock<Arc<Notify>> = OnceLock::new();

fn ready_signal() -> &'static Arc<Notify> {
    READY_NOTIFY.get_or_init(|| Arc::new(Notify::new()))
}

fn ready_count() -> u64 {
    READY_COUNTER.load(Ordering::SeqCst)
}

/// 等前端把新内容渲染完；超时兜底，避免窗口一直不显示
async fn wait_ready(previous: u64) {
    let signal = ready_signal().clone();
    let waiter = async move {
        loop {
            if ready_count() > previous {
                return;
            }
            signal.notified().await;
        }
    };

    let _ = tokio::time::timeout(std::time::Duration::from_millis(800), waiter).await;
}

fn show_window(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.set_always_on_top(true);
    let _ = window.show();
    let _ = window.set_focus();
    schedule_release_topmost(window);
}

/// 弹出后 500ms 内保持置顶，之后解除，避免一直遮挡其他窗口
fn schedule_release_topmost(window: &tauri::WebviewWindow) {
    let sequence = TOPMOST_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    let topmost = window.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        /* 只有最后一次接管的定时器才能取消置顶 */
        if TOPMOST_SEQ.load(Ordering::SeqCst) == sequence {
            let _ = topmost.set_always_on_top(false);
        }
    });
}

pub fn accept(
    app: &tauri::AppHandle,
    url: &str,
    file_name: Option<String>,
    save_dir: Option<String>,
    referrer: String,
    cookies: String,
    user_agent: String,
    file_size: u64,
) -> Result<PendingTakeover, String> {
    let trimmed = url.trim().to_string();
    if trimmed.is_empty() {
        return Err("链接为空".to_string());
    }

    let settings = crate::settings::current();

    let name = file_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| crate::transfer::file_name_from_url(&trimmed));

    let directory = save_dir
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or(settings.save_dir);

    let pending = PendingTakeover {
        url: trimmed,
        file_name: name,
        save_dir: directory,
        referrer: referrer.trim().to_string(),
        cookies: cookies.trim().to_string(),
        user_agent: user_agent.trim().to_string(),
        file_size,
    };

    if let Ok(mut slot) = PENDING.lock() {
        *slot = Some(pending.clone());
    }

    if let Some(window) = app.get_webview_window(WIN_LABEL) {
        /* 单实例：复用窗口，把内容换成新的接管请求 */
        /* 先把内容换掉；置顶/聚焦延迟到前端渲染完成之后 */
        let previous = ready_count();
        let _ = window.emit("takeover:pending", pending.clone());

        let handle = window.clone();
        tauri::async_runtime::spawn(async move {
            wait_ready(previous).await;
            /* 再等 300ms，确保新内容已经上屏再置顶 */
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            show_window(&handle);
        });

        return Ok(pending);
    }

    let url = format!("popup.html?theme={}", crate::boot_theme_param());
    let window = WebviewWindowBuilder::new(app, WIN_LABEL, WebviewUrl::App(url.into()))
        .title("新建下载")
        .background_color(crate::boot_background())
        .inner_size(WIN_WIDTH, WIN_HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .visible(false)
        .center()
        .data_directory(crate::paths::data_dir().join("webview2"))
        .build()
        .map_err(|error| format!("无法创建接管窗口：{error}"))?;

    /* 关闭只隐藏，下次接管继续复用 */
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(window) = handle.get_webview_window(WIN_LABEL) {
                let _ = window.hide();
            }
        }
    });

    let previous = ready_count();
    let handle = window.clone();
    tauri::async_runtime::spawn(async move {
        wait_ready(previous).await;
        show_window(&handle);
    });

    Ok(pending)
}

#[tauri::command]
pub fn takeover_begin(
    app: tauri::AppHandle,
    url: String,
    file_name: Option<String>,
    save_dir: Option<String>,
    referrer: Option<String>,
    cookies: Option<String>,
    user_agent: Option<String>,
    file_size: Option<u64>,
) -> Result<PendingTakeover, String> {
    accept(
        &app,
        &url,
        file_name,
        save_dir,
        referrer.unwrap_or_default(),
        cookies.unwrap_or_default(),
        user_agent.unwrap_or_default(),
        file_size.unwrap_or(0),
    )
}

#[tauri::command]
pub fn takeover_ready() {
    READY_COUNTER.fetch_add(1, Ordering::SeqCst);
    ready_signal().notify_one();
}

#[tauri::command]
pub fn takeover_pending() -> Option<PendingTakeover> {
    PENDING.lock().ok().and_then(|slot| slot.clone())
}

#[tauri::command]
pub fn takeover_close(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(WIN_LABEL) {
        let _ = window.hide();
    }
}
