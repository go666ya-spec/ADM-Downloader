//! 进程调度相关：避免托盘后台程序被 Windows 的「效率模式」降频。
//!
//! Windows 11 会对后台/托盘程序应用 EcoQoS（处理器节能节流），
//! 表现为整个进程（窗口、托盘、菜单）响应变慢。这里显式退出节流并提高优先级。

#[cfg(windows)]
pub fn escape_power_throttling() {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, ProcessPowerThrottling, SetPriorityClass, SetProcessInformation,
        ABOVE_NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_STATE,
    };

    unsafe {
        let process = GetCurrentProcess();

        /* 优先级略高于普通，保证托盘菜单、窗口消息及时处理 */
        let _ = SetPriorityClass(process, ABOVE_NORMAL_PRIORITY_CLASS);

        /* 关闭执行速度节流：StateMask = 0 表示不施加 EcoQoS */
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: 1,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: 0,
        };

        let _ = SetProcessInformation(
            process,
            ProcessPowerThrottling,
            &state as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

/// 有任务下载时阻止系统睡眠；false 时恢复系统默认策略。
/// 注意：这个 API 是按线程生效的，必须始终在主线程调用。
#[cfg(windows)]
pub fn set_keep_awake(keep: bool) {
    use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};

    unsafe {
        let flags = if keep {
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        let _ = SetThreadExecutionState(flags);
    }
}

#[cfg(not(windows))]
pub fn set_keep_awake(_keep: bool) {}

#[cfg(not(windows))]
pub fn escape_power_throttling() {}
