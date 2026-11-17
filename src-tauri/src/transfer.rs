//! HTTP 下载核心：探测 → 分段规划 → 断点元数据 → 并发分段下载 → 合并落盘。
//!
//! 设计要点：
//! - 全程 async（tokio），每个分片是一个异步任务，不占用独立线程
//! - 分段数量多于并发数，快的任务可以继续"抢"剩下的段，避免长尾
//! - 断点元数据先刷数据再写，写入用临时文件 + 改名，保证原子性
//! - 错误分类：普通抖动重试，鉴权/404 直接失败，取消/暂停各有语义

use std::collections::VecDeque;
use crate::sync::LockExt;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

pub const MIB: u64 = 1024 * 1024;
pub const GIB: u64 = 1024 * MIB;
const CHECKPOINT_BYTES: u64 = 8 * MIB;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

/// 单次请求最多重试次数

/* ============================== 请求与状态 ============================== */

#[derive(Clone, Debug)]
pub struct TransferRequest {
    pub id: u64,
    pub url: String,
    pub file_name: String,
    pub save_dir: PathBuf,
    pub referrer: Option<String>,
    pub cookies: Option<String>,
    /// 浏览器扩展带来的真实 User-Agent；None 用软件默认值
    pub user_agent: Option<String>,
    pub connections: usize,
    /// 最小分段字节数（0 表示不限制）
    pub min_segment_bytes: u64,
    /// 例如 127.0.0.1:7890；None 表示直连
    pub proxy: Option<String>,
    /// 需要 HTTP Basic 认证时的用户名与密码
    pub auth: Option<(String, String)>,
    /// 手动填写的完整 Authorization 头（Basic / Bearer）
    pub authorization: Option<String>,
    /// 服务器实际忽略 Range 后，强制单连接重下
    pub force_single: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferStatus {
    Pending,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct TransferProgress {
    pub status: TransferStatus,
    pub downloaded: u64,
    pub total: u64,
    pub active_workers: usize,
    pub needs_auth: bool,
    pub error: Option<String>,
    /// 每个连接（worker）的实时速率，下标即连接序号
    pub connection_speeds: Vec<u64>,
    /// 每个连接的状态：idle / connecting / downloading / error
    pub connection_states: Vec<String>,
}

pub struct DownloadOutcome {
    pub final_path: Option<PathBuf>,
    pub part_path: PathBuf,
    pub warning: Option<String>,
}

/// 下载控制句柄：进度读写、暂停/取消信号、限速、临时文件位置
#[derive(Clone)]
pub struct TransferControl {
    downloaded: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    paused: Arc<AtomicBool>,
    /// 暂停时让本次会话彻底结束，释放所有连接
    pause_stop: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    workers: Arc<AtomicUsize>,
    status: Arc<Mutex<TransferStatus>>,
    error: Arc<Mutex<Option<String>>>,
    limit_bps: Arc<AtomicU64>,
    needs_auth: Arc<AtomicBool>,
    /// 每个 worker 的速率采样，由下载核心自己更新
    speeds: Arc<std::sync::Mutex<Vec<SlotSpeed>>>,
    /// 目标 worker 数：运行中可增可减
    speed_slots: Arc<AtomicUsize>,
    /// 每个 worker 的状态码：0 空闲 / 1 连接中 / 2 传输中 / 3 出错
    slot_states: Arc<Mutex<Vec<u8>>>,
    /// 当前真正需要的 worker 数（队列里剩余分段 + 正在传输的）

    /// 目标变化时唤醒会话监督者
    wake: Arc<tokio::sync::Notify>,
    part_path: Arc<Mutex<Option<PathBuf>>>,
    /// 下载会话是否还活着（暂停时保持，取消/失败时结束）
    session_active: Arc<AtomicBool>,
    /// 是否已经收到过 429
    rate_limited: Arc<AtomicBool>,
    /// 第一个探路连接是否已经拿到响应头
    first_response: Arc<AtomicBool>,
    /// 服务器忽略 Range，当前响应转为单连接
    single_mode: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Default)]
struct SlotSpeed {
    bytes: u64,
    speed: u64,
    since: Option<std::time::Instant>,
}

impl TransferControl {
    pub fn new() -> Self {
        Self {
            downloaded: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(0)),
            paused: Arc::new(AtomicBool::new(false)),
            pause_stop: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            workers: Arc::new(AtomicUsize::new(0)),
            status: Arc::new(Mutex::new(TransferStatus::Pending)),
            error: Arc::new(Mutex::new(None)),
            limit_bps: Arc::new(AtomicU64::new(0)),
            needs_auth: Arc::new(AtomicBool::new(false)),
            speeds: Arc::new(std::sync::Mutex::new(Vec::new())),
            speed_slots: Arc::new(AtomicUsize::new(1)),
            slot_states: Arc::new(Mutex::new(vec![0u8; 64])),

            wake: Arc::new(tokio::sync::Notify::new()),
            part_path: Arc::new(Mutex::new(None)),
            session_active: Arc::new(AtomicBool::new(false)),
            rate_limited: Arc::new(AtomicBool::new(false)),
            first_response: Arc::new(AtomicBool::new(false)),
            single_mode: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 临时文件位置，供上层在清除任务时删除缓存
    pub fn set_part_path(&self, path: PathBuf) {
        *self.part_path.lock_ok() = Some(path);
    }

    pub fn current_part_path(&self) -> Option<PathBuf> {
        self.part_path.lock_ok().clone()
    }

    pub fn snapshot(&self) -> TransferProgress {
        TransferProgress {
            status: *self.status.lock_ok(),
            downloaded: self.downloaded.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            active_workers: self.workers.load(Ordering::Relaxed),
            needs_auth: self.needs_auth.load(Ordering::Relaxed),
            error: self.error.lock_ok().clone(),
            connection_speeds: self
                .speeds
                .lock()
                .map(|list| {
                    let target = self.speed_slots.load(Ordering::Relaxed).min(list.len());
                    list.iter().take(target).map(|slot| slot.speed).collect()
                })
                .unwrap_or_default(),
            connection_states: self
                .slot_states
                .lock()
                .map(|list| {
                    let target = self.speed_slots.load(Ordering::Relaxed).min(list.len());
                    list.iter()
                        .take(target)
                        .map(|code| match code {
                            1 => "connecting".to_string(),
                            2 => "downloading".to_string(),
                            3 => "error".to_string(),
                            _ => "idle".to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// 运行中调整连接数：不中断会话，由监督者增减 worker
    pub(crate) fn set_speed_slots(&self, count: usize) {
        self.speed_slots.store(count.clamp(1, 64), Ordering::Relaxed);
        self.wake.notify_waiters();
    }



    pub(crate) fn set_slot_state(&self, slot: usize, state: u8) {
        if let Ok(mut list) = self.slot_states.lock() {
            if let Some(entry) = list.get_mut(slot) {
                *entry = state;
            }
        }
    }

    /// 暂停/取消时把速率与状态全部归零，界面不再显示上一秒的残留
    pub(crate) fn clear_connection_view(&self) {
        if let Ok(mut list) = self.speeds.lock() {
            for entry in list.iter_mut() {
                entry.bytes = 0;
                entry.speed = 0;
                entry.since = None;
            }
        }
        if let Ok(mut states) = self.slot_states.lock() {
            for state in states.iter_mut() {
                *state = 0;
            }
        }
    }

    /// worker 让位时清掉它的速率，界面上变成 0
    pub(crate) fn reset_speed(&self, slot: usize) {
        if let Ok(mut list) = self.speeds.lock() {
            if let Some(entry) = list.get_mut(slot) {
                entry.bytes = 0;
                entry.speed = 0;
                entry.since = None;
            }
        }
    }

    /// 下载核心按连接数初始化采样槽
    pub(crate) fn init_speeds(&self, count: usize) {
        if let Ok(mut list) = self.speeds.lock() {
            *list = vec![SlotSpeed::default(); count];
        }
    }

    /// 每个 worker 写入数据后调用；满 1 秒结算一次真实速率
    pub(crate) fn sample_speed(&self, slot: usize, bytes: u64) {
        let Ok(mut list) = self.speeds.lock() else {
            return;
        };
        let Some(entry) = list.get_mut(slot) else {
            return;
        };

        let now = std::time::Instant::now();
        let since = *entry.since.get_or_insert(now);
        entry.bytes += bytes;

        let elapsed = now.duration_since(since);
        if elapsed >= std::time::Duration::from_secs(1) {
            entry.speed = (entry.bytes as f64 / elapsed.as_secs_f64()) as u64;
            entry.bytes = 0;
            entry.since = Some(now);
        }
    }

    pub fn is_session_active(&self) -> bool {
        self.session_active.load(Ordering::SeqCst)
    }

    pub fn set_session_active(&self, value: bool) {
        self.session_active.store(value, Ordering::SeqCst);
    }

    /// 暂停：结束会话并释放连接，续传时重新请求
    pub fn stop_for_pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
        self.pause_stop.store(true, Ordering::Relaxed);
        self.wake.notify_waiters();
    }

    pub fn mark_single_mode(&self) {
        self.single_mode.store(true, Ordering::Relaxed);
        self.wake.notify_waiters();
    }

    pub fn is_single_mode(&self) -> bool {
        self.single_mode.load(Ordering::Relaxed)
    }

    pub fn mark_first_response(&self) {
        self.first_response.store(true, Ordering::Relaxed);
        self.wake.notify_waiters();
    }

    pub async fn wait_first_response(&self) {
        loop {
            if self.first_response.load(Ordering::Relaxed) {
                return;
            }
            self.wake.notified().await;
        }
    }

    pub fn mark_rate_limited(&self) {
        self.rate_limited.store(true, Ordering::Relaxed);
        self.wake.notify_waiters();
    }

    pub fn is_rate_limited(&self) -> bool {
        self.rate_limited.load(Ordering::Relaxed)
    }

    pub fn is_pause_stop(&self) -> bool {
        self.pause_stop.load(Ordering::Relaxed)
    }

    /// 暂停等待，但配额下降 / 让位时也要醒来退出，真正释放多余连接
    pub async fn wait_if_paused_or_stop(&self, _slot: usize) {
        loop {
            if !self.is_paused()
                || self.is_cancelled()
                || self.is_pause_stop()
            {
                return;
            }
            let notified = self.wake.notified();
            if !self.is_paused()
                || self.is_cancelled()
                || self.is_pause_stop()
            {
                return;
            }
            notified.await;
        }
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
        self.wake.notify_waiters();
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.wake.notify_waiters();
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn set_limit(&self, bytes_per_second: u64) {
        self.limit_bps.store(bytes_per_second, Ordering::Relaxed);
    }

    pub fn require_auth(&self) {
        self.needs_auth.store(true, Ordering::Relaxed);
    }

    pub fn limit(&self) -> u64 {
        self.limit_bps.load(Ordering::Relaxed)
    }

    fn set_status(&self, status: TransferStatus) {
        *self.status.lock_ok() = status;
    }

    pub(crate) fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }

    fn add_downloaded(&self, bytes: u64) {
        self.downloaded.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(crate) fn set_downloaded(&self, bytes: u64) {
        self.downloaded.store(bytes, Ordering::Relaxed);
    }

    fn worker_in(&self) {
        self.workers.fetch_add(1, Ordering::Relaxed);
    }

    fn worker_out(&self) {
        let _ = self
            .workers
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_sub(1))
            });
    }

    pub(crate) fn fail(&self, message: String) {
        *self.error.lock_ok() = Some(message);
        self.set_status(TransferStatus::Failed);
    }
}

impl Default for TransferControl {
    fn default() -> Self {
        Self::new()
    }
}

/* ============================== 错误分类 ============================== */

#[derive(Debug)]
enum RangeError {
    Cancelled,
    /// 429：停止爬坡，交给调度器做最后一次单连接重试
    RateLimited,
    /// 网络抖动、5xx，可重试
    Transient(String),
    /// 404 / 410 / 不可恢复，任务直接失败
    Fatal(String),
}

/* ============================== 探测 ============================== */

#[derive(Clone)]
pub(crate) struct Probe {
    pub total: u64,
    pub supports_ranges: bool,
    pub etag: String,
    pub last_modified: String,
    pub content_disposition: Option<String>,
}

pub(crate) fn build_client(
    proxy: &Option<String>,
    user_agent: Option<&str>,
) -> Result<reqwest::Client, String> {
    let agent = user_agent
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(USER_AGENT);

    let mut builder = reqwest::Client::builder()
        .user_agent(agent)
        .connect_timeout(Duration::from_secs(5));

    if let Some(address) = proxy {
        let proxy = reqwest::Proxy::all(format!("http://{address}"))
            .map_err(|error| format!("代理地址无效：{error}"))?;
        builder = builder.proxy(proxy);
    }

    builder.build().map_err(|error| error.to_string())
}

struct ContentRange {
    start: u64,
    end: u64,
    total: Option<u64>,
}

fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = end.trim().parse().ok()?;
    if end < start {
        return None;
    }
    let total = if total.trim() == "*" {
        None
    } else {
        Some(total.trim().parse().ok()?)
    };
    if total.is_some_and(|value| end >= value) {
        return None;
    }
    Some(ContentRange { start, end, total })
}

fn auth_scheme(headers: &reqwest::header::HeaderMap) -> Option<&'static str> {
    let value = headers
        .get("www-authenticate")
        .and_then(|value| value.to_str().ok())?
        .to_ascii_lowercase();
    if value.contains("basic") {
        Some("Basic")
    } else if value.contains("bearer") {
        Some("Bearer")
    } else {
        None
    }
}

async fn probe(client: &reqwest::Client, req: &TransferRequest) -> Result<Probe, String> {
    let mut request = client
        .get(&req.url)
        .header("Accept-Encoding", "identity")
        .header("Range", "bytes=0-1");

    if let Some(referrer) = &req.referrer {
        request = request.header("Referer", referrer);
    }
    if let Some(cookies) = &req.cookies {
        let clean = cookies.replace(['\r', '\n'], "");
        if !clean.is_empty() {
            request = request.header("Cookie", clean);
        }
    }
    if let Some(header) = auth_header(req) {
        request = request.header("Authorization", header);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("probe_failed|{error}"))?;

    let status = response.status();

    if status.as_u16() == 401 || status.as_u16() == 403 {
        if let Some(scheme) = auth_scheme(response.headers()) {
            return Err(format!("auth_required|{}|{scheme}", status.as_u16()));
        }
        return Err(format!("http_status|{}", status.as_u16()));
    }

    let etag = header_text(&response, "etag");
    let last_modified = header_text(&response, "last-modified");
    let content_disposition = response
        .headers()
        .get("content-disposition")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());

    if status.as_u16() == 206 {
        let range = response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_range)
            .ok_or_else(|| "no_size|206".to_string())?;
        if range.start != 0 || range.end > 1 {
            return Err("no_size|206".to_string());
        }
        let total = range
            .total
            .ok_or_else(|| "no_size|206".to_string())?;

        return Ok(Probe {
            total,
            supports_ranges: true,
            etag,
            last_modified,
            content_disposition,
        });
    }

