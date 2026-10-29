<p align="center"><img src="app/Resources/AppIcon.png" alt="MicDeck logo" width="160"></p>

<h1 align="center">MicDeck</h1>

<p align="center">Open-source control software for USB mics that have no native macOS app.<br>First supported mic: HyperX DuoCast.</p>

> **Status: early development.** There is no downloadable release yet; see [Build from source](#build-from-source). MicDeck is not affiliated with HyperX or HP.

## What it does
MicDeck lives in your menu bar (Apple Silicon Macs, macOS 13 or newer).

| Feature | What you get |
|---|---|
| **Lights** | Effects (Solid, Breathing, Cycle, Confetti, Sun, Twilight), a color wheel, swatches and brightness for the DuoCast's two lights. |
| **Mute indicator** | The menu-bar icon and panel show Live or Muted, following the mic's physical mute button. |
| **Meeting mode** | Cleans your voice in real time: background noise, keyboard and mouse clicks, and quiet background voices are reduced. |

Everything runs on your Mac. MicDeck sends nothing over the network.

## How Meeting mode works
A meeting app (Meet, Zoom, Teams) can only use microphones that macOS lists. A normal app cannot add a new microphone to that list, so MicDeck cannot clean your audio "inside" the DuoCast. Instead:

```
 Your mic  ->  MicDeck (cleans it)  ->  BlackHole 2ch  ->  Meeting app
 (DuoCast)                              (shows up as a       (hears the
                                         new microphone)      cleaned voice)
```

**BlackHole 2ch** is a free virtual audio device. It is the "new microphone" that carries the cleaned sound to your meeting app.

To use it:
1. **Install BlackHole 2ch** once (see [Requirements](#requirements)).
2. In the MicDeck panel, switch **Meeting mode** on. **Your mic** is the real mic, and **Send to** should be **BlackHole 2ch**. The **Strength** slider sets how strongly background sound is removed (60 is a good start; raise it if the room is noisy).
3. In your meeting app, choose **BlackHole 2ch** as the microphone.
4. Keep MicDeck running during the call. Turning Meeting mode off, or quitting MicDeck, stops the cleanup, so switch your meeting app back to the normal mic first.

Tips: the biggest improvement comes from putting the mic close to your mouth and the keyboard away from it. The mic's mute button silences the mic, so the meeting hears nothing, but the meeting app does not know about it and will not show its muted icon; use the meeting app's own mute if you want others to see it.

## Requirements
- A Mac with Apple Silicon, macOS 13 or newer.
- For lights and the mute indicator: a HyperX DuoCast. Nothing else to install.
- **Only for Meeting mode: BlackHole 2ch**, a free virtual audio device made by Existential Audio. It is a separate app by another developer: MicDeck does not include it, install it, or maintain it. Read what it is first: [website](https://existential.audio/blackhole/), [source code and license](https://github.com/ExistentialAudio/BlackHole). Install it yourself only if you want Meeting mode, from its website or with `brew install blackhole-2ch`.
- **Without BlackHole, MicDeck still works.** Lights and the mute indicator are unaffected; only Meeting mode is switched off (greyed out in the app). In your meeting app you then pick your normal mic.

macOS may ask for permission to use the microphone the first time Meeting mode starts. MicDeck needs it to hear the mic it cleans.

## Build from source
You need Rust (via [rustup](https://rustup.rs)) and the Swift tools that come with Xcode or the Xcode command line tools.

```sh
git clone https://github.com/ikarthikng/micdeck.git
cd micdeck
cargo build --release -p micdeck-ffi    # the Rust library the app uses
cd app && swift build                    # or open app/Package.swift in Xcode and press Run
```

Look for the MicDeck icon in the menu bar.

To build the installer yourself, run `tools/build_dmg.sh`. It makes `dist/MicDeck-<version>-arm64.dmg` (Apple Silicon, ad-hoc signed). macOS asks you to confirm once when opening an app that is not signed with a paid Apple account; the `READ ME FIRST.txt` inside the .dmg explains how (System Settings > Privacy & Security > Open Anyway).

## Command line
The same features are available without the app. Build with `cargo build` and run `cargo run -p micdeck -- <command>`:

| Command | Does |
|---|---|
| `light on <effect> [rrggbb] [brightness]`, `light off`, `light status` | Lights in the background |
| `meeting on [strength] [output]`, `meeting off`, `meeting status` | Meeting mode in the background |
| `watch` | Prints Muted/Unmuted on each press of the mute button |
| `devices`, `audio`, `mute on\|off`, `gain <0-100>` | Audio devices and Core Audio controls |
| `settings`, `settings set <strength\|output> <value>` | Saved settings |

## Project layout
- `core/`: audio, cleanup (noise reduction), lighting effects, settings (Rust).
- `drivers/duocast/`: the DuoCast lighting protocol and mute-button decoding. One folder per supported mic.
- `ffi/`: the small C interface the app calls.
- `app/`: the SwiftUI menu-bar app.
- `cli/`: the `micdeck` command-line tool.
- `tools/`: helper scripts (artwork, USB capture decoder).

The lighting protocol was worked out from our own USB captures. We do not copy code from other projects with incompatible licenses.

## License
GPL-3.0-only. See [LICENSE](LICENSE).
