# MicDeck: project context

Open-source Mac control software for USB mics that lack a native macOS app. First target: HyperX DuoCast. Long term: many mics, one app.

The owner is new to Rust and hardware work and relies on Claude for standards and best practice. Explain steps simply.

## Git workflow (always follow)
- **Never commit directly to `master`.** Every new feature, bug fix or chore gets its own branch, and the PR is how it reaches `master`.
- **Start each branch from an up-to-date `master`:** `git switch master && git pull --ff-only`, then `git switch -c <type>/<short-name>`. Types: `feature/`, `fix/`, `chore/`.
- **Commit only on that branch.** Small, clear commit messages.
- **Pull request required.** The owner reviews the PR and merges it on GitHub. Claude never merges, and never pushes to `master`.
- **Ask before pushing** a branch or opening a PR (it publishes work); the owner has said they will do or approve this.
- After the owner merges: switch back to `master`, pull, and delete the finished local branch.

## Decisions made
- **Name:** MicDeck (checked: free on Homebrew, crates.io, npm, PyPI; `micdeck.com` is taken; a small unrelated Windows repo `MicDeck` exists on GitHub). Trademark search not done.
- **License:** GPL-3.0-only. The goal is credit plus keeping improvements open. Commercial use is allowed. The `LICENSE` file holds the official GPLv3 text, downloaded from gnu.org.
- **Do not copy code from quadcastrgb or OpenRGB.** quadcastrgb is GPL-2.0-only, which cannot be combined into a GPL-3.0 project. Write our own Rust code from the protocol facts, and from our own USB captures where needed.
- **Never implement firmware flashing.**
- **Avoid "HyperX" in the product name** (trademark). Not affiliated with HyperX or HP.
- **Language:** Rust core and CLI. Native SwiftUI menu-bar app as the frontend later (lightest option on macOS).
- **Dependencies:** as few as possible. Prefer macOS built-in frameworks. Current: only `nnnoiseless` 0.5.2 (pure-Rust RNNoise port, BSD-3-Clause, GPL-3 compatible, no system libs; pulls ~25 pure-Rust FFT/math crates; works on any macOS/arch). Ask before adding more.
- **Modularity:** one driver crate per mic under `drivers/`, chosen at build time (Cargo features). The core defines a `Device` trait. Each driver declares its capabilities and frontends show only those controls.
- **Order of work:** core library, then the `micdeck` CLI, then the GUI. Frontend and CLI both link the core library, with no background service unless hotplug needs one.
- **Also planned:** hotplug detection, saved profiles, a fake mic for tests without hardware, a driver template plus a contributor guide, a universal (Apple Silicon + Intel) build.
- **Audio controls** (gain, sample rate) go through Core Audio, as a separate kind of module from USB lighting.
- **Distribution:** GitHub releases with a .dmg. Unsigned at first (users right-click, Open). Signing and notarization later. App Store later.

## Hardware facts (from the owner's Mac)
- HyperX DuoCast (audio): USB vendor 0x03f0, product 0x0a8c.
- HyperX DuoCast Controller: vendor 0x03f0, product 0x098c. A HID device, and the likely lighting channel. One interface seen uses the Consumer usage page (media/mute keys). Lighting is probably on a separate vendor-specific interface, not yet inspected.
- OpenRGB notes a quirk on related HyperX mics: the interface reports HID but has no interrupt IN endpoint, so a libusb route works where hidraw does not.
- The mic keeps its lighting settings in its own memory (they survived moving from Windows to Mac).
- The owner has a Windows PC for USB captures (Wireshark + USBPcap + NGENUITY) if the protocol needs it.

## Toolchain
- Rust 1.99 via Homebrew `rustup` (keg-only). PATH needs `/opt/homebrew/opt/rustup/bin`.
- Workspace: `core/` (micdeck-core), `drivers/duocast/` (micdeck-duocast), `cli/` (micdeck).