    if status.as_u16() == 200 {
        let total = content_length(&response).ok_or_else(|| "no_size|0".to_string())?;
        return Ok(Probe {
            total,
            supports_ranges: false,
            etag,
            last_modified,
            content_disposition,
        });
    }

    Err(format!("http_status|{}", status.as_u16()))
}

pub(crate) async fn prepare(req: &TransferRequest) -> Result<Probe, String> {
    let client = build_client(&req.proxy, req.user_agent.as_deref())?;
    probe(&client, req).await
}

pub(crate) fn ensure_disk_space(dir: &Path, total: u64) -> Result<(), String> {
    const RESERVE: u64 = 64 * MIB;
    if let Some(free) = free_space_for(dir) {
        let needed = total.saturating_add(RESERVE);
        if free < needed {
            return Err(format!("disk_space|{needed}|{free}"));
        }
    }
    Ok(())
}

pub(crate) async fn ensure_disk_space_async(dir: PathBuf, total: u64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || ensure_disk_space(&dir, total))
        .await
        .map_err(|error| format!("disk_space|磁盘检查任务失败：{error}"))?
}

/// 生成 Basic 认证头

pub(crate) fn auth_header(req: &TransferRequest) -> Option<String> {
    if let Some(value) = req
        .authorization
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }
    basic_auth(req)
}

