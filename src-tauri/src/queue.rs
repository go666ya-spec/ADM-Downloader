//! 下载服务：队列池 + 并行池 + 两种推送。
//!
//! 生命周期约定：
//! - 主程序（前端确认后）只负责「把任务丢进队列池」，之后不再干预
//! - 服务自己从队列池抓任务进并行池，这是服务内部行为
//! - 下载完成的任务**从服务中彻底移除**，不保留在列表里
//! - 推送分两种：
//!   1. 定时：只推并行池里正在下载的任务（每秒一次，没有下载任务时不推）
//!   2. 事件：列表结构变化（新增/完成移除/暂停/失败/删除/并行池补位）→ 推一次完整列表

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::sync::LockExt;
use crate::transfer::{self, TransferControl, TransferRequest, TransferStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// 队列中
    Queued,
    /// 下载中
    Downloading,
    /// 暂停中
    Paused,
    /// 已停止（失败）
    Stopped,
    /// 已完成：不参与任何池，只保留记录供前端展示
    Completed,
}

/// 结构变化的增量：只包含变化的条目与被删除的 id
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatch {
    pub revision: u64,
    pub upserts: Vec<TaskSnapshot>,
    pub removed: Vec<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListSnapshot {
    pub revision: u64,
    pub tasks: Vec<TaskSnapshot>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub state: TaskState,
    pub downloaded: u64,
    pub total: u64,
    /// 字节/秒，服务端采样并平滑
    pub speed: u64,
    /// 百分比 0~100，服务端算好，前端只负责插值渲染
    pub percent: f64,
    /// 已用时（秒）
    pub elapsed_seconds: u64,
    /// 预计剩余（秒），算不出来时为 null
    pub remaining_seconds: Option<u64>,
    /// 以下为服务端算好的展示文本，前端不参与换算
    pub size_text: String,
    pub downloaded_text: String,
    pub speed_text: String,
    pub peak_speed: u64,
    pub peak_speed_text: String,
    pub elapsed_text: String,
    pub remaining_text: String,
    pub connections: u32,
    /// 当前真正连上的连接数
    pub active_connections: u32,
    /// 需要用户填写用户名密码
    pub needs_auth: bool,
    /// 每个连接的实时速率
    pub connection_speeds: Vec<u64>,
    /// 每个连接的状态：idle / connecting / downloading / error
    pub connection_states: Vec<String>,
    /// 是否来自磁盘恢复（重启后首次开始需要播 0→当前 动画）
    pub restored: bool,
    /// 任务是否带用户名密码
    pub has_auth: bool,
    /// 已保存的用户名（密码不回传）
    pub auth_user: Option<String>,
    /// basic / bearer；没有当前凭据时为 null
    pub auth_type: Option<String>,
    /// 成品所在路径（完成后才存在）
    pub path: Option<String>,
    /// 已完成之后的附加警告，例如最终文件改名失败
    pub completion_warning: Option<String>,
    pub error: Option<String>,
    /// 保存目录（复制路径、打开文件夹用）
    pub save_dir: String,
}

struct Task {
    id: u64,
    /// 本次进程内唯一的任务实例标识；ID 被复用时防止旧回调误写新任务。
    instance_id: u64,
    request: TransferRequest,
    control: TransferControl,
    state: TaskState,
    error: Option<String>,
    completion_warning: Option<String>,
    last_bytes: u64,
    last_sample: Instant,
    speed: u64,
    /// 本次下载出现过的最高速度
    peak_speed: u64,
    /// 本轮进入并行池的时刻
    started_at: Option<Instant>,
    /// 之前累计的下载用时（暂停后继续时累加）
    accumulated: Duration,
    /// 被降并行踢回队列，等传输收尾后转成「队列中」
    demoted: bool,
    /// 本次实际分配到的连接数（同站点公平分配的结果）
    allocated: u32,
    /// 配额变化需要重启传输以增减 worker
    pending_restart: bool,
    /// 是否是从磁盘恢复的任务（重启后第一次开始要播 0 → 当前 的动画）
    restored: bool,
    /// 创建/恢复后 HTTP 探测结果，正式下载不再重复探测
    prepared: Option<transfer::Probe>,
    /// HTTP 探测正在进行，避免重复发起
    preparing: bool,
    /// 入队预检未完成：先探一次是否需要认证，避免进并行池才失败
    /// 完成后的成品路径（可能因为重名带 (1) 后缀）
    finished_path: Option<PathBuf>,
    /// 已保存的用户名（密码不落盘）
    saved_auth_user: Option<String>,
    /// 重新下载中的任务：传输收尾后做文件复位
    redownload_pending: bool,
    /// 代际号：重新下载会 +1，旧会话的结果直接丢弃
    generation: u64,
    /// 每次进入并行池都会递增，用来识别同一任务的旧下载会话
    session_id: u64,
    /// 创建方给的预计大小（扩展/浏览器），探测到真实大小前用于显示
    size_hint: u64,
}

struct Inner {
    tasks: Vec<Task>,
    /// 并行池容量
    parallel: u32,
    connections: usize,
    /// 新任务的默认保存目录（前端没传时兜底，理论上前端总会传）
    default_dir: PathBuf,
    /// 并行池中正在运行的 id
    running: Vec<u64>,
    /// 计划任务是否已开启
    plan_active: bool,
}

static NEXT_INSTANCE_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

fn next_instance_id() -> u64 {
    NEXT_INSTANCE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

fn next_task_id(ids: impl IntoIterator<Item = u64>) -> u64 {
    ids.into_iter().max().unwrap_or(0) + 1
}

pub struct Queue {
    inner: Arc<Mutex<Inner>>,
    app: AppHandle,
    /// 进度聚合循环是否在跑（只在有下载任务时启动）
    ticker: std::sync::atomic::AtomicBool,
    /// 当前是否阻止了系统睡眠
    keeping_awake: std::sync::atomic::AtomicBool,
    /// 每任务独立配置的事件驱动串行写入者
    task_store: crate::store::TaskStoreWriter,
    /// 上一次推送的状态，用于只推变化的条目
    last_emit: Mutex<HashMap<u64, String>>,
    /// 结构变化版本号，前端用来发现漏事件
    revision: std::sync::atomic::AtomicU64,
    /// 启动时的配置恢复提示，取一次就清空
    storage_notice: Mutex<Option<String>>,
}

impl Queue {
    pub fn new(app: AppHandle, data_dir: PathBuf, default_dir: PathBuf) -> Arc<Self> {
        let (saved, storage_notice) = crate::store::load_tasks(&data_dir);
        let store_app = app.clone();
        let task_store = crate::store::TaskStoreWriter::new_with_handler(
            crate::store::tasks_dir(&data_dir),
            &saved.tasks,
            Arc::new(move |message| {
                let _ = store_app.emit("storage:error", message);
            }),
        );
        let mut tasks: Vec<Task> = Vec::with_capacity(saved.tasks.len());

        for item in &saved.tasks {
            let state = match item.state.as_str() {
                "completed" => TaskState::Completed,
                _ if item.needs_auth => TaskState::Paused,
                _ if item.error.is_some() => TaskState::Stopped,
                _ => TaskState::Paused,
            };

            let control = TransferControl::new();
            control.set_limit(effective_limits(&crate::settings::current()));

            /* 进度不写 json：暂停任务从断点元数据恢复显示进度 */
            if state == TaskState::Completed {
                control.set_total(item.total);
                control.set_downloaded(item.total);
            } else if let Some(part) = &item.part_path {
                match crate::transfer::resume_progress(&PathBuf::from(part)) {
                    Some((downloaded, total)) => {
                        control.set_total(total);
                        control.set_downloaded(downloaded);
                    }
                    None => {
                        control.set_total(item.total);
                    }
                }
            } else {
                control.set_total(item.total);
            }

            if item.needs_auth {
                control.require_auth();
            }
            /* 恢复断点文件位置，删除任务时才能一起清理 */
            if let Some(part) = &item.part_path {
                control.set_part_path(std::path::PathBuf::from(part));
            }

            /* 恢复出来的任务一律不进并行池：完成/失败/其余分别归位 */
            

            tasks.push(Task {
                id: item.id,
                instance_id: next_instance_id(),
                request: TransferRequest {
                    id: item.id,
                    url: item.url.clone(),
                    file_name: item.file_name.clone(),
                    save_dir: PathBuf::from(&item.save_dir),
                    referrer: None,
                    cookies: None,
                    user_agent: item.user_agent.clone(),
                    authorization: None,
                    force_single: false,
                    connections: saved.connections,
                    min_segment_bytes: 0,
                    proxy: None,
                    auth: None,
                },
                control,
                state,
                error: item.error.clone(),
                completion_warning: item.completion_warning.clone(),
                last_bytes: item.downloaded,
                last_sample: Instant::now(),
                speed: 0,
                peak_speed: item.peak_speed,
                started_at: None,
                accumulated: Duration::from_secs(item.elapsed_seconds),
                demoted: false,
                allocated: 0,
                pending_restart: false,
                restored: true,
                prepared: None,
                preparing: false,
                finished_path: item.finished_path.as_ref().map(PathBuf::from),
                saved_auth_user: item.auth_user.clone(),
                redownload_pending: false,
                generation: 0,
                session_id: 0,
                size_hint: item.size_hint,
            });
        }

        Arc::new(Self {
            inner: Arc::new(Mutex::new(Inner {
                tasks,
                parallel: saved.parallel.clamp(1, 16),
                connections: saved.connections.clamp(1, 64),
                default_dir,
                running: Vec::new(),
                    plan_active: false,
            })),
            app,
            ticker: std::sync::atomic::AtomicBool::new(false),
            keeping_awake: std::sync::atomic::AtomicBool::new(false),
            task_store,
            last_emit: Mutex::new(HashMap::new()),
            revision: std::sync::atomic::AtomicU64::new(0),
            storage_notice: Mutex::new(storage_notice),
        })
    }

    fn saved_task(task: &Task) -> crate::store::SavedTask {
        let (_downloaded, total) = progress_bytes(task);
        crate::store::SavedTask {
            id: task.id,
            url: task.request.url.clone(),
            user_agent: task.request.user_agent.clone(),
            file_name: task.request.file_name.clone(),
            save_dir: task.request.save_dir.display().to_string(),
            downloaded: 0,
            total,
            error: task.error.clone(),
            completion_warning: task.completion_warning.clone(),
            paused: task.state != TaskState::Downloading,
            state: state_name(task.state).to_string(),
            finished_path: task
                .finished_path
                .as_ref()
                .map(|path| path.display().to_string()),
            needs_auth: task.control.snapshot().needs_auth,
            size_hint: task.size_hint,
            elapsed_seconds: task.accumulated.as_secs(),
            peak_speed: task.peak_speed,
            auth_user: task
                .request
                .auth
                .as_ref()
                .map(|(user, _)| user.clone())
                .or_else(|| task.saved_auth_user.clone()),
            part_path: task
                .control
                .current_part_path()
                .map(|path| path.display().to_string()),
            meta_path: task.control.current_part_path().map(|path| {
                crate::transfer::meta_path_for(&path).display().to_string()
            }),
        }
    }

    fn persist_tasks(&self, ids: &[u64]) {
        if ids.is_empty() {
            return;
        }
        let tasks: Vec<crate::store::SavedTask> = {
            let inner = self.inner.lock_ok();
            ids.iter()
                .filter_map(|id| inner.tasks.iter().find(|task| task.id == *id))
                .map(Self::saved_task)
                .collect()
        };
        for task in tasks {
            self.task_store.save(task);
        }
    }

    fn delete_task_files(&self, ids: &[u64]) {
        for id in ids {
            self.task_store.delete(*id);
        }
    }

    fn flush_store(&self) {
        self.task_store.flush();
    }

    /* ---------------- 推送 ---------------- */

    /// 列表结构变化：写盘 + 推一次完整列表
    fn emit_changed(&self) {
        // 完成、暂停、失败、删除等所有状态变化都会经过这里；无活跃下载时立即解除阻止睡眠。
        self.sync_keep_awake();

        let snapshot = self.snapshot();
        let mut last = self.last_emit.lock_ok();

        let mut upserts: Vec<TaskSnapshot> = Vec::new();
        let mut current: HashMap<u64, String> = HashMap::with_capacity(snapshot.len());

        for task in &snapshot {
            let encoded = serde_json::to_string(task).unwrap_or_default();
            if last.get(&task.id) != Some(&encoded) {
                upserts.push(task.clone());
            }
            current.insert(task.id, encoded);
        }

        let removed: Vec<u64> = last
            .keys()
            .copied()
            .filter(|id| !current.contains_key(id))
            .collect();

        *last = current;
        drop(last);

        /* 没有变化就不推 */
        if upserts.is_empty() && removed.is_empty() {
            return;
        }

        let upsert_ids: Vec<u64> = upserts.iter().map(|task| task.id).collect();
        self.persist_tasks(&upsert_ids);
        self.delete_task_files(&removed);

        let revision = self
            .revision
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;

        let _ = self.app.emit(
            "tasks:changed",
            TaskPatch {
                revision,
                upserts,
                removed,
            },
        );

    }

    /// 定时推送：只包含并行池中正在下载的任务
    fn emit_progress(&self) {
        let list: Vec<TaskSnapshot> = {
            let inner = self.inner.lock_ok();
            inner
                .tasks
                .iter()
                .filter(|task| task.state == TaskState::Downloading)
                .map(|task| {
                    snapshot_of(task)
                })
                .collect()
        };

        if list.is_empty() {
            return;
        }
        let _ = self.app.emit("tasks:progress", list);
    }

    /* ---------------- 对外操作 ---------------- */

    /// 主程序只做这一件事：把任务丢进队列池。
    /// 只接受 HTTP / HTTPS 下载地址。
    pub fn enqueue(
        &self,
        url: String,
        file_name: Option<String>,
        save_dir: PathBuf,
        referrer: Option<String>,
        cookies: Option<String>,
        user_agent: Option<String>,
        auth: Option<(String, String)>,
        authorization: Option<String>,
        expected_size: u64,
    ) -> u64 {
        let id = {
            let mut inner = self.inner.lock_ok();
            /* ID 不保存计数器：每次创建时取当前最大 ID + 1。 */
            let id = next_task_id(inner.tasks.iter().map(|task| task.id));

            /* 前端总会带目录；万一为空才用默认目录兜底 */
            let save_dir = if save_dir.as_os_str().is_empty() {
                inner.default_dir.clone()
            } else {
                save_dir
            };

            /* 任务名在入队时就确定：先用 URL 推导，下载时再按 Content-Disposition 校正 */
            let resolved_name = match file_name {
                Some(name) if !name.trim().is_empty() => name,
                _ => transfer::file_name_from_url(&url),
            };

            let settings = crate::settings::current();
            let proxy = if settings.proxy_mode == "system" {
                crate::system::system_proxy_endpoint()
            } else {
                None
            };

            let control = TransferControl::new();
            control.set_limit(effective_limits(&settings));

            /* 创建时就把最终文件名定死：任务表 + 磁盘占用都检查 */
            let file_name = reserve_name(&inner.tasks, &save_dir, &resolved_name, None);

            let request = TransferRequest {
                id,
                url: url.clone(),
                file_name,
                save_dir,
                referrer: referrer.filter(|value| !value.trim().is_empty()),
                cookies: cookies.filter(|value| !value.trim().is_empty()),
                user_agent: user_agent.clone(),
                authorization: authorization.clone(),
                force_single: false,
                connections: settings.connections as usize,
                min_segment_bytes: settings.min_segment_mib as u64 * transfer::MIB,
                proxy,
                auth: auth.clone(),
            };

            inner.tasks.push(Task {
                id,
                instance_id: next_instance_id(),
                request,
                control,
                state: TaskState::Queued,
                error: None,
                completion_warning: None,
                last_bytes: 0,
                last_sample: Instant::now(),
                speed: 0,
                peak_speed: 0,
                started_at: None,
                accumulated: Duration::ZERO,
                demoted: false,
                allocated: 0,
                pending_restart: false,
                restored: false,
                prepared: None,
                preparing: false,
                    finished_path: None,
                saved_auth_user: auth.as_ref().map(|(user, _)| user.clone()),
                redownload_pending: false,
                generation: 0,
                session_id: 0,
                size_hint: expected_size,
            });

            id
        };

        self.emit_changed();
        self.prepare_http(id);
        id
    }

    fn prepare_http(&self, id: u64) {
        let preparation = {
            let mut inner = self.inner.lock_ok();
            let Some(task) = inner.tasks.iter_mut().find(|task| task.id == id) else {
                return;
            };
            if task.prepared.is_some() || task.preparing {
                return;
            }
            task.preparing = true;
            (
                task.request.clone(),
                task.generation,
                task.instance_id,
            )
        };

        let (request, generation, instance_id) = preparation;
        let inner = Arc::clone(&self.inner);
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let result = match transfer::prepare(&request).await {
                Ok(probe) => transfer::ensure_disk_space_async(
                    request.save_dir.clone(),
                    probe.total,
                )
                .await
                .map(|()| probe),
                Err(error) => Err(error),
            };

            let mut guard = inner.lock_ok();
            let Some(index) = guard.tasks.iter().position(|task| task.id == id) else {
                return;
            };
            if guard.tasks[index].instance_id != instance_id {
                return;
            }
            if guard.tasks[index].generation != generation {
                guard.tasks[index].preparing = false;
                return;
            }

            let task = &mut guard.tasks[index];
            task.preparing = false;
            match result {
                Ok(probe) => {
                    task.control.set_total(probe.total);
                    task.size_hint = probe.total;
                    task.prepared = Some(probe);
                    task.error = None;
                }
                Err(message) if message.starts_with("auth_required") => {
                    task.control.fail(message.clone());
                    task.state = TaskState::Paused;
                    task.error = Some(message);
                    task.control.require_auth();
                }
                Err(message) => {
                    task.control.fail(message.clone());
                    task.state = TaskState::Stopped;
                    task.error = Some(message);
                }
            }
            drop(guard);

            let queue = app.state::<Arc<Queue>>().inner().clone();
            queue.emit_changed();
            queue.pump();
            queue.emit_changed();
            queue.maybe_finish_plan();
        });
    }

    fn prepare_queued_http(&self) {
        let ids: Vec<u64> = {
            let inner = self.inner.lock_ok();
            inner
                .tasks
                .iter()
                .filter(|task| {
                    task.state == TaskState::Queued
                        && task.prepared.is_none()
                        && !task.preparing
                })
                .map(|task| task.id)
                .collect()
        };
        for id in ids {
            self.prepare_http(id);
        }
    }

    pub fn set_parallel(&self, parallel: u32) {
        let target = parallel.clamp(1, 16);
        {
            let mut inner = self.inner.lock_ok();
            inner.parallel = target;
            demote_excess(&mut inner, target);
        }
        self.pump();
        self.emit_changed();
    }

    /// 启动时把偏好里的并行数落到队列（不触发推送，前端会自己拉一次）
    pub fn apply_initial_settings(&self, settings: &crate::settings::Settings) {
        let mut inner = self.inner.lock_ok();
        inner.parallel = settings.parallel.clamp(1, 16);
        inner.connections = settings.connections as usize;
        inner.default_dir = PathBuf::from(&settings.save_dir);
        for task in &mut inner.tasks {
            task.control.set_limit(effective_limits(settings));
        }
    }

    /// 应用一整套偏好设置：落盘 + 立即作用到运行中的任务
    pub fn apply_settings(&self, next: crate::settings::Settings) -> Result<(), String> {
        let previous = crate::settings::current();
        let saved = crate::settings::replace(next)?;

        /* 开机启动是系统级开关，配置变化时立刻同步 */
        if previous.autostart != saved.autostart {
            if let Err(error) = crate::startup::set_enabled(saved.autostart) {
                eprintln!("[startup] 设置开机启动失败: {error}");
            }
        }

        {
            let mut inner = self.inner.lock_ok();
            let target = saved.parallel.clamp(1, 16);
            inner.parallel = target;
            inner.connections = saved.connections as usize;
            inner.default_dir = PathBuf::from(&saved.save_dir);

            for task in &mut inner.tasks {
                task.control.set_limit(effective_limits(&saved));
                /* 连接数只影响新任务：已有任务的设置保持创建时的值 */
            }

            demote_excess(&mut inner, target);
        }

        self.refresh_proxy();
        self.pump();
        self.emit_changed();
        Ok(())
    }

    pub fn pause(&self, id: u64) {
        let was_pool = {
            let mut inner = self.inner.lock_ok();
            let mut was_pool = false;

            if let Some(task) = inner.tasks.iter_mut().find(|task| task.id == id) {
                was_pool = matches!(
                    task.state,
                    TaskState::Downloading | TaskState::Queued
                );
                if was_pool {
                    /* 前端暂停立即生效；旧核心之后的返回结果已过期。 */
                    task.session_id = task.session_id.wrapping_add(1);
                }
                task.control.stop_for_pause();
                if was_pool {
                    task.state = TaskState::Paused;
                    task.control.clear_connection_view();
                }
                if let Some(started) = task.started_at.take() {
                    task.accumulated += started.elapsed();
                }
                task.speed = 0;
            }

            if was_pool {
                inner.running.retain(|running| *running != id);
            }

            was_pool
        };

        /* 手动暂停并行/队列里的任务 = 计划任务作废 */
        if was_pool {
            self.cancel_plan_if_active();
        }

        self.pump();
        self.emit_changed();
    }

    pub fn resume(&self, id: u64) {
        {
            let mut inner = self.inner.lock_ok();
            if let Some(task) = inner.tasks.iter_mut().find(|task| task.id == id) {
                /* 需要认证且没有当前运行期密码：保持暂停，等用户填写 */
                let has_credentials = task
                    .request
                    .auth
                    .as_ref()
                    .map(|(_, password)| !password.is_empty())
                    .unwrap_or(false)
                    || task
                        .request
                        .authorization
                        .as_deref()
                        .map(|value| !value.trim().is_empty())
                        .unwrap_or(false);
                let needs_password = task.control.snapshot().needs_auth && !has_credentials;

                if needs_password {
                    return;
                }

                if task.state == TaskState::Paused {
                    /* 记下暂停前的进度，续传时界面从原位置继续，而不是先归零 */
                    let previous = task.control.snapshot();
                    let part = task.control.current_part_path();

                    let fresh = TransferControl::new();
                    fresh.set_downloaded(previous.downloaded);
                    fresh.set_total(previous.total);
                    fresh.set_limit(effective_limits(&crate::settings::current()));
                    if let Some(path) = part {
                        fresh.set_part_path(path);
                    }

                    task.control = fresh;
                    task.state = TaskState::Queued;
                    task.error = None;
                    task.speed = 0;
                }
            }
        }

        self.prepare_http(id);
        self.pump();
        self.ensure_ticker();
        self.emit_changed();
    }

    /// 整列表暂停：只暂停「当前列表里」的下载中与排队中任务。
    /// 不设常驻开关——暂停之后新建的任务照常进并行池。
    pub fn pause_all(&self) {
        let paused_pool = {
            let mut inner = self.inner.lock_ok();
            let mut paused_pool = false;

            for task in &mut inner.tasks {
                if !matches!(
                    task.state,
                    TaskState::Downloading | TaskState::Queued
                ) {
                    continue;
                }

                paused_pool = true;
                task.session_id = task.session_id.wrapping_add(1);

                task.control.stop_for_pause();

                if let Some(started) = task.started_at.take() {
                    task.accumulated += started.elapsed();
                }
                task.state = TaskState::Paused;
                task.speed = 0;
                task.control.clear_connection_view();
            }

            inner.running.clear();
            paused_pool
        };

        /* 整列表暂停同样会让计划任务作废 */
        if paused_pool {
            self.cancel_plan_if_active();
        }

        self.emit_changed();
    }

    /// 整列表开始：暂停池里的任务全部回队列池，然后由服务自主抓取
    pub fn start_all(&self) {
        {
            let mut inner = self.inner.lock_ok();
            let limit = effective_limits(&crate::settings::current());
            for task in &mut inner.tasks {
                if task.state != TaskState::Paused {
                    continue;
                }
                let _ = prepare_resume(task, limit);
            }
        }

        self.prepare_queued_http();
        self.pump();
        self.ensure_ticker();
        self.emit_changed();
    }

    pub fn clear(&self) {
        let controls: Vec<(u64, TransferControl)> = {
            let mut inner = self.inner.lock_ok();
            let items = inner
                .tasks
                .iter()
                .map(|task| {
                    task.control.cancel();
                    (task.id, task.control.clone())
                })
                .collect::<Vec<_>>();
            inner.tasks.clear();
            inner.running.clear();
            items
        };

        for (id, control) in controls {
            self.task_store.delete(id);
            cleanup_cache(&control);
        }

        self.emit_changed();
        /* 手动清空列表：计划任务作废，不能因此触发关机 */
        self.cancel_plan_if_active();
    }

    /// 清除/取消：停止传输、删除下载缓存、从服务中移除任务。
    /// 只删除 HTTP 临时文件，不动已完成的成品。
    pub fn discard(&self, id: u64) {
        let removed = {
            let mut inner = self.inner.lock_ok();
            let Some(index) = inner.tasks.iter().position(|task| task.id == id) else {
                return;
            };
            inner.tasks[index].control.cancel();
            let task = inner.tasks.remove(index);
            inner.running.retain(|running| *running != id);
            task
        };

        let removed = removed;
        self.task_store.delete(id);
        let was_pool = matches!(
            removed.state,
            TaskState::Downloading | TaskState::Queued
        );

        cleanup_cache(&removed.control);

        self.pump();
        self.emit_changed();

        /* 手动删掉并行/队列里的任务同样让计划作废；其余情况看是否已收尾 */
        if was_pool {
            self.cancel_plan_if_active();
        } else {
            self.maybe_finish_plan();
        }
    }

    pub fn remove(&self, id: u64) {
        self.discard(id);
    }

    /// 系统代理变化：更新所有任务，正在下载的重启会话以使用新代理
    pub fn refresh_proxy(&self) {
        let settings = crate::settings::current();
        let target = if settings.proxy_mode == "system" {
            crate::system::system_proxy_endpoint()
        } else {
            None
        };

        let mut restarted = false;
        let mut changed = false;

        {
            let mut inner = self.inner.lock_ok();
            for task in &mut inner.tasks {
                if task.request.proxy == target {
                    continue;
                }

                changed = true;
                task.request.proxy = target.clone();

                if task.state == TaskState::Downloading {
                    task.pending_restart = true;
                    task.control.cancel();
                    restarted = true;
                }
            }
        }

        if restarted {
            self.pump();
        }
        if changed {
            self.emit_changed();
        }
    }

    /// 重新下载：静态数据不变，按占用情况决定沿用旧名还是另存新名
    pub fn redownload(&self, id: u64) {
        {
            let mut inner = self.inner.lock_ok();
            let Some(task) = inner.tasks.iter_mut().find(|task| task.id == id) else {
                return;
            };

            if task.state == TaskState::Downloading || task.control.is_session_active()
            {
                /* 立即把任务投回队列：前端秒重置，旧会话交给代际号作废 */
                let old_control = task.control.clone();

                reset_task_for_download(task);
                task.generation = task.generation.wrapping_add(1);
                task.redownload_pending = true;
                task.finished_path = None;

                /* 立刻退出下载池，把并行名额让出来 */
                inner.running.retain(|running| *running != id);

                drop(inner);
                old_control.cancel();

                self.prepare_queued_http();
                self.pump();
                self.emit_changed();
                return;
            }

            /* 已完成的看成品是否占用，占用就在任务表内预留新名字 */
            if task.state == TaskState::Completed {
                let dir = task.request.save_dir.clone();
                let base = task.request.file_name.clone();
                let final_path = task
                    .finished_path
                    .clone()
                    .unwrap_or_else(|| dir.join(&base));

                if final_path.exists() {
                    let tasks = &inner.tasks;
                    let new_name = reserve_name(tasks, &dir, &base, Some(id));
                    if let Some(task) = inner.tasks.iter_mut().find(|task| task.id == id) {
                        task.request.file_name = new_name;
                    }
                }
            }

            if let Some(task) = inner.tasks.iter_mut().find(|task| task.id == id) {
                perform_redownload(task);
            }
        }

        self.prepare_queued_http();
        self.pump();
        self.emit_changed();
    }

    pub fn set_auth(
        &self,
        id: u64,
        user: String,
        password: String,
        auth_type: String,
        token: String,
    ) {
        {
            let mut inner = self.inner.lock_ok();
            let settings = crate::settings::current();

            if let Some(task) = inner.tasks.iter_mut().find(|task| task.id == id) {
                if auth_type == "bearer" {
                    let token = token.trim();
                    if token.is_empty() {
                        return;
                    }
                    task.saved_auth_user = None;
                    task.request.auth = None;
                    task.request.authorization = Some(if token
                        .to_ascii_lowercase()
                        .starts_with("bearer ")
                    {
                        token.to_string()
                    } else {
                        format!("Bearer {token}")
                    });
                } else {
                    if user.trim().is_empty() || password.is_empty() {
                        return;
                    }
                    task.saved_auth_user = Some(user.trim().to_string());
                    task.request.auth = Some((user.trim().to_string(), password));
                    task.request.authorization = None;
                }
                task.request.proxy = if settings.proxy_mode == "system" {
                    crate::system::system_proxy_endpoint()
                } else {
                    None
                };

                let part = task.control.current_part_path();
                let fresh = TransferControl::new();
                fresh.set_limit(effective_limits(&settings));
                if let Some(path) = part {
                    fresh.set_part_path(path);
                }

                task.control = fresh;
                task.state = TaskState::Queued;
                task.error = None;
                task.speed = 0;
                task.started_at = None;
                    }
        }

        self.prepare_queued_http();
        self.pump();
        self.emit_changed();
    }

    pub fn snapshot(&self) -> Vec<TaskSnapshot> {
        let inner = self.inner.lock_ok();
        inner.tasks.iter().map(snapshot_of).collect()
    }

    /* ---------------- 计划任务 ---------------- */

    pub fn plan_enable(&self) -> PlanInfo {
        let mut inner = self.inner.lock_ok();
        inner.plan_active = true;
        PlanInfo {
            active: true,
            action: crate::settings::current().plan_action,
        }
    }

    pub fn plan_cancel(&self) -> PlanInfo {
        let mut inner = self.inner.lock_ok();
        inner.plan_active = false;
        PlanInfo {
            active: false,
            action: crate::settings::current().plan_action,
        }
    }

    pub fn plan_info(&self) -> PlanInfo {
        let inner = self.inner.lock_ok();
        PlanInfo {
            active: inner.plan_active,
            action: crate::settings::current().plan_action,
        }
    }

    /// 计划任务是否需要收尾：由「任务结束」事件触发，不做任何定时检查。
    /// 只统计并行池和队列池；暂停、停止、已完成都不参与判断。
    fn maybe_finish_plan(&self) {
        let (trigger, action) = {
            let mut inner = self.inner.lock_ok();
            if !inner.plan_active {
                return;
            }

            let pending = inner
                .tasks
                .iter()
                .any(|task| {
                    matches!(
                        task.state,
                        TaskState::Downloading | TaskState::Queued
                    )
                });

            if pending {
                return;
            }

            inner.plan_active = false;
            (true, crate::settings::current().plan_action)
        };

        if trigger {
            crate::toast::plan_done(&action);
            let _ = self.app.emit("plan:changed", self.plan_info());
            run_plan_action(&action);
        }
    }

    /// 手动暂停/删除并行池或队列池任务时，取消计划任务（避免悄悄执行关机）
    fn cancel_plan_if_active(&self) {
        let cancelled = {
            let mut inner = self.inner.lock_ok();
            if !inner.plan_active {
                false
            } else {
                inner.plan_active = false;
                true
            }
        };

        if cancelled {
            let _ = self.app.emit("plan:changed", self.plan_info());
        }
    }

    /// 是否有任务正在传输（退出前判断要不要弹确认）
    pub fn has_downloading(&self) -> bool {
        self.inner
            .lock()
            .map(|inner| {
                inner
                    .tasks
                    .iter()
                    .any(|task| {
                        task.state == TaskState::Downloading && !task.demoted
                    })
            })
            .unwrap_or(false)
    }

    /* ---------------- 抓取队列 → 并行池 ---------------- */

    /// 服务内部自主行为：并行池有空位时，抓「最旧创建」的排队任务进并行池。
    /// 注意：服务只做「队列 → 并行」这一个方向，
    /// 暂停/继续/取消全部由前端发起，服务不会自作主张挪动任务。
    fn pump(&self) {
        loop {
            let job = {
                let mut inner = self.inner.lock_ok();
                if inner.running.len() >= inner.parallel as usize {
                    break;
                }
                let Some(index) = inner
                    .tasks
                    .iter()
                    .position(|task| {
                        task.state == TaskState::Queued
                            && !task.redownload_pending
                            && task.prepared.is_some()
                    })
                else {
                    break;
                };

                /* 每个任务只按自己的最大连接数分配 */
                let allocated = inner.tasks[index].request.connections.max(1) as u32;

                let id = inner.tasks[index].id;
                let instance_id = inner.tasks[index].instance_id;
                let session_generation = inner.tasks[index].generation;
                inner.tasks[index].session_id = inner.tasks[index].session_id.wrapping_add(1);
                let session_id = inner.tasks[index].session_id;
                /* 连接数在任务创建时固定 */
                let request = inner.tasks[index].request.clone();
                let prepared = inner.tasks[index].prepared.clone();
                inner.tasks[index].allocated = allocated;

                /* 断点组合不完整：本次是全新下载，动态数据归零 */
                let base = request.save_dir.join(&request.file_name);
                let part = crate::transfer::part_path_for(&base);
                let resumable =
                    part.exists() && crate::transfer::meta_path_for(&part).exists();

                if !resumable {
                    inner.tasks[index].control.set_downloaded(0);
                    inner.tasks[index].accumulated = Duration::ZERO;
                }

                inner.tasks[index].state = TaskState::Downloading;
                inner.tasks[index].last_sample = Instant::now();
                inner.tasks[index].started_at = Some(Instant::now());
                inner.tasks[index].last_bytes = progress_bytes(&inner.tasks[index]).0;
                inner.running.push(id);

                (
                    id,
                    instance_id,
                    request,
                    session_generation,
                    session_id,
                    prepared,
                )
            };

            let (
                id,
                instance_id,
                request,
                session_generation,
                session_id,
                prepared,
            ) = job;

            {
                let control = {
                    let inner = self.inner.lock_ok();
                    inner
                        .tasks
                        .iter()
                        .find(|task| task.id == id)
                        .map(|task| task.control.clone())
                };
                let Some(control) = control else {
                    break;
                };
                let Some(prepared) = prepared else {
                    break;
                };
                control.resume();
                self.ensure_ticker();

                let inner = Arc::clone(&self.inner);
                let app = self.app.clone();

                tauri::async_runtime::spawn(async move {
                    let mut request = request;
                    let mut control = control;
                    let mut session_id = session_id;
                    let mut notify: Option<(bool, String)> = None;

                    loop {
                    let result =
                        transfer::download(request.clone(), control.clone(), prepared.clone()).await;
                        let mut guard = inner.lock_ok();
                        let Some(index) = guard.tasks.iter().position(|task| task.id == id) else {
                            drop(guard);
                            break;
                        };
                        if guard.tasks[index].instance_id != instance_id {
                            drop(guard);
                            return;
                        }
                        if guard.tasks[index].session_id != session_id {
                            drop(guard);
                            return;
                        }

                        let generation_changed =
                            guard.tasks[index].generation != session_generation;
                        if !generation_changed && guard.tasks[index].pending_restart {
                            let task = &mut guard.tasks[index];
                            task.pending_restart = false;
                            task.error = None;
                            task.speed = 0;
                            task.demoted = false;

                            let previous = task.control.snapshot();
                            let part = task.control.current_part_path();
                            let fresh = TransferControl::new();
                            fresh.set_downloaded(previous.downloaded);
                            fresh.set_total(previous.total);
                            fresh.set_limit(effective_limits(&crate::settings::current()));
                            if let Some(path) = part {
                                fresh.set_part_path(path);
                            }
                            task.control = fresh.clone();
                            task.session_id = task.session_id.wrapping_add(1);
                            request = task.request.clone();
                            control = fresh;
                            session_id = task.session_id;
                            drop(guard);

                            let queue = app.state::<Arc<Queue>>().inner().clone();
                            queue.emit_changed();
                            continue;
                        }

                        guard.running.retain(|running| *running != id);

                        if generation_changed {
                            if guard.tasks[index].redownload_pending {
                                guard.tasks[index].redownload_pending = false;
                                guard.tasks[index].pending_restart = false;
                                let dir = guard.tasks[index].request.save_dir.clone();
                                let name = guard.tasks[index].request.file_name.clone();
                                let url = guard.tasks[index].request.url.clone();
                                let part = crate::transfer::part_path_for(&dir.join(&name));

                                if part.exists() {
                                    if let Ok(file) =
                                        std::fs::OpenOptions::new().write(true).open(&part)
                                    {
                                        let _ = file.set_len(0);
                                    }
                                    crate::transfer::reset_breakpoint(&part, &url);
                                }
                            }
                            drop(guard);
                            break;
                        }

                        let status = control.snapshot().status;
                        let name = guard.tasks[index].request.file_name.clone();
                        let task_id = guard.tasks[index].id;
                        match result {
                            Ok(outcome) => {
                                /* 下载完成：不从记录里删除，改为"已完成"，退出所有池 */
                                let task = &mut guard.tasks[index];
                                if let Some(started) = task.started_at.take() {
                                    task.accumulated += started.elapsed();
                                }
                                task.finished_path = outcome.final_path;
                                task.state = TaskState::Completed;
                                task.error = None;
                                task.completion_warning = outcome.warning;
                                task.speed = 0;
                                task.allocated = 0;

                                let previous = task.control.snapshot();
                                let fresh = TransferControl::new();
                                fresh.set_total(previous.total);
                                fresh.set_downloaded(previous.total);
                                fresh.set_part_path(outcome.part_path);
                                task.control = fresh;

                                let done = snapshot_of(task);
                                let _ = app.emit("task:completed", done);
                                notify = Some((true, name));
                            }
                            Err(message) => {
                                let is_range_single = message.starts_with("range_single");
                                let task = &mut guard.tasks[index];
                                if task.demoted {
                                    /* 被降并行踢回队列：不算失败，回到队列池等下一次机会 */
                                    task.demoted = false;
                                    task.state = TaskState::Queued;
                                    task.error = None;
                                    task.speed = 0;
                                    if let Some(started) = task.started_at.take() {
                                        task.accumulated += started.elapsed();
                                    }
                                    /* 换一个干净的控制句柄：清掉暂停标记，保留断点路径 */
                                    let part = task.control.current_part_path();
                                    let fresh = TransferControl::new();
                                    fresh.set_limit(effective_limits(&crate::settings::current()));
                                    if let Some(path) = part {
                                        fresh.set_part_path(path);
                                    }
                                    task.control = fresh;
                                } else {
                                    /* 用户暂停：不显示错误；真失败才进终止池 */
                                    if status == TransferStatus::Paused {
                                        task.state = TaskState::Paused;
                                        task.error = None;
                                    } else if message.starts_with("range_single") {
                                        /* 旧版本兼容：改单连接重下，但不删除原缓存文件。 */
                                        task.request.force_single = true;
                                        task.state = TaskState::Queued;
                                        task.error = None;
                                        task.speed = 0;

                                        let part = task.control.current_part_path();
                                        let fresh = TransferControl::new();
                                        fresh.set_limit(effective_limits(&crate::settings::current()));
                                        if let Some(part) = part {
                                            fresh.set_part_path(part);
                                        }
                                        task.control = fresh;
                                    } else if message.starts_with("auth_required") {
                                        /* 认证失败进暂停池：补全凭据后可以继续 */
                                        task.state = TaskState::Paused;
                                        task.error = Some(message);
                                        task.speed = 0;
                                        task.control.require_auth();
                                    } else {
                                        /* 终止任务不能继续，但错误处理不自动删除本地缓存。 */
                                        task.state = TaskState::Stopped;
                                        task.error = Some(message);
                                        task.speed = 0;
                                        notify = Some((false, name));
                                    }
                                    task.speed = 0;
                                    if is_range_single {
                                        guard.running.retain(|id| *id != task_id);
                                    }
                                }
                            }
                        }
                        drop(guard);
                        break;
                    }

                    if let Some((ok, name)) = notify {
                        if crate::settings::current().notify_on_finish {
                            if ok {
                                crate::toast::download_done_async(name);
                            } else {
                                crate::toast::download_failed_async(name);
                            }
                        }
                    }

                    let queue = app.state::<Arc<Queue>>().inner().clone();
                            queue.pump();
                    queue.emit_changed();
                    queue.maybe_finish_plan();
                });
            }
        }
    }

    /* ---------------- 定时聚合 ---------------- */

    fn tick(&self) {
        let has_active = {
            let mut inner = self.inner.lock_ok();
            let now = Instant::now();
            let mut has_active = false;

            for task in &mut inner.tasks {
                if task.state != TaskState::Downloading {
                    continue;
                }
                has_active = true;

                let (downloaded, total) = progress_bytes(task);
                let elapsed = now.duration_since(task.last_sample).as_secs_f64();
                if elapsed > 0.0 {
                    let delta = downloaded.saturating_sub(task.last_bytes);
                    let instant = (delta as f64 / elapsed) as u64;
                    /* 0.6/0.4 平滑，避免数字抖动 */
                    task.speed = (task.speed as f64 * 0.6 + instant as f64 * 0.4) as u64;
                    task.peak_speed = task.peak_speed.max(task.speed);
                }
                task.last_bytes = downloaded;
                task.last_sample = now;
                let _ = total;
            }

            has_active
        };

        self.sync_keep_awake();
        if !has_active {
            return;
        }

        self.emit_progress();
    }

    /// 有下载任务且设置允许时阻止系统睡眠；状态变化才调用系统 API
    fn sync_keep_awake(&self) {
        use std::sync::atomic::Ordering;

        let keep = crate::settings::current().keep_awake && self.has_downloading();
        let previous = self.keeping_awake.swap(keep, Ordering::Relaxed);

        if previous == keep {
            return;
        }

        /* SetThreadExecutionState 按线程生效，统一丢到主线程执行 */
        let app = self.app.clone();
        let _ = app.run_on_main_thread(move || {
            crate::priority::set_keep_awake(keep);
        });
    }

    /// 有下载任务时才启动进度聚合；没有下载任务时完全停止，不空转。
    fn ensure_ticker(&self) {
        use std::sync::atomic::Ordering;

        if self.ticker.swap(true, Ordering::SeqCst) {
            return;
        }

        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(1000)).await;

                let queue = app.state::<Arc<Queue>>().inner().clone();
                queue.tick();

                if !queue.has_downloading() {
                    queue.ticker.store(false, Ordering::SeqCst);

                    if queue.has_downloading() {
                        queue.ensure_ticker();
                    }
                    break;
                }
            }
        });
    }
}

