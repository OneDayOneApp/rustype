use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The three dictation modes exposed by the product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Dictation,
    Command,
    Translation,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Dictation => "dictation",
            Mode::Command => "command",
            Mode::Translation => "translation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    Char(char),
    Space,
    Enter,
    Escape,
    Tab,
    /// Physical right-hand modifier keys. Left and right modifiers share the
    /// same CGEvent flag, so the tap tracks their keycodes separately.
    RightAlt,
    RightCtrl,
    RightShift,
    RightCmd,
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Char(c) => write!(f, "{c}"),
            Key::Space => write!(f, "space"),
            Key::Enter => write!(f, "enter"),
            Key::Escape => write!(f, "esc"),
            Key::Tab => write!(f, "tab"),
            Key::RightAlt => write!(f, "right-alt"),
            Key::RightCtrl => write!(f, "right-ctrl"),
            Key::RightShift => write!(f, "right-shift"),
            Key::RightCmd => write!(f, "right-cmd"),
        }
    }
}

impl Key {
    pub fn parse(token: &str) -> Option<Self> {
        let t = token.trim().to_ascii_lowercase();
        match t.as_str() {
            "space" | "spacebar" => return Some(Key::Space),
            "enter" | "return" => return Some(Key::Enter),
            "esc" | "escape" => return Some(Key::Escape),
            "tab" => return Some(Key::Tab),
            "right-alt" | "right-option" | "right-opt" | "ralt" => return Some(Key::RightAlt),
            "right-ctrl" | "right-control" | "rctrl" => return Some(Key::RightCtrl),
            "right-shift" | "rshift" => return Some(Key::RightShift),
            "right-cmd" | "right-command" | "rcmd" => return Some(Key::RightCmd),
            _ => {}
        }
        let mut chars = t.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_alphanumeric() => Some(Key::Char(c)),
            _ => None,
        }
    }
}

/// A key combination such as `fn`, `fn+ctrl` or `ctrl+cmd+v`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Shortcut {
    pub fn_key: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    /// Right-hand modifier that must be physically held (left/right share a
    /// flag, so this is tracked by keycode).
    pub right_mod: Option<Key>,
    pub key: Option<Key>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutError(pub String);

impl fmt::Display for ShortcutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ShortcutError {}

impl FromStr for Shortcut {
    type Err = ShortcutError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.trim().is_empty() {
            return Err(ShortcutError("shortcut is empty".into()));
        }
        let mut sc = Shortcut::default();
        for raw in s.split('+') {
            let token = raw.trim().to_ascii_lowercase();
            match token.as_str() {
                "" => {
                    return Err(ShortcutError(format!("invalid shortcut {s:?}")));
                }
                "fn" | "function" => sc.fn_key = true,
                "ctrl" | "control" => sc.ctrl = true,
                "alt" | "option" | "opt" => sc.alt = true,
                "shift" => sc.shift = true,
                "cmd" | "command" | "super" | "meta" => sc.cmd = true,
                other => {
                    let key = Key::parse(other)
                        .ok_or_else(|| ShortcutError(format!("unknown key {other:?}")))?;
                    // A right-hand modifier also sets the generic flag so the
                    // exact-match rules stay intuitive.
                    match key {
                        Key::RightAlt => {
                            sc.alt = true;
                            if sc.right_mod.replace(Key::RightAlt).is_some() {
                                return Err(ShortcutError(format!(
                                    "shortcut {s:?} has more than one right-hand modifier"
                                )));
                            }
                        }
                        Key::RightCtrl => {
                            sc.ctrl = true;
                            if sc.right_mod.replace(Key::RightCtrl).is_some() {
                                return Err(ShortcutError(format!(
                                    "shortcut {s:?} has more than one right-hand modifier"
                                )));
                            }
                        }
                        Key::RightShift => {
                            sc.shift = true;
                            if sc.right_mod.replace(Key::RightShift).is_some() {
                                return Err(ShortcutError(format!(
                                    "shortcut {s:?} has more than one right-hand modifier"
                                )));
                            }
                        }
                        Key::RightCmd => {
                            sc.cmd = true;
                            if sc.right_mod.replace(Key::RightCmd).is_some() {
                                return Err(ShortcutError(format!(
                                    "shortcut {s:?} has more than one right-hand modifier"
                                )));
                            }
                        }
                        _ => {
                            if sc.key.is_some() {
                                return Err(ShortcutError(format!(
                                    "shortcut {s:?} has more than one non-modifier key"
                                )));
                            }
                            sc.key = Some(key);
                        }
                    }
                }
            }
        }
        if !sc.fn_key && !sc.ctrl && !sc.alt && !sc.shift && !sc.cmd && sc.key.is_none() {
            return Err(ShortcutError(format!("invalid shortcut {s:?}")));
        }
        Ok(sc)
    }
}

impl Shortcut {
    pub fn parse(s: &str) -> Result<Self, ShortcutError> {
        s.parse()
    }