pub(crate) fn basic_auth(req: &TransferRequest) -> Option<String> {
    let (user, password) = req.auth.as_ref()?;
    let raw = format!("{user}:{password}");
    Some(format!("Basic {}", base64(raw.as_bytes())))
}

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();

    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let value = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;

        output.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        output.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(value & 0x3f) as usize] as char
        } else {
            '='
        });
    }

    output
}

fn header_text(response: &reqwest::Response, name: &str) -> String {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

fn content_length(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

fn response_total(response: &reqwest::Response) -> Option<u64> {
    if response.status().as_u16() == 206 {
        response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_range)
            .and_then(|range| range.total)
    } else {
        content_length(response)
    }
}

/* ============================== 分段规划 ============================== */

#[derive(Clone, Debug)]
struct Work {
    start: u64,
    end: u64,
    next: u64,
}

/// 分段规则：
/// - 小于 1 MiB：单段
/// - 小于 1 GiB：段数 = 连接数
/// - 大于等于 1 GiB：段数 = 连接数 × 4（细粒度便于负载均衡）
/// - 若设置了最小分段，段数会被限制为 total / 最小分段
fn segment_count(total: u64, connections: usize, min_segment_bytes: u64) -> usize {
    let connections = connections.clamp(1, 64);

    if min_segment_bytes == 0 {
        /* 兼容旧调用：小文件 1 倍，1 GiB 以上 4 倍 */
        let effective = if total < MIB {
            1
        } else {
            connections.min((total / MIB).max(1) as usize)
        };
        let multiplier = if total >= GIB { 4 } else { 1 };
        return (effective * multiplier).max(1);
    }

    /* 每个最小分段算一段；连接数最多 4 倍段数 */
    let by_min = (total / min_segment_bytes) as usize;
    by_min.clamp(1, connections * 4)
}

/// 把一个区间均分成 count 段，最后一段只差余数，不产生小尾巴
fn split_range(start: u64, end: u64, count: usize) -> Vec<Work> {
    let count = count.max(1);
    let length = end.saturating_sub(start).saturating_add(1);
    let mut items = Vec::with_capacity(count);

    for index in 0..count {
        let begin = start + length * index as u64 / count as u64;
        let finish = start + length * (index as u64 + 1) / count as u64 - 1;
        items.push(Work {
            start: begin,
            end: finish,
            next: begin,
        });
    }

    items
}

fn plan_segments(total: u64, connections: usize, min_segment_bytes: u64) -> Vec<Work> {
    if total == 0 {
        return Vec::new();
    }

    let count = segment_count(total, connections, min_segment_bytes);
    split_range(0, total - 1, count)
}

/// 根据已完成的区间，把剩余缺口切成新的工作项
fn plan_remaining(
    total: u64,
    ranges: &[(u64, u64)],
    connections: usize,
    min_segment_bytes: u64,
) -> Vec<Work> {
    let mut gaps: Vec<(u64, u64)> = Vec::new();
    let mut cursor = 0u64;
    for (start, end) in ranges {
        if *start > cursor {
            gaps.push((cursor, start.saturating_sub(1)));
        }
        cursor = cursor.max(end.saturating_add(1));
    }
    if cursor < total {
        gaps.push((cursor, total - 1));
    }

    let remaining: u64 = gaps
        .iter()
        .map(|(start, end)| end.saturating_sub(*start).saturating_add(1))
        .sum();
    if remaining == 0 {
        return Vec::new();
    }

    let count = segment_count(remaining, connections, min_segment_bytes);
    let mut items = Vec::new();

    for (gap_start, gap_end) in gaps {
        let length = gap_end.saturating_sub(gap_start).saturating_add(1);
        let share = ((length as u128 * count as u128 / remaining as u128) as usize)
            .max(1)
            .min(length as usize);
        items.extend(split_range(gap_start, gap_end, share));
    }

    items
}

/* ============================== 断点元数据 ============================== */

#[derive(Clone, Serialize, Deserialize, Default)]
struct ResumeMeta {
    version: u32,
    url: String,
    total: u64,
    etag: String,
    last_modified: String,
    ranges: Vec<(u64, u64)>,
}

fn normalize_ranges(ranges: &mut Vec<(u64, u64)>) {
    ranges.sort_by_key(|range| range.0);
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (start, end) in ranges.drain(..) {
        if let Some(last) = merged.last_mut() {
            if start <= last.1.saturating_add(1) {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    *ranges = merged;
}

fn sum_ranges(ranges: &[(u64, u64)]) -> u64 {
    ranges
        .iter()
        .map(|(start, end)| end.saturating_sub(*start).saturating_add(1))
        .sum()
}

fn read_meta(path: &Path) -> Option<ResumeMeta> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<ResumeMeta>(&text).ok()
}

fn write_meta(path: &Path, meta: &ResumeMeta) -> Result<(), String> {
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    let text = serde_json::to_string(meta).map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp).map_err(|error| error.to_string())?;
        file.write_all(text.as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    std::fs::rename(&tmp, path).map_err(|error| error.to_string())
}

/* ============================== 文件名与路径 ============================== */

pub fn file_name_from_url(url: &str) -> String {
    let without_query = url.split('?').next().unwrap_or(url);
    let without_fragment = without_query.split('#').next().unwrap_or(without_query);
    let host = host_from_url(without_fragment);

    /* 只看路径部分，避免把域名当成最后一段 */
    let rest = without_fragment
        .strip_prefix("https://")
        .or_else(|| without_fragment.strip_prefix("http://"))
        .unwrap_or(without_fragment);
    let path = rest.splitn(2, '/').nth(1).unwrap_or("");

    let candidate = path.rsplit('/').find(|segment| !segment.is_empty()).unwrap_or("");

    let decoded = percent_decode(candidate);
    let lower = decoded.to_ascii_lowercase();

    /* 通用词不能当文件名：用「域名_download」代替 */
    const GENERIC: [&str; 7] = [
        "download",
        "downloadfile",
        "index",
        "file",
        "get",
        "default",
        "home",
    ];

    if decoded.is_empty() || GENERIC.contains(&lower.as_str()) {
        let base = if host.is_empty() {
            "download".to_string()
        } else {
            format!("{host}_download")
        };
        return sanitize_file_name(&base);
    }

    sanitize_file_name(&decoded)
}

/// 从 URL 里取主机名；端口号里的冒号换成下划线，避免非法字符
fn host_from_url(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let authority = rest.split('/').next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or(authority);
    host.replace(':', "_")
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(byte) = hex.and_then(|text| u8::from_str_radix(text, 16).ok()) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn sanitize_file_name(name: &str) -> String {
    const ILLEGAL: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let cleaned: String = name
        .chars()
        .map(|ch| if ILLEGAL.contains(&ch) || ch.is_control() { '_' } else { ch })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() {
        return "download".to_string();
    }

    /* Windows 保留设备名不能直接做文件名，例如 CON、NUL、COM1、LPT1 */
    let path = Path::new(&trimmed);
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| trimmed.clone());

    if is_reserved_name(&stem) {
        let extension = path
            .extension()
            .map(|value| format!(".{}", value.to_string_lossy()))
            .unwrap_or_default();
        return format!("{stem}_download{extension}");
    }

    trimmed
}

fn is_reserved_name(stem: &str) -> bool {
    let upper = stem.trim().to_ascii_uppercase();

    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }

    for prefix in ["COM", "LPT"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            if rest.len() == 1 && matches!(rest.as_bytes()[0], b'1'..=b'9') {
                return true;
            }
        }
    }

    false
}

/// 同目录下若已有同名文件，追加 (1)、(2)…
#[allow(dead_code)]
pub(crate) fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }

    let path = Path::new(name);
    let stem = path.file_stem().and_then(|v| v.to_str()).unwrap_or("download");
    let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("");

    for index in 1..10_000 {
        let name = if extension.is_empty() {
            format!("{stem} ({index})")
        } else {
            format!("{stem} ({index}).{extension}")
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }

    dir.join(name)
}