/// 并行任务数超过上限时，把最新的任务降级回队列
/// 给基础名字加序号后缀：aaa.exe → aaa (1).exe
fn suffixed_name(base: &str, index: u32) -> String {
    let path = std::path::Path::new(base);
    match (path.file_stem(), path.extension()) {
        (Some(stem), Some(ext)) => format!(
            "{} ({}).{}",
            stem.to_string_lossy(),
            index,
            ext.to_string_lossy()
        ),
        _ => format!("{base} ({index})"),
    }
}

/// 创建任务时预留唯一名字：任务表 + 成品 + 缓存，任何一处占用都往后排
fn reserve_name(tasks: &[Task], dir: &std::path::Path, base: &str, exclude: Option<u64>) -> String {
    let mut index = 0u32;
    loop {
        let candidate = if index == 0 {
            base.to_string()
        } else {
            suffixed_name(base, index)
        };

        let path = dir.join(&candidate);
        let part = crate::transfer::part_path_for(&path);
        let on_disk = path.exists() || part.exists();
        let in_tasks = tasks.iter().any(|task| {
            Some(task.id) != exclude
                && task.request.save_dir == dir
                && task.request.file_name == candidate
        });

        if !on_disk && !in_tasks {
            return candidate;
        }
        index += 1;
    }
}