## Done
- USB exploration (done early, then removed with `rusb` in the ponytail cleanup because nothing else used it; re-add `rusb` only if lighting needs raw USB). Findings of 03f0:098c: interface 0 = HID, interrupt OUT 0x01 (8 bytes); interface 1 = HID, interrupt IN 0x82 (8 bytes, probably the mute/media keys). Interface 0's OUT endpoint is the likely lighting channel. HID report descriptors: interface 0 = vendor-defined (usage page 0xFF90, no report ID): 64-byte input report, 64-byte output report, and a feature report. The 64-byte output report is the likely lighting command. The endpoint max packet is only 8, so a report may be sent in chunks or via control transfer (SET_REPORT). Interface 1 = consumer keys (report ID 3, 16-bit usage) plus a vendor report ID 5 (7 bytes in). macOS refuses raw USB claims on the HID interfaces (even with sudo); use IOKit HID (`core/src/hid.rs`) instead.

- Core Audio module `core/src/audio.rs` (hand-written FFI, no dependencies, macOS only). CLI: `micdeck audio | mute on|off | gain <0-100>`. Tested: mute on/off works on the DuoCast. The DuoCast exposes NO software gain to macOS (only mute); gain is the physical knob. Gain code works for other mics (AirPods, webcam).
- DuoCast audio device is found by name ("duocast"); Core Audio does not expose USB IDs.
- 03f0:098c is listed in QuadcastRGB's supported devices (same protocol family as QuadCast S). Packet format is NOT documented outside its GPL-2.0 source; rule: facts only, no code copying. Decision pending: read-for-facts vs own Windows capture.

- `core/src/fake.rs`: `FakeMic` implements `Device` for tests/frontends without hardware (2 tests, `cargo test`).

- `core/src/hid.rs`: macOS IOHIDManager listener (hand-written FFI). Works without claiming the device and without sudo (raw USB claim and sudo both fail: macOS HID driver owns the interface). IMPORTANT: IOHIDManagerSetDeviceMatching must always be called (NULL = all devices), otherwise nothing matches. Keyboards need Input Monitoring permission for the terminal/VS Code app; mice and the mic do not.
- Physical mute button: Core Audio does NOT see it (mic mutes internally). It sends HID events from the AUDIO device (03f0:0a8c, not the 098c Controller): usage page 0xffc1, usage 0x00f0, value 6 = unmuted, 262 (0x106) = muted. Decoded by `micdeck_duocast::muted_from_hid` (tested). `micdeck watch` prints "muted"/"unmuted" on each press.

## Meeting mode (current priority)
Goal: the owner cannot use the DuoCast in meetings because background voices are heard (cardioid/omni and app noise cancellation did not help; AirPods work). Must work in Meet, Teams and Zoom, so the fix is a system-wide virtual mic fed by MicDeck, not per-app settings. The mic hardware has no DSP/noise features to control.
Plan: (1) offline test on recorded WAVs [done, works well at strength 90]; (2) live processing [done via stdin/stdout, to be tested by owner]; (3) virtual mic (BlackHole first, own driver later); (4) `micdeck meeting on|off`. RNNoise (BSD) is a candidate dependency for fan/keyboard noise; it does NOT remove background speech. A speaker-isolation ML model is the real fix if the gate is not enough.
- `core/src/meeting.rs`: 100 Hz high-pass + downward expander (gate), plus minimal mono WAV read/write; 3 tests. CLI: `micdeck meeting <file.wav> [strength 0-100]` (default 60) writes `<file>.s<strength>.wav`. Strength is relative to the measured voice level (`threshold_for_strength`: 0 = voice-40 dB, 100 = voice-6 dB).
- Chain in meeting.rs (`Gate`): RNNoise (48 kHz only, +10 ms) -> high-pass 100 Hz -> gate -> 3:1 compressor (threshold 12 dB under voice level, +8 dB make-up scaled by how open the gate is) -> soft limiter. The gate measures loudness with a slow 30 ms rise and delays audio 30 ms, plus a 150 ms hold, so keyboard/mouse clicks (a few ms long) do not open it but speech does. Lesson: a fast gate plus unconditional make-up gain let background and typing through. Presence EQ is a possible next addition.
- Measured on the owner's real clip (`clips/typing.wav`: silence, typing, mouse clicks, speech; keyboard sits in front of the mic and the voice is quiet, only ~4 dB above typing, so level-only gating cannot separate typing): RNNoise + gate takes typing and clicks to silence (-120 dB) while speech drops <1 dB average. Level alone could not do it; RNNoise was the fix. Physical advice given: mic closer to mouth (stand/arm), keyboard away. Not yet tested: typing WHILE speaking, and by-ear quality.
- Measured on `clips/bgtalk.wav` (background talking only for ~7 s, then owner talking over it): background-only part drops from -54 dB avg to -66 dB at strength 60 (peaks still -53 dB, audible) and -95 dB at strength 90 (gone). While the owner talks, background cannot be separated (same loudness, the voice is only ~6-10 dB above the background at this mic distance); only a closer mic helps there. Decision: strength stays user-adjustable with default 60 for now; later, named modes (meeting, podcast, outside, ...) will each carry their own default strength and settings.
- Record test clips with ffmpeg (installed): `ffmpeg -f avfoundation -i ":HyperX DuoCast" -t 20 -ac 1 -ar 48000 -c:a pcm_s16le clip.wav`