pub(crate) fn part_path_for(final_path: &Path) -> PathBuf {
    let extension = final_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    final_path.with_extension(format!("{extension}.adm-part"))
}

/// 断点元数据统一放到 datareakpoints，用路径哈希命名，避免污染下载目录
/// 读断点元数据，返回 (已下载字节, 总大小)
/// 重新下载：保留数据文件，只把断点重置为空区间（文件原地覆盖）
pub(crate) fn reset_breakpoint(part_path: &Path, url: &str) {
    let meta = ResumeMeta {
        version: 1,
        url: url.to_string(),
        total: 0,
        etag: String::new(),
        last_modified: String::new(),
        ranges: Vec::new(),
    };
    let _ = write_meta(&meta_path_for(part_path), &meta);
}

pub(crate) fn resume_progress(part_path: &Path) -> Option<(u64, u64)> {
    let meta = read_meta(&meta_path_for(part_path))?;
    let mut ranges = meta.ranges.clone();
    normalize_ranges(&mut ranges);
    Some((sum_ranges(&ranges).min(meta.total), meta.total))
}

pub(crate) fn meta_path_for(part_path: &Path) -> PathBuf {
    let key = part_path.display().to_string().to_ascii_lowercase();
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in key.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }

    crate::paths::data_dir()
        .join("breakpoints")
        .join(format!("{hash:016x}.json"))
}

/// 断点缓存超过上限时，删最旧的元数据（对应任务只能从头下）
fn enforce_breakpoint_limit(limit_bytes: u64) {
    let dir = crate::paths::data_dir().join("breakpoints");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };

    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    let mut total = 0u64;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        total += meta.len();
        files.push((modified, meta.len(), path));
    }

    if total <= limit_bytes {
        return;
    }

    files.sort_by_key(|item| item.0);
    for (_, size, path) in files {
        if total <= limit_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

/* ============================== 写盘 ============================== */

#[cfg(windows)]
fn write_at(file: &std::fs::File, buffer: &[u8], offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut written = 0usize;
    while written < buffer.len() {
        let count = file.seek_write(&buffer[written..], offset + written as u64)?;
        if count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "写入返回 0",
            ));
        }
        written += count;
    }
    Ok(())
}

#[cfg(unix)]
fn write_at(file: &std::fs::File, buffer: &[u8], offset: u64) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(buffer, offset)
}

/* ============================== 单段下载 ============================== */

struct Shared {
    file: std::fs::File,
    meta: Mutex<ResumeMeta>,
    meta_path: PathBuf,
    supports_ranges: bool,
    pending: Mutex<Vec<Option<(u64, u64)>>>,
    unsynced: AtomicU64,
    checkpoint_gate: AsyncMutex<()>,
    disk_check: Arc<DiskCheckState>,
}

struct DiskCheckState {
    started: AtomicBool,
    finished: AtomicBool,
    wake: tokio::sync::Notify,
}

impl DiskCheckState {
    fn new() -> Self {
        Self {
            started: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            wake: tokio::sync::Notify::new(),
        }
    }
}

impl Shared {
    /// worker 只登记已达到的区间；检查点会统一决定何时刷盘。
    fn report_progress(&self, slot: usize, start: u64, end: u64, bytes: u64) {
        if end < start {
            return;
        }
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(entry) = pending.get_mut(slot) {
                *entry = Some(match *entry {
                    Some((old_start, old_end)) if old_start == start => (old_start, old_end.max(end)),
                    _ => (start, end),
                });
            }
        }
        self.unsynced.fetch_add(bytes, Ordering::AcqRel);
    }

    fn should_checkpoint(&self) -> bool {
        self.unsynced.load(Ordering::Acquire) >= CHECKPOINT_BYTES
    }

    fn start_parallel_disk_check(
        &self,
        dir: PathBuf,
        total: u64,
        control: TransferControl,
    ) {
        if self.disk_check.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let completed = sum_ranges(&self.meta.lock_ok().ranges);
        let remaining = total.saturating_sub(completed);
        let state = Arc::clone(&self.disk_check);
        tauri::async_runtime::spawn(async move {
            let result = tauri::async_runtime::spawn_blocking(move || {
                ensure_disk_space(&dir, remaining)
            })
            .await
            .map_err(|error| format!("write|磁盘检查任务失败：{error}"))
            .and_then(|result| result);

            if let Err(error) = result {
                control.fail(error);
                control.cancel();
            }
            state.finished.store(true, Ordering::Release);
            state.wake.notify_waiters();
        });
    }

    async fn wait_parallel_disk_check(&self) {
        if !self.disk_check.started.load(Ordering::Acquire) {
            return;
        }
        loop {
            if self.disk_check.finished.load(Ordering::Acquire) {
                return;
            }
            self.disk_check.wake.notified().await;
        }
    }

    /// 服务器忽略 Range 时保留原缓存文件，但从偏移 0 单连接完整重写。
    fn reset_for_single_stream(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.fill(None);
        }
        self.unsynced.store(0, Ordering::Release);
        if let Ok(mut meta) = self.meta.lock() {
            meta.ranges.clear();
            if let Err(error) = write_meta(&self.meta_path, &meta) {
                eprintln!("[transfer] 单连接断点重置失败: {error}");
            }
        }
    }

    /// 累计约 8 MiB 后统一刷盘；只有刷盘成功的区间才能写入断点。
    async fn checkpoint(&self) {
        if !self.should_checkpoint() {
            return;
        }
        let _guard = self.checkpoint_gate.lock().await;
        let flushed_bytes = self.unsynced.swap(0, Ordering::AcqRel);
        if flushed_bytes < CHECKPOINT_BYTES {
            self.unsynced.fetch_add(flushed_bytes, Ordering::AcqRel);
            return;
        }

        let ranges: Vec<(usize, u64, u64)> = self
            .pending
            .lock_ok()
            .iter_mut()
            .enumerate()
            .filter_map(|(slot, range)| range.take().map(|(start, end)| (slot, start, end)))
            .collect();

        if let Err(error) = self.file.sync_data() {
            eprintln!("[transfer] 检查点刷盘失败: {error}");
            if let Ok(mut pending) = self.pending.lock() {
                for (slot, start, end) in ranges {
                    if let Some(entry) = pending.get_mut(slot) {
                        *entry = Some(match *entry {
                            Some((old_start, old_end)) if old_start == start => {
                                (old_start, old_end.max(end))
                            }
                            _ => (start, end),
                        });
                    }
                }
            }
            self.unsynced.fetch_add(flushed_bytes, Ordering::AcqRel);
            return;
        }

        if let Ok(mut meta) = self.meta.lock() {
            meta.ranges
                .extend(ranges.into_iter().map(|(_, start, end)| (start, end)));
            normalize_ranges(&mut meta.ranges);
            if let Err(error) = write_meta(&self.meta_path, &meta) {
                eprintln!("[transfer] 断点元数据写入失败: {error}");
            }
        }
    }

    fn record(&self, slot: usize, start: u64, end: u64) -> Result<(), String> {
        if end < start {
            return Ok(());
        }
        self.file
            .sync_data()
            .map_err(|error| format!("write|刷盘失败：{error}"))?;
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(entry) = pending.get_mut(slot) {
                *entry = None;
            }
        }
        let mut meta = self.meta.lock_ok();
        meta.ranges.push((start, end));
        normalize_ranges(&mut meta.ranges);
        if let Err(error) = write_meta(&self.meta_path, &meta) {
            eprintln!("[transfer] 断点元数据写入失败: {error}");
            return Err(format!("write|断点元数据写入失败：{error}"));
        }
        Ok(())
    }
}