fn demote_excess(inner: &mut Inner, target: u32) {
    let target = target as usize;

    loop {
        let mut active: Vec<u64> = inner
            .tasks
            .iter()
            .filter(|task| task.state == TaskState::Downloading && !task.demoted)
            .map(|task| task.id)
            .collect();

        if active.len() <= target {
            break;
        }

        active.sort_unstable();
        let Some(newest) = active.pop() else {
            break;
        };
        let Some(index) = inner.tasks.iter().position(|task| task.id == newest) else {
            break;
        };

        inner.tasks[index].demoted = true;
        inner.running.retain(|running| *running != newest);
        /* 结束旧会话，传输收尾后由完成回调转成「队列中」 */
        inner.tasks[index].control.cancel();
    }
}

/// 把「带宽模式 + 偏好下载限速」换算成实际生效的限速。
fn effective_limits(settings: &crate::settings::Settings) -> u64 {
    let limited = settings.bandwidth_mode == "limited";
    if limited {
        settings.download_limit_bps()
    } else {
        0
    }
}

/// 计划任务触发后执行系统动作。
/// 关机给 60 秒缓冲；休眠只是通知，是否休眠交给系统电源管理决定。
fn run_plan_action(action: &str) {
    if action == "sleep" {
        /* 不强制休眠：任务结束后由系统电源计划自行决定 */
        return;
    }

    let _ = std::process::Command::new("shutdown")
        .args(["/s", "/t", "60"])
        .spawn();
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInfo {
    pub active: bool,
    pub action: String,
}

/// 恢复任务：保留断点进度，生成新的控制句柄（返回 false 表示需要密码被拦下）
fn prepare_resume(task: &mut Task, limit: u64) -> bool {
    let has_credentials = task
        .request
        .auth
        .as_ref()
        .map(|(_, password)| !password.is_empty())
        .unwrap_or(false)
        || task
            .request
            .authorization
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
    let needs_password = task.control.snapshot().needs_auth && !has_credentials;

    if needs_password {
        return false;
    }

    let previous = task.control.snapshot();
    let part = task.control.current_part_path();

    let fresh = TransferControl::new();
    fresh.set_downloaded(previous.downloaded);
    fresh.set_total(previous.total);
    fresh.set_limit(limit);
    if let Some(path) = part {
        fresh.set_part_path(path);
    }

    task.control = fresh;
    task.state = TaskState::Queued;
    task.error = None;
    task.completion_warning = None;
    task.speed = 0;
    task.started_at = None;
    task.demoted = false;
    true
}

/// 真正的重新下载：已完成看成品占用决定另存，未完成原地复用缓存
fn perform_redownload(task: &mut Task) {
    let save_dir = task.request.save_dir.clone();
    let base_name = task.request.file_name.clone();
    let base_path = save_dir.join(&base_name);

    if task.state == TaskState::Completed {
        /* 名字已在调用方预留好，这里只清成品路径 */
    } else {
        /* 未完成：复用原缓存文件，原地重置断点（不删除、不换名） */
        let part = task
            .control
            .current_part_path()
            .unwrap_or_else(|| crate::transfer::part_path_for(&base_path));

        if part.exists() {
            /* 文件缓存直接清零；元数据同步重置为空区间 */
            if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&part) {
                let _ = file.set_len(0);
            }
            crate::transfer::reset_breakpoint(&part, &task.request.url);
        }
        task.request.file_name = base_name;
    }

    task.finished_path = None;
    reset_task_for_download(task);
}

