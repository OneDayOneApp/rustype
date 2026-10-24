//! Core, platform-independent logic for Rustype.
//!
//! The crate is deliberately free of macOS APIs so it can be unit tested on any
//! host. Device access, text insertion and the event tap live in the `rustype`
//! binary.

pub mod asr;
pub mod audio;
pub mod config;
pub mod error;
pub mod guard;
pub mod history;
pub mod hotkey;
pub mod http;
pub mod llm;
pub mod prompt;
pub mod state;

pub use config::Config;
pub use hotkey::Mode;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// Serializes tests that mutate process environment variables.
    pub fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