fn classify_status(status: u16) -> Option<RangeError> {
    match status {
        401 | 403 => Some(RangeError::Fatal(format!("http_status|{status}"))),
        404 | 410 => Some(RangeError::Fatal(format!("http_status|{status}"))),
        408 | 425 | 429 | 500..=599 => Some(RangeError::Transient(format!("http_status|{status}"))),
        416 => Some(RangeError::Fatal("http_status|416".to_string())),
        _ => None,
    }
}

/// 等待暂停或取消事件；用于打断网络读取、限速等待和重试退避
async fn wait_for_stop(control: &TransferControl) {
    loop {
        if control.is_cancelled() || control.is_pause_stop() {
            return;
        }
        let notified = control.wake.notified();
        if control.is_cancelled() || control.is_pause_stop() {
            return;
        }
        notified.await;
    }
}

async fn backoff_or_stop(
    control: &TransferControl,
    attempt: u32,
) -> Result<(), RangeError> {
    tokio::select! {
        _ = backoff(attempt) => Ok(()),
        _ = wait_for_stop(control) => Err(RangeError::Cancelled),
    }
}

async fn download_range(
    client: &reqwest::Client,
    req: &TransferRequest,
    control: &TransferControl,
    shared: &Shared,
    work: &mut Work,
    if_range: &str,
    slot: usize,
    max_attempts: u32,
) -> Result<(), RangeError> {
    let mut attempt = 0u32;
    let mut throttle = Throttle::new();

    loop {
        if control.is_cancelled() {
            return Err(RangeError::Cancelled);
        }
        control.wait_if_paused_or_stop(slot).await;
        if control.is_cancelled() {
            return Err(RangeError::Cancelled);
        }
        if work.next > work.end {
            return Ok(());
        }

        let mut request = client
            .get(&req.url)
            .header("Accept-Encoding", "identity");

        if !req.force_single {
            request = request.header("Range", format!("bytes={}-{}", work.next, work.end));
            if !if_range.is_empty() {
                request = request.header("If-Range", if_range);
            }
        }
        if let Some(referrer) = &req.referrer {
            request = request.header("Referer", referrer);
        }
        if let Some(cookies) = &req.cookies {
            let clean = cookies.replace(['\r', '\n'], "");
            if !clean.is_empty() {
                request = request.header("Cookie", clean);
            }
        }
        if let Some(header) = auth_header(req) {
            request = request.header("Authorization", header);
        }

        let sent = tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(10), request.send()) => result,
            _ = wait_for_stop(control) => return Err(RangeError::Cancelled),
        };

        let response = match sent {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                attempt += 1;
                if attempt > max_attempts {
                    return Err(RangeError::Transient(format!("network|{error}")));
                }
                backoff_or_stop(control, attempt).await?;
                continue;
            }
            Err(_) => {
                attempt += 1;
                if attempt > max_attempts {
                    return Err(RangeError::Transient("network|first_byte_timeout".to_string()));
                }
                backoff_or_stop(control, attempt).await?;
                continue;
            }
        };

        let status = response.status().as_u16();

        if status == 401 || status == 403 {
            if let Some(scheme) = auth_scheme(response.headers()) {
                control.set_slot_state(slot, 3);
                return Err(RangeError::Fatal(format!(
                    "auth_required|{status}|{scheme}"
                )));
            }
        }

        if status == 429 {
            control.mark_rate_limited();
            control.mark_first_response();
            return Err(RangeError::RateLimited);
        }

        if let Some(error) = classify_status(status) {
            match error {
                RangeError::Transient(_) => {
                    attempt += 1;
                    if attempt > max_attempts {
                        return Err(error);
                    }
                    backoff(attempt).await;
                    continue;
                }
                other => {
                    control.set_slot_state(slot, 3);
                    return Err(other);
                }
            }
        }

        if status != 200 && status != 206 {
            control.set_slot_state(slot, 3);
            return Err(RangeError::Fatal(format!("http_status|{status}")));
        }

        /* 服务器忽略了 Range，返回整份内容：当前响应直接转为单连接 */
        if status == 200 && shared.supports_ranges && !req.force_single {
            let total = shared.meta.lock_ok().total;
            work.start = 0;
            work.next = 0;
            work.end = total.saturating_sub(1);
            shared.reset_for_single_stream();
            control.set_downloaded(0);
            control.mark_single_mode();
        }

        let expected_total = shared.meta.lock_ok().total;
        let response_size = response_total(&response);
        if response_size.is_some_and(|total| total != expected_total) {
            control.set_slot_state(slot, 3);
            let message = "incomplete|服务器文件大小在排队后发生变化".to_string();
            control.fail(message.clone());
            return Err(RangeError::Fatal(message));
        }
        let disk_total = response_size.unwrap_or(expected_total);
        shared.start_parallel_disk_check(
            req.save_dir.clone(),
            disk_total,
            control.clone(),
        );

        control.mark_first_response();

        control.set_slot_state(slot, 2);
        let mut response = response;
        let mut offset = work.next;

        loop {
            if control.is_cancelled() {
                return Err(RangeError::Cancelled);
            }
            control.wait_if_paused_or_stop(slot).await;
            if control.is_cancelled() || control.is_pause_stop() {
                return Err(RangeError::Cancelled);
            }

            let received = tokio::select! {
                result = tokio::time::timeout(Duration::from_secs(30), response.chunk()) => result,
                _ = wait_for_stop(control) => return Err(RangeError::Cancelled),
            };

            let chunk = match received {
                Ok(Ok(Some(chunk))) => chunk,
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    if control.is_single_mode() {
                        return Err(RangeError::Fatal(format!("network|{error}")));
                    }
                    attempt += 1;
                    if attempt > max_attempts {
                        return Err(RangeError::Transient(format!("network|{error}")));
                    }
                    backoff_or_stop(control, attempt).await?;
                    /* 已写入的部分要记录下来，重新按新位置续 */
                    if let Err(error) =
                        shared.record(slot, work.start, offset.saturating_sub(1))
                    {
                        return Err(RangeError::Fatal(error));
                    }
                    work.next = offset;
                    break;
                }
                Err(_) => {
                    if control.is_single_mode() {
                        return Err(RangeError::Fatal("network|idle_timeout".to_string()));
                    }
                    attempt += 1;
                    if attempt > max_attempts {
                        return Err(RangeError::Transient("network|idle_timeout".to_string()));
                    }
                    backoff_or_stop(control, attempt).await?;
                    if let Err(error) =
                        shared.record(slot, work.start, offset.saturating_sub(1))
                    {
                        return Err(RangeError::Fatal(error));
                    }
                    work.next = offset;
                    break;
                }
            };

            if chunk.is_empty() {
                continue;
            }

            if let Err(error) = write_at(&shared.file, &chunk, offset) {
                return Err(RangeError::Fatal(format!("write|{error}")));
            }

            offset += chunk.len() as u64;
            /* 立即同步到 work.next：暂停/出错时外层记录的区间才是准的 */
            work.next = offset;
            control.add_downloaded(chunk.len() as u64);
            control.sample_speed(slot, chunk.len() as u64);

            shared.report_progress(
                slot,
                work.start,
                work.next.saturating_sub(1),
                chunk.len() as u64,
            );
            /* 按任务累计约 8 MiB 触发；由 Shared 串行完成刷盘和断点提交。 */
            if shared.should_checkpoint() {
                shared.checkpoint().await;
            }
            tokio::select! {
                _ = throttle.apply(chunk.len() as u64, control.limit()) => {}
                _ = wait_for_stop(control) => return Err(RangeError::Cancelled),
            }
        }

        if offset > work.next {
            work.next = offset;
        }

        if control.is_single_mode() {
            if work.next > work.end {
                return Ok(());
            }
            return Err(RangeError::Fatal("incomplete|".to_string()));
        }

        if work.next > work.end {
            return Ok(());
        }

        attempt += 1;
        if attempt > max_attempts {
            return Err(RangeError::Transient("network|retries_exhausted".to_string()));
        }
        backoff_or_stop(control, attempt).await?;
    }
}

