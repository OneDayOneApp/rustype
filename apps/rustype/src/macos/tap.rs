//! Global keyboard observation built on a CGEventTap.
//!
//! A raw tap is used instead of `global-hotkey` because the product trigger is
//! the Fn/Globe key, which Carbon hotkeys cannot register. The tap also lets us
//! swallow character keys that are part of a configured chord (for example
//! `fn+space`) so nothing leaks into the focused application.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;

use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPort;
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use crossbeam_channel::{Receiver, Sender};
use rustype_core::hotkey::{InputState, Key, Shortcut};

type CGEventRef = *mut c_void;
type CFMachPortRef = *mut c_void;

const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
const K_CG_EVENT_KEY_DOWN: u32 = 10;
const K_CG_EVENT_KEY_UP: u32 = 11;
const K_CG_EVENT_FLAGS_CHANGED: u32 = 12;
pub const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
pub const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

const MASK_SECONDARY_FN: u64 = 0x0080_0000;
const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_ALTERNATE: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;

const TAP_LOCATION_HID: u32 = 0;
const TAP_PLACE_HEAD: u32 = 0;
const TAP_OPTION_DEFAULT: u32 = 0;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: extern "C" fn(*const c_void, u32, CGEventRef, *const c_void) -> CGEventRef,
        user_info: *const c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventGetFlags(event: CGEventRef) -> u64;
}

struct Shared {
    tx: Sender<InputState>,
    shortcuts: Vec<Shortcut>,
    state: InputState,
}

static SHARED: OnceLock<Mutex<Shared>> = OnceLock::new();
static TAP_REF: AtomicUsize = AtomicUsize::new(0);

fn keycode_to_key(code: u16) -> Option<Key> {
    Some(match code {
        49 => Key::Space,
        36 => Key::Enter,
        48 => Key::Tab,
        53 => Key::Escape,
        61 => Key::RightAlt,
        62 => Key::RightCtrl,
        60 => Key::RightShift,
        54 => Key::RightCmd,
        0 => Key::Char('a'),
        1 => Key::Char('s'),
        2 => Key::Char('d'),
        3 => Key::Char('f'),
        4 => Key::Char('h'),
        5 => Key::Char('g'),
        6 => Key::Char('z'),
        7 => Key::Char('x'),
        8 => Key::Char('c'),
        9 => Key::Char('v'),
        11 => Key::Char('b'),
        12 => Key::Char('q'),
        13 => Key::Char('w'),
        14 => Key::Char('e'),
        15 => Key::Char('r'),
        16 => Key::Char('y'),
        17 => Key::Char('t'),
        18 => Key::Char('1'),
        19 => Key::Char('2'),
        20 => Key::Char('3'),
        21 => Key::Char('4'),
        22 => Key::Char('6'),
        23 => Key::Char('5'),
        24 => Key::Char('='),
        25 => Key::Char('9'),
        26 => Key::Char('7'),
        27 => Key::Char('-'),
        28 => Key::Char('8'),
        29 => Key::Char('0'),
        30 => Key::Char(']'),
        31 => Key::Char('o'),
        32 => Key::Char('u'),
        33 => Key::Char('['),
        34 => Key::Char('i'),
        35 => Key::Char('p'),
        37 => Key::Char('l'),
        38 => Key::Char('j'),
        39 => Key::Char('\''),
        40 => Key::Char('k'),
        41 => Key::Char(';'),
        42 => Key::Char('\\'),
        43 => Key::Char(','),
        44 => Key::Char('/'),
        45 => Key::Char('n'),
        46 => Key::Char('m'),
        47 => Key::Char('.'),
        _ => return None,
    })
}

