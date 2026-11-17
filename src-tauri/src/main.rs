#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::error::Error as StdError;
use std::path::{Path, PathBuf};

use tauri::{App, Manager, WebviewUrl, WebviewWindowBuilder};

mod delete;
mod paths;
mod sync;
mod open;
mod pick;
mod queue;
mod settings;
mod startup;
mod store;
mod takeover;
mod toast;
mod bridge_host;
mod bridge_server;
mod priority;
mod tray;
mod system;
mod transfer;
mod watchers;

/// 主窗口默认尺寸（逻辑像素）。调整窗口大小只需要改这两个值。
const WINDOW_WIDTH: f64 = 1100.0;
const WINDOW_HEIGHT: f64 = 800.0;

/// 最小窗口：保证仪表盘始终 4 + 2 + 1，不触发 2 + 2 + 2 + 1
const MIN_WINDOW_WIDTH: f64 = 980.0;
const MIN_WINDOW_HEIGHT: f64 = 720.0;

/// 最小尺寸 = 默认尺寸 × 该比例，跟随上面的常量自动变化。
/* 启动底色：和 tokens.css 里的 --bg 保持一致，避免 WebView2 就绪前白屏 */
const DARK_BG: tauri::utils::config::Color = tauri::utils::config::Color(0x17, 0x14, 0x1f, 255);
const LIGHT_BG: tauri::utils::config::Color = tauri::utils::config::Color(0xea, 0xf0, 0xf6, 255);

/// 建窗口前判断主题：固定模式读配置，跟随系统才读一次注册表
pub fn boot_is_dark() -> bool {
    match crate::settings::current().theme.as_str() {
        "dark" => true,
        "light" => false,
        _ => !crate::system::apps_use_light(),
    }
}

pub fn boot_background() -> tauri::utils::config::Color {
    if boot_is_dark() {
        DARK_BG
    } else {
        LIGHT_BG
    }
}

pub fn boot_theme_param() -> &'static str {
    if boot_is_dark() {
        "dark"
    } else {
        "light"
    }
}

/// 浏览器接管确认窗：固定尺寸，不随主窗口变化。

fn prepare_data_dir(_app: &App) -> Result<PathBuf, Box<dyn StdError>> {
    println!("[adm] ① 解析应用数据目录");
    let dir = crate::paths::data_dir();
    println!("[adm]    -> {}", dir.display());

    println!("[adm] ② 创建目录");
    std::fs::create_dir_all(&dir)?;

    println!("[adm] ③ 写入探测文件");
    let probe = dir.join("write-probe.txt");
    std::fs::write(&probe, b"ok")?;
    println!("[adm]    -> 可写: {}", probe.display());

    Ok(dir)
}

fn build_main_window(app: &mut App, data_dir: &Path) -> tauri::Result<()> {
    println!("[adm] ④ 创建窗口与 WebView2");
    let url = format!("index.html?theme={}", boot_theme_param());

    /* 开机自启：从创建那一刻就隐藏，避免先闪一下窗口再消失 */
    let background = std::env::args().any(|arg| arg == "--background");

    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(url.into()))
        .title("ADM 下载器")
        .background_color(boot_background())
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .min_inner_size(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)
        .resizable(true)
        .decorations(false)
        .visible(!background)
        .skip_taskbar(background)
        .center()
        .data_directory(data_dir.join("webview2"))
        .build()?;
    println!("[adm] ⑤ 主窗口创建完成");

    /* 关闭按钮永远只是隐藏到托盘：主窗口没有"退出程序"这个动作 */
    let handle = app.handle().clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(window) = handle.get_webview_window("main") {
                /* 隐藏后同时从任务栏和 Alt-Tab 里移除 */
                let _ = window.set_skip_taskbar(true);
                let _ = window.hide();
            }
        }
    });

    Ok(())
}

fn report_chain(stage: &str, err: &dyn StdError) {
    eprintln!("[adm] 失败于 {stage}: {err}");
    let mut source = err.source();
    while let Some(inner) = source {
        eprintln!("[adm]   原因: {inner}");
        source = inner.source();
    }
}

fn main() {
    /* 避免托盘程序被系统按后台降频 */
    priority::escape_power_throttling();


    /* 被浏览器以 Native Messaging 调起时，只处理一条消息就退出 */
    if bridge_host::try_native_host() {
        return;
    }

    /* 主窗口只允许一个实例；重复启动就把已有窗口叫到前台 */
    if !bridge_host::acquire_single_instance() {
        bridge_host::activate_existing();
        return;
    }

    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            system::system_static,
            system::system_route,
            queue::tasks_snapshot,
            queue::storage_notice,
            queue::task_enqueue,
            queue::task_pause,
            queue::task_resume,
            queue::tasks_pause_all,
            queue::tasks_start_all,
            queue::task_cancel,
            queue::task_remove,
            queue::task_redownload,
            queue::task_set_auth,
            queue::task_clear,
            queue::queue_set_parallel,
            queue::app_exit,
            queue::plan_enable,
            queue::plan_cancel,
            queue::plan_state,
            queue::extension_register,
            queue::extension_unregister,
            queue::extension_registered,
            queue::settings_snapshot,
            queue::settings_save,
            queue::pick_folder,
            queue::open_file,
            queue::open_folder,
            takeover::takeover_begin,
            takeover::takeover_pending,
            takeover::takeover_ready,
            takeover::takeover_close
        ])
        .setup(|app| {
            let data_dir = match prepare_data_dir(app) {
                Ok(dir) => dir,
                Err(err) => {
                    report_chain("准备数据目录", err.as_ref());
                    return Err(err);
                }
            };

            /* 配置要在建窗口前就绪：关闭行为、开机启动、托盘都依赖它 */
            let download_dir = app
                .path()
                .download_dir()
                .unwrap_or_else(|_| data_dir.clone());
            settings::init(
                &data_dir,
                settings::Settings {
                    save_dir: download_dir.display().to_string(),
                    ..Default::default()
                },
            );

            if let Err(err) = build_main_window(app, &data_dir) {
                report_chain("创建窗口", &err);
                return Err(Box::new(err) as Box<dyn StdError>);
            }

            /* 通知与开机启动：都需要读配置，所以放在窗口之后、队列之前。
               放到后台线程，避免 PowerShell 之类的外部进程阻塞主线程。 */
            std::thread::spawn(|| {
                toast::init();
                if settings::current().autostart {
                    startup::refresh_if_enabled();
                }
            });

            /* 托盘常驻 */
            if let Err(err) = tray::install(app.handle()) {
                eprintln!("[tray] 托盘创建失败: {err}");
            }

            /* 带 --background 启动时（开机自启）只留托盘，不弹窗口 */
            if std::env::args().any(|arg| arg == "--background") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_skip_taskbar(true);
                    let _ = window.hide();
                }
            }

            /* 本机接管通道：浏览器扩展通过它把下载交过来 */
            bridge_server::start(app.handle().clone());

            /* 向系统挂号监听代理设置变化 */
            watchers::start(app.handle().clone());

            /* 下载服务：任务队列 + 进度聚合推送 */
            queue::setup(app);

            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(err) = result {
        report_chain("运行事件循环", &err);
        std::process::exit(1);
    }
}
