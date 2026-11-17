//! 系统状态监听：向 Windows 挂号，变化时由系统主动唤醒，不做任何轮询。
//!
//! 目前挂了两处：
//! 1. 注册表 Internet Settings —— 系统代理开关 / 端点变化
//! 2. 路由表 —— TUN 虚拟网卡接管 / 释放（会改动默认路由）

use std::thread;

use tauri::{AppHandle, Emitter, Manager};

/// 启动所有监听线程。在 setup 里调用即可。
pub fn start(app: AppHandle) {
    #[cfg(windows)]
    {
        let proxy_handle = app.clone();
        thread::spawn(move || watch_internet_settings(proxy_handle));

        let route_handle = app.clone();
        thread::spawn(move || watch_routes(route_handle));
    }

    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

#[cfg(windows)]
fn watch_internet_settings(app: AppHandle) {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_NOTIFY,
        REG_NOTIFY_CHANGE_LAST_SET,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};

    unsafe {
        let mut key = HKEY::default();

        let opened = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings"),
            None,
            KEY_NOTIFY,
            &mut key,
        );
        if opened.is_err() {
            return;
        }

        loop {
            /* 每个循环用一个新事件对象，避免上一次的通知残留 */
            let event = match CreateEventW(None, false, false, PCWSTR::null()) {
                Ok(handle) => handle,
                Err(_) => break,
            };

            /* 挂号：这个键一旦被修改就置位 event，之后线程阻塞等待，不占 CPU */
            let registered =
                RegNotifyChangeKeyValue(key, true, REG_NOTIFY_CHANGE_LAST_SET, Some(event), true);

            if registered.is_err() {
                let _ = CloseHandle(event);
                break;
            }

            let signaled = WaitForSingleObject(event, INFINITE);
            let _ = CloseHandle(event);

            if signaled == WAIT_OBJECT_0 {
                let _ = app.emit("route-changed", ());
                /* 代理/路由变化：让下载服务重新读取系统代理 */
                if let Some(queue) = app.try_state::<std::sync::Arc<crate::queue::Queue>>() {
                    queue.refresh_proxy();
                }
            } else {
                break;
            }
        }

        let _ = RegCloseKey(key);
    }
}

#[cfg(windows)]
fn watch_routes(app: AppHandle) {
    use std::ffi::c_void;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WIN32_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{
        NotifyRouteChange2, MIB_IPFORWARD_ROW2, MIB_NOTIFICATION_TYPE,
    };
    use windows::Win32::Networking::WinSock::AF_UNSPEC;
    use windows::Win32::System::Threading::{
        CreateEventW, ResetEvent, SetEvent, WaitForSingleObject, INFINITE,
    };

    /// 路由表变化时由系统调用，只负责把等待中的线程叫醒
    unsafe extern "system" fn on_route_changed(
        context: *const c_void,
        _row: *const MIB_IPFORWARD_ROW2,
        _kind: MIB_NOTIFICATION_TYPE,
    ) {
        let event = HANDLE(context as *mut c_void);
        let _ = SetEvent(event);
    }

    unsafe {
        /* 手动重置事件：唤醒后由线程自己清零 */
        let event = match CreateEventW(None, true, false, PCWSTR::null()) {
            Ok(handle) => handle,
            Err(_) => return,
        };

        let mut notification: HANDLE = HANDLE::default();
        let result = NotifyRouteChange2(
            AF_UNSPEC,
            Some(on_route_changed),
            event.0 as *const c_void,
            false,
            &mut notification,
        );

        if result != WIN32_ERROR(0) {
            return;
        }

        loop {
            if WaitForSingleObject(event, INFINITE) != WAIT_OBJECT_0 {
                break;
            }
            let _ = ResetEvent(event);
            let _ = app.emit("route-changed", ());
        }
    }
}