/// 重新下载前的统一复位：动态数据全部归零
fn reset_task_for_download(task: &mut Task) {
    let fresh = TransferControl::new();
    fresh.set_limit(effective_limits(&crate::settings::current()));

    task.control = fresh;
    task.state = TaskState::Queued;
    task.error = None;
    task.speed = 0;
    task.last_bytes = 0;
    task.last_sample = Instant::now();
    task.peak_speed = 0;
    task.started_at = None;
    task.accumulated = Duration::ZERO;
    task.demoted = false;
    task.allocated = 0;
    task.pending_restart = false;
    task.redownload_pending = false;
    task.restored = false;
    task.prepared = None;
    task.preparing = false;
}

fn state_name(state: TaskState) -> &'static str {
    match state {
        TaskState::Queued => "queued",
        TaskState::Downloading => "downloading",
        TaskState::Paused => "paused",
        TaskState::Stopped => "stopped",
        TaskState::Completed => "completed",
    }
}

/// 取任务的已下载/总大小。
fn progress_bytes(task: &Task) -> (u64, u64) {
    let progress = task.control.snapshot();
    (progress.downloaded, progress.total)
}

impl Task {
    /// 已用时：累计值 + 本轮正在下载的时长（暂停时不计入）
    fn elapsed_seconds(&self) -> u64 {
        let mut duration = self.accumulated;
        if let Some(started) = self.started_at {
            duration += started.elapsed();
        }
        duration.as_secs()
    }
}