/// 简单的分片内限速：按已发送字节与目标速率比较，超速则等待。
/// 速率每次现取，运行中改设置也能立刻生效。
struct Throttle {
    sent: u64,
    started: Instant,
}

impl Throttle {
    fn new() -> Self {
        Self {
            sent: 0,
            started: Instant::now(),
        }
    }

    async fn apply(&mut self, bytes: u64, limit: u64) {
        if limit == 0 {
            /* 取消限速后重置统计，避免之后突然被补账 */
            self.sent = 0;
            self.started = Instant::now();
            return;
        }
        self.sent += bytes;
        let expected = Duration::from_secs_f64(self.sent as f64 / limit as f64);
        let actual = self.started.elapsed();
        if expected > actual {
            tokio::time::sleep(expected - actual).await;
        }
    }
}

async fn backoff(_attempt: u32) {
    /* 超时、429、5xx 统一冷却 2 秒 + 抖动 */
    let jitter = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.subsec_millis() as u64)
        .unwrap_or(0))
        % 400;
    tokio::time::sleep(Duration::from_millis(2_000 + jitter)).await;
}

/* ============================== 磁盘空间 ============================== */

/// 目标目录所在磁盘的可用字节数；取不到时返回 None（跳过检查，不阻塞下载）
#[cfg(windows)]
fn free_space_for(dir: &Path) -> Option<u64> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let text = dir.display().to_string();
    if text.len() < 2 || !text[1..].starts_with(':') {
        return None;
    }
    let root = format!("{}\\", &text[..2]);
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut free = 0u64;
        GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None).ok()?;
        Some(free)
    }
}

#[cfg(not(windows))]
fn free_space_for(_dir: &Path) -> Option<u64> {
    None
}

/// 同一任务只允许一个下载核心会话；不同任务使用不同锁，互不阻塞。
fn task_session_gate(task_id: u64) -> Arc<AsyncMutex<()>> {
    static GATES: OnceLock<Mutex<HashMap<u64, Weak<AsyncMutex<()>>>>> = OnceLock::new();
    let gates = GATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut gates = gates.lock_ok();
    gates.retain(|_, weak| weak.strong_count() > 0);

    if let Some(gate) = gates.get(&task_id).and_then(Weak::upgrade) {
        return gate;
    }

    let gate = Arc::new(AsyncMutex::new(()));
    gates.insert(task_id, Arc::downgrade(&gate));
    gate
}

/* ============================== 主流程 ============================== */

pub async fn download(
    req: TransferRequest,
    control: TransferControl,
    probe: Probe,
) -> Result<DownloadOutcome, String> {
    let session_gate = task_session_gate(req.id);
    let _session_guard = session_gate.lock().await;
    if control.is_cancelled() {
        return Err("cancelled|".to_string());
    }

    let client = match build_client(&req.proxy, req.user_agent.as_deref()) {
        Ok(client) => client,
        Err(error) => {
            control.fail(error.clone());
            return Err(error);
        }
    };

    if probe.total == 0 {
        let error = "empty_file|".to_string();
        control.fail(error.clone());
        return Err(error);
    }

    control.set_total(probe.total);
    control.set_status(TransferStatus::Downloading);

    let url_name = file_name_from_url(&req.url);
    let mut name = if req.file_name.trim().is_empty() {
        url_name.clone()
    } else {
        req.file_name.clone()
    };
    if req.file_name.trim().is_empty() || req.file_name == url_name {
        if let Some(header) = filename_from_disposition(probe.content_disposition.as_deref()) {
            name = header;
        }
    }
    let name = sanitize_file_name(&name);

    if let Err(error) = std::fs::create_dir_all(&req.save_dir) {
        let message = format!("create_dir|{error}");
        control.fail(message.clone());
        return Err(message);
    }

    /* 进入并行下载时再判断一次磁盘，排队期间其他任务可能已占用空间。 */
    if let Err(message) = ensure_disk_space(&req.save_dir, probe.total) {
        control.fail(message.clone());
        return Err(message);
    }

    /* 续传要求 .adm-part 与 .adm-part.json 同时存在；缺一个就整体作废 */
    let requested = req.save_dir.join(&name);
    let resume_part = part_path_for(&requested);
    let resume_meta = meta_path_for(&resume_part);
    let resumable = resume_part.exists() && resume_meta.exists();

    let (final_path, part_path) = if resumable {
        (requested.clone(), resume_part)
    } else {
        let _ = std::fs::remove_file(&resume_part);
        let _ = std::fs::remove_file(&resume_meta);

        /* 名字在创建时就预留好了；真被外部占用则直接报错，不再偷偷改名 */
        if requested.exists() {
            let message = "name_taken|".to_string();
            control.fail(message.clone());
            return Err(message);
        }

        (requested.clone(), resume_part)
    };
    let meta_path = meta_path_for(&part_path);
    control.set_part_path(part_path.clone());

    let limit = crate::settings::current().cache_mib.max(1) as u64 * MIB;
    enforce_breakpoint_limit(limit);

    let mut meta = if part_path.exists() {
        read_meta(&meta_path)
            .filter(|meta| {
                meta.url == req.url
                    && meta.total == probe.total
                    && (meta.etag.is_empty() || meta.etag == probe.etag)
                    && (meta.last_modified.is_empty() || meta.last_modified == probe.last_modified)
            })
            .unwrap_or_else(|| ResumeMeta {
                version: 1,
                url: req.url.clone(),
                total: probe.total,
                etag: probe.etag.clone(),
                last_modified: probe.last_modified.clone(),
                ranges: Vec::new(),
            })
    } else {
        ResumeMeta {
            version: 1,
            url: req.url.clone(),
            total: probe.total,
            etag: probe.etag.clone(),
            last_modified: probe.last_modified.clone(),
            ranges: Vec::new(),
        }
    };

    if !part_path.exists() {
        meta.ranges.clear();
    }

    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        /* 允许清除操作在下载进行中就把文件标记为「关闭即删除」 */
        options.share_mode(0x1 | 0x2 | 0x4);
    }

    let file = match options.open(&part_path) {
        Ok(file) => file,
        Err(error) => {
            let message = format!("create_file|{error}");
            control.fail(message.clone());
            return Err(message);
        }
    };

    /* 预占空间：长度对齐总大小；旧文件更大时截断，避免成品多出尾部字节 */
    if let Ok(metadata) = file.metadata() {
        if metadata.len() != probe.total {
            if let Err(error) = file.set_len(probe.total) {
                let message = format!("reserve|{error}");
                control.fail(message.clone());
                return Err(message);
            }
        }
    }

    /* 续传时按已有区间补齐进度 */
    normalize_ranges(&mut meta.ranges);
    let existing = sum_ranges(&meta.ranges).min(probe.total);
    control.set_downloaded(existing);

    let shared = Arc::new(Shared {
        file,
        meta: Mutex::new(meta.clone()),
        meta_path: meta_path.clone(),
        supports_ranges: probe.supports_ranges,
        pending: Mutex::new(vec![None; 64]),
        unsynced: AtomicU64::new(0),
        checkpoint_gate: AsyncMutex::new(()),
        disk_check: Arc::new(DiskCheckState::new()),
    });

    let works = if req.force_single || !probe.supports_ranges {
        plan_segments(probe.total, 1, req.min_segment_bytes)
    } else {
        plan_remaining(probe.total, &meta.ranges, req.connections, req.min_segment_bytes)
    };

    if works.is_empty() && existing >= probe.total {
        /* 全部区间已完成：先松开文件句柄，再由系统重命名 */
        drop(shared);
        return Ok(finish(part_path, final_path, &control));
    }

    let planned_works = works.len();
    let queue = Arc::new(AsyncMutex::new(VecDeque::from(works)));
    let if_range = if probe.etag.is_empty() {
        probe.last_modified.clone()
    } else {
        probe.etag.clone()
    };

    let workers = if probe.supports_ranges && !req.force_single {
        /* 小文件段数不足时，不为空段启动多余 worker */
        req.connections
            .clamp(1, 64)
            .min(planned_works.max(1))
    } else {
        1
    };

    control.init_speeds(64);
    control.set_speed_slots(workers);

    control.set_session_active(true);

    /* 连接数在任务创建时就固定：直接启动 N 个 worker，等它们全部结束。
       暂停/取消通过标记 + Notify 唤醒 worker，不再有监督轮询。 */
    let mut set: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();

    let step = (req.connections / 16).max(1);
    let per_connection_attempts = ((req.connections / workers.max(1)).max(1)) as u32;
    let mut launched = 0usize;

    while launched < workers {
        let batch = if launched == 0 {
            1
        } else {
            (workers - launched).min(step)
        };

        for slot in launched..launched + batch {
            let client = client.clone();
            let req = req.clone();
            let control = control.clone();
            let shared = Arc::clone(&shared);
            let queue = Arc::clone(&queue);
            let if_range = if_range.clone();

            control.reset_speed(slot);
            control.set_slot_state(slot, 0);
            set.spawn(async move {
                run_worker(slot, client, req, control, shared, queue, if_range, per_connection_attempts).await;
            });
        }

        launched += batch;
        if launched >= workers {
            break;
        }

        if launched == 1 {
            /* 等第一个探路连接拿到响应头，再决定是否继续开连接 */
            tokio::select! {
                _ = control.wait_first_response() => {}
                _ = tokio::time::sleep(Duration::from_secs(10)) => {}
                _ = wait_for_stop(&control) => break,
            }

            if control.is_single_mode() {
                break;
            }
        } else {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                _ = wait_for_stop(&control) => break,
            }
        }

        if control.is_cancelled()
            || control.is_rate_limited()
            || control.snapshot().status == TransferStatus::Failed
        {
            break;
        }
    }

    while set.join_next().await.is_some() {}
    shared.wait_parallel_disk_check().await;

    /* 第一次 429 后：所有已进入的连接都退出，若仍未下完，只留一个连接重试 2 次 */
    let mut final_retries = 0u32;
    loop {
        let done_now = {
            let guard = shared.meta.lock_ok();
            sum_ranges(&guard.ranges)
        };
        if !control.is_rate_limited()
            || done_now >= probe.total
            || final_retries >= 2
            || control.is_cancelled()
        {
            break;
        }

        final_retries += 1;
        let client = client.clone();
        let req = req.clone();
        let control = control.clone();
        let shared = Arc::clone(&shared);
        let queue = Arc::clone(&queue);
        let if_range = if_range.clone();

        control.reset_speed(0);
        control.set_slot_state(0, 0);
        set.spawn(async move {
            run_worker(0, client, req, control, shared, queue, if_range, per_connection_attempts)
                .await;
        });

        while set.join_next().await.is_some() {}
    }

    if control.is_rate_limited() {
        let done_now = {
            let guard = shared.meta.lock_ok();
            sum_ranges(&guard.ranges)
        };
        if done_now < probe.total {
            control.fail("http_status|429".to_string());
            return Err("http_status|429".to_string());
        }
    }

    let progress = control.snapshot();
    if progress.status == TransferStatus::Failed {
        return Err(progress.error.unwrap_or_else(|| "下载失败".to_string()));
    }
    if control.is_cancelled() {
        control.set_status(TransferStatus::Cancelled);
        return Err("cancelled|".to_string());
    }

    let remaining = {
        let guard = shared.meta.lock_ok();
        guard.ranges.clone()
    };
    let done = sum_ranges(&remaining);

    /*
     * 这里必须显式释放缓存文件句柄：
     * - 完成时要改名，Windows 上保留句柄会让重命名失败
     * - 暂停/取消/失败时也不能继续持有，否则上层删缓存会一直被占用
     */
    drop(queue);
    drop(shared);
    control.set_session_active(false);

    if done >= probe.total {
        Ok(finish(part_path, final_path, &control))
    } else if control.is_pause_stop() {
        control.set_status(TransferStatus::Paused);
        Err("paused|".to_string())
    } else {
        let message = "incomplete|".to_string();
        control.fail(message.clone());
        Err(message)
    }
}

