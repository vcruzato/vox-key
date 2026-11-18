use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Recording,
    Transcribing,
    Error,
}

/// Freedesktop icon name for the current state (paused wins). While recording,
/// `blink_on` swaps the red record dot for the mic icon to produce a blink.
pub fn icon_for(state: State, paused: bool, blink_on: bool) -> &'static str {
    if paused {
        return "media-playback-pause";
    }
    match state {
        State::Idle => "audio-input-microphone",
        State::Recording => {
            if blink_on {
                "audio-input-microphone"
            } else {
                "" // steady frame: the orange square is supplied via icon_pixmap()
            }
        }
        State::Transcribing => "appointment-soon",
        State::Error => "dialog-error",
    }
}

/// Human-readable status text (paused wins). Shows a counter while recording.
pub fn status_for(state: State, paused: bool, elapsed_s: u32, total_s: u32) -> String {
    if paused {
        return "voxkey — pausado".to_string();
    }
    match state {
        State::Idle => "voxkey — pronto".to_string(),
        State::Recording => {
            format!("voxkey — gravando {} / {}", fmt_mmss(elapsed_s), fmt_mmss(total_s))
        }
        State::Transcribing => "voxkey — transcrevendo".to_string(),
        State::Error => "voxkey — erro".to_string(),
    }
}

/// Formats seconds as `M:SS`.
pub fn fmt_mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// RGB of the recording marker — orange, deliberately distinct from the system's
/// red screen-record dot.
const RECORD_RGB: [u8; 3] = [0xFF, 0x7A, 0x00];

/// Builds the "recording" tray icon: a solid orange square (`#FF7A00`) centered
/// on a transparent canvas with a small margin so it doesn't touch the edges.
/// Format is ARGB32 in network byte order — one pixel is `[A, R, G, B]` — which
/// is what ksni sends over D-Bus.
fn record_square(size: i32) -> ksni::Icon {
    let s = size.max(1) as usize;
    let margin = (s / 6).max(1); // ~16% padding per side
    let mut data = vec![0u8; s * s * 4]; // transparent canvas
    for y in margin..s.saturating_sub(margin) {
        for x in margin..s.saturating_sub(margin) {
            let i = (y * s + x) * 4;
            data[i] = 0xFF; // A (opaque)
            data[i + 1] = RECORD_RGB[0]; // R
            data[i + 2] = RECORD_RGB[1]; // G
            data[i + 3] = RECORD_RGB[2]; // B
        }
    }
    ksni::Icon { width: size, height: size, data }
}

/// Base size (px) of the recording pixmap; SNI hosts scale it to the panel.
const RECORD_ICON_SIZE: i32 = 32;

/// Built once — the marker never changes color or size, so there's nothing to
/// recompute per frame; the blink just toggles whether it's reported.
static RECORD_SQUARE: LazyLock<ksni::Icon> = LazyLock::new(|| record_square(RECORD_ICON_SIZE));

pub struct VoxTray {
    pub state: State,
    pub paused: Arc<AtomicBool>,
    pub elapsed_s: u32,
    pub total_s: u32,
    pub blink_on: bool,
}

impl ksni::Tray for VoxTray {
    fn id(&self) -> String {
        "voxkey".into()
    }

    fn icon_name(&self) -> String {
        icon_for(self.state, self.paused.load(Ordering::Relaxed), self.blink_on).into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        // An empty icon name (the recording steady frame) means the host should
        // fall back to our custom pixmap: the orange square. `.clone()` copies the
        // cached Icon via Deref — no rebuild.
        if icon_for(self.state, self.paused.load(Ordering::Relaxed), self.blink_on).is_empty() {
            vec![RECORD_SQUARE.clone()]
        } else {
            vec![]
        }
    }

    fn title(&self) -> String {
        status_for(
            self.state,
            self.paused.load(Ordering::Relaxed),
            self.elapsed_s,
            self.total_s,
        )
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            CheckmarkItem {
                label: "Pausar".into(),
                checked: self.paused.load(Ordering::Relaxed),
                activate: Box::new(|this: &mut Self| {
                    let cur = this.paused.load(Ordering::Relaxed);
                    this.paused.store(!cur, Ordering::Relaxed);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Sair".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_match_state() {
        assert_eq!(icon_for(State::Idle, false, false), "audio-input-microphone");
        assert_eq!(icon_for(State::Recording, false, false), "");
        assert_eq!(icon_for(State::Transcribing, false, false), "appointment-soon");
        assert_eq!(icon_for(State::Error, false, false), "dialog-error");
    }

    #[test]
    fn blink_swaps_record_icon() {
        assert_eq!(icon_for(State::Recording, false, true), "audio-input-microphone");
        assert_eq!(icon_for(State::Recording, false, false), "");
    }

    #[test]
    fn paused_overrides_icon_and_status() {
        assert_eq!(icon_for(State::Recording, true, false), "media-playback-pause");
        assert!(status_for(State::Idle, true, 0, 180).contains("pausado"));
    }

    #[test]
    fn status_shows_counter() {
        assert_eq!(
            status_for(State::Recording, false, 72, 180),
            "voxkey — gravando 1:12 / 3:00"
        );
    }

    #[test]
    fn mmss_formats() {
        assert_eq!(fmt_mmss(0), "0:00");
        assert_eq!(fmt_mmss(9), "0:09");
        assert_eq!(fmt_mmss(72), "1:12");
        assert_eq!(fmt_mmss(180), "3:00");
    }

    #[test]
    fn record_square_is_orange_with_transparent_margin() {
        let icon = record_square(32);
        assert_eq!(icon.width, 32);
        assert_eq!(icon.height, 32);
        assert_eq!(icon.data.len(), 32 * 32 * 4);

        // Center pixel is opaque orange, in [A, R, G, B] order.
        let c = ((16 * 32) + 16) * 4;
        assert_eq!(
            &icon.data[c..c + 4],
            &[0xFF, RECORD_RGB[0], RECORD_RGB[1], RECORD_RGB[2]]
        );

        // Top-left corner sits in the margin: fully transparent.
        assert_eq!(&icon.data[0..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn recording_steady_frame_reports_pixmap() {
        use ksni::Tray;
        use std::sync::{atomic::AtomicBool, Arc};
        let tray = VoxTray {
            state: State::Recording,
            paused: Arc::new(AtomicBool::new(false)),
            elapsed_s: 0,
            total_s: 60,
            blink_on: false,
        };
        // Empty name so the host falls back to our custom pixmap.
        assert_eq!(tray.icon_name(), "");
        assert_eq!(tray.icon_pixmap().len(), 1);
    }

    #[test]
    fn other_frames_report_theme_name_not_pixmap() {
        use ksni::Tray;
        use std::sync::{atomic::AtomicBool, Arc};
        let mut tray = VoxTray {
            state: State::Idle,
            paused: Arc::new(AtomicBool::new(false)),
            elapsed_s: 0,
            total_s: 60,
            blink_on: false,
        };
        assert_eq!(tray.icon_name(), "audio-input-microphone");
        assert!(tray.icon_pixmap().is_empty());

        // Blink frame during recording shows the mic name, not the pixmap.
        tray.state = State::Recording;
        tray.blink_on = true;
        assert_eq!(tray.icon_name(), "audio-input-microphone");
        assert!(tray.icon_pixmap().is_empty());
    }
}
