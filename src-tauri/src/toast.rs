//! Windows 系统通知（Toast）。
//!
//! 用系统自带的 Windows.UI.Notifications，而不是自己画一个提示窗。

#[cfg(windows)]
pub const AUMID: &str = "ADM.Downloader";

/// 注册应用标识与显示信息，否则 Toast 会报「找不到元素」
#[cfg(windows)]
pub fn init() {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    fn write_string(key: HKEY, name: &str, value: &str) {
        let mut bytes: Vec<u8> = value
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        bytes.shrink_to_fit();

        let name_wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

        unsafe {
            let status = RegSetValueExW(
                key,
                PCWSTR(name_wide.as_ptr()),
                None,
                REG_SZ,
                Some(&bytes),
            );
            if status != ERROR_SUCCESS {
                eprintln!("[toast] 写注册表失败 {name}: {status:?}");
            }
        }
    }

    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
        let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(AUMID));
    }

    /* 桌面通知要求这个 AppUserModelId 键存在，否则发送时报 0x80070490 */
    let mut key = HKEY::default();
    unsafe {
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Classes\\AppUserModelId\\ADM.Downloader"),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        );

        if status == ERROR_SUCCESS {
            write_string(key, "DisplayName", "ADM 下载器");
            if let Ok(exe) = std::env::current_exe() {
                write_string(key, "IconUri", &exe.display().to_string());
            }
            let _ = RegCloseKey(key);
        } else {
            eprintln!("[toast] 创建 AUMID 注册表键失败: {status:?}");
        }
    }
}

#[cfg(not(windows))]
pub fn init() {}

#[cfg(windows)]
pub fn download_done(name: &str) {
    show("下载完成", name);
}

#[cfg(windows)]
pub fn download_failed(name: &str) {
    show("下载失败", name);
}

#[cfg(not(windows))]
pub fn download_done(_name: &str) {}

#[cfg(not(windows))]
pub fn download_failed(_name: &str) {}

/// 在独立线程里弹通知：Windows Toast 的 Show 可能阻塞几百毫秒，不能卡进度循环
pub fn download_done_async(name: String) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        download_done(&name);
    });
}

pub fn download_failed_async(name: String) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        download_failed(&name);
    });
}

#[cfg(windows)]
pub fn plan_done(action: &str) {
    if action == "sleep" {
        show("计划任务已完成", "任务已全部结束，休眠由系统电源管理决定");
    } else {
        show("计划任务已完成", "电脑将在 60 秒后关机，可用 shutdown /a 取消");
    }
}

#[cfg(not(windows))]
pub fn plan_done(_action: &str) {}

#[cfg(windows)]
fn show(title: &str, body: &str) {
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\">\
         <text>{}</text><text>{}</text>\
         </binding></visual></toast>",
        escape(title),
        escape(body)
    );

    let result = (|| -> windows::core::Result<()> {
        let document = XmlDocument::new()?;
        document.LoadXml(&HSTRING::from(xml))?;
        let notification = ToastNotification::CreateToastNotification(&document)?;
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))
            .or_else(|_| ToastNotificationManager::CreateToastNotifier())?;
        notifier.Show(&notification)?;
        Ok(())
    })();

    if let Err(error) = result {
        eprintln!("[toast] 发送系统通知失败: {error}");
    }
}

#[cfg(windows)]
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
