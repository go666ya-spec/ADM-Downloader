//! 程序目录下的 data 文件夹：配置、任务记录、缓存都放这里。

use std::path::PathBuf;

/// 数据目录 = 可执行文件所在目录 / data
pub fn data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join("data");
        }
    }

    /* 极少数情况取不到 exe 路径，退回用户数据目录，保证程序能起来 */
    std::env::var("LOCALAPPDATA")
        .map(|base| PathBuf::from(base).join("com.adm.desktop"))
        .unwrap_or_else(|_| PathBuf::from("data"))
}
