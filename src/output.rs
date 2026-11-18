use anyhow::{Context, Result};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode};
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const UINPUT_NAME: &str = "voxkey-virtual-keyboard";
const KEY_DELAY: Duration = Duration::from_micros(500);
/// Time to let the focused app read the pasted selection before we restore the
/// previous clipboard. Heuristic — generous for a local paste.
const PASTE_SETTLE: Duration = Duration::from_millis(120);

pub struct Output {
    device: VirtualDevice,
}

impl Output {
    pub fn new() -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(KeyCode::KEY_LEFTSHIFT);
        keys.insert(KeyCode::KEY_INSERT);
        let device = VirtualDevice::builder()
            .context("creating uinput builder — is /dev/uinput accessible? Check udev rule and 'input' group")?
            .name(UINPUT_NAME)
            .with_keys(&keys)?
            .build()
            .context("building uinput virtual device")?;
        Ok(Self { device })
    }

    /// Sets clipboard + primary to `text`, pastes via Shift+Insert, then restores
    /// the previous selections. Both selections are set so one Shift+Insert pastes
    /// our text whether the target reads clipboard (editors) or primary (terminals).
    /// Empty text is a no-op (never emit a stray Shift+Insert).
    pub fn paste_text(&mut self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let saved_clipboard = read_selection(false);
        let saved_primary = read_selection(true);

        set_selection(false, text)?;
        set_selection(true, text)?;

        self.tap_shift_insert()?;
        thread::sleep(PASTE_SETTLE);

        restore_selection(false, saved_clipboard);
        restore_selection(true, saved_primary);
        Ok(())
    }

    fn tap_shift_insert(&mut self) -> Result<()> {
        let press = 1;
        let release = 0;
        let events = [
            InputEvent::new(EventType::KEY.0, KeyCode::KEY_LEFTSHIFT.code(), press),
            InputEvent::new(EventType::KEY.0, KeyCode::KEY_INSERT.code(), press),
            InputEvent::new(EventType::KEY.0, KeyCode::KEY_INSERT.code(), release),
            InputEvent::new(EventType::KEY.0, KeyCode::KEY_LEFTSHIFT.code(), release),
        ];
        self.device.emit(&events).context("emit uinput events")?;
        thread::sleep(KEY_DELAY);
        Ok(())
    }
}

/// Reads the clipboard (or primary) selection as text. Returns `None` when empty
/// or non-text (wl-paste exits non-zero) — nothing to restore in that case.
fn read_selection(primary: bool) -> Option<String> {
    let mut cmd = Command::new("wl-paste");
    cmd.arg("--no-newline");
    if primary {
        cmd.arg("--primary");
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Sets the clipboard (or primary) selection to `text` via wl-copy.
fn set_selection(primary: bool, text: &str) -> Result<()> {
    let mut cmd = Command::new("wl-copy");
    if primary {
        cmd.arg("--primary");
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .spawn()
        .context("spawning wl-copy — is wl-clipboard installed?")?;
    child
        .stdin
        .take()
        .expect("piped stdin is present")
        .write_all(text.as_bytes())
        .context("writing text to wl-copy")?;
    child.wait().context("waiting for wl-copy")?;
    Ok(())
}

/// Restores a selection to its saved value, or clears it if there was nothing.
fn restore_selection(primary: bool, saved: Option<String>) {
    match saved {
        Some(text) => {
            if let Err(e) = set_selection(primary, &text) {
                tracing::warn!(error = %e, "failed to restore clipboard selection after paste");
            }
        }
        None => {
            let mut cmd = Command::new("wl-copy");
            cmd.arg("--clear");
            if primary {
                cmd.arg("--primary");
            }
            if let Err(e) = cmd.status() {
                tracing::warn!(error = %e, "failed to clear clipboard selection after paste");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Needs a running Wayland clipboard (wl-clipboard). Run with:
    //   cargo test --lib output -- --ignored
    #[test]
    #[ignore]
    fn clipboard_roundtrip() {
        set_selection(false, "voxkey-test-çãõ").unwrap();
        assert_eq!(read_selection(false).as_deref(), Some("voxkey-test-çãõ"));
    }
}
