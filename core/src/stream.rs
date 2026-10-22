//! Live audio: record from one device, clean it (meeting mode), and play it into another, such as
//! headphones or a virtual mic like BlackHole. Hand-written Core Audio bindings, no dependencies.
//!
//! The HAL hands us 32-bit float samples (interleaved or one buffer per channel); both devices must run at 48 kHz.

use crate::audio::{check, fc, get, Addr, AudioDevice, Id, GLOBAL};
use crate::meeting::LiveGate;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::Mutex;
use std::time::Duration;

const RATE: u32 = 48_000;
/// Keep about 20 ms queued between the two devices so small timing differences do not cause gaps.
const PREFILL: usize = 960;
/// ...and never more than 100 ms, so the delay cannot build up when one device runs slightly faster.
const MAX_QUEUE: usize = 4_800;

#[repr(C)]
struct Buffer {
    channels: u32,
    bytes: u32,
    data: *mut f32,
}

/// Core Audio's AudioBufferList: a count, then that many buffers laid out one after another.
#[repr(C)]
struct BufferList {
    count: u32,
    first: Buffer,
}

type IoProc = extern "C" fn(Id, *const c_void, *const BufferList, *const c_void, *mut BufferList, *const c_void, *mut c_void) -> i32;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioDeviceCreateIOProcID(dev: Id, proc_: IoProc, ctx: *mut c_void, out: *mut *const c_void) -> i32;
    fn AudioDeviceStart(dev: Id, proc_id: *const c_void) -> i32;
    fn AudioObjectSetPropertyData(id: Id, addr: *const Addr, qn: u32, q: *const c_void, size: u32, data: *const c_void) -> i32;
}

// ponytail: plain mutexes in the audio callbacks. Fine while only two threads touch them; swap for a
// lock-free ring buffer if glitches show up under load.
struct Shared {
    gate: Mutex<LiveGate>,
    queue: Mutex<VecDeque<f32>>,
}

fn buffers<'a>(list: *const BufferList) -> &'a [Buffer] {
    // SAFETY: Core Audio passes a valid list with `count` buffers for the length of the callback.
    unsafe { std::slice::from_raw_parts(&(*list).first, (*list).count as usize) }
}

fn frames(bufs: &[Buffer]) -> usize {
    bufs.first().map_or(0, |b| b.bytes as usize / 4 / b.channels.max(1) as usize)
}

extern "C" fn on_input(_: Id, _: *const c_void, input: *const BufferList, _: *const c_void, _: *mut BufferList, _: *const c_void, ctx: *mut c_void) -> i32 {
    let shared = unsafe { &*ctx.cast::<Shared>() };
    let bufs = buffers(input);
    let channels: usize = bufs.iter().map(|b| b.channels as usize).sum();
    if channels == 0 {
        return 0;
    }
    let (mut gate, mut queue) = (shared.gate.lock().unwrap(), shared.queue.lock().unwrap());
    for f in 0..frames(bufs) {
        // Mix all channels down to one.
        let mut sum = 0.0;
        for b in bufs {
            let n = b.channels as usize;
            let data = unsafe { std::slice::from_raw_parts(b.data, b.bytes as usize / 4) };
            sum += data[f * n..f * n + n].iter().sum::<f32>();
        }
        queue.push_back(gate.run(sum / channels as f32));
    }
    while queue.len() > MAX_QUEUE {
        queue.pop_front();
    }
    0
}

extern "C" fn on_output(_: Id, _: *const c_void, _: *const BufferList, _: *const c_void, output: *mut BufferList, _: *const c_void, ctx: *mut c_void) -> i32 {
    let shared = unsafe { &*ctx.cast::<Shared>() };
    let bufs = buffers(output);
    let mut queue = shared.queue.lock().unwrap();
    for f in 0..frames(bufs) {
        let s = queue.pop_front().unwrap_or(0.0); // silence if the mic has not delivered yet
        for b in bufs {
            let n = b.channels as usize;
            let data = unsafe { std::slice::from_raw_parts_mut(b.data, b.bytes as usize / 4) };
            data[f * n..f * n + n].fill(s);
        }
    }
    0
}

/// Make sure the device runs at 48 kHz (RNNoise needs it), switching it if it does not. This also changes
/// the rate for other apps using the device.
fn ensure_rate(dev: &AudioDevice) -> Result<(), String> {
    let now = || get::<f64>(dev.id, fc(b"nsrt"), GLOBAL, 0);
    if now()? == RATE as f64 {
        return Ok(());
    }
    let addr = Addr { selector: fc(b"nsrt"), scope: GLOBAL, element: 0 };
    let rate = RATE as f64;
    check(unsafe { AudioObjectSetPropertyData(dev.id, &addr, 0, std::ptr::null(), 8, (&rate as *const f64).cast()) }, "set sample rate")?;
    std::thread::sleep(Duration::from_millis(300)); // the device needs a moment to switch
    if now()? != rate {
        return Err(format!("{} cannot run at 48 kHz", dev.name));
    }
    Ok(())
}

fn start(dev: &AudioDevice, io: IoProc, shared: *mut Shared) -> Result<(), String> {
    let mut id = std::ptr::null();
    check(unsafe { AudioDeviceCreateIOProcID(dev.id, io, shared.cast(), &mut id) }, &format!("open {}", dev.name))?;
    check(unsafe { AudioDeviceStart(dev.id, id) }, &format!("start {}", dev.name))
}

/// Record from `input`, clean the audio with the given strength (0-100), play it into `output`.
/// Runs until the process is stopped.
pub fn run(input: &AudioDevice, output: &AudioDevice, strength: u8) -> Result<(), String> {
    ensure_rate(input)?;
    ensure_rate(output)?;
    // Leaked on purpose: the callbacks use it until the process exits.
    let shared = Box::leak(Box::new(Shared {
        gate: Mutex::new(LiveGate::new(RATE, strength)),
        queue: Mutex::new(VecDeque::from(vec![0.0; PREFILL])),
    }));
    start(output, on_output, shared)?;
    start(input, on_input, shared)?;
    loop {
        std::thread::park();
    }
}
