use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::System;

/// 静态信息：软件启动时读一次，整个生命周期复用，不参与任何轮询
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub cpu_model: String,
    /// 内存总量，单位字节
    pub memory_total: u64,
}

/// 下载线路信息：随代理设置变化，需要按需刷新
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteInfo {
    /// direct / system-proxy
    pub route_kind: String,
    /// 代理端点，直连时为空
    pub route_detail: String,
    /// 系统默认路由是否被 TUN 虚拟网卡接管
    pub tun: bool,
}

/* ---------------- CPU 型号归一化 ---------------- */

fn normalize_cpu_model(raw: &str) -> String {
    let mut without_parens = String::with_capacity(raw.len());
    let mut depth = 0usize;
    for ch in raw.chars() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => without_parens.push(ch),
            _ => {}
        }
    }

    let base = without_parens.split('@').next().unwrap_or("").trim();

    let mut tokens: Vec<&str> = Vec::new();
    for token in base.split_whitespace() {
        let lower = token.to_ascii_lowercase();

        if lower == "with" {
            break;
        }
        if lower == "cpu" || lower == "processor" {
            continue;
        }
        if let Some(prefix) = lower.strip_suffix("-core") {
            if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
        }
        tokens.push(token);
    }

    let joined = tokens.join(" ").trim().to_string();
    if joined.is_empty() {
        raw.trim().to_string()
    } else {
        joined
    }
}

#[tauri::command]
pub fn system_static() -> Result<SystemInfo, String> {
    let mut system = System::new().map_err(|error| error.to_string())?;
    system.refresh_memory();
    system.refresh_cpu_all();

    let raw_model = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().to_string())
        .unwrap_or_default();

    Ok(SystemInfo {
        cpu_model: normalize_cpu_model(&raw_model),
        memory_total: system.total_memory(),
    })
}

/* ---------------- 系统代理探测 ---------------- */

#[derive(Clone, Default)]
struct ProxyState {
    enabled: bool,
    server: Option<String>,
    pac: bool,
}

const PROBE_TTL: Duration = Duration::from_secs(5);

static PROXY_CACHE: OnceLock<Mutex<Option<(ProxyState, Instant)>>> = OnceLock::new();
static TUN_CACHE: OnceLock<Mutex<Option<(bool, Instant)>>> = OnceLock::new();

/// 系统应用主题（浅色 true / 深色 false），读一次注册表
pub fn apps_use_light() -> bool {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
        REG_VALUE_TYPE,
    };

    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            None,
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return false;
        }

        let name = HSTRING::from("AppsUseLightTheme");
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0u32;
        let probe = RegQueryValueExW(key, &name, None, Some(&mut kind), None, Some(&mut size));
        if probe.is_err() || size < 4 {
            let _ = RegCloseKey(key);
            return false;
        }

        let mut buffer = vec![0u8; size as usize];
        let status = RegQueryValueExW(
            key,
            &name,
            None,
            Some(&mut kind),
            Some(buffer.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);

        if status.is_err() {
            return false;
        }

        u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) == 1
    }
}

