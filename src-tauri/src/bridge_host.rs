//! 浏览器扩展的 Native Messaging 注册，以及与主程序之间的本机消息通道。
//!
//! 注册表只负责把 `com.adm.downloader` 指向 exe 同目录的 manifest；
//! 扩展通过这个 host 收到接管请求，再用本地 TCP 交给主程序。

use std::io::{BufRead, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

pub const HOST_NAME: &str = "com.adm.downloader";
const MANIFEST_NAME: &str = "com.adm.downloader.json";
pub const PORTS: [u16; 3] = [51837, 51838, 51839];

/// 与扩展里的 ipc_token.txt 一致；扩展已经写好并固定使用这个值
pub const IPC_TOKEN: &str = "2fc7688c7084b41f3f188f2daa21f9f09866a241f54859d4c026f07979aa5268";

pub fn manifest_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|err| format!("获取程序路径失败：{err}"))?;
    let dir = exe.parent().ok_or_else(|| "无法获取程序目录".to_string())?;
    Ok(dir.join(MANIFEST_NAME))
}

fn registry_key(browser: &str) -> Result<&'static str, String> {
    match browser {
        "edge" => Ok(r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.adm.downloader"),
        "chrome" => Ok(r"HKCU\Software\Google\Chrome\NativeMessagingHosts\com.adm.downloader"),
        other => Err(format!("不支持的浏览器：{other}")),
    }
}

pub fn validate_extension_id(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|ch| ('a'..='p').contains(&ch))
}

fn write_manifest(id: &str) -> Result<PathBuf, String> {
    if !validate_extension_id(id) {
        return Err("扩展 ID 必须是 32 位小写字母 a-p".to_string());
    }

    let path = manifest_path()?;
    let exe = std::env::current_exe().map_err(|err| format!("获取程序路径失败：{err}"))?;

    let value = serde_json::json!({
        "name": HOST_NAME,
        "description": "ADM native host",
        "path": exe,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{id}/")]
    });

    let text = serde_json::to_string_pretty(&value)
        .map_err(|err| format!("生成 manifest 失败：{err}"))?;
    std::fs::write(&path, format!("{text}\n")).map_err(|err| format!("写入 manifest 失败：{err}"))?;
    Ok(path)
}

fn run_reg(args: &[&str]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let status = std::process::Command::new("reg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|err| format!("执行 reg.exe 失败：{err}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("reg.exe 返回失败，可能没有写入权限".to_string())
    }
}

pub fn register(browser: &str, id: &str) -> Result<(), String> {
    let manifest = write_manifest(id)?;
    let key = registry_key(browser)?;
    run_reg(&[
        "add",
        key,
        "/ve",
        "/d",
        &manifest.display().to_string(),
        "/f",
    ])
}

pub fn unregister(browser: &str) -> Result<(), String> {
    let key = registry_key(browser)?;
    let _ = run_reg(&["delete", key, "/f"]);
    Ok(())
}

