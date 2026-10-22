//! Live audio: record from one device, clean it (meeting mode), and play it into another, such as
//! headphones or a virtual mic like BlackHole. Hand-written Core Audio bindings, no dependencies.
//!
//! The HAL hands us 32-bit float samples (interleaved or one buffer per channel); both devices must run at 48 kHz.
//!
//! How it works: Core Audio calls a function of ours ("callback") over and over, a few hundred times a
//! second, on its own high-priority audio thread. The INPUT device's callback gives us a small block of
//! recorded samples; we clean them and put them in a shared waiting line (the queue). The OUTPUT device's
//! callback asks us for a block of samples to play; we take them from the same queue. The queue is what
//! connects the two, because the two devices run on separate clocks and never call us at the same moment.

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

/// One block of audio from Core Audio. If `channels` > 1 the samples are interleaved: left, right, left,
/// right... If a device uses one Buffer per channel instead, each Buffer has `channels` = 1.
#[repr(C)]
struct Buffer {
    channels: u32,
    bytes: u32,     // size of the block in bytes (4 bytes per f32 sample)
    data: *mut f32, // pointer to the first sample
}

/// Core Audio's AudioBufferList: a count, then that many buffers laid out one after another.
#[repr(C)]
struct BufferList {
    count: u32,
    first: Buffer,
}

/// The shape Core Audio requires for our callbacks. Most arguments (timestamps etc.) we do not need; the
/// ones that matter are the input block, the output block, and `ctx`, the pointer to our own data.
type IoProc = extern "C" fn(Id, *const c_void, *const BufferList, *const c_void, *mut BufferList, *const c_void, *mut c_void) -> i32;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioDeviceCreateIOProcID(dev: Id, proc_: IoProc, ctx: *mut c_void, out: *mut *const c_void) -> i32;
    fn AudioDeviceStart(dev: Id, proc_id: *const c_void) -> i32;
    fn AudioDeviceStop(dev: Id, proc_id: *const c_void) -> i32;
    fn AudioDeviceDestroyIOProcID(dev: Id, proc_id: *const c_void) -> i32;
    fn AudioObjectSetPropertyData(id: Id, addr: *const Addr, qn: u32, q: *const c_void, size: u32, data: *const c_void) -> i32;
}

// ponytail: plain mutexes in the audio callbacks. Fine while only two threads touch them; swap for a
// lock-free ring buffer if glitches show up under load.
/// Data shared by both callbacks. A `Mutex` makes sure only one thread touches its contents at a time.
struct Shared {
    gate: Mutex<LiveGate>,        // the cleanup; only the input callback uses it
    queue: Mutex<VecDeque<f32>>,  // cleaned samples waiting to be played; input pushes at the back, output pops from the front
}

/// View Core Audio's list of buffers as a normal Rust slice, which is much easier to loop over.
fn buffers<'a>(list: *const BufferList) -> &'a [Buffer] {
    // SAFETY: Core Audio passes a valid list with `count` buffers for the length of the callback.
    unsafe { std::slice::from_raw_parts(&(*list).first, (*list).count as usize) }
}

/// How many "frames" (one sample per channel at one instant) a block holds: bytes / 4 per sample / channels.
fn frames(bufs: &[Buffer]) -> usize {
    bufs.first().map_or(0, |b| b.bytes as usize / 4 / b.channels.max(1) as usize)
}

/// Called by Core Audio with a block of freshly recorded audio: mix to mono, clean each sample, queue it.
extern "C" fn on_input(_: Id, _: *const c_void, input: *const BufferList, _: *const c_void, _: *mut BufferList, _: *const c_void, ctx: *mut c_void) -> i32 {
    // `ctx` is the raw pointer to `Shared` that we gave Core Audio in `start`; turn it back into a reference.
    let shared = unsafe { &*ctx.cast::<Shared>() };
    let bufs = buffers(input);
    let channels: usize = bufs.iter().map(|b| b.channels as usize).sum();
    if channels == 0 {
        return 0;
    }
    // Lock both for the whole block, instead of once per sample, to keep the callback fast.
    let (mut gate, mut queue) = (shared.gate.lock().unwrap(), shared.queue.lock().unwrap());
    for f in 0..frames(bufs) {
        // Mix all channels down to one.
        let mut sum = 0.0;
        for b in bufs {
            let n = b.channels as usize;
            let data = unsafe { std::slice::from_raw_parts(b.data, b.bytes as usize / 4) };
            // Frame `f` of an interleaved block is the `n` samples starting at position f * n; add them up.
            sum += data[f * n..f * n + n].iter().sum::<f32>();
        }
        queue.push_back(gate.run(sum / channels as f32));
    }
    // If the queue has grown too long (the mic runs a bit faster than the output), drop the oldest samples
    // so the delay does not creep up over time.
    while queue.len() > MAX_QUEUE {
        queue.pop_front();
    }
    0
}

