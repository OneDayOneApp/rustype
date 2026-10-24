use std::time::{Duration, Instant};

use crate::config::Hotkeys;
use crate::hotkey::{InputState, Mode, Shortcut, ShortcutError};

/// Delay within which a second dictation tap means "hands-free".
pub const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(350);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    pub mode: Mode,
    pub hands_free: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Start { mode: Mode, hands_free: bool },
    SwitchMode(Mode),
    Stop,
    Cancel,
    PasteLast,
}

/// Turns raw key-state snapshots into dictation actions.
///
/// The engine is pure: the event tap feeds it snapshots and the platform layer
/// executes the returned actions. This mirrors the observed Typeless behaviour:
/// hold the trigger to talk, double-tap for hands-free, and chord a modifier to
/// switch modes mid-utterance.
pub struct Engine {
    /// Tap-to-toggle instead of push-to-talk.
    toggle: bool,
    dictation: Shortcut,
    /// The dictation trigger doubles as the session's "still talking" key:
    /// releasing it (or its right-hand modifier) ends the utterance.
    base: Shortcut,
    command: Shortcut,
    translation: Shortcut,
    paste_last: Shortcut,
    prev: InputState,
    prev_active: Option<Mode>,
    recording: Option<Recording>,
    /// Set after an out-of-band stop (menu bar / floating pill). Starts are
    /// ignored until every trigger is released, so stopping with the mouse
    /// while the hotkey is still held does not immediately restart.
    latched: bool,
    last_press: Option<Instant>,
}

impl Engine {
    pub fn new(hotkeys: &Hotkeys) -> Result<Self, ShortcutError> {
        let dictation = Shortcut::parse(&hotkeys.dictation)?;
        Ok(Self {
            toggle: hotkeys.mode != "hold",
            base: dictation,
            dictation,
            command: Shortcut::parse(&hotkeys.command)?,
            translation: Shortcut::parse(&hotkeys.translation)?,
            paste_last: Shortcut::parse(&hotkeys.paste_last)?,
            prev: InputState::default(),
            prev_active: None,
            recording: None,
            latched: false,
            last_press: None,
        })
    }

    pub fn recording(&self) -> Option<&Recording> {
        self.recording.as_ref()
    }

    /// Discard the current session (stopped from the UI) and ignore further
    /// presses until the trigger is physically released.
    pub fn abort_recording(&mut self) {
        self.recording = None;
        self.latched = true;
        // A stop from the UI should not turn the next quick press into a
        // hands-free double tap.
        self.last_press = None;
    }

    fn active_mode(&self, state: &InputState) -> Option<Mode> {
        if self.command.matches(state) {
            Some(Mode::Command)
        } else if self.translation.matches(state) {
            Some(Mode::Translation)
        } else if self.dictation.matches(state) {
            Some(Mode::Dictation)
        } else {
            None
        }
    }

