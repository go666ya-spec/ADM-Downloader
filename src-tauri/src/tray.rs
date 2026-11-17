//! 系统托盘：常驻图标 + 右键菜单（显示主窗口 / 退出）。

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .tooltip("ADM 下载器")
        .menu(&menu)
        .show_menu_on_left_click(false);

    /* 托盘图标：直接用 32px 那张，和旧代码一样 */
    match tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png")) {
        Ok(icon) => builder = builder.icon(icon),
        Err(error) => {
            eprintln!("[tray] 加载托盘图标失败: {error}");
            if let Some(icon) = app.default_window_icon() {
                builder = builder.icon(icon.clone());
            }
        }
    }

    builder
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "quit" => {
                /* 有任务在下载且开启提醒时，先把窗口叫回来确认；否则直接退出 */
                let ask = crate::settings::current().quit_confirm
                    && app
                        .state::<std::sync::Arc<crate::queue::Queue>>()
                        .has_downloading();

                if ask {
                    show_main(app);
                }
                let _ = app.emit("app:quit-requested", ask);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            /* 左键单击托盘图标：把主窗口叫回来 */
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        /* 恢复时重新登记到任务栏与 Alt-Tab 列表 */
        let _ = window.set_skip_taskbar(false);
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

