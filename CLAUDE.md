# MicDeck: project context

Open-source Mac control software for USB mics that lack a native macOS app. First target: HyperX DuoCast. Long term: many mics, one app.

The owner is new to Rust and hardware work and relies on Claude for standards and best practice. Explain steps simply.

## Decisions made
- **Name:** MicDeck (checked: free on Homebrew, crates.io, npm, PyPI; `micdeck.com` is taken; a small unrelated Windows repo `MicDeck` exists on GitHub). Trademark search not done.
- **License:** GPL-3.0-only. The goal is credit plus keeping improvements open. Commercial use is allowed. The `LICENSE` file holds the official GPLv3 text, downloaded from gnu.org.
- **Do not copy code from quadcastrgb or OpenRGB.** quadcastrgb is GPL-2.0-only, which cannot be combined into a GPL-3.0 project. Write our own Rust code from the protocol facts, and from our own USB captures where needed.
- **Never implement firmware flashing.**
- **Avoid "HyperX" in the product name** (trademark). Not affiliated with HyperX or HP.
- **Language:** Rust core and CLI. Native SwiftUI menu-bar app as the frontend later (lightest option on macOS).
- **Dependencies:** as few as possible. Prefer macOS built-in frameworks. The first dependency planned is `rusb` for USB.
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
- `micdeck` CLI (rusb, vendored libusb, no brew needed) prints 03f0:098c interfaces/endpoints. Result: interface 0 = HID, interrupt OUT 0x01 (8 bytes); interface 1 = HID, interrupt IN 0x82 (8 bytes, probably the mute/media keys). Interface 0's OUT endpoint is the likely lighting channel.

- CLI also reads HID report descriptors (works on macOS without claiming the interface). Interface 0 = vendor-defined (usage page 0xFF90, no report ID): 64-byte input report, 64-byte output report, and a feature report. The 64-byte output report is the likely lighting command. Note the endpoint max packet is only 8, so a report may be sent in chunks or via control transfer (SET_REPORT). Interface 1 = consumer keys (report ID 3, 16-bit usage) plus a vendor report ID 5 (7 bytes in).

- Core Audio module `core/src/audio.rs` (hand-written FFI, no dependencies, macOS only). CLI: `micdeck audio | mute on|off | gain <0-100>`. Tested: mute on/off works on the DuoCast. The DuoCast exposes NO software gain to macOS (only mute); gain is the physical knob. Gain code works for other mics (AirPods, webcam).
- DuoCast audio device is found by name ("duocast"); Core Audio does not expose USB IDs.
- 03f0:098c is listed in QuadcastRGB's supported devices (same protocol family as QuadCast S). Packet format is NOT documented outside its GPL-2.0 source; rule: facts only, no code copying. Decision pending: read-for-facts vs own Windows capture.

- `core/src/fake.rs`: `FakeMic` implements `Device` for tests/frontends without hardware (2 tests, `cargo test`).

- `core/src/hid.rs`: macOS IOHIDManager listener (hand-written FFI). Works without claiming the device and without sudo (raw USB claim and sudo both fail: macOS HID driver owns the interface). IMPORTANT: IOHIDManagerSetDeviceMatching must always be called (NULL = all devices), otherwise nothing matches. Keyboards need Input Monitoring permission for the terminal/VS Code app; mice and the mic do not.
- Physical mute button: Core Audio does NOT see it (mic mutes internally). It sends HID events from the AUDIO device (03f0:0a8c, not the 098c Controller): usage page 0xffc1, usage 0x00f0, value 6 = unmuted, 262 (0x106) = muted. Decoded by `micdeck_duocast::muted_from_hid` (tested). `micdeck watch` prints "muted"/"unmuted" on each press.

## Next step
Decide how to get the lighting protocol (read QuadcastRGB for facts only vs own Windows capture). Optional before that: hotplug detection, saved profiles.
