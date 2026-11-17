//! 本机接管通道：扩展的 native host 通过 127.0.0.1 的固定端口把请求交给主程序。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use tauri::AppHandle;

use crate::bridge_host::{is_http_url, IPC_TOKEN, PORTS};

const MAX_REQUEST: u64 = 512 * 1024;

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut listener: Option<TcpListener> = None;
        let mut bound = 0u16;

        for port in PORTS {
            match TcpListener::bind(("127.0.0.1", port)) {
                Ok(value) => {
                    listener = Some(value);
                    bound = port;
                    break;
                }
                Err(error) => eprintln!("[bridge] 端口 {port} 绑定失败: {error}"),
            }
        }

        let Some(listener) = listener else {
            eprintln!("[bridge] 本地接管通道启动失败：端口全部被占用");
            return;
        };

        write_endpoint(bound);
        eprintln!("[bridge] 本地接管通道监听 127.0.0.1:{bound}");

        for stream in listener.incoming().flatten() {
            let app = app.clone();
            std::thread::spawn(move || handle(stream, app));
        }
    });
}

fn write_endpoint(port: u16) {
    let dir = crate::paths::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let value = serde_json::json!({ "port": port, "pid": std::process::id() });
    let _ = std::fs::write(
        dir.join("ipc.json"),
        format!("{}\n", value),
    );
}

fn handle(mut stream: TcpStream, app: AppHandle) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));

    let mut line = String::new();
    let mut reader = BufReader::new((&stream).take(MAX_REQUEST + 1));

    if reader.read_line(&mut line).is_err() || line.len() as u64 > MAX_REQUEST {
        let _ = respond(&mut stream, serde_json::json!({ "ok": false, "error": "请求过大或超时" }));
        return;
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
        let _ = respond(&mut stream, serde_json::json!({ "ok": false, "error": "请求格式错误" }));
        return;
    };

    let token_ok = value.get("token").and_then(|v| v.as_str()) == Some(IPC_TOKEN);
    if !token_ok {
        let _ = respond(&mut stream, serde_json::json!({ "ok": false, "error": "校验码无效" }));
        return;
    }

    let action = value.get("action").and_then(|v| v.as_str()).unwrap_or("");

    let response = match action {
        "ping" => serde_json::json!({ "ok": true }),
        "activate" => {
            crate::tray::show_main(&app);
            serde_json::json!({ "ok": true })
        }
        "takeover" => {
            let url = value.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let file_name = value
                .get("file_name")
                .and_then(|v| v.as_str())
                .filter(|text| !text.trim().is_empty())
                .map(str::to_string);
            let save_path = value
                .get("save_path")
                .and_then(|v| v.as_str())
                .filter(|text| !text.trim().is_empty())
                .map(str::to_string);
            let referrer = value
                .get("referrer")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let cookies = value
                .get("cookies")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let user_agent = value
                .get("user_agent")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let file_size = value
                .get("file_size")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            if url.trim().is_empty() {
                serde_json::json!({ "ok": false, "error": "空下载地址" })
            } else if !is_http_url(url) {
                serde_json::json!({ "ok": false, "error": "只接受 HTTP/HTTPS 下载" })
            } else {
                match crate::takeover::accept(&app, url, file_name, save_path, referrer, cookies, user_agent, file_size) {
                    Ok(_) => serde_json::json!({ "ok": true }),
                    Err(error) => serde_json::json!({ "ok": false, "error": error }),
                }
            }
        }
        _ => serde_json::json!({ "ok": false, "error": "未知操作" }),
    };

    let _ = respond(&mut stream, response);
}

fn respond(stream: &mut TcpStream, value: serde_json::Value) -> std::io::Result<()> {
    let text = value.to_string();
    stream.write_all(text.as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
}
