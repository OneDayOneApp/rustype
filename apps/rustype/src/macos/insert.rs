//! Text insertion: clipboard + synthesized Cmd+V (the method Typeless's
//! `libInputHelper` implements), with a direct unicode typing fallback.

use std::time::Duration;

use anyhow::{bail, Context as _, Result};
use arboard::Clipboard;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

use crate::macos::permissions;

const V_KEYCODE: u16 = 9;

pub fn insert_text(
    text: &str,
    method: &str,
    restore_clipboard: bool,
    restore_delay_ms: u64,
) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    if !permissions::accessibility_trusted() {
        bail!(
            "Accessibility permission is required to insert text. \
             Grant it in System Settings → Privacy & Security → Accessibility."
        );
    }
    match method {
        "type" => type_text(text),
        _ => paste_text(text, restore_clipboard, restore_delay_ms),
    }
}

fn source() -> Result<CGEventSource> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("could not create CGEventSource"))
}

fn press_command_v() -> Result<()> {
    let src = source()?;
    let down = CGEvent::new_keyboard_event(src.clone(), V_KEYCODE, true)
        .map_err(|_| anyhow::anyhow!("could not create key event"))?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    down.post(CGEventTapLocation::HID);
    std::thread::sleep(Duration::from_millis(20));
    let up = CGEvent::new_keyboard_event(src, V_KEYCODE, false)
        .map_err(|_| anyhow::anyhow!("could not create key event"))?;
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    up.post(CGEventTapLocation::HID);
    Ok(())
}

fn paste_text(text: &str, restore: bool, delay_ms: u64) -> Result<()> {
    let mut clipboard = Clipboard::new().context("could not open the clipboard")?;
    let previous = clipboard.get_text().ok();
    clipboard
        .set_text(text)
        .context("could not write the clipboard")?;
    // Give the pasteboard server a moment to publish the new contents.
    std::thread::sleep(Duration::from_millis(30));
    press_command_v()?;
    if restore {
        if let Some(prev) = previous {
            std::thread::sleep(Duration::from_millis(delay_ms));
            let mut clipboard = Clipboard::new().context("could not reopen the clipboard")?;
            let _ = clipboard.set_text(prev);
        }
    }
    Ok(())
}

fn type_text(text: &str) -> Result<()> {
    let src = source()?;
    let utf16: Vec<u16> = text.encode_utf16().collect();
    for chunk in utf16.chunks(16) {
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(src.clone(), 0, down)
                .map_err(|_| anyhow::anyhow!("could not create key event"))?;
            event.set_string_from_utf16_unchecked(chunk);
            event.post(CGEventTapLocation::HID);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

/// Read the current selection by copying it, then restore the pasteboard.
#[allow(dead_code)]
pub fn copy_selection() -> Option<String> {
    let mut clipboard = Clipboard::new().ok()?;
    let previous = clipboard.get_text().ok();
    let src = source().ok()?;
    let c_keycode = 8;
    let down = CGEvent::new_keyboard_event(src.clone(), c_keycode, true).ok()?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    down.post(CGEventTapLocation::HID);
    let up = CGEvent::new_keyboard_event(src, c_keycode, false).ok()?;
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    up.post(CGEventTapLocation::HID);
    std::thread::sleep(Duration::from_millis(120));
    let selected = Clipboard::new().ok()?.get_text().ok();
    if let Some(prev) = previous {
        if let Ok(mut c) = Clipboard::new() {
            let _ = c.set_text(prev);
        }
    }
    selected
}
