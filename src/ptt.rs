use anyhow::{anyhow, Context, Result};
use evdev::{Device, EventSummary, EventType, KeyCode};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

// Skip synthetic uinput keyboards named "*-virtual-keyboard" (voxkey's own and
// other tools using the same convention). A real PTT press comes from hardware;
// reading a virtual device would let another tool's emitted AltGr spuriously
// trigger us.
const VIRTUAL_KEYBOARD_MARKER: &str = "virtual-keyboard";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttEvent {
    Press,
    Release,
}

/// Maps a friendly key name (from config) to an evdev key code.
pub fn key_from_name(name: &str) -> Option<KeyCode> {
    Some(match name {
        "RightAlt" | "AltGr" => KeyCode::KEY_RIGHTALT,
        "LeftAlt" => KeyCode::KEY_LEFTALT,
        "RightCtrl" => KeyCode::KEY_RIGHTCTRL,
        "LeftCtrl" => KeyCode::KEY_LEFTCTRL,
        "RightShift" => KeyCode::KEY_RIGHTSHIFT,
        "CapsLock" => KeyCode::KEY_CAPSLOCK,
        "Menu" => KeyCode::KEY_COMPOSE,
        "ScrollLock" => KeyCode::KEY_SCROLLLOCK,
        "Pause" => KeyCode::KEY_PAUSE,
        "F12" => KeyCode::KEY_F12,
        _ => return None,
    })
}

/// Spawns a reader thread per physical keyboard; forwards only Press/Release of
/// `ptt` on the returned channel. Autorepeat (value 2) is ignored.
pub fn watch(ptt: KeyCode) -> Result<mpsc::Receiver<PttEvent>> {
    let (tx, rx) = mpsc::channel::<PttEvent>();
    let devices = enumerate_keyboards()?;
    if devices.is_empty() {
        return Err(anyhow!(
            "no keyboard devices found in /dev/input/. Is your user in the 'input' group?"
        ));
    }
    for (path, device) in devices {
        let tx = tx.clone();
        thread::spawn(move || {
            if let Err(e) = read_loop(path, device, ptt, tx) {
                tracing::error!(error = %e, "ptt reader thread crashed");
            }
        });
    }
    Ok(rx)
}

fn read_loop(
    path: PathBuf,
    mut device: Device,
    ptt: KeyCode,
    tx: mpsc::Sender<PttEvent>,
) -> Result<()> {
    tracing::info!(?path, name = device.name().unwrap_or(""), "watching device");
    loop {
        for event in device.fetch_events()? {
            if event.event_type() != EventType::KEY {
                continue;
            }
            let EventSummary::Key(_, key_code, value) = event.destructure() else {
                continue;
            };
            if key_code != ptt {
                continue;
            }
            // value: 1 = press, 0 = release, 2 = autorepeat (ignored)
            let ev = match value {
                1 => PttEvent::Press,
                0 => PttEvent::Release,
                _ => continue,
            };
            if tx.send(ev).is_err() {
                return Ok(());
            }
        }
    }
}

// --- enumeration ---

fn enumerate_keyboards() -> Result<Vec<(PathBuf, Device)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir("/dev/input")
        .context("reading /dev/input — is your user in the 'input' group?")?
    {
        let entry = entry?;
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !name.starts_with("event") {
            continue;
        }
        match Device::open(&path) {
            Ok(dev) => {
                if is_keyboard(&dev) && !is_virtual_keyboard(&dev) {
                    out.push((path, dev));
                }
            }
            Err(e) => tracing::debug!(?path, error = %e, "skipping device"),
        }
    }
    Ok(out)
}

fn is_keyboard(dev: &Device) -> bool {
    dev.supported_keys()
        .is_some_and(|k| k.contains(KeyCode::KEY_A))
}

fn is_virtual_keyboard(dev: &Device) -> bool {
    dev.name().unwrap_or("").contains(VIRTUAL_KEYBOARD_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evdev::KeyCode;

    #[test]
    fn maps_known_keys() {
        assert_eq!(key_from_name("RightAlt"), Some(KeyCode::KEY_RIGHTALT));
        assert_eq!(key_from_name("AltGr"), Some(KeyCode::KEY_RIGHTALT));
        assert_eq!(key_from_name("CapsLock"), Some(KeyCode::KEY_CAPSLOCK));
        assert_eq!(key_from_name("F12"), Some(KeyCode::KEY_F12));
    }

    #[test]
    fn unknown_key_is_none() {
        assert_eq!(key_from_name("Banana"), None);
    }
}
