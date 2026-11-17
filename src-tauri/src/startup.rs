//! 开机启动：在「启动」文件夹放一个快捷方式，不写注册表。

use std::path::{Path, PathBuf};

const SHORTCUT_NAME: &str = "ADM Downloader.lnk";

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let path = shortcut_path()?;
        if enabled {
            create_shortcut(&path)
        } else {
            match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(format!("删除启动快捷方式失败：{error}")),
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Ok(())
    }
}

/// 当前是否已启用（以快捷方式是否存在为准）
/// 目前界面以配置文件为准，这个用于以后读取系统实际状态（可能被任务管理器禁用）
#[allow(dead_code)]
pub fn is_enabled() -> bool {
    shortcut_path().map(|path| path.exists()).unwrap_or(false)
}

/// 开机启动时刷新一次快捷方式，保证指向当前 exe
pub fn refresh_if_enabled() {
    #[cfg(windows)]
    {
        let Ok(path) = shortcut_path() else {
            return;
        };
        if path.exists() {
            let _ = create_shortcut(&path);
        }
    }
}

#[cfg(windows)]
fn shortcut_path() -> Result<PathBuf, String> {
    let appdata =
        std::env::var("APPDATA").map_err(|_| "找不到 APPDATA，无法创建启动快捷方式".to_string())?;
    Ok(PathBuf::from(appdata)
        .join("Microsoft\\Windows\\Start Menu\\Programs\\Startup")
        .join(SHORTCUT_NAME))
}

#[cfg(windows)]
fn create_shortcut(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建启动文件夹失败：{error}"))?;
    }

    let exe = std::env::current_exe().map_err(|error| format!("获取程序路径失败：{error}"))?;
    let exe = exe.display().to_string();
    let workdir = Path::new(&exe)
        .parent()
        .map(|path| path.display().to_string())
        .unwrap_or_default();

    /* 单引号字符串里把单引号转义，避免路径破坏脚本 */
    fn quote(value: &str) -> String {
        value.replace('\'', "''")
    }

    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');\
         $s.TargetPath='{}';$s.WorkingDirectory='{}';$s.Arguments='--background';\
         $s.IconLocation='{},0';$s.Save()",
        quote(&path.display().to_string()),
        quote(&exe),
        quote(&workdir),
        quote(&exe)
    );

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| format!("启动 PowerShell 失败：{error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("创建启动快捷方式失败".to_string())
    }
}