fn record_progress(
    shared: &Shared,
    control: &TransferControl,
    slot: usize,
    start: u64,
    end: u64,
) -> bool {
    match shared.record(slot, start, end) {
        Ok(()) => true,
        Err(error) => {
            control.fail(error);
            false
        }
    }
}

/// 单个 worker：从共享队列取分段，支持运行中被"让位"
async fn run_worker(
    slot: usize,
    client: reqwest::Client,
    req: TransferRequest,
    control: TransferControl,
    shared: Arc<Shared>,
    queue: Arc<AsyncMutex<VecDeque<Work>>>,
    if_range: String,
    max_attempts: u32,
) {
    control.worker_in();

    loop {
        control.wait_if_paused_or_stop(slot).await;
        if control.is_cancelled() || control.is_pause_stop() {
            break;
        }

        let work = {
            let mut guard = queue.lock().await;
            guard.pop_front()
        };
        let Some(mut work) = work else {
            break;
        };

        control.set_slot_state(slot, 1);

        match download_range(&client, &req, &control, &shared, &mut work, &if_range, slot, max_attempts).await {
            Ok(()) => {
                if !record_progress(&shared, &control, slot, work.start, work.end) {
                    break;
                }
            }
            Err(RangeError::RateLimited) => {
                if !record_progress(
                    &shared,
                    &control,
                    slot,
                    work.start,
                    work.next.saturating_sub(1),
                ) {
                    break;
                }
                let mut guard = queue.lock().await;
                guard.push_front(work);
                control.reset_speed(slot);
                control.set_slot_state(slot, 3);
                break;
            }
            Err(RangeError::Cancelled) => {
                if control.is_pause_stop() {
                    if !record_progress(
                        &shared,
                        &control,
                        slot,
                        work.start,
                        work.next.saturating_sub(1),
                    ) {
                        break;
                    }
                }
                control.set_slot_state(slot, 0);
                break;
            }
            Err(RangeError::Transient(message)) => {
                let recorded = record_progress(
                    &shared,
                    &control,
                    slot,
                    work.start,
                    work.next.saturating_sub(1),
                );
                let mut guard = queue.lock().await;
                guard.push_front(work);
                control.set_slot_state(slot, 3);
                if recorded {
                    control.fail(message);
                }
                break;
            }
            Err(RangeError::Fatal(message)) => {
                let recorded = record_progress(
                    &shared,
                    &control,
                    slot,
                    work.start,
                    work.next.saturating_sub(1),
                );
                control.set_slot_state(slot, 3);
                if recorded {
                    control.fail(message);
                }
                break;
            }
        }
    }

    control.worker_out();
    control.wake.notify_waiters();
}

fn finish(
    part_path: PathBuf,
    final_path: PathBuf,
    control: &TransferControl,
) -> DownloadOutcome {
    control.set_status(TransferStatus::Completed);
    match std::fs::rename(&part_path, &final_path) {
        Ok(()) => DownloadOutcome {
            final_path: Some(final_path),
            part_path,
            warning: None,
        },
        Err(error) => DownloadOutcome {
            final_path: None,
            part_path,
            warning: Some(format!("rename|{error}")),
        },
    }
}

