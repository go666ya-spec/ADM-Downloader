//! 持久化：设置 + 任务列表。
//!
//! 只保存"把任务恢复出来"所需的最小字段，不保存运行时句柄、速度等瞬时数据。
//! 写入用临时文件 + 改名，避免断电写坏；
//! 读取时做版本与字段校验，坏文件直接当空列表处理，不让程序起不来。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedTask {
    #[serde(default)]
    pub id: u64,
    pub url: String,
    /// 浏览器扩展带来的 User-Agent
    #[serde(default)]
    pub user_agent: Option<String>,
    pub file_name: String,
    pub save_dir: String,
    pub downloaded: u64,
    pub total: u64,
    pub error: Option<String>,
    /// 已完成之后的附加警告，例如最终文件改名失败
    #[serde(default)]
    pub completion_warning: Option<String>,
    /// 恢复时是否应当停在暂停池（true）还是当作失败任务（false 且有 error）
    pub paused: bool,
    /// 下载中的临时文件（断点数据）
    #[serde(default)]
    pub part_path: Option<String>,
    /// 断点元数据（存于 datareakpoints）
    #[serde(default)]
    pub meta_path: Option<String>,
    /// queued / downloading / paused / stopped / completed
    #[serde(default = "default_state")]
    pub state: String,
    /// 完成后的成品路径
    #[serde(default)]
    pub finished_path: Option<String>,
    /// 创建时探测出的"需要认证"
    #[serde(default)]
    pub needs_auth: bool,
    /// 已保存的用户名（密码不落盘）
    #[serde(default)]
    pub auth_user: Option<String>,
    /// 创建时已知的预计大小
    #[serde(default)]
    pub size_hint: u64,
    /// 已累计用时（秒）
    #[serde(default)]
    pub elapsed_seconds: u64,
    /// 本次下载的最高速度
    #[serde(default)]
    pub peak_speed: u64,
}

fn default_state() -> String {
    "paused".to_string()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedState {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_parallel")]
    pub parallel: u32,
    #[serde(default = "default_connections")]
    pub connections: usize,
    #[serde(default)]
    pub tasks: Vec<SavedTask>,
}

fn default_version() -> u32 {
    VERSION
}

fn default_parallel() -> u32 {
    1
}

fn default_connections() -> usize {
    8
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            version: VERSION,
            parallel: default_parallel(),
            connections: default_connections(),
            tasks: Vec::new(),
        }
    }
}

pub fn tasks_dir(dir: &Path) -> PathBuf {
    dir.join("tasks")
}

fn task_identity_path(task: &SavedTask) -> PathBuf {
    task.finished_path
        .as_ref()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&task.save_dir).join(&task.file_name))
}

fn stable_path_hash(path: &Path) -> u64 {
    let key = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_lowercase();
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in key.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn task_file_path(dir: &Path, task: &SavedTask) -> PathBuf {
    dir.join(format!("{:016x}.json", stable_path_hash(&task_identity_path(task))))
}

fn write_task_file(dir: &Path, task: &SavedTask) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("创建任务配置目录失败：{error}"))?;
    let path = task_file_path(dir, task);
    let temp = path.with_extension("json.tmp");
    let text = serde_json::to_vec(task).map_err(|error| format!("序列化任务配置失败：{error}"))?;
    {
        use std::io::Write;
        let mut file =
            std::fs::File::create(&temp).map_err(|error| format!("创建任务临时配置失败：{error}"))?;
        file.write_all(&text)
            .map_err(|error| format!("写入任务临时配置失败：{error}"))?;
        file.sync_all()
            .map_err(|error| format!("刷新任务临时配置失败：{error}"))?;
    }
    std::fs::rename(&temp, &path).map_err(|error| format!("替换任务配置失败：{error}"))?;
    Ok(path)
}

enum StoreEvent {
    Save(SavedTask),
    Delete(u64),
    Flush(mpsc::Sender<()>),
}

#[derive(Clone)]
pub struct TaskStoreWriter {
    sender: mpsc::Sender<StoreEvent>,
}

impl TaskStoreWriter {
    #[cfg(test)]
    pub fn new(dir: PathBuf, tasks: &[SavedTask]) -> Self {
        Self::new_with_handler(
            dir,
            tasks,
            Arc::new(|message: String| eprintln!("[store] {message}")),
        )
    }

