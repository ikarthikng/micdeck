//! Listen to a HID device's input values (button presses) through macOS IOHIDManager.
//! Unlike raw USB, this works while macOS's own driver owns the device. Hand-written bindings.

use std::ffi::{c_char, c_void, CString};

type Ref = *const c_void;
type ValueCallback = extern "C" fn(ctx: *mut c_void, result: i32, sender: Ref, value: Ref);

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDManagerCreate(alloc: Ref, options: u32) -> Ref;
    fn IOHIDManagerSetDeviceMatching(m: Ref, matching: Ref);
    fn IOHIDManagerRegisterInputValueCallback(m: Ref, cb: ValueCallback, ctx: *mut c_void);
    fn IOHIDValueGetElement(v: Ref) -> Ref;
    fn IOHIDValueGetIntegerValue(v: Ref) -> isize;
    fn IOHIDElementGetUsagePage(e: Ref) -> u32;
    fn IOHIDElementGetUsage(e: Ref) -> u32;
    fn IOHIDManagerScheduleWithRunLoop(m: Ref, run_loop: Ref, mode: Ref);
    fn IOHIDManagerOpen(m: Ref, options: u32) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopDefaultMode: Ref;
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopRun();
    fn CFStringCreateWithCString(alloc: Ref, s: *const c_char, encoding: u32) -> Ref;
    fn CFNumberCreate(alloc: Ref, kind: isize, value: *const c_void) -> Ref;
    fn CFDictionaryCreate(alloc: Ref, keys: *const Ref, values: *const Ref, n: isize, kcb: *const c_void, vcb: *const c_void) -> Ref;
}

const UTF8: u32 = 0x0800_0100;
const SINT32: isize = 3; // kCFNumberSInt32Type

fn cf_str(s: &str) -> Ref {
    let c = CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) }
}

fn cf_int(n: i32) -> Ref {
    unsafe { CFNumberCreate(std::ptr::null(), SINT32, (&n as *const i32).cast()) }
}

extern "C" fn trampoline<F: FnMut(u32, u32, isize)>(ctx: *mut c_void, _result: i32, _sender: Ref, value: Ref) {
    // SAFETY: ctx is the `&mut F` that `watch` passed in and keeps alive while the run loop runs.
    let f = unsafe { &mut *ctx.cast::<F>() };
    unsafe {
        let e = IOHIDValueGetElement(value);
        f(IOHIDElementGetUsagePage(e), IOHIDElementGetUsage(e), IOHIDValueGetIntegerValue(value));
    }
}

/// Call `on_value(usage_page, usage, value)` whenever a button or control on the device changes. Runs until the process exits.
/// macOS may ask you to allow Input Monitoring for your terminal the first time.
pub fn watch<F: FnMut(u32, u32, isize)>(vid: u16, pid: u16, mut on_value: F) -> Result<(), String> {
    unsafe {
        let mgr = IOHIDManagerCreate(std::ptr::null(), 0);
        let keys = [cf_str("VendorID"), cf_str("ProductID")];
        let values = [cf_int(vid.into()), cf_int(pid.into())];
        // pid 0 = any product from this vendor
        let matching = CFDictionaryCreate(
            std::ptr::null(), keys.as_ptr(), values.as_ptr(), if pid == 0 { 1 } else { 2 },
            (&raw const kCFTypeDictionaryKeyCallBacks).cast(), (&raw const kCFTypeDictionaryValueCallBacks).cast(),
        );
        // Always call this: without it the manager matches nothing.
        IOHIDManagerSetDeviceMatching(mgr, matching);
        IOHIDManagerRegisterInputValueCallback(mgr, trampoline::<F>, (&raw mut on_value).cast());
        IOHIDManagerScheduleWithRunLoop(mgr, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
        match IOHIDManagerOpen(mgr, 0) as u32 {
            0 => {}
            // kIOReturnNotPermitted
            0xE00002E2 => return Err("not permitted: allow your terminal under System Settings > Privacy & Security > Input Monitoring, then retry".into()),
            code => return Err(format!("could not open HID devices (IOKit error {code:#x})")),
        }
        CFRunLoopRun();
    }
    Ok(())
}
