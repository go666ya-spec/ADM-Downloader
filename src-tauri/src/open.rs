//! 用系统默认方式打开文件或定位文件。

use std::path::Path;

/// 用默认程序打开文件（或文件夹）。
#[cfg(windows)]
pub fn open_path(path: &Path) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if !path.exists() {
        return Err(format!("文件不存在：{}", path.display()));
    }

    let wide: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let result = ShellExecuteW(
            None,
            windows::core::w!("open"),
            PCWSTR(wide.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );

        /* ShellExecuteW 约定：返回值大于 32 才算成功 */
        if (result.0 as isize) > 32 {
            Ok(())
        } else {
            Err("系统无法打开该文件".to_string())
        }
    }
}

#[cfg(not(windows))]
pub fn open_path(_path: &Path) -> Result<(), String> {
    Err("当前平台不支持".to_string())
}

/// 在资源管理器里定位文件。
/// 用 Shell 官方接口让系统自己打开文件夹并选中文件，不直接启动 explorer.exe。
#[cfg(windows)]
pub fn reveal_path(path: &Path) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{SHOpenFolderAndSelectItems, SHParseDisplayName};

    if !path.exists() {
        return Err(format!("文件不存在：{}", path.display()));
    }

    let wide: Vec<u16> = path
        .display()
        .to_string()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();

        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        let parsed = SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None);

        if parsed.is_err() {
            if initialized {
                CoUninitialize();
            }
            return Err(format!("无法解析文件路径：{}", path.display()));
        }

        let opened = SHOpenFolderAndSelectItems(pidl, None, 0);
        CoTaskMemFree(Some(pidl as *const core::ffi::c_void));

        if initialized {
            CoUninitialize();
        }

        opened.map_err(|error| format!("无法定位文件：{error}"))
    }
}

#[cfg(not(windows))]
pub fn reveal_path(_path: &Path) -> Result<(), String> {
    Err("当前平台不支持".to_string())
}