    pub fn new_with_handler(
        dir: PathBuf,
        tasks: &[SavedTask],
        on_error: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let mut paths: HashMap<u64, PathBuf> = tasks
            .iter()
            .filter(|task| task.id > 0)
            .map(|task| (task.id, task_file_path(&dir, task)))
            .collect();
        let mut path_owners: HashMap<PathBuf, u64> = paths
            .iter()
            .map(|(id, path)| (path.clone(), *id))
            .collect();

        std::thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                match event {
                    StoreEvent::Save(task) => {
                        let id = task.id;
                        let expected_path = task_file_path(&dir, &task);
                        if path_owners
                            .get(&expected_path)
                            .is_some_and(|owner| *owner != id)
                        {
                            on_error(format!(
                                "任务 {id} 的配置路径哈希与另一任务冲突，已拒绝覆盖"
                            ));
                            continue;
                        }
                        match write_task_file(&dir, &task) {
                            Ok(next_path) => {
                                if let Some(old_path) = paths.insert(id, next_path.clone()) {
                                    if path_owners.get(&old_path) == Some(&id) {
                                        path_owners.remove(&old_path);
                                    }
                                    if old_path != next_path {
                                        crate::delete::remove_cache_file(&old_path);
                                    }
                                }
                                path_owners.insert(next_path, id);
                            }
                            Err(error) => {
                                on_error(format!("保存任务 {id} 的配置失败：{error}"))
                            }
                        }
                    }
                    StoreEvent::Delete(id) => {
                        if let Some(path) = paths.remove(&id) {
                            if path_owners.get(&path) == Some(&id) {
                                path_owners.remove(&path);
                            }
                            crate::delete::remove_cache_file(&path);
                        }
                    }
                    StoreEvent::Flush(ack) => {
                        let _ = ack.send(());
                    }
                }
            }
        });

        Self { sender }
    }

    pub fn save(&self, task: SavedTask) {
        let _ = self.sender.send(StoreEvent::Save(task));
    }

    pub fn delete(&self, id: u64) {
        let _ = self.sender.send(StoreEvent::Delete(id));
    }

    pub fn flush(&self) {
        let (ack, wait) = mpsc::channel();
        if self.sender.send(StoreEvent::Flush(ack)).is_ok() {
            let _ = wait.recv_timeout(std::time::Duration::from_secs(3));
        }
    }
}

/// 加载每个任务一个文件的新格式。
pub fn load_tasks(dir: &Path) -> (SavedState, Option<String>) {
    let directory = tasks_dir(dir);
    let _ = std::fs::create_dir_all(&directory);

    let mut tasks = Vec::new();
    let mut ids = HashSet::new();
    let mut bad_files = 0usize;
    if let Ok(entries) = std::fs::read_dir(&directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                bad_files += 1;
                continue;
            };
            /* 旧版本的非 HTTP 任务配置不再恢复。 */
            if serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|value| value.get("kind").and_then(|kind| kind.as_str()).map(str::to_owned))
                .is_some_and(|kind| kind != "http")
            {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let Ok(task) = serde_json::from_str::<SavedTask>(&text) else {
                bad_files += 1;
                continue;
            };
            if task_file_path(&directory, &task) != path {
                bad_files += 1;
                continue;
            }
            if task.id == 0 || !ids.insert(task.id) {
                bad_files += 1;
                continue;
            }
            tasks.push(task);
        }
    }

    tasks.sort_by_key(|task| task.id);
    let notice = (bad_files > 0).then(|| format!("有 {bad_files} 个任务配置损坏，已跳过"));
    (
        SavedState {
            version: VERSION,
            parallel: 1,
            connections: 8,
            tasks,
        },
        notice,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_task(id: u64, name: &str, dir: &str) -> SavedTask {
        SavedTask {
            id,
            url: "https://example.com/a.zip".into(),
            user_agent: None,
            file_name: name.into(),
            save_dir: dir.into(),
            downloaded: 0,
            total: 4096,
            error: None,
            completion_warning: None,
            paused: true,
            part_path: None,
            meta_path: None,
            state: "paused".into(),
            finished_path: None,
            needs_auth: false,
            auth_user: None,
            size_hint: 0,
            elapsed_seconds: 0,
            peak_speed: 0,
        }
    }

    #[test]
    fn task_files_round_trip_and_delete_individually() {
        let dir = std::env::temp_dir().join(format!(
            "adm-task-files-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建测试目录");
        let files_dir = tasks_dir(&dir);
        let writer = TaskStoreWriter::new(files_dir.clone(), &[]);

        writer.save(sample_task(2, "second.bin", "C:\\Downloads"));
        writer.save(sample_task(1, "first.bin", "C:\\Downloads"));
        writer.flush();

        let (loaded, notice) = load_tasks(&dir);
        assert!(notice.is_none());
        assert_eq!(loaded.tasks.iter().map(|task| task.id).collect::<Vec<_>>(), vec![1, 2]);
        let json_files = std::fs::read_dir(&files_dir)
            .expect("读取任务目录")
            .flatten()
            .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
            .count();
        assert_eq!(json_files, 2);

        let first_text = std::fs::read_to_string(task_file_path(
            &files_dir,
            &sample_task(1, "first.bin", "C:\\Downloads"),
        ))
        .expect("读取任务文件");
        assert!(!first_text.contains('\n'), "任务 JSON 应为紧凑格式");

        writer.delete(1);
        writer.flush();
        let (loaded, _) = load_tasks(&dir);
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].id, 2);

        drop(writer);
        let _ = std::fs::remove_dir_all(&dir);
    }

}
