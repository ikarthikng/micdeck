//! "Meeting mode" audio cleanup. Removes keyboard/mouse/fan noise and turns down anything quieter than
//! your own voice (background talkers), then evens out the level of what is left.
//!
//! Audio here is a stream of "samples": one number per 1/48000 of a second, between -1.0 and 1.0, that
//! describes how far the microphone membrane is pushed at that instant. Every stage below takes one
//! sample in and returns one sample out, remembering a little state between calls. That keeps the same
//! code usable for a finished recording (loop over all samples) and for live audio (call once per sample).
//!
//! Levels are often quoted in dB (decibels): 0 dB is the loudest possible sample, and every -6 dB is half
//! as big. -40 dB is quiet speech, -60 dB is a quiet room.

use std::f32::consts::PI;

/// High-pass filter: lets high sounds through and removes low rumble (desk thumps, air conditioning).
/// It is a standard "biquad" filter: each output is a weighted mix of the current input, the last two
/// inputs and the last two outputs. The weights (b, a) are computed once from the cutoff frequency using
/// the well-known "RBJ cookbook" formulas.
struct HighPass {
    b: [f32; 3], // weights for the current and previous two inputs
    a: [f32; 2], // weights for the previous two outputs
    x: [f32; 2], // the previous two inputs (state)
    y: [f32; 2], // the previous two outputs (state)
}

