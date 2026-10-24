use std::ffi::c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        c_str: *const i8,
        encoding: u32,
    ) -> *const c_void;
    fn CFBooleanGetTypeID() -> usize;
    fn CFGetTypeID(cf: *const c_void) -> usize;
    static kCFBooleanTrue: *const c_void;
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    fn CFRelease(cf: *const c_void);
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

pub fn accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Ask macOS to show the "grant Accessibility access" prompt.
pub fn prompt_accessibility() -> bool {
    unsafe {
        let key_name = b"AXTrustedCheckOptionPrompt\0";
        let key = CFStringCreateWithCString(
            std::ptr::null(),
            key_name.as_ptr().cast(),
            K_CF_STRING_ENCODING_UTF8,
        );
        if key.is_null() {
            return AXIsProcessTrusted();
        }
        let value = kCFBooleanTrue;
        let keys = [key as *const c_void];
        let values = [value as *const c_void];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const u8 as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const u8 as *const c_void,
        );
        let trusted = if options.is_null() {
            AXIsProcessTrusted()
        } else {
            let r = AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
            r
        };
        CFRelease(key);
        trusted
    }
}

/// Kept to silence the unused warning while documenting the CFBoolean check we
/// intentionally avoid (the constant is always a CFBoolean).
#[allow(dead_code)]
fn _assert_cfboolean(value: *const c_void) -> bool {
    unsafe { CFGetTypeID(value) == CFBooleanGetTypeID() }
}
