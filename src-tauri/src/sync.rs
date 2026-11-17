use std::sync::{Mutex, MutexGuard};

/// 拿锁失败（线程崩溃导致锁中毒）时恢复内部数据继续用，而不是让整个程序崩溃
pub trait LockExt<T> {
    fn lock_ok(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lock_ok(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}