/// Called by Core Audio when the output device needs a block of audio to play: fill it from the queue.
extern "C" fn on_output(_: Id, _: *const c_void, _: *const BufferList, _: *const c_void, output: *mut BufferList, _: *const c_void, ctx: *mut c_void) -> i32 {
    let shared = unsafe { &*ctx.cast::<Shared>() };
    let bufs = buffers(output);
    let mut queue = shared.queue.lock().unwrap();
    for f in 0..frames(bufs) {
        let s = queue.pop_front().unwrap_or(0.0); // silence if the mic has not delivered yet
        for b in bufs {
            let n = b.channels as usize;
            let data = unsafe { std::slice::from_raw_parts_mut(b.data, b.bytes as usize / 4) };
            // Write the same mono sample into every channel (left and right) of this frame.
            data[f * n..f * n + n].fill(s);
        }
    }
    0
}

/// Make sure the device runs at 48 kHz (RNNoise needs it), switching it if it does not. This also changes
/// the rate for other apps using the device.
fn ensure_rate(dev: &AudioDevice) -> Result<(), String> {
    // 'nsrt' is the nominal sample rate property; it is a 64-bit float in Hz.
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

/// A running cleanup pipeline: DuoCast in, cleaned audio out. Audio flows from the moment `start` returns
/// until the `Engine` is dropped, which stops the devices and frees everything. The future menu-bar app
/// will own one of these directly; the CLI's `meeting on` runs one in a background process.
pub struct Engine {
    /// The callbacks that are running, as (device, handle Core Audio gave us for our callback).
    running: Vec<(Id, *const c_void)>,
    /// The data both callbacks share. We hold it as a raw pointer because Core Audio keeps a copy of the
    /// pointer; it is freed in `drop`, after the callbacks have been stopped.
    shared: *mut Shared,
}

impl Engine {
    /// Record from `input`, clean the audio with the given strength (0-100), play it into `output`.
    pub fn start(input: &AudioDevice, output: &AudioDevice, strength: u8) -> Result<Engine, String> {
        ensure_rate(input)?;
        ensure_rate(output)?;
        // `Box::into_raw` moves the data to the heap and hands us a plain pointer; we free it ourselves in `drop`.
        let shared = Box::into_raw(Box::new(Shared {
            gate: Mutex::new(LiveGate::new(RATE, strength)),
            queue: Mutex::new(VecDeque::from(vec![0.0; PREFILL])),
        }));
        // Create the Engine before starting anything, so that if the second device fails to start, `drop`
        // still cleans up the first one.
        let mut engine = Engine { running: vec![], shared };
        // Start the output first so it is ready to play as soon as the first cleaned samples arrive.
        engine.attach(output, on_output)?;
        engine.attach(input, on_input)?;
        Ok(engine)
    }

    /// Register a callback with a device and start it.
    fn attach(&mut self, dev: &AudioDevice, io: IoProc) -> Result<(), String> {
        let mut id = std::ptr::null();
        check(unsafe { AudioDeviceCreateIOProcID(dev.id, io, self.shared.cast(), &mut id) }, &format!("open {}", dev.name))?;
        self.running.push((dev.id, id)); // recorded before starting, so `drop` destroys it even if starting fails
        check(unsafe { AudioDeviceStart(dev.id, id) }, &format!("start {}", dev.name))
    }
}

impl Drop for Engine {
    /// Runs automatically when the Engine goes out of scope (or the program exits normally).
    fn drop(&mut self) {
        // Stop every callback first. `AudioDeviceStop` does not return until a callback that is running
        // right now has finished, so after this loop nothing touches `shared` any more.
        for &(dev, id) in &self.running {
            unsafe { AudioDeviceStop(dev, id) };
        }
        for &(dev, id) in &self.running {
            unsafe { AudioDeviceDestroyIOProcID(dev, id) };
        }
        // Now it is safe to free the shared data: turn the raw pointer back into a Box and drop it.
        drop(unsafe { Box::from_raw(self.shared) });
    }
}

/// Run an `Engine` in the foreground until the process is stopped (Ctrl-C, or a kill signal).
pub fn run(input: &AudioDevice, output: &AudioDevice, strength: u8) -> Result<(), String> {
    let _engine = Engine::start(input, output, strength)?;
    // The audio work happens on Core Audio's threads. This thread has nothing to do, so it sleeps. `park` can
    // wake up spuriously, which is why it sits in a loop.
    loop {
        std::thread::park();
    }
}
