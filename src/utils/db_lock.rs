//! Poison-safe helpers for `std::sync::Mutex` around SQLite connections.
//!
//! Prefer these on request paths so a poisoned mutex returns a controlled error
//! instead of panicking the worker thread.

use crate::error::AppError;
use std::sync::{Mutex, MutexGuard};
use tracing::error;

/// Acquire a database mutex, mapping poison to [`AppError::Internal`].
///
/// Logs the mutex name (not connection contents or secrets).
pub fn lock_db<'a, T>(
    mutex: &'a Mutex<T>,
    name: &'static str,
) -> Result<MutexGuard<'a, T>, AppError> {
    mutex.lock().map_err(|e| {
        error!(mutex = name, error = %e, "database mutex poisoned");
        AppError::Internal(format!("{name} mutex poisoned"))
    })
}

/// Acquire a database mutex, mapping poison to a plain error string.
///
/// Useful for handlers that return `(StatusCode, String)` rather than `AppError`.
pub fn lock_db_str<'a, T>(
    mutex: &'a Mutex<T>,
    name: &'static str,
) -> Result<MutexGuard<'a, T>, String> {
    mutex.lock().map_err(|e| {
        error!(mutex = name, error = %e, "database mutex poisoned");
        format!("{name} mutex poisoned")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn lock_db_succeeds_on_healthy_mutex() {
        let m = Mutex::new(42);
        let g = lock_db(&m, "test").unwrap();
        assert_eq!(*g, 42);
    }

    #[test]
    fn lock_db_maps_poison() {
        let m = Mutex::new(1);
        let _ = std::panic::catch_unwind(|| {
            let _g = m.lock().unwrap();
            panic!("poison");
        });
        let err = lock_db(&m, "poisoned_db").unwrap_err();
        match err {
            AppError::Internal(msg) => assert!(msg.contains("poisoned_db")),
            other => panic!("unexpected {other:?}"),
        }
    }
}