    pub fn update(&mut self, state: &InputState, now: Instant) -> Vec<Action> {
        let active = self.active_mode(state);
        let mut actions = Vec::new();

        let escape = state.escape_down() && !self.prev.escape_down();
        if escape && self.recording.is_some() {
            self.recording = None;
            actions.push(Action::Cancel);
        }

        let paste_last = self.paste_last.matches(state) && !self.paste_last.matches(&self.prev);
        if paste_last {
            actions.push(Action::PasteLast);
        }

        match self.recording.clone() {
            Some(mut rec) => {
                let dictation_pressed =
                    active == Some(Mode::Dictation) && self.prev_active != Some(Mode::Dictation);

                if self.toggle {
                    // Tap toggles: only a fresh physical press of the dictation
                    // trigger ends the session. Releasing a chord (which makes
                    // the plain trigger match again) must not stop it.
                    if self.base.just_pressed(state, &self.prev) {
                        self.recording = None;
                        actions.push(Action::Stop);
                    } else if let Some(mode) = active {
                        if mode != Mode::Dictation && mode != rec.mode {
                            rec.mode = mode;
                            self.recording = Some(rec);
                            actions.push(Action::SwitchMode(mode));
                        }
                    }
                } else if rec.hands_free && dictation_pressed {
                    self.recording = None;
                    actions.push(Action::Stop);
                } else {
                    if let Some(mode) = active {
                        // Only ever upgrade to a more specific chord while
                        // talking; releasing the chord must not flip back.
                        if mode != Mode::Dictation && mode != rec.mode {
                            rec.mode = mode;
                            actions.push(Action::SwitchMode(mode));
                        }
                    }
                    // Stop on release of the trigger itself, not on "no
                    // shortcut matches": brushing an unrelated modifier must
                    // not end the dictation.
                    if !rec.hands_free && !self.base.held_loose(state) {
                        self.recording = None;
                        actions.push(Action::Stop);
                    } else {
                        self.recording = Some(rec);
                    }
                }
            }
            None => {
                if self.latched {
                    if active.is_none() {
                        self.latched = false;
                    }
                } else if let Some(mode) = active.filter(|m| self.prev_active != Some(*m)) {
                    let hands_free = if self.toggle {
                        true
                    } else if mode == Mode::Dictation {
                        let double = self
                            .last_press
                            .map(|t| now.saturating_duration_since(t) < DOUBLE_TAP_WINDOW)
                            .unwrap_or(false);
                        self.last_press = Some(now);
                        double
                    } else {
                        false
                    };
                    self.recording = Some(Recording { mode, hands_free });
                    actions.push(Action::Start { mode, hands_free });
                }
            }
        }

        self.prev = state.clone();
        self.prev_active = active;
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn fn_hotkeys() -> Hotkeys {
        Hotkeys {
            mode: "hold".into(),
            dictation: "fn".into(),
            command: "fn+ctrl".into(),
            translation: "fn+alt".into(),
            paste_last: "ctrl+cmd+v".into(),
        }
    }

    fn engine() -> Engine {
        Engine::new(&fn_hotkeys()).unwrap()
    }

    fn fn_down() -> InputState {
        InputState {
            fn_down: true,
            ..Default::default()
        }
    }

    fn idle() -> InputState {
        InputState::default()
    }

    #[test]
    fn hold_and_release_dictates() {
        let mut e = engine();
        let t0 = Instant::now();
        assert_eq!(
            e.update(&fn_down(), t0),
            vec![Action::Start {
                mode: Mode::Dictation,
                hands_free: false
            }]
        );
        assert_eq!(
            e.update(&idle(), t0 + Duration::from_secs(1)),
            vec![Action::Stop]
        );
        assert!(e.recording().is_none());
    }

    #[test]
    fn double_tap_enters_hands_free_and_next_tap_stops() {
        let mut e = engine();
        let t0 = Instant::now();
        e.update(&fn_down(), t0);
        e.update(&idle(), t0 + Duration::from_millis(100));
        let t1 = t0 + Duration::from_millis(250);
        assert_eq!(
            e.update(&fn_down(), t1),
            vec![Action::Start {
                mode: Mode::Dictation,
                hands_free: true
            }]
        );
        // Release keeps recording.
        assert!(e
            .update(&idle(), t1 + Duration::from_millis(200))
            .is_empty());
        assert_eq!(
            e.update(&fn_down(), t1 + Duration::from_secs(2)),
            vec![Action::Stop]
        );
    }

    #[test]
    fn chord_switches_mode_and_keeps_it_after_release() {
        let mut e = engine();
        let t0 = Instant::now();
        assert_eq!(
            e.update(&fn_down(), t0),
            vec![Action::Start {
                mode: Mode::Dictation,
                hands_free: false
            }]
        );

        let mut command = fn_down();
        command.ctrl = true;
        assert_eq!(
            e.update(&command, t0 + Duration::from_millis(80)),
            vec![Action::SwitchMode(Mode::Command)]
        );

        // Releasing ctrl while Fn is still held must not switch back.
        assert!(e
            .update(&fn_down(), t0 + Duration::from_millis(200))
            .is_empty());
        assert_eq!(
            e.update(&idle(), t0 + Duration::from_millis(500)),
            vec![Action::Stop]
        );
    }

    #[test]
    fn escape_cancels() {
        let mut e = engine();
        let t0 = Instant::now();
        e.update(&fn_down(), t0);
        let mut esc = fn_down();
        esc.keys = BTreeSet::from([crate::hotkey::Key::Escape]);
        assert_eq!(
            e.update(&esc, t0 + Duration::from_millis(100)),
            vec![Action::Cancel]
        );
        assert!(e
            .update(&idle(), t0 + Duration::from_millis(200))
            .is_empty());
    }

    #[test]
    fn extra_modifier_does_not_stop_recording() {
        let mut e = engine();
        let t0 = Instant::now();
        e.update(&fn_down(), t0);
        let mut with_cmd = fn_down();
        with_cmd.cmd = true;
        assert!(e
            .update(&with_cmd, t0 + Duration::from_millis(200))
            .is_empty());
        assert_eq!(
            e.update(&idle(), t0 + Duration::from_millis(400)),
            vec![Action::Stop]
        );
    }

    fn right_alt() -> InputState {
        let mut s = InputState {
            alt: true,
            ..Default::default()
        };
        s.keys.insert(crate::hotkey::Key::RightAlt);
        s
    }

    #[test]
    fn right_option_is_an_independent_trigger() {
        let hotkeys = Hotkeys {
            mode: "hold".into(),
            dictation: "right-alt".into(),
            command: "right-alt+ctrl".into(),
            translation: "right-alt+shift".into(),
            paste_last: "ctrl+alt+v".into(),
        };
        let mut e = Engine::new(&hotkeys).unwrap();
        let t0 = Instant::now();
        assert_eq!(
            e.update(&right_alt(), t0),
            vec![Action::Start {
                mode: Mode::Dictation,
                hands_free: false
            }]
        );

        // Shift upgrades to translation, releasing it keeps recording.
        let mut translating = right_alt();
        translating.shift = true;
        assert_eq!(
            e.update(&translating, t0 + Duration::from_millis(100)),
            vec![Action::SwitchMode(Mode::Translation)]
        );
        assert!(e
            .update(&right_alt(), t0 + Duration::from_millis(200))
            .is_empty());

        // Releasing right Option stops even though the generic alt flag is off
        // in the same snapshot.
        assert_eq!(
            e.update(&idle(), t0 + Duration::from_millis(600)),
            vec![Action::Stop]
        );
    }

    #[test]
    fn abort_latches_until_release() {
        let mut e = engine();
        let t0 = Instant::now();
        e.update(&fn_down(), t0);
        e.abort_recording();
        // Still held: must not restart.
        assert!(e
            .update(&fn_down(), t0 + Duration::from_millis(100))
            .is_empty());
        // Released: unlatch.
        assert!(e
            .update(&idle(), t0 + Duration::from_millis(200))
            .is_empty());
        // Fresh press starts again.
        assert_eq!(
            e.update(&fn_down(), t0 + Duration::from_millis(300)),
            vec![Action::Start {
                mode: Mode::Dictation,
                hands_free: false
            }]
        );
    }

    #[test]
    fn paste_last_is_independent_of_recording() {
        let mut e = engine();
        let mut state = InputState {
            ctrl: true,
            cmd: true,
            ..Default::default()
        };
        state.keys.insert(crate::hotkey::Key::Char('v'));
        assert_eq!(e.update(&state, Instant::now()), vec![Action::PasteLast]);
        // Holding it does not repeat.
        assert!(e.update(&state, Instant::now()).is_empty());
    }

    #[test]
    fn toggle_tap_starts_and_second_tap_stops() {
        let hotkeys = Hotkeys {
            mode: "toggle".into(),
            ..fn_hotkeys()
        };
        let mut e = Engine::new(&hotkeys).unwrap();
        let t0 = Instant::now();
        assert_eq!(
            e.update(&fn_down(), t0),
            vec![Action::Start {
                mode: Mode::Dictation,
                hands_free: true
            }]
        );
        // Releasing must not stop in toggle mode.
        assert!(e
            .update(&idle(), t0 + Duration::from_millis(200))
            .is_empty());
        assert_eq!(
            e.update(&fn_down(), t0 + Duration::from_secs(3)),
            vec![Action::Stop]
        );
        assert!(e.update(&idle(), t0 + Duration::from_secs(4)).is_empty());
    }

    #[test]
    fn toggle_allows_mode_chords() {
        let hotkeys = Hotkeys {
            mode: "toggle".into(),
            ..fn_hotkeys()
        };
        let mut e = Engine::new(&hotkeys).unwrap();
        let t0 = Instant::now();
        e.update(&fn_down(), t0);
        let mut command = fn_down();
        command.ctrl = true;
        assert_eq!(
            e.update(&command, t0 + Duration::from_millis(100)),
            vec![Action::SwitchMode(Mode::Command)]
        );
        assert!(e
            .update(&idle(), t0 + Duration::from_millis(200))
            .is_empty());
        assert_eq!(
            e.update(&fn_down(), t0 + Duration::from_secs(2)),
            vec![Action::Stop]
        );
    }

    #[test]
    fn toggle_chord_release_does_not_stop() {
        let hotkeys = Hotkeys {
            mode: "toggle".into(),
            command: "fn+space".into(),
            ..fn_hotkeys()
        };
        let mut e = Engine::new(&hotkeys).unwrap();
        let t0 = Instant::now();
        assert_eq!(
            e.update(&fn_down(), t0),
            vec![Action::Start { mode: Mode::Dictation, hands_free: true }]
        );
        let mut cmd = fn_down();
        cmd.keys.insert(crate::hotkey::Key::Space);
        assert_eq!(
            e.update(&cmd, t0 + Duration::from_millis(100)),
            vec![Action::SwitchMode(Mode::Command)]
        );
        // Releasing space falls back to "fn only" - must keep recording.
        assert!(e.update(&fn_down(), t0 + Duration::from_millis(200)).is_empty());
        assert!(e.update(&idle(), t0 + Duration::from_millis(300)).is_empty());
        assert_eq!(
            e.update(&fn_down(), t0 + Duration::from_secs(3)),
            vec![Action::Stop]
        );
    }
}
