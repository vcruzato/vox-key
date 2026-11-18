//! Pure state machine for latch-style push-to-talk: hold to start, tap to stop.
//! Time lives in the caller; this reacts only to semantic events.

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Idle,
    Arming,
    Recording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Press,
    Release,
    HoldElapsed,
    LimitReached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Start,
    Stop,
}

#[derive(Default)]
pub struct Latch {
    mode: Mode,
}

impl Latch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn on(&mut self, event: Event) -> Action {
        match (self.mode, event) {
            (Mode::Idle, Event::Press) => {
                self.mode = Mode::Arming;
                Action::Nothing
            }
            (Mode::Arming, Event::Release) => {
                self.mode = Mode::Idle;
                Action::Nothing
            }
            (Mode::Arming, Event::HoldElapsed) => {
                self.mode = Mode::Recording;
                Action::Start
            }
            (Mode::Recording, Event::Press) | (Mode::Recording, Event::LimitReached) => {
                self.mode = Mode::Idle;
                Action::Stop
            }
            // Residual release after the start-hold, and any event that does not
            // apply to the current state: keep state, do nothing.
            _ => Action::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_starts_recording() {
        let mut l = Latch::new();
        assert_eq!(l.on(Event::Press), Action::Nothing);
        assert_eq!(l.mode(), Mode::Arming);
        assert_eq!(l.on(Event::HoldElapsed), Action::Start);
        assert_eq!(l.mode(), Mode::Recording);
    }

    #[test]
    fn early_release_cancels() {
        let mut l = Latch::new();
        l.on(Event::Press);
        assert_eq!(l.on(Event::Release), Action::Nothing);
        assert_eq!(l.mode(), Mode::Idle);
    }

    #[test]
    fn tap_stops_recording() {
        let mut l = Latch::new();
        l.on(Event::Press);
        l.on(Event::HoldElapsed);
        assert_eq!(l.on(Event::Press), Action::Stop);
        assert_eq!(l.mode(), Mode::Idle);
    }

    #[test]
    fn residual_release_ignored_while_recording() {
        let mut l = Latch::new();
        l.on(Event::Press);
        l.on(Event::HoldElapsed);
        assert_eq!(l.on(Event::Release), Action::Nothing);
        assert_eq!(l.mode(), Mode::Recording);
    }

    #[test]
    fn limit_stops_recording() {
        let mut l = Latch::new();
        l.on(Event::Press);
        l.on(Event::HoldElapsed);
        assert_eq!(l.on(Event::LimitReached), Action::Stop);
        assert_eq!(l.mode(), Mode::Idle);
    }

    #[test]
    fn idle_release_is_noop() {
        let mut l = Latch::new();
        assert_eq!(l.on(Event::Release), Action::Nothing);
        assert_eq!(l.mode(), Mode::Idle);
    }

    #[test]
    fn extra_press_while_arming_is_noop() {
        let mut l = Latch::new();
        l.on(Event::Press);
        assert_eq!(l.on(Event::Press), Action::Nothing);
        assert_eq!(l.mode(), Mode::Arming);
    }
}