extern "C" fn callback(
    _proxy: *const c_void,
    etype: u32,
    event: CGEventRef,
    _user: *const c_void,
) -> CGEventRef {
    // Re-enable a tap the system disabled (timeout or user input).
    if etype == TAP_DISABLED_BY_TIMEOUT || etype == TAP_DISABLED_BY_USER_INPUT {
        let tap = TAP_REF.load(Ordering::Relaxed) as CFMachPortRef;
        if !tap.is_null() {
            unsafe { CGEventTapEnable(tap, true) };
        }
        return std::ptr::null_mut();
    }
    if event.is_null() {
        return std::ptr::null_mut();
    }
    let Some(shared) = SHARED.get() else {
        return event;
    };
    let Ok(mut shared) = shared.lock() else {
        return event;
    };

    let flags = unsafe { CGEventGetFlags(event) };
    shared.state.fn_down = flags & MASK_SECONDARY_FN != 0;
    shared.state.shift = flags & FLAG_SHIFT != 0;
    shared.state.ctrl = flags & FLAG_CONTROL != 0;
    shared.state.alt = flags & FLAG_ALTERNATE != 0;
    shared.state.cmd = flags & FLAG_COMMAND != 0;

    let mut swallowed = false;
    let code = unsafe { CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) } as u16;

    if etype == K_CG_EVENT_KEY_DOWN || etype == K_CG_EVENT_KEY_UP {
        if let Some(key) = keycode_to_key(code) {
            if etype == K_CG_EVENT_KEY_DOWN {
                shared.state.keys.insert(key);
            } else {
                shared.state.keys.remove(&key);
            }
            // Swallow character keys that belong to a configured chord (for
            // example the space in `fn+space`) so they do not reach the app.
            swallowed = shared
                .shortcuts
                .iter()
                .any(|sc| sc.key == Some(key) && sc.modifiers_match(&shared.state));
        }
    } else if etype == K_CG_EVENT_FLAGS_CHANGED {
        // Left and right modifiers share a flag, so the right-hand variants
        // are tracked by the keycode carried on the flagsChanged event.
        let right = match code {
            61 => Some((Key::RightAlt, FLAG_ALTERNATE)),
            62 => Some((Key::RightCtrl, FLAG_CONTROL)),
            60 => Some((Key::RightShift, FLAG_SHIFT)),
            54 => Some((Key::RightCmd, FLAG_COMMAND)),
            _ => None,
        };
        if let Some((key, mask)) = right {
            if flags & mask != 0 {
                shared.state.keys.insert(key);
            } else {
                shared.state.keys.remove(&key);
            }
        }
    } else {
        return event;
    }

    let snapshot = shared.state.clone();
    let _ = shared.tx.send(snapshot);
    drop(shared);

    if swallowed {
        std::ptr::null_mut()
    } else {
        event
    }
}

/// Install the tap on a dedicated thread and return the stream of key-state
/// snapshots.
///
/// The thread retries until Accessibility permission is granted, so the tray
/// can come up immediately and hotkeys start working without a restart.
pub fn spawn(shortcuts: Vec<Shortcut>) -> anyhow::Result<Receiver<InputState>> {
    let (tx, rx) = crossbeam_channel::unbounded();

    SHARED
        .set(Mutex::new(Shared {
            tx,
            shortcuts,
            state: InputState::default(),
        }))
        .map_err(|_| anyhow::anyhow!("event tap already installed"))?;

    thread::Builder::new()
        .name("rustype-eventtap".into())
        .spawn(move || loop {
            let mask = (1u64 << K_CG_EVENT_KEY_DOWN)
                | (1u64 << K_CG_EVENT_KEY_UP)
                | (1u64 << K_CG_EVENT_FLAGS_CHANGED);
            let tap_ref = unsafe {
                CGEventTapCreate(
                    TAP_LOCATION_HID,
                    TAP_PLACE_HEAD,
                    TAP_OPTION_DEFAULT,
                    mask,
                    callback,
                    std::ptr::null(),
                )
            };
            if tap_ref.is_null() {
                let trusted = crate::macos::permissions::accessibility_trusted();
                log_line!(
                    "[rustype] event tap unavailable (AX trusted: {trusted}); retrying in 2s"
                );
                thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }
            TAP_REF.store(tap_ref as usize, Ordering::Relaxed);
            let port = unsafe { CFMachPort::wrap_under_create_rule(tap_ref as _) };
            let source = match port.create_runloop_source(0) {
                Ok(s) => s,
                Err(_) => {
                    log_line!("[rustype] could not create run loop source; retrying");
                    thread::sleep(std::time::Duration::from_secs(2));
                    continue;
                }
            };
            let run_loop = CFRunLoop::get_current();
            unsafe {
                run_loop.add_source(&source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap_ref, true);
            }
            log_line!("[rustype] hotkeys active");
            CFRunLoop::run_current();
            return;
        })?;

    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_keys() {
        assert_eq!(keycode_to_key(49), Some(Key::Space));
        assert_eq!(keycode_to_key(9), Some(Key::Char('v')));
        assert_eq!(keycode_to_key(53), Some(Key::Escape));
        assert_eq!(keycode_to_key(999), None);
    }
}
