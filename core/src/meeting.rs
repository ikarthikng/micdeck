//! "Meeting mode" audio cleanup: cut rumble, then turn down anything quieter than your own voice.
//! Pure Rust, no dependencies. Works on mono f32 samples in -1.0..1.0.

use std::f32::consts::PI;

/// 2nd-order high-pass filter (RBJ biquad, Butterworth Q), one sample at a time.
struct HighPass {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl HighPass {
    fn new(rate: f32, cutoff: f32) -> Self {
        let w = 2.0 * PI * cutoff / rate;
        let alpha = w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let c = w.cos();
        let a0 = 1.0 + alpha;
        Self {
            b: [(1.0 + c) / 2.0 / a0, -(1.0 + c) / a0, (1.0 + c) / 2.0 / a0],
            a: [-2.0 * c / a0, (1.0 - alpha) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn run(&mut self, s: f32) -> f32 {
        let y = self.b[0] * s + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [s, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// Follows a level: rises with the `attack` time constant, falls with `release` (seconds).
struct Follower {
    attack: f32,
    release: f32,
    level: f32,
}

impl Follower {
    fn new(rate: f32, attack: f32, release: f32) -> Self {
        let coef = |secs: f32| (-1.0 / (rate * secs)).exp();
        Self { attack: coef(attack), release: coef(release), level: 0.0 }
    }

    fn run(&mut self, x: f32) -> f32 {
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

/// Below the threshold the gain falls with the square of the level, down to -30 dB.
fn gain(level: f32, threshold: f32) -> f32 {
    (level / threshold).min(1.0).powi(2).max(0.03)
}

pub fn db(level: f32) -> f32 {
    20.0 * level.max(1e-9).log10()
}

/// Your voice level in a whole recording: the loudness that only the loudest 5% of it exceeds.
pub fn voice_level_db(x: &[f32], rate: u32) -> f32 {
    let mut f = loudness(rate as f32);
    let mut env: Vec<f32> = x.iter().map(|s| f.run(s.abs())).collect();
    env.sort_by(|a, b| a.total_cmp(b));
    db(env.get(env.len() * 95 / 100).copied().unwrap_or(0.0))
}

/// Turn the 0-100 strength knob into a threshold. 0 = barely touches anything (40 dB below your voice),
/// 100 = everything more than 6 dB quieter than your voice is turned down. 60 is a reasonable start.
pub fn threshold_for_strength(voice_db: f32, strength: u8) -> f32 {
    voice_db - 40.0 + 34.0 * strength.min(100) as f32 / 100.0
}

/// RNNoise: a small neural network that removes non-speech noise (keyboard, mouse clicks, fans) by its sound,
/// not its loudness. It works on 10 ms frames at 48 kHz, so it adds 10 ms of delay.
struct Denoiser {
    state: Box<nnnoiseless::DenoiseState<'static>>,
    input: [f32; nnnoiseless::FRAME_SIZE],
    output: [f32; nnnoiseless::FRAME_SIZE],
    pos: usize,
}

impl Denoiser {
    fn new() -> Self {
        Self { state: nnnoiseless::DenoiseState::new(), input: [0.0; nnnoiseless::FRAME_SIZE], output: [0.0; nnnoiseless::FRAME_SIZE], pos: 0 }
    }

    fn run(&mut self, s: f32) -> f32 {
        let out = self.output[self.pos];
        self.input[self.pos] = s * 32768.0; // RNNoise wants 16-bit-sized numbers
        self.pos += 1;
        if self.pos == self.input.len() {
            self.state.process_frame(&mut self.output, &self.input);
            self.pos = 0;
        }
        out / 32768.0
    }
}

/// Stop peaks from clipping: pass anything under 0.7 untouched, bend louder samples smoothly toward 0.95.
fn limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.7 { x } else { x.signum() * (0.7 + 0.25 * ((a - 0.7) / 0.25).tanh()) }
}

/// Radio-style chain: remove keyboard/click/fan noise (RNNoise), cut rumble below 100 Hz, fade out everything quieter than a threshold (the gate),
/// then even out loud and soft speech (3:1 compressor with +8 dB make-up) and limit peaks.
/// Shared by the file and live versions. The gate is kind to speech: the audio is delayed 30 ms so the gate has opened by the time a word starts, it stays open 150 ms
/// after loud sound so soft words and word endings survive, and the gain changes smoothly.
struct Gate {
    denoise: Option<Denoiser>,
    hp: HighPass,
    env: Follower,
    gain: Follower,
    comp: Follower,
    hold: usize,
    hold_len: usize,
    delay: Vec<f32>,
    pos: usize,
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

    /// `voice` is your voice level (linear); the compressor starts 12 dB below it.
    fn run(&mut self, s: f32, threshold: f32, voice: f32) -> f32 {
        let s = self.denoise.as_mut().map_or(s, |d| d.run(s));
        let y = self.hp.run(s);
        let level = self.env.run(y.abs());
        self.hold = if level >= threshold { self.hold_len } else { self.hold.saturating_sub(1) };
        let target = if self.hold > 0 { 1.0 } else { gain(level, threshold) };
        let g = self.gain.run(target);
        let late = std::mem::replace(&mut self.delay[self.pos], y);
        self.pos = (self.pos + 1) % self.delay.len();
        let gated = late * g;
        let ct = voice * 0.25;
        let det = self.comp.run(gated.abs());
        let squeeze = if det > ct { (det / ct).powf(1.0 / 3.0 - 1.0) } else { 1.0 };
        // 2.5 = +8 dB, what a 3:1 squeeze takes off a peak 12 dB over the threshold. The lift is scaled by how
        // open the gate is, so background that slips through is not boosted.
        limit(gated * squeeze * (1.0 + 1.5 * g))
    }
}

/// Clean a whole recording: everything quieter than `threshold_db` is faded out.
pub fn process(x: &[f32], rate: u32, threshold_db: f32) -> Vec<f32> {
    let (mut gate, thr) = (Gate::new(rate), 10f32.powf(threshold_db / 20.0));
    let voice = 10f32.powf(voice_level_db(x, rate) / 20.0);
    x.iter().map(|&s| gate.run(s, thr, voice)).collect()
}

/// The same cleanup for live audio: it learns your voice level as you talk, so there is no whole recording to measure.
pub struct LiveGate {
    gate: Gate,
    voice: Follower,
    offset: f32,
}

impl LiveGate {
    pub fn new(rate: u32, strength: u8) -> Self {
        Self {
            gate: Gate::new(rate),
            // ponytail: voice level rises in 0.3 s and forgets over 30 s of silence; if you are silent
            // for minutes, background can leak until you speak again. Add a floor/hold if that bites.
            voice: Follower::new(rate as f32, 0.3, 30.0),
            offset: 10f32.powf(threshold_for_strength(0.0, strength) / 20.0),
        }
    }

    pub fn run(&mut self, s: f32) -> f32 {
        // The gate measures loudness itself; reuse its level so the voice tracker sees the same thing.
        let voice = self.voice.run(self.gate.env.level).max(0.001); // never trust a voice level below -60 dB
        self.gate.run(s, voice * self.offset, voice)
    }
}

/// Read a mono 16-bit PCM or 32-bit float WAV file. Returns (sample rate, samples).
pub fn read_wav(bytes: &[u8]) -> Result<(u32, Vec<f32>), String> {
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
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
                let data = &bytes[body..(body + len).min(bytes.len())];
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

/// Write mono 16-bit PCM WAV bytes.
pub fn write_wav(rate: u32, x: &[f32]) -> Vec<u8> {
    let data_len = (x.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + x.len() * 2);
    out.extend(b"RIFF");
    out.extend((36 + data_len).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend([1, 0, 1, 0]); // PCM, mono
    out.extend(rate.to_le_bytes());
    out.extend((rate * 2).to_le_bytes());
    out.extend([2, 0, 16, 0]); // block align, bits
    out.extend(b"data");
    out.extend(data_len.to_le_bytes());
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
