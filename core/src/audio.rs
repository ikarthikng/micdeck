//! Mic gain, mute and device lists through macOS Core Audio. Hand-written bindings, no dependencies.
//!
//! Core Audio is a C library, so Rust cannot call it directly in safe code. We declare the C functions
//! we need in the `extern "C"` blocks below and call them inside `unsafe { }`. Everything in this file
//! that touches C is wrapped in a small safe function, so the rest of the program never needs `unsafe`.
//!
//! Core Audio describes everything as a "property" of an object (a device, or the system itself). To read
//! one you give three things, which is what `Addr` holds: WHICH property (selector), for WHICH direction
//! (scope: input, output or global), and for WHICH channel (element).

use std::ffi::{c_char, c_void};
use std::mem::{size_of, MaybeUninit};

/// Core Audio's number for one audio object (a device, or `SYSTEM` for the machine as a whole).
pub(crate) type Id = u32;

/// The "address" of one property: which one, in which direction, on which channel.
/// `#[repr(C)]` makes Rust lay the fields out in memory exactly like the C version, which Core Audio expects.
#[repr(C)]
pub(crate) struct Addr {
    pub selector: u32,
    pub scope: u32,
    pub element: u32,
}

// `#[link]` tells the linker to pull in Apple's CoreAudio framework. The functions below are declared here
// but written by Apple. Each returns a status code (0 = success). The `*const`/`*mut` types are C pointers.
#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectHasProperty(id: Id, addr: *const Addr) -> u8;
    fn AudioObjectGetPropertyDataSize(id: Id, addr: *const Addr, qn: u32, q: *const c_void, size: *mut u32) -> i32;
    fn AudioObjectGetPropertyData(id: Id, addr: *const Addr, qn: u32, q: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
    fn AudioObjectSetPropertyData(id: Id, addr: *const Addr, qn: u32, q: *const c_void, size: u32, data: *const c_void) -> i32;
}

// CoreFoundation is Apple's base library; we need it only to turn its string type (CFString) into text.
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringGetCString(s: *const c_void, buf: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFRelease(p: *const c_void);
}

// Core Audio names its constants with four-letter codes such as 'glob' or 'volm'. A four-character code is
// really just a 32-bit number, so this turns b"glob" into that number. `const fn` lets us use it in constants.
pub(crate) const fn fc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}
pub(crate) const SYSTEM: Id = 1; // the object that stands for the whole audio system
pub(crate) const GLOBAL: u32 = fc(b"glob"); // scope: applies to the device as a whole
pub(crate) const INPUT: u32 = fc(b"inpt"); // scope: the recording side of a device
pub(crate) const OUTPUT: u32 = fc(b"outp"); // scope: the playback side of a device
const DEVICES: u32 = fc(b"dev#"); // property: list of all devices
const STREAMS: u32 = fc(b"stm#"); // property: list of a device's audio streams (none = can't record/play)
const NAME: u32 = fc(b"lnam"); // property: the device's human-readable name
const VOLUME: u32 = fc(b"volm"); // property: gain/volume, 0.0 to 1.0
const MUTE: u32 = fc(b"mute"); // property: muted, 0 or 1
const UTF8: u32 = 0x0800_0100; // CoreFoundation's code for UTF-8 text

/// Turn a Core Audio status code into a Rust `Result`: 0 means success, anything else is an error.
pub(crate) fn check(status: i32, what: &str) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("{what} failed (Core Audio error {status})")) }
}

/// Read one property. `T` is the type of value stored there (e.g. `f32` for volume, `u32` for mute); the
/// caller picks it, and Core Audio fills in that many bytes. Only use plain numbers/pointers for `T`.
pub(crate) fn get<T>(id: Id, selector: u32, scope: u32, element: u32) -> Result<T, String> {
    let addr = Addr { selector, scope, element };
    // We tell Core Audio how many bytes we have room for; it updates `size` with how many it wrote.
    let mut size = size_of::<T>() as u32;
    // A zero-filled block of memory big enough for T that Core Audio will write into.
    let mut out = MaybeUninit::<T>::zeroed();
    // SAFETY: `out` is `size` bytes; callers only use plain numbers and pointers for T.
    check(unsafe { AudioObjectGetPropertyData(id, &addr, 0, std::ptr::null(), &mut size, out.as_mut_ptr().cast()) }, "read")?;
    // `assume_init` says "this memory now holds a real T". It is true because `check` passed above.
    Ok(unsafe { out.assume_init() })
}

/// Write one property on the recording side of a device (the same idea as `get`, in the other direction).
fn set<T>(id: Id, selector: u32, element: u32, value: T) -> Result<(), String> {
    let addr = Addr { selector, scope: INPUT, element };
    check(unsafe { AudioObjectSetPropertyData(id, &addr, 0, std::ptr::null(), size_of::<T>() as u32, (&value as *const T).cast()) }, "write")
}

/// Which channels of a device have this property. Some devices have one control for the whole device
/// (channel 0, the "main" channel); others have one per channel (1 = left, 2 = right). When the main
/// control exists we use only it, otherwise we return every per-channel one so callers can set them all.
fn elements(id: Id, selector: u32) -> Vec<u32> {
    // A closure (a small inline function): "does channel `e` have this property?"
    let has = |e| unsafe { AudioObjectHasProperty(id, &Addr { selector, scope: INPUT, element: e }) } != 0;
    if has(0) { vec![0] } else { (1..=2).filter(|&e| has(e)).collect() }
}