- `core/src/stream.rs`: live Core Audio in -> LiveGate -> out (IOProc callbacks, mutex-guarded queue, both devices forced to 48 kHz). CLI: `micdeck devices` (list outputs), `micdeck meeting-run [strength] [output name]` (input is the DuoCast, output defaults to the current output). VERIFIED by the owner end to end with BlackHole 2ch as output: recorded `ffmpeg -f avfoundation -i ":BlackHole 2ch"` while `meeting-run 60 BlackHole` ran; result had clear voice and NO audible typing/mouse clicks (mic had been moved closer to the mouth, ~20 dB louder voice). Google Meet saw the BlackHole mic meter move. Install: `brew install blackhole-2ch`. `meeting-live` (stdin/stdout) was removed.

- `stream::Engine` (start/stop pipeline; `Drop` stops the devices and frees the shared data) is the API the future menu-bar app will use in-process. `micdeck meeting on [strength] [output]` / `off` / `status` (cli/src/background.rs) is only a thin shim: it starts `micdeck meeting-run` detached (setsid), keeps the pid in `$TMPDIR/micdeck-meeting.pid`, errors go to `$TMPDIR/micdeck-meeting.log`; `on` waits 1.5 s so start-up failures are shown immediately. Default output is "BlackHole". No restart after crash/reboot. Decision: NOT launchd now, because a bare launchd binary has no app identity so macOS may silently deny microphone access (needs an app bundle), and a daemon design would need an IPC protocol; revisit with the menu-bar app.

- Real-call feedback from the owner: with the mic close to the mouth it works as intended; with the mic on the desk near keyboard/mouse the listener heard clicks (little while talking); a quiet phone video was audible mildly while the owner talked and cut away in silence (expected: gate is open while talking). Listener said the audio was much better with the meeting filter on. Owner is content with testing so far; fine-tuning can come later. Mic position is the biggest lever; "close mic" vs "desk mic" could become named modes.

## Next step
Lighting protocol (decision made: own USB capture on the owner's Windows PC with NGENUITY + Wireshark + USBPcap, NOT reading QuadcastRGB source). Captures go in `captures/` (gitignored). Decode the .pcapng with a small Python script (USBPcap link type 249), work out the color packet, then implement `micdeck color <rrggbb>` through the DuoCast driver using IOKit HID (SetReport) if possible. After that: hotplug + saved settings, then the menu-bar app. Open questions: typing WHILE speaking, background talkers while speaking (need a closer mic), automatic output level.

## Later
Lighting protocol (read QuadcastRGB for facts only vs own Windows capture), hotplug detection, saved profiles.
