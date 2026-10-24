//! Best-effort frontmost-application context for command/dictation prompts.
//!
//! This mirrors the context Typeless attaches to `/ai/voice_flow`: the focused
//! application, its window title and the current selection. Every call is
//! optional; when Accessibility access is missing the caller simply gets empty
//! fields.

use std::ffi::c_void;

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use objc2_app_kit::NSWorkspace;
use rustype_core::prompt::Context;

type AXUIElementRef = *const c_void;
type CFTypeRef = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFRelease(cf: CFTypeRef);
}

fn copy_attr(element: AXUIElementRef, name: &str) -> Option<CFTypeRef> {
    if element.is_null() {
        return None;
    }
    let attr = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    let err =
        unsafe { AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut value) };
    if err == 0 && !value.is_null() {
        Some(value)
    } else {
        None
    }
}

fn cf_string(value: CFTypeRef) -> Option<String> {
    unsafe {
        if CFGetTypeID(value) == CFStringGetTypeID() {
            let s = CFString::wrap_under_get_rule(value as CFStringRef);
            Some(s.to_string())
        } else {
            None
        }
    }
}

/// Read a string attribute; the returned CF value is released.
fn string_attr(element: AXUIElementRef, name: &str) -> Option<String> {
    let value = copy_attr(element, name)?;
    let out = cf_string(value);
    unsafe { CFRelease(value) };
    out
}

fn frontmost_app() -> (String, String, i32) {
    let workspace = NSWorkspace::sharedWorkspace();
    let Some(app) = workspace.frontmostApplication() else {
        return (String::new(), String::new(), -1);
    };
    let name = app
        .localizedName()
        .map(|s| s.to_string())
        .unwrap_or_default();
    let bundle = app
        .bundleIdentifier()
        .map(|s| s.to_string())
        .unwrap_or_default();
    (name, bundle, app.processIdentifier())
}

pub fn capture() -> Context {
    let (app_name, _bundle, pid) = frontmost_app();
    let mut ctx = Context {
        app_name,
        ..Default::default()
    };
    if pid <= 0 {
        return ctx;
    }
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return ctx;
        }
        if let Some(window) = copy_attr(app, "AXFocusedWindow") {
            if let Some(title) = string_attr(window as AXUIElementRef, "AXTitle") {
                ctx.window_title = title;
            }
            CFRelease(window);
        }
        if let Some(focused) = copy_attr(app, "AXFocusedUIElement") {
            if let Some(selected) = string_attr(focused as AXUIElementRef, "AXSelectedText") {
                ctx.selected_text =
                    rustype_core::prompt::truncate_chars(&selected, 6000).to_string();
            }
            CFRelease(focused);
        }
        CFRelease(app);
    }
    ctx
}