impl HighPass {
    /// `cutoff` is in Hz: sounds lower than this are reduced.
    fn new(rate: f32, cutoff: f32) -> Self {
        // Cutoff as an angle per sample, then the standard formulas (alpha sets the filter's sharpness; the
        // constant 1/sqrt(2) gives the flattest response that has no bump at the cutoff).
        let w = 2.0 * PI * cutoff / rate;
        let alpha = w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let c = w.cos();
        let a0 = 1.0 + alpha; // every weight is divided by a0 so the filter has gain 1
        Self {
            b: [(1.0 + c) / 2.0 / a0, -(1.0 + c) / a0, (1.0 + c) / 2.0 / a0],
            a: [-2.0 * c / a0, (1.0 - alpha) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    /// Filter one sample.
    fn run(&mut self, s: f32) -> f32 {
        let y = self.b[0] * s + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        // Shift the history along by one: newest sample in, oldest out.
        self.x = [s, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// Follows how loud a signal is, smoothly. Raw samples swing up and down hundreds of times a second, so
/// to know "how loud is it right now" we keep a running level that moves toward each new value: quickly
/// when the sound gets louder (attack) and more slowly when it gets quieter (release). Used for the
/// loudness meter, the gain smoothing, and the voice-level tracker.
struct Follower {
    attack: f32,  // how much of the old level is kept per sample when rising (closer to 1 = slower)
    release: f32, // the same, when falling
    level: f32,   // the current smoothed level
}

impl Follower {
    /// `attack` and `release` are times in seconds: roughly how long it takes to cover most of the way
    /// to a new level.
    fn new(rate: f32, attack: f32, release: f32) -> Self {
        // Turn a time in seconds into the per-sample "keep this much of the old value" fraction. This is
        // the standard formula for a smoothing filter with that time constant.
        let coef = |secs: f32| (-1.0 / (rate * secs)).exp();
        Self { attack: coef(attack), release: coef(release), level: 0.0 }
    }

    /// Feed in the next value (e.g. the size of a sample) and get the updated smoothed level back.
    fn run(&mut self, x: f32) -> f32 {
        // Pick the fast or slow speed depending on whether the sound is getting louder or quieter, then
        // move the level part of the way toward the new value.
        let c = if x > self.level { self.attack } else { self.release };
        self.level = c * self.level + (1.0 - c) * x;
        self.level
    }
}

/// Loudness of the signal: rises in 30 ms, falls in 50 ms. The slow rise is on purpose: speech lasts
/// longer than that, a keyboard or mouse click does not, so clicks barely register.
fn loudness(rate: f32) -> Follower {
    Follower::new(rate, 0.03, 0.05)
}

/// How much to turn the sound down, as a multiplier (1.0 = untouched, 0.03 = -30 dB). At or above the
/// threshold nothing changes. Below it the multiplier falls with the square of how far under we are:
/// half the threshold gives a quarter of the volume. The `max(0.03)` stops it going fully silent.
fn gain(level: f32, threshold: f32) -> f32 {
    (level / threshold).min(1.0).powi(2).max(0.03)
}

/// Convert a linear level (0.0 to 1.0) to decibels. The tiny minimum avoids taking the log of zero.
fn db(level: f32) -> f32 {
    20.0 * level.max(1e-9).log10()
}

/// Your voice level in a whole recording: the loudness that only the loudest 5% of it exceeds.
pub fn voice_level_db(x: &[f32], rate: u32) -> f32 {
    // Smoothed loudness at every moment of the recording...
    let mut f = loudness(rate as f32);
    let mut env: Vec<f32> = x.iter().map(|s| f.run(s.abs())).collect();
    // ...sorted from quietest to loudest, then pick the value 95% of the way up. That ignores the very
    // loudest 5% (a cough, a clap) but still lands on how loud your voice normally gets.
    env.sort_by(|a, b| a.total_cmp(b));
    db(env.get(env.len() * 95 / 100).copied().unwrap_or(0.0))
}

/// Turn the 0-100 strength knob into a threshold. 0 = barely touches anything (40 dB below your voice),
/// 100 = everything more than 6 dB quieter than your voice is turned down. 60 is a reasonable start.
pub fn threshold_for_strength(voice_db: f32, strength: u8) -> f32 {
    // Slide linearly from 40 dB under the voice (strength 0) to 6 dB under it (strength 100).
    voice_db - 40.0 + 34.0 * strength.min(100) as f32 / 100.0
}

/// RNNoise: a small neural network that removes non-speech noise (keyboard, mouse clicks, fans) by its sound,
/// not its loudness. It works on 10 ms frames at 48 kHz, so it adds 10 ms of delay.
///
/// RNNoise only works on whole 10 ms frames (480 samples), but we get audio one sample at a time. So we
/// collect samples into `input`; when it is full we process the whole frame into `output`, and meanwhile
/// hand back the samples of the PREVIOUS frame from `output`. That is why this stage adds 10 ms of delay.
struct Denoiser {
    state: Box<nnnoiseless::DenoiseState<'static>>, // the neural network's memory between frames
    input: [f32; nnnoiseless::FRAME_SIZE],
    output: [f32; nnnoiseless::FRAME_SIZE],
    pos: usize, // how far through the current frame we are
}

impl Denoiser {
    fn new() -> Self {
        Self { state: nnnoiseless::DenoiseState::new(), input: [0.0; nnnoiseless::FRAME_SIZE], output: [0.0; nnnoiseless::FRAME_SIZE], pos: 0 }
    }

    /// Take one sample in, return one (10 ms older, cleaned) sample out.
    fn run(&mut self, s: f32) -> f32 {
        let out = self.output[self.pos]; // from the previous, already-cleaned frame
        self.input[self.pos] = s * 32768.0; // RNNoise wants 16-bit-sized numbers
        self.pos += 1;
        if self.pos == self.input.len() {
            self.state.process_frame(&mut self.output, &self.input);
            self.pos = 0;
        }
        out / 32768.0
    }
}

/// Stop peaks from clipping (a sample cannot go past 1.0 without crackling). Anything under 0.7 passes
/// untouched; louder samples are bent smoothly toward a ceiling of 0.95 using `tanh`, a curve that
/// flattens out instead of cutting off sharply, so the result sounds natural.
fn limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.7 { x } else { x.signum() * (0.7 + 0.25 * ((a - 0.7) / 0.25).tanh()) }
}

/// The whole cleanup chain, in this order:
///  1. RNNoise removes keyboard, mouse-click and fan noise (it recognises them by sound, not volume).
///  2. High-pass filter removes rumble below 100 Hz.
///  3. The "gate" turns down everything quieter than a threshold, which silences background talkers.
///  4. The compressor evens out loud and soft speech (3:1 ratio, +8 dB make-up gain).
///  5. The limiter stops peaks from clipping.
///
/// Shared by the recording (`process`) and live (`LiveGate`) versions. The gate is made kind to speech:
/// the audio is delayed 30 ms so the gate has already opened when a word starts; it stays open for 150 ms
/// after loud sound so soft words and word endings survive; and its gain changes smoothly, with no clicks.
struct Gate {
    denoise: Option<Denoiser>, // None when the sample rate is not 48 kHz (RNNoise cannot handle it)
    hp: HighPass,
    env: Follower,   // how loud the (filtered) sound is right now; drives the gate
    gain: Follower,  // smooths the gate's volume changes so they do not click
    comp: Follower,  // how loud the output is, for the compressor
    hold: usize,     // samples left before the gate is allowed to start closing
    hold_len: usize, // how long `hold` restarts at (150 ms worth of samples)
    delay: Vec<f32>, // a 30 ms ring buffer of past samples; see `run`
    pos: usize,      // the current position in that ring buffer
}

impl Gate {
    fn new(rate: u32) -> Self {
        let r = rate as f32;
        Self {
            denoise: (rate == 48_000).then(Denoiser::new), // RNNoise only understands 48 kHz audio
            hp: HighPass::new(r, 100.0),
            env: loudness(r),
            gain: Follower { level: 1.0, ..Follower::new(r, 0.002, 0.03) }, // opens in ~2 ms, closes over ~30 ms
            comp: loudness(r), // same measure as the voice level, so "12 dB under your voice" means what it says
            hold: 0,
            hold_len: (r * 0.15) as usize,
            delay: vec![0.0; (r * 0.03) as usize], // matches the 30 ms rise time, so a word's start is not lost
            pos: 0,
        }
    }

    /// Clean one sample. `threshold` is the gate level (sounds quieter than this are turned down) and
    /// `voice` is your voice level; both are linear (not dB). The compressor starts 12 dB below `voice`.
    fn run(&mut self, s: f32, threshold: f32, voice: f32) -> f32 {
        // 1-2. Remove noise, then rumble. `map_or(s, ...)` means: if there is a denoiser run it, else pass `s` through.
        let s = self.denoise.as_mut().map_or(s, |d| d.run(s));
        let y = self.hp.run(s);

        // 3. The gate. First measure how loud things are (`abs` ignores whether the wave is up or down).
        let level = self.env.run(y.abs());
        // Loud enough: restart the 150 ms hold timer. Not loud enough: let it count down by one sample
        // (`saturating_sub` stops at 0 instead of going negative).
        self.hold = if level >= threshold { self.hold_len } else { self.hold.saturating_sub(1) };
        // While the hold timer runs the gate is fully open (1.0); after it ends, quiet sound is turned down.
        let target = if self.hold > 0 { 1.0 } else { gain(level, threshold) };
        let g = self.gain.run(target); // smooth it so the volume glides instead of jumping
        // The 30 ms delay: swap the newest sample into the ring buffer and take out the one stored 30 ms ago.
        // The gate decision above used the NEWEST sound, so it opens 30 ms before the delayed audio plays.
        let late = std::mem::replace(&mut self.delay[self.pos], y);
        self.pos = (self.pos + 1) % self.delay.len(); // wrap around to the start at the end
        let gated = late * g;

        // 4. The compressor: above `ct` (12 dB under your voice) the sound is squeezed 3:1, meaning every
        // 3 dB over `ct` becomes only 1 dB over. The exponent 1/3 - 1 is how that ratio is written as a multiplier.
        let ct = voice * 0.25;
        let det = self.comp.run(gated.abs());
        let squeeze = if det > ct { (det / ct).powf(1.0 / 3.0 - 1.0) } else { 1.0 };
        // 5. Make-up gain then limiter. The gain is 2.5 (+8 dB) when the gate is open, which cancels what a 3:1
        // squeeze takes off a peak 12 dB over the threshold, and about 1.0 when the gate is shut, so background
        // that slips through is not boosted.
        limit(gated * squeeze * (1.0 + 1.5 * g))
    }
}

/// Clean a whole recording: everything quieter than `threshold_db` is turned down. The caller usually
/// gets `threshold_db` from `threshold_for_strength`.
pub fn process(x: &[f32], rate: u32, threshold_db: f32) -> Vec<f32> {
    // dB -> linear: the inverse of `db`. (The gate works in linear levels because that is cheaper per sample.)
    let (mut gate, thr) = (Gate::new(rate), 10f32.powf(threshold_db / 20.0));
    let voice = 10f32.powf(voice_level_db(x, rate) / 20.0);
    x.iter().map(|&s| gate.run(s, thr, voice)).collect()
}

/// The same cleanup for live audio: it learns your voice level as you talk, so there is no whole recording to measure.
pub struct LiveGate {
    gate: Gate,
    voice: Follower, // a slow tracker of how loud you normally speak
    offset: f32,     // the strength setting as a multiplier: gate threshold = voice level x offset
}

impl LiveGate {
    /// `strength` is the 0-100 knob (higher = more background removed, at the risk of cutting soft words).
    pub fn new(rate: u32, strength: u8) -> Self {
        Self {
            gate: Gate::new(rate),
            // ponytail: voice level rises in 0.3 s and forgets over 30 s of silence; if you are silent
            // for minutes, background can leak until you speak again. Add a floor/hold if that bites.
            voice: Follower::new(rate as f32, 0.3, 30.0),
            // Using a voice level of 0 dB here gives just the offset (in dB), which we turn into a multiplier.
            offset: 10f32.powf(threshold_for_strength(0.0, strength) / 20.0),
        }
    }

    /// Clean one live sample.
    pub fn run(&mut self, s: f32) -> f32 {
        // The gate measures loudness itself; reuse its level so the voice tracker sees the same thing.
        let voice = self.voice.run(self.gate.env.level).max(0.001); // never trust a voice level below -60 dB
        self.gate.run(s, voice * self.offset, voice)
    }
}

/// Read a mono 16-bit PCM or 32-bit float WAV file. Returns (sample rate, samples).
///
/// A WAV file is a "RIFF" container: a 12-byte header, then a series of chunks, each with a 4-letter name
/// and a length. We need the "fmt " chunk (how the audio is stored) and the "data" chunk (the samples).
pub fn read_wav(bytes: &[u8]) -> Result<(u32, Vec<f32>), String> {
    // Small helpers to read little-endian numbers (the byte order WAV files use) at a position in the file.
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    // Walk the chunks one after another. `fmt` is remembered when we pass it: (type, channels, rate, bits).
    let (mut pos, mut fmt) = (12, None);
    while pos + 8 <= bytes.len() {
        let (id, len) = (&bytes[pos..pos + 4], u32_at(pos + 4) as usize);
        let body = pos + 8;
        match id {
            b"fmt " if body + 16 <= bytes.len() => fmt = Some((u16_at(body), u16_at(body + 2), u32_at(body + 4), u16_at(body + 14))),
            b"data" => {
                let (kind, channels, rate, bits) = fmt.ok_or("WAV has no fmt chunk before data")?;
                if channels != 1 {
                    return Err(format!("need mono audio, this file has {channels} channels"));
                }
                // The length may overshoot if the file was cut short (e.g. a recording that was interrupted).
                let data = &bytes[body..(body + len).min(bytes.len())];
                // Convert the raw bytes to f32 samples in -1.0..1.0. Type 1 = integers: 16-bit values run from
                // -32768 to 32767, so dividing by 32768 scales them. Type 3 = floats, which need no scaling.
                let samples = match (kind, bits) {
                    (1, 16) => data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect(),
                    (3, 32) => data.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect(),
                    _ => return Err(format!("unsupported WAV format (type {kind}, {bits} bits); use 16-bit PCM")),
                };
                return Ok((rate, samples));
            }
            _ => {}
        }
        pos = body + len + (len & 1); // chunks are padded to even sizes
    }
    Err("WAV has no data chunk".into())
}

/// Write mono 16-bit PCM WAV bytes: the 44-byte header followed by the samples.
pub fn write_wav(rate: u32, x: &[f32]) -> Vec<u8> {
    let data_len = (x.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + x.len() * 2);
    out.extend(b"RIFF");
    out.extend((36 + data_len).to_le_bytes()); // size of everything after these 8 bytes
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes()); // size of the fmt chunk
    out.extend([1, 0, 1, 0]); // PCM, mono
    out.extend(rate.to_le_bytes());
    out.extend((rate * 2).to_le_bytes()); // bytes per second: 2 bytes per sample, 1 channel
    out.extend([2, 0, 16, 0]); // block align, bits
    out.extend(b"data");
    out.extend(data_len.to_le_bytes());
    // Scale each f32 (-1.0..1.0) to a 16-bit integer, clamping first so a stray value cannot wrap around.
    out.extend(x.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amp: f32, secs: f32, rate: u32) -> Vec<f32> {
        (0..(secs * rate as f32) as usize).map(|i| amp * (2.0 * PI * 440.0 * i as f32 / rate as f32).sin()).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn quiet_sound_is_reduced_and_loud_sound_is_kept() {
        let rate = 48_000;
        let mut x = tone(0.5, 1.0, rate); // "your voice"
        x.extend(tone(0.01, 1.0, rate)); // "someone in the background"
        let y = process(&x, rate, -30.0);
        let (loud, quiet) = (rms(&y[rate as usize / 2..rate as usize]), rms(&y[rate as usize * 3 / 2..]));
        assert!(loud > 0.9 * rms(&x[rate as usize / 2..rate as usize]), "loud part changed too much: {loud}");
        assert!(quiet < 0.2 * rms(&x[rate as usize * 3 / 2..]), "quiet part not reduced: {quiet}");
    }

    #[test]
    fn live_gate_learns_voice_and_reduces_quiet_sound() {
        let rate = 48_000;
        let mut x = tone(0.5, 2.0, rate);
        x.extend(tone(0.01, 1.0, rate));
        let mut gate = LiveGate::new(rate, 90);
        let y: Vec<f32> = x.iter().map(|&s| gate.run(s)).collect();
        let n = rate as usize;
        assert!(rms(&y[n..2 * n]) > 0.8 * rms(&x[n..2 * n]), "voice was changed too much");
        assert!(rms(&y[5 * n / 2..]) < 0.2 * rms(&x[5 * n / 2..]), "background not reduced");
    }

    #[test]
    fn soft_word_right_after_loud_sound_is_kept() {
        let rate = 48_000;
        let mut x = tone(0.5, 0.5, rate);
        x.extend(tone(0.03, 0.1, rate)); // soft word, 24 dB quieter, inside the 150 ms hold
        x.extend(tone(0.03, 0.5, rate));
        let y = process(&x, rate, -30.0);
        let (a, b) = (rate as usize * 11 / 20, rate as usize * 12 / 20);
        assert!(rms(&y[a..b]) > 0.8 * rms(&x[a..b]), "soft word was cut: {}", rms(&y[a..b]));
    }

    #[test]
    fn compressor_evens_out_loud_and_soft_speech() {
        let rate = 48_000;
        let mut x = tone(0.5, 1.0, rate);
        x.extend(tone(0.1, 1.0, rate)); // 14 dB quieter, still well above the gate
        let y = process(&x, rate, -50.0);
        let n = rate as usize;
        let (before, after) = (rms(&x[..n]) / rms(&x[n..]), rms(&y[n / 2..n]) / rms(&y[3 * n / 2..]));
        assert!(after < 0.6 * before, "loud/soft ratio {before} became {after}");
    }

    #[test]
    fn peaks_never_clip() {
        let y = process(&tone(0.99, 1.0, 48_000), 48_000, -90.0);
        assert!(y.iter().all(|s| s.abs() < 0.96));
    }

    #[test]
    fn short_click_in_silence_is_mostly_removed() {
        let rate = 48_000;
        let mut x = tone(0.3, 1.0, rate); // speech, so the voice level is learned
        x.extend(vec![0.0; rate as usize / 2]);
        let click_at = x.len();
        x.extend(tone(0.5, 0.005, rate)); // 5 ms click
        x.extend(vec![0.0; rate as usize / 2]);
        let y = process(&x, rate, -20.0);
        let (a, b) = (click_at, click_at + rate as usize / 10);
        assert!(rms(&y[a..b]) < 0.4 * rms(&x[a..b]), "click passed: {} vs {}", rms(&y[a..b]), rms(&x[a..b]));
    }

    #[test]
    fn strength_knob_maps_to_threshold() {
        assert_eq!(threshold_for_strength(-20.0, 0), -60.0);
        assert_eq!(threshold_for_strength(-20.0, 100), -26.0);
        assert_eq!(threshold_for_strength(-20.0, 200), -26.0); // clamped
    }

    #[test]
    fn rumble_is_removed() {
        let rate = 48_000;
        let x: Vec<f32> = (0..rate as usize).map(|i| 0.5 * (2.0 * PI * 20.0 * i as f32 / rate as f32).sin()).collect();
        assert!(rms(&process(&x, rate, -90.0)[rate as usize / 2..]) < 0.05);
    }

    #[test]
    fn wav_round_trips() {
        let x = tone(0.5, 0.1, 48_000);
        let (rate, y) = read_wav(&write_wav(48_000, &x)).unwrap();
        assert_eq!(rate, 48_000);
        assert!(x.iter().zip(&y).all(|(a, b)| (a - b).abs() < 1e-3));
    }
}