fn filename_from_disposition(value: Option<&str>) -> Option<String> {
    let value = value?;
    for part in value.split(';').map(|item| item.trim()) {
        if let Some(rest) = part.strip_prefix("filename*=") {
            /* filename*=UTF-8''%E4%B8%AD%E6%96%87.zip */
            let encoded = rest.split("''").nth(1).unwrap_or(rest);
            return Some(sanitize_file_name(&percent_decode(encoded)));
        }
        if let Some(rest) = part.strip_prefix("filename=") {
            return Some(sanitize_file_name(rest.trim_matches('"')));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_windows_names_are_renamed() {
        assert_eq!(sanitize_file_name("CON"), "CON_download");
        assert_eq!(sanitize_file_name("CON.txt"), "CON_download.txt");
        assert_eq!(sanitize_file_name("com1.bin"), "com1_download.bin");
        assert_eq!(sanitize_file_name("NUL."), "NUL_download");
        assert_eq!(sanitize_file_name("normal.txt"), "normal.txt");
    }

    #[test]
    fn segments_follow_min_size_and_cap() {
        /* 63.5 MiB / 4 MiB = 15 段，均分后每段略大于 4 MiB */
        let size = 63 * MIB + MIB / 2;
        let works = plan_segments(size, 16, 4 * MIB);
        assert_eq!(works.len(), 15);
        assert_eq!(works[0].start, 0);
        assert_eq!(works[14].end, size - 1);
        for pair in works.windows(2) {
            assert_eq!(pair[0].end + 1, pair[1].start);
            assert!(pair[0].end - pair[0].start + 1 >= 4 * MIB);
        }

        assert_eq!(plan_segments(10 * MIB, 16, 4 * MIB).len(), 2);
        assert_eq!(plan_segments(10 * MIB, 32, 2 * MIB).len(), 5);
        assert_eq!(plan_segments(10 * MIB, 64, MIB).len(), 10);
        assert_eq!(plan_segments(100 * MIB, 16, 4 * MIB).len(), 25);
        assert_eq!(plan_segments(256 * MIB, 16, 4 * MIB).len(), 64);
        assert_eq!(plan_segments(257 * MIB, 16, 4 * MIB).len(), 64);
    }

    #[test]
    fn segments_for_small_file() {
        let works = plan_segments(512 * 1024, 8, 0);
        assert_eq!(works.len(), 1);
        assert_eq!(works[0].start, 0);
        assert_eq!(works[0].end, 512 * 1024 - 1);
    }

    #[test]
    fn segments_scale_with_size() {
        /* 100 MiB、8 连接 → 8 段 */
        let works = plan_segments(100 * MIB, 8, 0);
        assert_eq!(works.len(), 8);

        /* 2 GiB、8 连接 → 32 段 */
        let works = plan_segments(2 * GIB, 8, 0);
        assert_eq!(works.len(), 32);
    }

    #[test]
    fn ranges_merge() {
        let mut ranges = vec![(0, 9), (10, 19), (30, 39), (35, 49)];
        normalize_ranges(&mut ranges);
        assert_eq!(ranges, vec![(0, 19), (30, 49)]);
        assert_eq!(sum_ranges(&ranges), 40);
    }

    #[test]
    fn file_names() {
        assert_eq!(file_name_from_url("https://a.com/x/%E4%B8%AD.zip"), "中.zip");
        assert_eq!(sanitize_file_name("a/b:c?.zip"), "a_b_c_.zip");
    }

    #[test]
    fn task_session_gate_serializes_same_task_only() {
        let first = task_session_gate(u64::MAX);
        let same_task = task_session_gate(u64::MAX);
        assert!(Arc::ptr_eq(&first, &same_task));

        let guard = first.blocking_lock();
        assert!(same_task.try_lock().is_err());
        drop(guard);
        assert!(same_task.try_lock().is_ok());

        let other_task = task_session_gate(u64::MAX - 1);
        let _other_guard = other_task.try_lock().expect("不同任务不能互相阻塞");
    }

    #[test]
    fn rename_failure_keeps_completed_data_and_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "adm-rename-failure-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建测试目录");

        let part = dir.join("aaa.exe.adm-part");
        let meta = dir.join("breakpoint.json");
        let final_path = dir.join("missing-directory").join("aaa.exe");
        std::fs::write(&part, b"complete data").expect("写缓存");
        std::fs::write(&meta, b"{}").expect("写断点");

        let control = TransferControl::new();
        let outcome = finish(part.clone(), final_path, &control);

        assert!(outcome.final_path.is_none());
        assert!(outcome.warning.is_some());
        assert_eq!(control.snapshot().status, TransferStatus::Completed);
        assert!(part.exists(), "改名失败不能删除下载缓存");
        assert!(meta.exists(), "改名失败不能删除断点元数据");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_waits_for_eight_mib_across_workers() {
        let dir = std::env::temp_dir().join(format!(
            "adm-checkpoint-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建测试目录");

        let data_path = dir.join("checkpoint.adm-part");
        let meta_path = dir.join("checkpoint.json");
        let file = std::fs::File::create(&data_path).expect("创建数据文件");
        let meta = ResumeMeta {
            version: 1,
            url: "https://example.com/file".to_string(),
            total: 8 * MIB,
            etag: String::new(),
            last_modified: String::new(),
            ranges: Vec::new(),
        };
        let shared = Shared {
            file,
            meta: Mutex::new(meta),
            meta_path: meta_path.clone(),
            supports_ranges: true,
            pending: Mutex::new(vec![None; 64]),
            unsynced: AtomicU64::new(0),
            checkpoint_gate: AsyncMutex::new(()),
            disk_check: Arc::new(DiskCheckState::new()),
        };

        shared.report_progress(0, 0, 4 * MIB - 1, 4 * MIB);
        assert!(!shared.should_checkpoint());
        shared.report_progress(1, 4 * MIB, 8 * MIB - 1, 4 * MIB);
        assert!(shared.should_checkpoint());

        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("创建测试运行时");
        runtime.block_on(shared.checkpoint());

        let saved = read_meta(&meta_path).expect("读取断点");
        assert_eq!(saved.ranges, vec![(0, 8 * MIB - 1)]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn content_range_requires_a_real_total() {
        let valid = parse_content_range("bytes 0-1/1048576").expect("有效范围");
        assert_eq!(valid.start, 0);
        assert_eq!(valid.end, 1);
        assert_eq!(valid.total, Some(1_048_576));

        let unknown = parse_content_range("bytes 0-1/*").expect("总大小未知但格式有效");
        assert_eq!(unknown.total, None);
        assert!(parse_content_range("bytes 2-1/100").is_none());
        assert!(parse_content_range("items 0-1/100").is_none());
    }

    #[test]
    fn auth_challenge_recognizes_basic_and_bearer() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::WWW_AUTHENTICATE,
            reqwest::header::HeaderValue::from_static("Basic realm=\"download\""),
        );
        assert_eq!(auth_scheme(&headers), Some("Basic"));

        headers.insert(
            reqwest::header::WWW_AUTHENTICATE,
            reqwest::header::HeaderValue::from_static("Bearer realm=\"download\""),
        );
        assert_eq!(auth_scheme(&headers), Some("Bearer"));
    }

    #[test]
    fn single_stream_reset_keeps_existing_part_file() {
        let dir = std::env::temp_dir().join(format!(
            "adm-single-stream-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建测试目录");

        let data_path = dir.join("single.adm-part");
        let meta_path = dir.join("single.json");
        std::fs::write(&data_path, b"old data").expect("写缓存");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&data_path)
            .expect("打开缓存");
        let shared = Shared {
            file,
            meta: Mutex::new(ResumeMeta {
                version: 1,
                url: "https://example.com/file".to_string(),
                total: 8,
                etag: String::new(),
                last_modified: String::new(),
                ranges: vec![(0, 3)],
            }),
            meta_path: meta_path.clone(),
            supports_ranges: true,
            pending: Mutex::new(vec![None; 64]),
            unsynced: AtomicU64::new(4),
            checkpoint_gate: AsyncMutex::new(()),
            disk_check: Arc::new(DiskCheckState::new()),
        };

        shared.reset_for_single_stream();

        assert!(data_path.exists(), "切换单连接不能删除缓存文件");
        assert_eq!(read_meta(&meta_path).expect("读取断点").ranges, Vec::new());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disk_check_rejects_impossible_file_size() {
        let dir = std::env::temp_dir();
        let error = ensure_disk_space(&dir, u64::MAX).expect_err("超大文件不能通过磁盘检查");
        assert!(error.starts_with("disk_space|"));
    }
}
