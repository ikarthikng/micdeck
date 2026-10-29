# MicDeck

Open-source control software for USB mics that have no native macOS app. First target: HyperX DuoCast.

## What it does
- Lights for the HyperX DuoCast: effects, colors, brightness.
- Mute indicator that follows the mic's physical mute button.
- **Meeting mode:** cleans up the mic (background noise, typing, clicks) for any meeting app.

## Requirements
- A Mac with Apple Silicon.
- **Meeting mode needs BlackHole 2ch**, a free virtual audio device made by Existential Audio. It is a separate app: MicDeck does not include it, and neither installs nor maintains it. macOS only lets an audio driver, not an app, create a new microphone, and BlackHole is that driver. Install it with `brew install blackhole-2ch` (or from https://existential.audio/blackhole/), then in your meeting app choose **BlackHole 2ch** as the microphone.
- **Without BlackHole, MicDeck still works**: lights and the mute indicator are unaffected. Only Meeting mode is turned off (its switch is greyed out in the app). In your meeting app you then pick the normal mic.

Status: early development. Not affiliated with HyperX or HP.

License: GPL-3.0-only.