/// An audio device (a mic, speakers, a virtual device).
pub struct AudioDevice {
    pub(crate) id: Id,
    pub name: String,
}

/// Every audio device that has at least one stream in this direction (INPUT or OUTPUT).
fn devices(scope: u32) -> Result<Vec<AudioDevice>, String> {
    // Step 1: ask the system how many bytes the list of device ids takes up.
    let addr = Addr { selector: DEVICES, scope: GLOBAL, element: 0 };
    let mut size = 0;
    check(unsafe { AudioObjectGetPropertyDataSize(SYSTEM, &addr, 0, std::ptr::null(), &mut size) }, "list devices")?;
    // Step 2: make a list big enough (each id is 4 bytes) and let Core Audio fill it in.
    let mut ids = vec![0 as Id; size as usize / size_of::<Id>()];
    check(unsafe { AudioObjectGetPropertyData(SYSTEM, &addr, 0, std::ptr::null(), &mut size, ids.as_mut_ptr().cast()) }, "list devices")?;

    // Step 3: keep only the devices that have at least one stream in the direction we want. A mic has an
    // input stream; speakers have an output stream; headsets and the DuoCast have both.
    let mut out = vec![];
    for id in ids {
        let saddr = Addr { selector: STREAMS, scope, element: 0 };
        let mut n = 0;
        let has_stream = unsafe { AudioObjectGetPropertyDataSize(id, &saddr, 0, std::ptr::null(), &mut n) } == 0 && n > 0;
        if has_stream {
            out.push(AudioDevice { id, name: name(id)? });
        }
    }
    Ok(out)
}

/// Every audio device that can record.
pub fn input_devices() -> Result<Vec<AudioDevice>, String> {
    devices(INPUT)
}

/// Every audio device that can play.
pub fn output_devices() -> Result<Vec<AudioDevice>, String> {
    devices(OUTPUT)
}

/// The output macOS currently plays sound through.
pub fn default_output() -> Result<AudioDevice, String> {
    // 'dOut' is the system-wide property that names the default output device.
    let id: Id = get(SYSTEM, fc(b"dOut"), GLOBAL, 0)?;
    Ok(AudioDevice { id, name: name(id)? })
}

/// A device's name as normal Rust text. Core Audio gives it as a CFString (Apple's string type), which
/// we copy into a plain buffer and then release, because we are responsible for freeing it.
fn name(id: Id) -> Result<String, String> {
    let s: *const c_void = get(id, NAME, GLOBAL, 0)?;
    let mut buf = [0 as c_char; 256];
    // SAFETY: Core Audio gave us an owned CFString; we copy it out and release it.
    let ok = unsafe { CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as isize, UTF8) } != 0;
    unsafe { CFRelease(s) };
    if !ok {
        return Err("could not read device name".into());
    }
    // The buffer holds C text ending in a zero byte; `CStr` reads up to that zero. `to_string_lossy`
    // replaces any invalid UTF-8 instead of failing.
    Ok(unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
}

/// First recording device whose name contains `part` (case-insensitive).
pub fn find_input(part: &str) -> Result<Option<AudioDevice>, String> {
    let part = part.to_lowercase();
    Ok(input_devices()?.into_iter().find(|d| d.name.to_lowercase().contains(&part)))
}

impl AudioDevice {
    /// Input gain, 0.0 to 1.0.
    pub fn gain(&self) -> Result<f32, String> {
        let e = *elements(self.id, VOLUME).first().ok_or("this device has no gain control")?;
        get(self.id, VOLUME, INPUT, e)
    }

    /// Set the input gain (clamped to 0.0 to 1.0) on every channel the device has.
    pub fn set_gain(&self, v: f32) -> Result<(), String> {
        let els = elements(self.id, VOLUME);
        if els.is_empty() {
            return Err("this device has no gain control".into());
        }
        // `try_for_each` runs the closure on each channel and stops at the first error.
        els.into_iter().try_for_each(|e| set(self.id, VOLUME, e, v.clamp(0.0, 1.0)))
    }

    /// Whether the device is muted in macOS. (The DuoCast's own button is separate and not visible here.)
    pub fn muted(&self) -> Result<bool, String> {
        let e = *elements(self.id, MUTE).first().ok_or("this device has no mute control")?;
        Ok(get::<u32>(self.id, MUTE, INPUT, e)? != 0)
    }

    /// Mute or unmute the device in macOS on every channel it has.
    pub fn set_muted(&self, m: bool) -> Result<(), String> {
        let els = elements(self.id, MUTE);
        if els.is_empty() {
            return Err("this device has no mute control".into());
        }
        els.into_iter().try_for_each(|e| set(self.id, MUTE, e, m as u32))
    }
}

/// First playback device whose name contains `part` (case-insensitive).
pub fn find_output(part: &str) -> Result<Option<AudioDevice>, String> {
    let part = part.to_lowercase();
    Ok(output_devices()?.into_iter().find(|d| d.name.to_lowercase().contains(&part)))
}