fn registry_has(browser: &str) -> bool {
    let Ok(key) = registry_key(browser) else {
        return false;
    };
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    std::process::Command::new("reg")
        .args(["query", key, "/ve"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn registered_extension(browser: &str) -> Option<String> {
    if !registry_has(browser) {
        return None;
    }
    let path = manifest_path().ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let origin = value.get("allowed_origins")?.as_array()?.first()?.as_str()?;
    let id = origin.strip_prefix("chrome-extension://")?.trim_end_matches('/');
    if validate_extension_id(id) {
        Some(id.to_string())
    } else {
        None
    }
}

pub fn registered() -> Vec<(String, String)> {
    let mut list = Vec::new();
    for browser in ["edge", "chrome"] {
        if let Some(id) = registered_extension(browser) {
            list.push((browser.to_string(), id));
        }
    }
    list
}

/* ============================== 本机通道 ============================== */

fn connect(timeout: Duration) -> Option<TcpStream> {
    for port in PORTS {
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().ok()?;
        if let Ok(stream) = TcpStream::connect_timeout(&addr, timeout) {
            return Some(stream);
        }
    }
    None
}

fn ensure_main_running() -> bool {
    if connect(Duration::from_millis(200)).is_some() {
        return true;
    }

    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let _ = std::process::Command::new(exe)
        .arg("--background")
        .spawn();

    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if connect(Duration::from_millis(200)).is_some() {
            return true;
        }
    }
    false
}

fn native_reply(value: serde_json::Value) {
    let body = value.to_string();
    let mut output = std::io::stdout();
    let _ = output.write_all(&(body.len() as u32).to_le_bytes());
    let _ = output.write_all(body.as_bytes());
    let _ = output.flush();
}

/// 浏览器调起本程序时，走这个函数处理一次消息后退出。
/// 返回 true 表示"我是被浏览器拉起的"，调用方不要再初始化界面。
pub fn try_native_host() -> bool {
    use std::io::IsTerminal;

    /* 从终端启动（开发/命令行）时直接进 GUI，避免读 stdin 卡住 */
    if std::io::stdin().is_terminal() {
        return false;
    }

    /* 等待浏览器的第一帧；等不到说明是普通 GUI 启动 */
    let (sender, receiver) = mpsc::channel::<u32>();
    std::thread::spawn(move || {
        let mut input = std::io::stdin();
        let mut len_buf = [0u8; 4];
        if input.read_exact(&mut len_buf).is_ok() {
            let _ = sender.send(u32::from_le_bytes(len_buf));
        }
    });

    let Ok(len) = receiver.recv_timeout(Duration::from_millis(400)) else {
        return false;
    };

    let mut body = vec![0u8; len as usize];
    if std::io::stdin().read_exact(&mut body).is_err() {
        return true;
    }

    let Ok(message) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return true;
    };

    let action = message
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("download");

    if action == "formats" {
        native_reply(serde_json::json!({ "ok": true, "formats": [] }));
        return true;
    }

    let url = message
        .get("url")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    if url.is_empty() {
        native_reply(serde_json::json!({ "ok": false, "error": "空下载地址" }));
        return true;
    }

    if !ensure_main_running() {
        native_reply(serde_json::json!({
            "ok": false,
            "error": "主程序启动失败，未接管本次下载"
        }));
        return true;
    }

    let file_name = message
        .get("filename")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let referrer = message
        .get("referrer")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let cookies = message
        .get("cookies")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let user_agent = message
        .get("userAgent")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let file_size = message
        .get("fileSize")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);

    let payload = serde_json::json!({
        "action": "takeover",
        "token": IPC_TOKEN,
        "url": url,
        "file_name": file_name,
        "save_path": "",
        "referrer": referrer,
        "cookies": cookies,
        "user_agent": user_agent,
        "file_size": file_size,
    });

    let ok = send_local(&payload);
    native_reply(if ok {
        serde_json::json!({ "ok": true })
    } else {
        serde_json::json!({ "ok": false, "error": "主程序未能显示接管窗口" })
    });
    true
}

/// 把接管请求通过本地 TCP 发给主程序
pub fn send_local(payload: &serde_json::Value) -> bool {
    let Some(mut stream) = connect(Duration::from_millis(500)) else {
        return false;
    };

    let _ = stream.set_write_timeout(Some(Duration::from_millis(1000)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(3000)));

    let text = payload.to_string();
    if stream.write_all(text.as_bytes()).is_err() || stream.write_all(b"\n").is_err() {
        return false;
    }

    let mut reader = std::io::BufReader::new(&stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return false;
    }

    serde_json::from_str::<serde_json::Value>(&line)
        .ok()
        .and_then(|value| value.get("ok").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

/* ============================== 单实例 ============================== */

static INSTANCE: std::sync::OnceLock<isize> = std::sync::OnceLock::new();

/// 只在同一用户会话里允许一个主程序；返回 false 表示已有实例在跑
pub fn acquire_single_instance() -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    unsafe {
        let name = HSTRING::from("Local\\AdmDownloaderSingleInstance");
        match CreateMutexW(None, false, &name) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    false
                } else {
                    let _ = INSTANCE.set(handle.0 as isize);
                    true
                }
            }
            /* 创建互斥量失败时不阻止启动，避免程序完全打不开 */
            Err(_) => true,
        }
    }
}

/// 已有实例时把它的主窗口叫到前台
pub fn activate_existing() {
    let payload = serde_json::json!({
        "action": "activate",
        "token": IPC_TOKEN,
    });
    let _ = send_local(&payload);
}

pub fn is_http_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}