/// 删除临时缓存：`.adm-part` 与断点元数据。
/// 交给系统做「关闭即删除」：即使文件正被下载任务占用，也会在句柄释放后自动消失。
fn cleanup_cache(control: &TransferControl) {
    let Some(part) = control.current_part_path() else {
        return;
    };
    let meta = crate::transfer::meta_path_for(&part);

    crate::delete::remove_cache_file(&part);
    crate::delete::remove_cache_file(&meta);
}

/// 字节数 → 人类可读
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= 1024.0 && index < UNITS.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[index])
    }
}

/// 速率 → 人类可读
fn format_speed(bytes_per_second: u64) -> String {
    if bytes_per_second == 0 {
        return "0 KB/s".to_string();
    }
    let value = bytes_per_second as f64;
    if value < 1024.0 * 1024.0 {
        format!("{:.0} KB/s", value / 1024.0)
    } else {
        format!("{:.1} MB/s", value / 1024.0 / 1024.0)
    }
}

/// 秒数 → HH:MM:SS；未知统一 00:00:00
fn format_duration(seconds: Option<u64>) -> String {
    let total = seconds.unwrap_or(0);
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let secs = total % 60;
    format!("{hours:02}:{minutes:02}:{secs:02}")
}

fn snapshot_of(task: &Task) -> TaskSnapshot {
    let (downloaded, total) = progress_bytes(task);

    /* 只有下载中才显示活跃连接；暂停/排队/停止/完成一律为 0 */
    let active_connections = if task.state == TaskState::Downloading {
        task.control.snapshot().active_workers as u32
    } else {
        0
    };

    let percent = if total > 0 {
        (downloaded as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

    let elapsed_seconds = task.elapsed_seconds();
    let remaining_seconds = if task.speed > 0 && total > downloaded {
        Some(((total - downloaded) as f64 / task.speed as f64).round() as u64)
    } else {
        None
    };

    TaskSnapshot {
        id: task.id,
        name: task.request.file_name.clone(),
        url: task.request.url.clone(),
        state: task.state,
        downloaded,
        total,
        speed: task.speed,
        percent,
        elapsed_seconds,
        remaining_seconds,
        size_text: if total > 0 {
            format_bytes(total)
        } else if task.size_hint > 0 {
            format_bytes(task.size_hint)
        } else {
            format_bytes(0)
        },
        downloaded_text: format_bytes(downloaded),
        speed_text: format_speed(task.speed),
        peak_speed: task.peak_speed,
        peak_speed_text: format_speed(task.peak_speed),
        elapsed_text: format_duration(Some(elapsed_seconds)),
        remaining_text: format_duration(remaining_seconds),
        connections: task.request.connections as u32,
        active_connections: active_connections,
        needs_auth: task.control.snapshot().needs_auth,
        connection_speeds: task.control.snapshot().connection_speeds,
        connection_states: task.control.snapshot().connection_states,
        restored: task.restored,
        has_auth: task.request.auth.is_some() || task.request.authorization.is_some(),
        auth_user: task
            .request
            .auth
            .as_ref()
            .map(|(user, _)| user.clone())
            .or_else(|| task.saved_auth_user.clone()),
        auth_type: if task.request.auth.is_some() {
            Some("basic".to_string())
        } else if task.request.authorization.is_some() {
            Some("bearer".to_string())
        } else {
            None
        },
        path: task
            .finished_path
            .as_ref()
            .map(|path| path.display().to_string()),
        completion_warning: task.completion_warning.clone(),
        error: task.error.clone(),
        save_dir: task.request.save_dir.display().to_string(),
    }
}

/* ============================== IPC 命令 ============================== */

#[tauri::command]
pub fn storage_notice(queue: tauri::State<'_, Arc<Queue>>) -> Option<String> {
    queue.storage_notice.lock_ok().take()
}

#[tauri::command]
pub fn tasks_snapshot(queue: tauri::State<'_, Arc<Queue>>) -> TaskListSnapshot {
    let revision = queue
        .revision
        .load(std::sync::atomic::Ordering::SeqCst);
    TaskListSnapshot {
        revision,
        tasks: queue.snapshot(),
    }
}

#[tauri::command]
pub fn task_enqueue(
    queue: tauri::State<'_, Arc<Queue>>,
    url: String,
    file_name: Option<String>,
    save_dir: String,
    referrer: Option<String>,
    cookies: Option<String>,
    user: Option<String>,
    password: Option<String>,
    file_size: Option<u64>,
    user_agent: Option<String>,
    authorization: Option<String>,
) -> Result<u64, String> {
    let url = url.trim().to_string();
    if url.is_empty() || !crate::bridge_host::is_http_url(&url) {
        return Err("invalid_url|".to_string());
    }
    let save_dir = PathBuf::from(save_dir);
    if !save_dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&save_dir)
            .map_err(|error| format!("create_dir|创建保存目录失败：{error}"))?;
    }
    let auth = match (user, password) {
        (Some(user), Some(password)) if !user.trim().is_empty() => {
            Some((user.trim().to_string(), password))
        }
        _ => None,
    };

    Ok(queue.enqueue(
        url,
        file_name,
        save_dir,
        referrer,
        cookies,
        user_agent,
        auth,
        authorization,
        file_size.unwrap_or(0),
    ))
}

