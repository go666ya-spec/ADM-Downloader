//! 缓存删除：调用系统接口标记「关闭即删除」，而不是我们反复尝试删文件。
//!
//! 背景：Windows 上文件被下载任务占用时 `remove_file` 会失败。
//! 正确做法是给文件打上「删除处置」标记（FileDispositionInfo），
//! 之后由系统在最后一个句柄关闭时自动删除，期间不需要任何重试。

use std::path::Path;

/// 标记文件为「关闭即删除」。
/// 若文件当前没有其他句柄占用，调用返回后文件立刻消失；
/// 若仍被下载任务占用，则等它关闭后由系统删除。
#[cfg(windows)]
pub fn delete_when_closed(path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FileDispositionInfo, SetFileInformationByHandle, DELETE, FILE_DISPOSITION_INFO,
        FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        /* 以 DELETE 权限打开，并且允许别人同时读写删除 */
        let share = FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0);
        let handle = CreateFileW(
            PCWSTR(wide.as_ptr()),
            DELETE.0,
            share,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .map_err(|error| std::io::Error::other(error.to_string()))?;

        let info = FILE_DISPOSITION_INFO {
            DeleteFile: true.into(),
        };

        let result = SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        );

        let _ = CloseHandle(handle);

        result.map_err(|error| std::io::Error::other(error.to_string()))
    }
}

#[cfg(not(windows))]
pub fn delete_when_closed(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)
}

/// 删除一个缓存文件：先尝试系统「关闭即删除」，失败再兜底直删。
pub fn remove_cache_file(path: &Path) {
    if !path.exists() {
        return;
    }
    if delete_when_closed(path).is_ok() {
        return;
    }
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    #[test]
    fn mark_delete_removes_file_when_no_other_handle() {
        let path = std::env::temp_dir().join("adm-delete-on-close-test.bin");
        std::fs::write(&path, b"cache").unwrap();
        super::delete_when_closed(&path).expect("标记删除失败");
        assert!(!path.exists(), "文件应已被系统删除");
    }
}
