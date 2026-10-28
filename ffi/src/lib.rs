//! The C interface the Swift menu-bar app calls (see `micdeck.h` for the C view of these functions).
//!
//! Swift cannot call Rust directly, but it can call C. So every function here is `extern "C"` with a plain
//! name (`#[no_mangle]` stops Rust from renaming it). Each long-running job (lights, meeting cleanup) runs on
//! its own thread with a "stop" flag; starting a job returns at once, and stopping sets the flag and waits.
//! If a job fails after it started (for instance the mic is not plugged in) the message is kept and the app
//! fetches it with `md_take_error`.

use micdeck_core::{effects, stream, with_brightness};
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// A job running on its own thread, and the flag that tells it to stop.
struct Task {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Task {
    /// Run `job` on a new thread. If it returns an error, the message is saved for `md_take_error`.
    fn spawn(job: impl FnOnce(&AtomicBool) -> Result<(), String> + Send + 'static) -> Task {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            if let Err(e) = job(&flag) {
                *ERROR.lock().unwrap() = Some(e);
            }
        });
        Task { stop, thread }
    }

    /// Tell the job to stop and wait until it has.
    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.join();
    }
}

// Global state: there is only ever one light job and one meeting job.
static LIGHT: Mutex<Option<Task>> = Mutex::new(None);
static MEETING: Mutex<Option<Task>> = Mutex::new(None);
static ERROR: Mutex<Option<String>> = Mutex::new(None);

/// Stop whatever job is in `slot`, then put `new` there (`None` just stops it).
fn replace(slot: &Mutex<Option<Task>>, new: Option<Task>) {
    // Take the old job out first and stop it after the lock is released, so the lock is never held while waiting.
    let old = std::mem::replace(&mut *slot.lock().unwrap(), new);
    if let Some(task) = old {
        task.stop();
    }
}

/// Is there a job in `slot` that is still going?
fn running(slot: &Mutex<Option<Task>>) -> bool {
    slot.lock().unwrap().as_ref().is_some_and(|t| !t.thread.is_finished())
}

/// Read a C string from the app. A null pointer or invalid text becomes an empty string.
unsafe fn text(p: *const c_char) -> String {
    if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() }
}

/// Start (or switch to) a lighting effect: `effect` is a name such as "breathing", `r g b` the color, and
/// `brightness` 0-100. Returns false (and sets the error) if the effect name is unknown.
///
/// # Safety
/// `effect` must be a valid zero-terminated string.
#[no_mangle]
pub unsafe extern "C" fn md_light_start(effect: *const c_char, r: u8, g: u8, b: u8, brightness: u8) -> bool {
    let name = text(effect);
    let Some(effect) = effects::Effect::parse(&name) else {
        *ERROR.lock().unwrap() = Some(format!("unknown effect \"{name}\"; choose one of: {}", effects::Effect::names()));
        return false;
    };
    let task = Task::spawn(move |stop| {
        micdeck_duocast::DuoCast.play(stop, |t| {
            let zones = effects::frame(effect, t, [r, g, b], 2);
            [with_brightness(zones[0], brightness), with_brightness(zones[1], brightness)]
        })
    });
    replace(&LIGHT, Some(task));
    true
}

/// Stop the lights (the mic goes back to its own color).
#[no_mangle]
pub extern "C" fn md_light_stop() {
    replace(&LIGHT, None);
}

/// Is the lighting job running?
#[no_mangle]
pub extern "C" fn md_light_running() -> bool {
    running(&LIGHT)
}

/// Start (or restart) meeting cleanup: record from the device whose name contains `input`, play into the one
/// that contains `output`, with the given strength 0-100.
///
/// # Safety
/// `input` and `output` must be valid zero-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn md_meeting_start(input: *const c_char, output: *const c_char, strength: u8) {
    let (input, output) = (text(input), text(output));
    replace(&MEETING, Some(Task::spawn(move |stop| stream::run(stop, &input, &output, strength))));
}

/// Stop meeting cleanup.
#[no_mangle]
pub extern "C" fn md_meeting_stop() {
    replace(&MEETING, None);
}

/// Is the meeting job running?
#[no_mangle]
pub extern "C" fn md_meeting_running() -> bool {
    running(&MEETING)
}

/// The shape of the function the app gives us to be told about the mute button.
type MuteCallback = extern "C" fn(ctx: *mut c_void, muted: bool);

/// Start listening to the DuoCast's mute button for the rest of the program. `callback(ctx, muted)` is called
/// from another thread every time the button is pressed. Call this once.
#[no_mangle]
pub extern "C" fn md_mute_watch(callback: MuteCallback, ctx: *mut c_void) {
    // Raw pointers cannot move to another thread in Rust's eyes; as a plain number they can. The app
    // promises `ctx` stays valid, since it is the app's own object.
    let ctx = ctx as usize;
    std::thread::spawn(move || {
        let result = micdeck_core::hid::watch(micdeck_duocast::VENDOR_ID, micdeck_duocast::AUDIO_PRODUCT_ID, |page, usage, value| {
            if let Some(muted) = micdeck_duocast::muted_from_hid(page, usage, value) {
                callback(ctx as *mut c_void, muted);
            }
        });
        if let Err(e) = result {
            *ERROR.lock().unwrap() = Some(e);
        }
    });
}

/// The last background error, or null if there is none. The message is cleared by reading it. The app must
/// free the text with `md_free_string`.
#[no_mangle]
pub extern "C" fn md_take_error() -> *mut c_char {
    match ERROR.lock().unwrap().take() {
        Some(e) => CString::new(e.replace('\0', " ")).unwrap().into_raw(),
        None => std::ptr::null_mut(),
    }
}

/// Names of the devices that can record (`inputs` true) or play, one per line, for the app's pickers. Free
/// with `md_free_string`. Null if the list could not be read.
#[no_mangle]
pub extern "C" fn md_devices(inputs: bool) -> *mut c_char {
    let list = if inputs { micdeck_core::audio::input_devices() } else { micdeck_core::audio::output_devices() };
    match list {
        Ok(devices) => CString::new(devices.into_iter().map(|d| d.name).collect::<Vec<_>>().join("\n").replace('\0', " ")).unwrap().into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Give back a string that one of the functions above returned.
///
/// # Safety
/// `s` must come from this library and not be freed twice.
#[no_mangle]
pub unsafe extern "C" fn md_free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jobs that fail to start (no such effect or device) end cleanly and leave an error message behind.
    #[test]
    fn failed_jobs_report_an_error() {
        let bad = CString::new("nonsense").unwrap();
        assert!(!unsafe { md_light_start(bad.as_ptr(), 0, 0, 0, 100) });
        assert!(!md_take_error().is_null());

        unsafe { md_meeting_start(bad.as_ptr(), bad.as_ptr(), 60) };
        md_meeting_stop(); // waits for the thread to finish
        assert!(!md_meeting_running());
        let e = md_take_error();
        assert!(!e.is_null());
        unsafe { md_free_string(e) };
    }
}