#[tauri::command]
pub fn task_pause(queue: tauri::State<'_, Arc<Queue>>, id: u64) {
    queue.pause(id);
}

#[tauri::command]
pub fn task_resume(queue: tauri::State<'_, Arc<Queue>>, id: u64) {
    queue.resume(id);
}

#[tauri::command]
pub fn tasks_pause_all(queue: tauri::State<'_, Arc<Queue>>) {
    queue.pause_all();
}

#[tauri::command]
pub fn tasks_start_all(queue: tauri::State<'_, Arc<Queue>>) {
    queue.start_all();
}

#[tauri::command]
pub fn task_cancel(queue: tauri::State<'_, Arc<Queue>>, id: u64) {
    queue.discard(id);
}

#[tauri::command]
pub fn task_remove(queue: tauri::State<'_, Arc<Queue>>, id: u64) {
    queue.remove(id);
}

#[tauri::command]
pub fn task_set_auth(
    queue: tauri::State<'_, Arc<Queue>>,
    id: u64,
    user: String,
    password: String,
    auth_type: String,
    token: Option<String>,
) {
    queue.set_auth(id, user, password, auth_type, token.unwrap_or_default());
}

#[tauri::command]
pub fn task_redownload(queue: tauri::State<'_, Arc<Queue>>, id: u64) {
    queue.redownload(id);
}

