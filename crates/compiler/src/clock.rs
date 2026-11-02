//! The clock NOW, UTCNOW, TODAY and UTCTODAY read. Tests fix it, so the SQL and the
//! reference interpreter see the same moment.

use std::sync::Mutex;

static FIXED: Mutex<Option<String>> = Mutex::new(None);

/// Fixes the clock at `at` (`YYYY-MM-DD HH:MM:SS`), local time and UTC alike.
pub fn fix(at: &str) {
    *FIXED.lock().unwrap_or_else(|e| e.into_inner()) = Some(at.to_string());
}

/// The fixed moment, if a test fixed one.
pub fn fixed() -> Option<String> {
    FIXED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
