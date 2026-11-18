//! Best-effort desktop notifications via `notify-send` (libnotify). A missing
//! or failing notifier must never interrupt dictation, so failures only log.

use std::process::Command;

/// Shows a desktop notification. Logs a warning on failure; never errors.
pub fn notify(summary: &str, body: &str) {
    match Command::new("notify-send")
        .arg("--app-name=voxkey")
        .arg(summary)
        .arg(body)
        .status()
    {
        Ok(s) if s.success() => {}
        Ok(s) => tracing::warn!(code = ?s.code(), "notify-send exited non-zero"),
        Err(e) => tracing::warn!(error = %e, "failed to run notify-send"),
    }
}