#[tauri::command]
pub fn task_clear(queue: tauri::State<'_, Arc<Queue>>) {
    queue.clear();
}

#[tauri::command]
pub fn queue_set_parallel(queue: tauri::State<'_, Arc<Queue>>, parallel: u32) {
    queue.set_parallel(parallel);
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionEntry {
    pub browser: String,
    pub id: String,
}

#[tauri::command]
pub fn extension_register(browser: String, id: String) -> Result<(), String> {
    crate::bridge_host::register(&browser, &id)
}

#[tauri::command]
pub fn extension_unregister(browser: String) -> Result<(), String> {
    crate::bridge_host::unregister(&browser)
}

#[tauri::command]
pub fn extension_registered() -> Vec<ExtensionEntry> {
    crate::bridge_host::registered()
        .into_iter()
        .map(|(browser, id)| ExtensionEntry { browser, id })
        .collect()
}

#[tauri::command]
pub fn plan_enable(queue: tauri::State<'_, Arc<Queue>>) -> PlanInfo {
    queue.plan_enable()
}

#[tauri::command]
pub fn plan_cancel(queue: tauri::State<'_, Arc<Queue>>) -> PlanInfo {
    queue.plan_cancel()
}

#[tauri::command]
pub fn plan_state(queue: tauri::State<'_, Arc<Queue>>) -> PlanInfo {
    queue.plan_info()
}

#[tauri::command]
pub fn app_exit(app: tauri::AppHandle, queue: tauri::State<'_, Arc<Queue>>) {
    queue.pause_all();
    queue.flush_store();

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        app.exit(0);
    });
}

