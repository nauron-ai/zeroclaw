use std::sync::{Mutex, OnceLock};

pub(super) struct EnvGuard {
    key: &'static str,
    original: Option<String>,
}

impl EnvGuard {
    pub(super) fn set(key: &'static str, value: Option<&str>) -> Self {
        let original = std::env::var(key).ok();
        match value {
            Some(next) => std::env::set_var(key, next),
            None => std::env::remove_var(key),
        }
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(original) = self.original.as_deref() {
            std::env::set_var(self.key, original);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

pub(super) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().expect("env lock poisoned")
}
