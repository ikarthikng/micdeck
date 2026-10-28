mod background;

/// The stop flag the command-line tool passes to long-running loops: Ctrl-C ends the process, so it never changes.
static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Reads the command line and runs the matching command. Every command returns `Result<(), String>`:
/// `Ok` means it worked, `Err(message)` is printed to the user and the program exits with code 1.
fn main() {
    // Skip the program name, then borrow each argument as `&str` so the `match` below can compare it with
    // literal text like "meeting".
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // `match args[..]` checks the shape of the argument list: e.g. `["meeting", file, n]` matches exactly three
    // arguments whose first is "meeting", and names the other two `file` and `n`.
    let result = match args[..] {
        ["watch"] => watch(),
        // Meeting mode in the background (these must come before the `meeting <file>` arms below).
        ["meeting", "on"] => on(None, None),
        ["meeting", "on", n] => percent(n, "strength").and_then(|n| on(Some(n), None)),
        ["meeting", "on", n, out] => percent(n, "strength").and_then(|n| on(Some(n), Some(out))),
        ["meeting", "off"] => background::MEETING.stop(),
        ["meeting", "status"] => background::MEETING.status(),
        ["meeting", file] => meeting(file, None),
        ["meeting", file, n] => percent(n, "strength").and_then(|n| meeting(file, Some(n))),
        ["devices"] => devices(),
        ["meeting-run"] => run_live(None, None),
        ["meeting-run", n] => percent(n, "strength").and_then(|n| run_live(Some(n), None)),
        ["meeting-run", n, out] => percent(n, "strength").and_then(|n| run_live(Some(n), Some(out))),
        ["settings"] => settings_show(),
        ["settings", "set", key, value] => settings_set(key, value),
        ["audio"] => audio(None, None),
        ["gain", v] => v.parse().map_err(|_| "gain must be 0 to 100".to_string()).and_then(|v: f32| audio(Some(v / 100.0), None)),
        // `m @ ("on" | "off")` matches only those two words and keeps the one that matched in `m`.
        ["color", hex] => color(hex, 100, false),
        ["color", hex, "hold"] => color(hex, 100, true),
        ["color", hex, n] => percent(n, "brightness").and_then(|n| color(hex, n, false)),
        ["color", hex, n, "hold"] => percent(n, "brightness").and_then(|n| color(hex, n, true)),
        // The lights in the background: a color or effect that keeps streaming after the terminal is closed.
        ["light", "on", name] => light_on(name, "ff0000", 100),
        ["light", "on", name, hex] => light_on(name, hex, 100),
        ["light", "on", name, hex, n] => percent(n, "brightness").and_then(|n| light_on(name, hex, n)),
        ["light", "off"] => background::LIGHT.stop(),
        ["light", "status"] => background::LIGHT.status(),
        ["effect", name] => effect(name, "ff0000", 100),
        ["effect", name, hex] => effect(name, hex, 100),
        ["effect", name, hex, n] => percent(n, "brightness").and_then(|n| effect(name, hex, n)),
        ["mute", m @ ("on" | "off")] => audio(None, Some(m == "on")),
        _ => Err("usage: micdeck devices | watch | audio | meeting on [strength 0-100] [output name] | meeting off | meeting status | meeting <file.wav> [strength 0-100] | meeting-run [strength 0-100] [output name] | color <rrggbb> [brightness 0-100] [hold] | effect <name> [rrggbb] [brightness 0-100] | light on <effect> [rrggbb] [brightness 0-100] | light off | light status | settings | settings set <strength|output> <value> | gain <0-100> | mute on|off".into()),
    };
    // Print errors on stderr (not stdout) so they do not mix with a command's normal output.
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// Turn a hex code such as `ff0000` (a leading `#` is allowed) into [red, green, blue].
fn parse_color(hex: &str) -> Result<[u8; 3], String> {
    let hex = hex.trim_start_matches('#');
    // Six hex digits = three bytes: red, green, blue.
    let value = (hex.len() == 6).then(|| u32::from_str_radix(hex, 16).ok()).flatten().ok_or("color must be six hex digits, like ff0000")?;
    let [_, r, g, b] = value.to_be_bytes();
    Ok([r, g, b])
}

/// Set the mic's light color from a hex code such as `ff0000` (red). The mic treats this as a live preview
/// that fades back after a moment; add `hold` to keep sending it until Ctrl-C. `brightness` is 0-100
/// (100 = the color as typed).
fn color(hex: &str, brightness: u8, hold: bool) -> Result<(), String> {
    use micdeck_core::Device;
    let rgb = micdeck_core::with_brightness(parse_color(hex)?, brightness);
    if hold {
        println!("holding #{hex} at {brightness}% brightness; press Ctrl-C to stop");
        return micdeck_duocast::DuoCast.hold_color(&NEVER, rgb);
    }
    micdeck_duocast::DuoCast.set_color(rgb)?;
    println!("color set to #{hex} at {brightness}% brightness (a preview: the mic goes back to its own default; use `hold` to keep it)");
    Ok(())
}

/// Start a lighting effect in the background (see `background.rs`). `solid` with a color is a steady color.
/// The name and color are checked here, so a typo is reported at once.
fn light_on(name: &str, hex: &str, brightness: u8) -> Result<(), String> {
    micdeck_core::effects::Effect::parse(name).ok_or_else(|| format!("unknown effect \"{name}\"; choose one of: {}", micdeck_core::effects::Effect::names()))?;
    parse_color(hex)?;
    background::LIGHT.start(&["effect", name, hex, &brightness.to_string()], &format!("{name}, brightness {brightness}%"))
}

/// Play a lighting effect until Ctrl-C. Breathing and Solid use the color; the other effects make their own.
fn effect(name: &str, hex: &str, brightness: u8) -> Result<(), String> {
    use micdeck_core::{effects, with_brightness};
    let effect = effects::Effect::parse(name).ok_or_else(|| format!("unknown effect \"{name}\"; choose one of: {}", effects::Effect::names()))?;
    let base = parse_color(hex)?;
    println!("playing {name} at {brightness}% brightness; press Ctrl-C to stop");
    micdeck_duocast::DuoCast.play(&NEVER, |t| {
        // The DuoCast has two lights; dim each one by the brightness setting.
        let zones = effects::frame(effect, t, base, 2);
        [with_brightness(zones[0], brightness), with_brightness(zones[1], brightness)]
    })
}

/// Parse a 0-100 setting (strength, brightness) typed on the command line. `what` names it in the error message.
fn percent(text: &str, what: &str) -> Result<u8, String> {
    text.parse().ok().filter(|&n| n <= 100).ok_or_else(|| format!("{what} must be a number from 0 to 100"))
}

/// Show (and optionally change) the DuoCast's recording gain and mute.
fn audio(gain: Option<f32>, mute: Option<bool>) -> Result<(), String> {
    // `?` after a call means: if it failed, return that error from this function right away. `find_input`
    // returns an Option (maybe nothing found), and `ok_or` turns "nothing" into an error message.
    let dev = micdeck_core::audio::find_input("duocast")?.ok_or("DuoCast audio device not found")?;
    if let Some(g) = gain {
        dev.set_gain(g)?;
    }
    if let Some(m) = mute {
        dev.set_muted(m)?;
    }
    // The DuoCast has no software gain (it is a knob on the mic), so gain may be missing.
    let gain = dev.gain().map_or("not available".into(), |g| format!("{:.0}%", g * 100.0));
    println!("{}: gain {gain}, muted: {}", dev.name, dev.muted()?);
    Ok(())
}

/// Clean a mono WAV recording and save it next to the original as `<name>.s<strength>.wav`.
fn meeting(file: &str, strength: Option<u8>) -> Result<(), String> {
    use micdeck_core::meeting::*;
    // What was typed, else the saved setting.
    let strength = match strength {
        Some(n) => n,
        None => micdeck_core::settings::Settings::load()?.strength,
    };
    let (rate, samples) = read_wav(&std::fs::read(file).map_err(|e| format!("{file}: {e}"))?)?;
    // Measure how loud your voice is in this recording, then place the gate relative to that, so the same
    // strength works whether you were close to the mic or far from it.
    let voice = voice_level_db(&samples, rate);
    let threshold = threshold_for_strength(voice, strength);
    println!("voice level {voice:.1} dB, strength {strength}, threshold {threshold:.1} dB");
    // Name the result after the strength so several strengths can be compared side by side.
    let out = format!("{}.s{strength}.wav", file.trim_end_matches(".wav"));
    std::fs::write(&out, write_wav(rate, &process(&samples, rate, threshold))).map_err(|e| format!("{out}: {e}"))?;
    println!("wrote {out}");
    Ok(())
}

/// List the audio devices you can use as `meeting-run`'s output.
fn devices() -> Result<(), String> {
    use micdeck_core::audio::*;
    let default = default_output()?.name;
    println!("outputs:");
    for d in output_devices()? {
        println!("  {}{}", d.name, if d.name == default { "   (current default)" } else { "" });
    }
    Ok(())
}

/// The strength and output device for meeting mode: what was typed on the command line, else what is saved.
fn meeting_options(strength: Option<u8>, output: Option<&str>) -> Result<(u8, String), String> {
    let saved = micdeck_core::settings::Settings::load()?;
    Ok((strength.unwrap_or(saved.strength), output.map_or(saved.output, str::to_string)))
}

/// Start meeting mode in the background (see `background.rs`).
fn on(strength: Option<u8>, output: Option<&str>) -> Result<(), String> {
    let (strength, output) = meeting_options(strength, output)?;
    background::MEETING.start(&["meeting-run", &strength.to_string(), &output], &format!("DuoCast -> {output}, strength {strength}"))
}

/// Clean the DuoCast's audio live in this terminal and play it into the output device (a virtual mic such as
/// "BlackHole 2ch" by default, or headphones for testing). If the mic or the output is unplugged it waits
/// and carries on when the device is back. Ctrl-C to stop.
fn run_live(strength: Option<u8>, output: Option<&str>) -> Result<(), String> {
    let (strength, output) = meeting_options(strength, output)?;
    println!("DuoCast -> {output}, strength {strength}. Ctrl-C to stop.");
    micdeck_core::stream::run(&NEVER, "duocast", &output, strength)
}

/// Show the saved settings and where they are stored.
fn settings_show() -> Result<(), String> {
    let settings = micdeck_core::settings::Settings::load()?;
    println!("strength = {}\noutput   = {}", settings.strength, settings.output);
    println!("(stored in {})", micdeck_core::settings::Settings::path()?.display());
    Ok(())
}

/// Change one saved setting.
fn settings_set(key: &str, value: &str) -> Result<(), String> {
    let mut settings = micdeck_core::settings::Settings::load()?;
    settings.set(key, value)?;
    settings.save()?;
    println!("{key} saved as {value}");
    Ok(())
}

/// Print "muted" / "unmuted" whenever the mic's mute button is pressed. Ctrl-C to stop.
fn watch() -> Result<(), String> {
    println!("listening; press the mic's mute button (Ctrl-C to stop)");
    // The closure `|page, usage, v| { ... }` runs every time the mic reports a change. Most changes are not
    // the mute button, and `muted_from_hid` returns `None` for those, so the `if let` ignores them.
    micdeck_core::hid::watch(micdeck_duocast::VENDOR_ID, micdeck_duocast::AUDIO_PRODUCT_ID, |page, usage, v| {
        if let Some(m) = micdeck_duocast::muted_from_hid(page, usage, v) {
            println!("{}", if m { "muted" } else { "unmuted" });
        }
    })
}