#[tauri::command]
pub fn settings_snapshot() -> crate::settings::Settings {
    crate::settings::current()
}

#[tauri::command]
pub fn settings_save(
    queue: tauri::State<'_, Arc<Queue>>,
    settings: crate::settings::Settings,
) -> Result<(), String> {
    queue.apply_settings(settings)
}

#[tauri::command]
pub async fn pick_folder(title: Option<String>) -> Result<Option<String>, String> {
    let label = title.unwrap_or_else(|| "选择文件夹".to_string());
    tauri::async_runtime::spawn_blocking(move || crate::pick::pick_folder(&label))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn open_file(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::open::open_path(PathBuf::from(path).as_path()))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn open_folder(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::open::reveal_path(PathBuf::from(path).as_path()))
        .await
        .map_err(|error| error.to_string())?
}

/* ============================== 应用装配 ============================== */

pub fn setup(app: &tauri::App) {
    let data_dir = crate::paths::data_dir();
    let _ = std::fs::create_dir_all(&data_dir);
    /* 清理旧版本遗留的 BT 缓存目录。 */
    let _ = std::fs::remove_dir_all(data_dir.join("bt"));

    /* 默认保存目录：跟随系统「下载」文件夹，而不是应用数据目录 */
    let system_download = app
        .path()
        .download_dir()
        .unwrap_or_else(|_| data_dir.clone());
    let _ = system_download;

    /* 偏好与默认目录已由 main 在窗口创建前初始化完毕 */
    let settings = crate::settings::current();
    let save_dir = PathBuf::from(&settings.save_dir);

    let queue = Queue::new(app.handle().clone(), data_dir, save_dir);
    queue.apply_initial_settings(&settings);
    /* 启动时读一次系统代理，之后完全依赖系统通知 */
    queue.refresh_proxy();
    app.manage(queue);
}

#[cfg(test)]
mod tests {
    use super::next_task_id;

    #[test]
    fn next_task_id_uses_current_maximum() {
        assert_eq!(next_task_id([]), 1);
        assert_eq!(next_task_id([1, 2, 3, 4, 5]), 6);
        assert_eq!(next_task_id([1, 3, 5]), 6);
        assert_eq!(next_task_id([1, 2, 3, 5, 7]), 8);
        assert_eq!(next_task_id([2, 3, 4, 5]), 6);
    }

    #[test]
    fn next_task_id_reuses_the_old_maximum_after_it_is_deleted() {
        assert_eq!(next_task_id([1, 3]), 4);
        assert_eq!(next_task_id([1, 3, 6]), 7);
    }
}
