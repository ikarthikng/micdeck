//! Listen to a HID device's input values (button presses) through macOS IOHIDManager.
//! Unlike raw USB, this works while macOS's own driver owns the device. Hand-written bindings.
//!
//! How it works: IOHIDManager watches every HID device that matches a filter (here: a vendor and product
//! id). Whenever a button or control on one of them changes, macOS calls a function we registered (a
//! "callback") with the new value. We then park the thread in a "run loop", which is macOS's event loop:
//! it sleeps until something happens and delivers it to our callback.

use std::ffi::{c_char, c_void, CString};

/// Most CoreFoundation/IOKit objects are handed around as opaque pointers; we never look inside them.
type Ref = *const c_void;
/// The shape macOS requires for our callback: it receives our own data (`ctx`), an error code, the device
/// that sent the event, and the new value.
type ValueCallback = extern "C" fn(ctx: *mut c_void, result: i32, sender: Ref, value: Ref);

// IOKit is the Apple framework for talking to hardware. `kind = "framework"` links it into our program.
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
    // Not functions but values owned by Apple. We only ever pass their addresses back to Apple, so the
    // exact type does not matter (`u8` for the two structs we never read).
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

/// Make a CoreFoundation string. `CString` is Rust text with the zero byte at the end that C expects.
/// The result is never freed: we call this a few times at startup and the program runs once, so it is not worth tracking.
fn cf_str(s: &str) -> Ref {
    let c = CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) }
}

/// Make a CoreFoundation number (never freed, same reasoning as `cf_str`).
fn cf_int(n: i32) -> Ref {
    unsafe { CFNumberCreate(std::ptr::null(), SINT32, (&n as *const i32).cast()) }
}

/// macOS can only call a plain C function, but we want it to run our own Rust closure. This function is
/// the bridge ("trampoline"): macOS calls it, and it turns the raw pointer back into our closure and calls
/// that. `F` is the closure's type, so a separate copy of this function is made for each closure type.
extern "C" fn trampoline<F: FnMut(u32, u32, isize)>(ctx: *mut c_void, _result: i32, _sender: Ref, value: Ref) {
    // SAFETY: ctx is the `&mut F` that `watch` passed in and keeps alive while the run loop runs.
    let f = unsafe { &mut *ctx.cast::<F>() };
    // Ask macOS which control sent this (its "element") and read its usage page and usage numbers. These
    // identify the control, e.g. page 0xffc1 usage 0xf0 is the DuoCast's mute button.
    unsafe {
        let e = IOHIDValueGetElement(value);
        f(IOHIDElementGetUsagePage(e), IOHIDElementGetUsage(e), IOHIDValueGetIntegerValue(value));
    }
}

/// Call `on_value(usage_page, usage, value)` whenever a button or control on the device changes. Runs until the process exits.
/// macOS may ask you to allow Input Monitoring for your terminal the first time.
pub fn watch<F: FnMut(u32, u32, isize)>(vid: u16, pid: u16, mut on_value: F) -> Result<(), String> {
    // The whole body is `unsafe` because every call below goes to C. Order matters: build the filter, register
    // the callback, attach to the run loop, open, then run.
    unsafe {
        let mgr = IOHIDManagerCreate(std::ptr::null(), 0);
        // The filter is a dictionary like {VendorID: 1008, ProductID: 2700}.
        let keys = [cf_str("VendorID"), cf_str("ProductID")];
        let values = [cf_int(vid.into()), cf_int(pid.into())];
        let matching = CFDictionaryCreate(
            std::ptr::null(), keys.as_ptr(), values.as_ptr(), 2,
            (&raw const kCFTypeDictionaryKeyCallBacks).cast(), (&raw const kCFTypeDictionaryValueCallBacks).cast(),
        );
        // Always call this: without it the manager matches nothing.
        IOHIDManagerSetDeviceMatching(mgr, matching);
        // Register the trampoline, and hand it a pointer to our closure so it can call it later. `&raw mut`
        // takes that pointer without creating a Rust reference; `.cast()` turns it into the untyped pointer C wants.
        IOHIDManagerRegisterInputValueCallback(mgr, trampoline::<F>, (&raw mut on_value).cast());
        IOHIDManagerScheduleWithRunLoop(mgr, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
        // Open the devices. The status is shown as an unsigned number so it matches Apple's documented error codes.
        match IOHIDManagerOpen(mgr, 0) as u32 {
            0 => {}
            // kIOReturnNotPermitted
            0xE00002E2 => return Err("not permitted: allow your terminal under System Settings > Privacy & Security > Input Monitoring, then retry".into()),
            code => return Err(format!("could not open HID devices (IOKit error {code:#x})")),
        }
        // Sleep here forever, waking up whenever a device event arrives. This call only returns if the loop is stopped.
        CFRunLoopRun();
    }
    Ok(())
}