    /// Modifier flags only, ignoring the non-modifier key.
    pub fn modifiers_match(&self, state: &InputState) -> bool {
        self.fn_key == state.fn_down
            && self.ctrl == state.ctrl
            && self.alt == state.alt
            && self.shift == state.shift
            && self.cmd == state.cmd
    }

    /// True when the trigger currently has all of its own keys held. Unlike
    /// [`Shortcut::matches`] this tolerates extra modifiers, which keeps a
    /// push-to-talk session alive if the user accidentally brushes another
    /// modifier.
    pub fn held_loose(&self, state: &InputState) -> bool {
        (!self.fn_key || state.fn_down)
            && (!self.ctrl || state.ctrl)
            && (!self.alt || state.alt)
            && (!self.shift || state.shift)
            && (!self.cmd || state.cmd)
            && self.right_mod.map_or(true, |k| state.keys.contains(&k))
            && match self.key {
                Some(k) => state.keys.contains(&k),
                None => true,
            }
    }

    /// True on the snapshot where the trigger was physically pressed. Extra
    /// keys (like the space in a chord) do not count as a new press, which is
    /// what toggle mode needs to avoid stopping when a chord is released.
    pub fn just_pressed(&self, state: &InputState, prev: &InputState) -> bool {
        if !self.held_loose(state) {
            return false;
        }
        let right_rise = self
            .right_mod
            .map_or(false, |k| state.keys.contains(&k) && !prev.keys.contains(&k));
        let key_rise = self
            .key
            .map_or(false, |k| state.keys.contains(&k) && !prev.keys.contains(&k));
        let modifier_rise = (self.fn_key && state.fn_down && !prev.fn_down)
            || (self.ctrl && state.ctrl && !prev.ctrl)
            || (self.alt && state.alt && !prev.alt)
            || (self.shift && state.shift && !prev.shift)
            || (self.cmd && state.cmd && !prev.cmd);
        right_rise || key_rise || modifier_rise
    }

    /// True when every modifier and the optional key match exactly.
    pub fn matches(&self, state: &InputState) -> bool {
        self.fn_key == state.fn_down
            && self.ctrl == state.ctrl
            && self.alt == state.alt
            && self.shift == state.shift
            && self.cmd == state.cmd
            && self.right_mod.map_or(true, |k| state.keys.contains(&k))
            && match self.key {
                Some(k) => state.keys.contains(&k),
                None => true,
            }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputState {
    pub fn_down: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    pub keys: BTreeSet<Key>,
}

impl InputState {
    pub fn escape_down(&self) -> bool {
        self.keys.contains(&Key::Escape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_and_keys() {
        let sc: Shortcut = "Fn+Space".parse().unwrap();
        assert!(sc.fn_key && sc.key == Some(Key::Space));

        let sc: Shortcut = "right-alt+space".parse().unwrap();
        assert!(sc.alt && sc.right_mod == Some(Key::RightAlt) && sc.key == Some(Key::Space));

        let sc: Shortcut = "ctrl+cmd+v".parse().unwrap();
        assert!(sc.ctrl && sc.cmd && sc.key == Some(Key::Char('v')));

        assert!("fn+".parse::<Shortcut>().is_err());
        assert!("hyper".parse::<Shortcut>().is_err());
        assert!("a+b".parse::<Shortcut>().is_err());
    }

    #[test]
    fn exact_matching() {
        let mut state = InputState::default();
        state.fn_down = true;
        assert!("fn".parse::<Shortcut>().unwrap().matches(&state));
        assert!(!"fn+ctrl".parse::<Shortcut>().unwrap().matches(&state));

        state.ctrl = true;
        assert!("fn+ctrl".parse::<Shortcut>().unwrap().matches(&state));
        assert!(!"fn".parse::<Shortcut>().unwrap().matches(&state));

        // Shortcuts without a non-modifier key ignore other keys held at the
        // same time; precedence (command > translation > dictation) decides
        // which chord wins.
        state.keys.insert(Key::Char('v'));
        assert!("fn+ctrl+v".parse::<Shortcut>().unwrap().matches(&state));
        assert!("fn+ctrl".parse::<Shortcut>().unwrap().matches(&state));
    }

    #[test]
    fn right_modifier_is_distinct_from_generic_flag() {
        let mut state = InputState::default();
        state.alt = true;
        state.keys.insert(Key::RightAlt);
        let sc: Shortcut = "right-alt".parse().unwrap();
        assert!(sc.matches(&state));
        assert_eq!(sc.alt, true);

        // Left Option alone sets the flag but not the key.
        state.keys.clear();
        assert!(!sc.matches(&state));
        assert!(!sc.held_loose(&state));
    }

    #[test]
    fn held_loose_tolerates_extra_modifiers() {
        let mut state = InputState {
            fn_down: true,
            ..Default::default()
        };
        state.cmd = true; // unconfigured extra modifier
        assert!("fn".parse::<Shortcut>().unwrap().held_loose(&state));
        assert!(!"fn".parse::<Shortcut>().unwrap().matches(&state));
    }
}
