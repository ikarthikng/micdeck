//! Mic gain and mute through macOS Core Audio. Hand-written bindings, no dependencies.

use std::ffi::{c_char, c_void};
use std::mem::{size_of, MaybeUninit};

type Id = u32;

#[repr(C)]
struct Addr {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectHasProperty(id: Id, addr: *const Addr) -> u8;
    fn AudioObjectGetPropertyDataSize(id: Id, addr: *const Addr, qn: u32, q: *const c_void, size: *mut u32) -> i32;
    fn AudioObjectGetPropertyData(id: Id, addr: *const Addr, qn: u32, q: *const c_void, size: *mut u32, data: *mut c_void) -> i32;
    fn AudioObjectSetPropertyData(id: Id, addr: *const Addr, qn: u32, q: *const c_void, size: u32, data: *const c_void) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringGetCString(s: *const c_void, buf: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFRelease(p: *const c_void);
}

// Core Audio names its constants with four-letter codes.
const fn fc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}
const SYSTEM: Id = 1;
const GLOBAL: u32 = fc(b"glob");
const INPUT: u32 = fc(b"inpt");
const DEVICES: u32 = fc(b"dev#");
const STREAMS: u32 = fc(b"stm#");
const NAME: u32 = fc(b"lnam");
const VOLUME: u32 = fc(b"volm");
const MUTE: u32 = fc(b"mute");
const UTF8: u32 = 0x0800_0100;

fn check(status: i32, what: &str) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("{what} failed (Core Audio error {status})")) }
}

fn get<T>(id: Id, selector: u32, scope: u32, element: u32) -> Result<T, String> {
    let addr = Addr { selector, scope, element };
    let mut size = size_of::<T>() as u32;
    let mut out = MaybeUninit::<T>::zeroed();
    // SAFETY: `out` is `size` bytes; callers only use plain numbers and pointers for T.
    check(unsafe { AudioObjectGetPropertyData(id, &addr, 0, std::ptr::null(), &mut size, out.as_mut_ptr().cast()) }, "read")?;
    Ok(unsafe { out.assume_init() })
}

fn set<T>(id: Id, selector: u32, element: u32, value: T) -> Result<(), String> {
    let addr = Addr { selector, scope: INPUT, element };
    check(unsafe { AudioObjectSetPropertyData(id, &addr, 0, std::ptr::null(), size_of::<T>() as u32, (&value as *const T).cast()) }, "write")
}

/// Channels that have this property: the main one (0) if present, else channels 1 and 2.
fn elements(id: Id, selector: u32) -> Vec<u32> {
    let has = |e| unsafe { AudioObjectHasProperty(id, &Addr { selector, scope: INPUT, element: e }) } != 0;
    if has(0) { vec![0] } else { (1..=2).filter(|&e| has(e)).collect() }
}

pub struct InputDevice {
    id: Id,
    pub name: String,
}

/// Every audio device that can record.
pub fn input_devices() -> Result<Vec<InputDevice>, String> {
    let addr = Addr { selector: DEVICES, scope: GLOBAL, element: 0 };
    let mut size = 0;
    check(unsafe { AudioObjectGetPropertyDataSize(SYSTEM, &addr, 0, std::ptr::null(), &mut size) }, "list devices")?;
    let mut ids = vec![0 as Id; size as usize / size_of::<Id>()];
    check(unsafe { AudioObjectGetPropertyData(SYSTEM, &addr, 0, std::ptr::null(), &mut size, ids.as_mut_ptr().cast()) }, "list devices")?;

    let mut out = vec![];
    for id in ids {
        let saddr = Addr { selector: STREAMS, scope: INPUT, element: 0 };
        let mut n = 0;
        let has_input = unsafe { AudioObjectGetPropertyDataSize(id, &saddr, 0, std::ptr::null(), &mut n) } == 0 && n > 0;
        if has_input {
            out.push(InputDevice { id, name: name(id)? });
        }
    }
    Ok(out)
}

fn name(id: Id) -> Result<String, String> {
    let s: *const c_void = get(id, NAME, GLOBAL, 0)?;
    let mut buf = [0 as c_char; 256];
    // SAFETY: Core Audio gave us an owned CFString; we copy it out and release it.
    let ok = unsafe { CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as isize, UTF8) } != 0;
    unsafe { CFRelease(s) };
    if !ok {
        return Err("could not read device name".into());
    }
    Ok(unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
}

/// First recording device whose name contains `part` (case-insensitive).
pub fn find_input(part: &str) -> Result<Option<InputDevice>, String> {
    let part = part.to_lowercase();
    Ok(input_devices()?.into_iter().find(|d| d.name.to_lowercase().contains(&part)))
}

impl InputDevice {
    /// Input gain, 0.0 to 1.0.
    pub fn gain(&self) -> Result<f32, String> {
        let e = *elements(self.id, VOLUME).first().ok_or("this device has no gain control")?;
        get(self.id, VOLUME, INPUT, e)
    }

    pub fn set_gain(&self, v: f32) -> Result<(), String> {
        let els = elements(self.id, VOLUME);
        if els.is_empty() {
            return Err("this device has no gain control".into());
        }
        els.into_iter().try_for_each(|e| set(self.id, VOLUME, e, v.clamp(0.0, 1.0)))
    }

    pub fn muted(&self) -> Result<bool, String> {
        let e = *elements(self.id, MUTE).first().ok_or("this device has no mute control")?;
        Ok(get::<u32>(self.id, MUTE, INPUT, e)? != 0)
    }

    pub fn set_muted(&self, m: bool) -> Result<(), String> {
        let els = elements(self.id, MUTE);
        if els.is_empty() {
            return Err("this device has no mute control".into());
        }
        els.into_iter().try_for_each(|e| set(self.id, MUTE, e, m as u32))
    }
}
