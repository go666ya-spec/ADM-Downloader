//! 偏好设置：一份 JSON，所有模块共享。
//!
//! 读取走全局单例，写入立即落盘（设置改动是低频操作，不需要合并写）。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 下载文件存放目录
    pub save_dir: String,
    /// 单任务最大连接数
    pub connections: u32,
    /// 单服务器最大连接数（预留，当前按主机分配）
    /// 最小分段大小（MiB）
    pub min_segment_mib: u32,
    /// 断点缓存（MiB，预留）
    pub cache_mib: u32,
    /// 并行任务数
    pub parallel: u32,
    /// 下载限速 KiB/s，0 表示不限
    pub download_limit_kib: u32,
    /// "system" 或 "direct"
    pub proxy_mode: String,
    /// 性能档位名称，仅用于界面显示
    pub speed_mode: String,
    /// 开机启动
    pub autostart: bool,
    /// 有任务下载时，退出前先确认
    pub quit_confirm: bool,
    /// 下载完成或失败时发系统通知
    pub notify_on_finish: bool,
    /// 主题模式："system" | "light" | "dark"
    pub theme: String,
    /// 界面语言："system" | "zh-cn" | "zh-tw" | "en" | "ko" | "ja"
    pub language: String,
    /// 带宽模式："smart" | "limited" | "fast"
    /// - smart：不限速，由下载引擎自己决定
    /// - limited：按偏好里的上传/下载限速执行
    /// - fast：完全放开，疯狂跑
    pub bandwidth_mode: String,
    /// 计划任务完成后的动作："shutdown" | "sleep"
    pub plan_action: String,
    /// 有任务下载时阻止系统自动睡眠
    pub keep_awake: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            save_dir: String::new(),
            connections: 16,
            min_segment_mib: 4,
            cache_mib: 8,
            parallel: 1,
            download_limit_kib: 0,
            proxy_mode: "system".to_string(),
            speed_mode: "balanced".to_string(),
            autostart: false,
            quit_confirm: true,
            notify_on_finish: true,
            theme: "system".to_string(),
            language: "system".to_string(),
            bandwidth_mode: "smart".to_string(),
            plan_action: "shutdown".to_string(),
            keep_awake: true,
        }
    }
}

impl Settings {
    /// 下载限速换算成字节/秒，0 表示不限
    pub fn download_limit_bps(&self) -> u64 {
        self.download_limit_kib as u64 * 1024
    }


}

struct Holder {
    path: PathBuf,
    value: Mutex<Settings>,
}

static HOLDER: OnceLock<Holder> = OnceLock::new();

fn file_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

/// 初始化：读盘并登记全局单例。默认值由调用方提供（通常是系统「下载」目录）。
pub fn init(data_dir: &Path, defaults: Settings) {
    let path = file_path(data_dir);
    let mut value = defaults;

    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(saved) = serde_json::from_str::<Settings>(&text) {
            value = saved;
        }
    }
    if value.save_dir.trim().is_empty() {
        value.save_dir = std::env::var("USERPROFILE")
            .map(|home| format!("{home}\\Downloads"))
            .unwrap_or_default();
    }
    value.connections = value.connections.clamp(1, 64);
    value.parallel = value.parallel.clamp(1, 16);
    value.min_segment_mib = value.min_segment_mib.clamp(1, 1024);

    let _ = HOLDER.set(Holder {
        path,
        value: Mutex::new(value),
    });
}

pub fn current() -> Settings {
    HOLDER
        .get()
        .and_then(|holder| holder.value.lock().ok().map(|guard| guard.clone()))
        .unwrap_or_default()
}

/// 覆盖保存，返回落盘后的值
pub fn replace(mut next: Settings) -> Result<Settings, String> {
    next.connections = next.connections.clamp(1, 64);
    next.parallel = next.parallel.clamp(1, 16);
    next.min_segment_mib = next.min_segment_mib.clamp(1, 1024);

    if let Some(holder) = HOLDER.get() {
        write(&holder.path, &next)?;
        if let Ok(mut guard) = holder.value.lock() {
            *guard = next.clone();
        }
    }
    Ok(next)
}

fn write(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建设置目录失败：{error}"))?;
    }
    let text = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("序列化设置失败：{error}"))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text.as_bytes())
        .map_err(|error| format!("写入设置临时文件失败：{error}"))?;
    std::fs::rename(&temp, path).map_err(|error| format!("替换设置文件失败：{error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_convert() {
        let mut settings = Settings::default();
        settings.download_limit_kib = 300;
        assert_eq!(settings.download_limit_bps(), 300 * 1024);
    }

}