/// 读注册表里的一个值。直接走 Win32 注册表 API，不启动子进程（避免阻塞主线程）。
fn query_reg_value(name: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
        REG_DWORD, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
    };

    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings"),
            None,
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return None;
        }

        let value_name = HSTRING::from(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut size = 0u32;

        let probe = RegQueryValueExW(key, &value_name, None, Some(&mut kind), None, Some(&mut size));
        if probe.is_err() || size == 0 {
            let _ = RegCloseKey(key);
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        let status = RegQueryValueExW(
            key,
            &value_name,
            None,
            Some(&mut kind),
            Some(buffer.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);

        if status.is_err() {
            return None;
        }

        match kind {
            REG_SZ | REG_EXPAND_SZ => {
                let units = buffer.chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect::<Vec<u16>>();
                let text = String::from_utf16_lossy(&units);
                let text = text.trim_end_matches('\0').trim().to_string();
                if text.is_empty() { None } else { Some(text) }
            }
            REG_DWORD => {
                if buffer.len() >= 4 {
                    Some(u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]).to_string())
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// 把 `http=127.0.0.1:7890;https=127.0.0.1:7891` 归一成单个端点
fn normalize_proxy(raw: &str) -> String {
    if !raw.contains('=') {
        return raw.to_string();
    }

    let mut https = None;
    let mut http = None;
    for part in raw.split(';') {
        if let Some(value) = part.strip_prefix("https=") {
            https = Some(value.to_string());
        } else if let Some(value) = part.strip_prefix("http=") {
            http = Some(value.to_string());
        }
    }

    https.or(http).unwrap_or_else(|| raw.to_string())
}

fn read_proxy_state() -> ProxyState {
    /* ProxyEnable 关掉后 ProxyServer 往往还留着，必须同时看开关 */
    let enabled = query_reg_value("ProxyEnable")
        .map(|value| value.trim() != "0")
        .unwrap_or(false);

    let server = if enabled {
        query_reg_value("ProxyServer").map(|raw| normalize_proxy(&raw))
    } else {
        None
    };

    ProxyState {
        enabled,
        server,
        pac: query_reg_value("AutoConfigURL").is_some(),
    }
}

/// 系统代理端点（形如 127.0.0.1:7890）。
/// 仅当系统代理开启且写的是固定端点时返回；PAC 脚本无法直接套用。
pub fn system_proxy_endpoint() -> Option<String> {
    let state = proxy_state_cached();
    if !state.enabled || state.pac {
        return None;
    }
    state.server.filter(|server| !server.trim().is_empty())
}

fn proxy_state_cached() -> ProxyState {
    let cache = PROXY_CACHE.get_or_init(|| Mutex::new(None));

    if let Ok(guard) = cache.lock() {
        if let Some((state, at)) = guard.as_ref() {
            if at.elapsed() < PROBE_TTL {
                return state.clone();
            }
        }
    }

    let state = read_proxy_state();
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((state.clone(), Instant::now()));
    }
    state
}

/// 把宽字符串（UTF-16）转成 Rust 字符串
#[cfg(windows)]
unsafe fn wide_to_string(pointer: windows::core::PWSTR) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let mut length = 0usize;
    while *pointer.0.add(length) != 0 {
        length += 1;
    }
    let slice = std::slice::from_raw_parts(pointer.0, length);
    String::from_utf16_lossy(slice)
}

/// 网卡名 / 驱动描述里出现这些词，基本可以断定是隧道型虚拟网卡
#[cfg(windows)]
fn looks_like_tunnel(text: &str) -> bool {
    const KEYWORDS: [&str; 9] = [
        "wintun",
        "tap-windows",
        "tap-win",
        "tunnel",
        "clash",
        "mihomo",
        "sing-box",
        "wireguard",
        "v2ray",
    ];
    KEYWORDS.iter().any(|keyword| text.contains(keyword))
}

/// 判断系统是否被 TUN 虚拟网卡接管。
///
/// 做法：枚举所有网卡，只要存在一块「已连接、且是隧道型」的虚拟网卡，就认为 TUN 生效。
/// 不依赖具体网段（198.18 / 172.19 之类），换客户端也能识别。
#[cfg(windows)]
fn detect_tun() -> bool {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_PREFIX, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::AF_UNSPEC;

    const ERROR_SUCCESS: u32 = 0;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;
    const IF_TYPE_TUNNEL: u32 = 131;

    unsafe {
        let mut size: u32 = 16 * 1024;
        let mut buffer: Vec<u8> = Vec::new();

        loop {
            buffer.resize(size as usize, 0);
            let result = GetAdaptersAddresses(
                u32::from(AF_UNSPEC.0),
                GAA_FLAG_INCLUDE_PREFIX,
                None,
                Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            );

            if result == ERROR_BUFFER_OVERFLOW {
                continue;
            }
            if result != ERROR_SUCCESS {
                return false;
            }
            break;
        }

        let mut current = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        while !current.is_null() {
            let adapter = &*current;

            if adapter.OperStatus == IfOperStatusUp {
                let description = wide_to_string(adapter.Description).to_ascii_lowercase();
                let friendly_name = wide_to_string(adapter.FriendlyName).to_ascii_lowercase();

                if adapter.IfType == IF_TYPE_TUNNEL
                    || looks_like_tunnel(&description)
                    || looks_like_tunnel(&friendly_name)
                {
                    return true;
                }
            }

            current = adapter.Next;
        }
    }

    false
}

#[cfg(not(windows))]
fn detect_tun() -> bool {
    false
}

fn tun_cached() -> bool {
    let cache = TUN_CACHE.get_or_init(|| Mutex::new(None));

    if let Ok(guard) = cache.lock() {
        if let Some((value, at)) = guard.as_ref() {
            if at.elapsed() < PROBE_TTL {
                return *value;
            }
        }
    }

    let value = detect_tun();
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((value, Instant::now()));
    }
    value
}

/// 下载器自己走的线路：
/// 1. 偏好设为不使用代理 → 直连
/// 2. 偏好设为使用系统代理 → 系统代理开启则走代理端点，否则直连
fn decide_route(proxy_mode: &str) -> (String, String) {
    if proxy_mode == "direct" {
        return ("direct".into(), String::new());
    }

    let proxy = proxy_state_cached();
    if proxy.enabled {
        return ("system-proxy".into(), proxy.server.unwrap_or_default());
    }
    if proxy.pac {
        return ("system-proxy".into(), "PAC".into());
    }

    ("direct".into(), String::new())
}

#[tauri::command]
pub fn system_route(proxy_mode: Option<String>, force: Option<bool>) -> RouteInfo {
    if force.unwrap_or(false) {
        if let Ok(mut guard) = PROXY_CACHE.get_or_init(|| Mutex::new(None)).lock() {
            *guard = None;
        }
        if let Ok(mut guard) = TUN_CACHE.get_or_init(|| Mutex::new(None)).lock() {
            *guard = None;
        }
    }

    let (route_kind, route_detail) = decide_route(proxy_mode.as_deref().unwrap_or("system"));

    RouteInfo {
        route_kind,
        route_detail,
        tun: tun_cached(),
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_cpu_model, normalize_proxy};

    #[test]
    fn intel_models() {
        assert_eq!(
            normalize_cpu_model("Intel(R) Core(TM) Ultra 7 155H"),
            "Intel Core Ultra 7 155H"
        );
        assert_eq!(
            normalize_cpu_model("Intel(R) Core(TM) i7-12700H CPU @ 2.30GHz"),
            "Intel Core i7-12700H"
        );
    }

    #[test]
    fn amd_models() {
        assert_eq!(
            normalize_cpu_model("AMD Ryzen 7 5800X 8-Core Processor"),
            "AMD Ryzen 7 5800X"
        );
        assert_eq!(
            normalize_cpu_model("AMD Ryzen 9 7945HX3D with Radeon Graphics"),
            "AMD Ryzen 9 7945HX3D"
        );
    }

    #[test]
    fn proxy_formats() {
        assert_eq!(normalize_proxy("127.0.0.1:7890"), "127.0.0.1:7890");
        assert_eq!(
            normalize_proxy("http=127.0.0.1:7890;https=127.0.0.1:7891"),
            "127.0.0.1:7891"
        );
    }

    #[test]
    fn tun_probe() {
        /* 仅探测，不做断言：不同机器环境不同，只验证不崩溃 */
        println!("detect_tun() = {}", super::detect_tun());
    }
}
