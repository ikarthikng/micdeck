use std::time::Duration;
use rusb::{Direction, TransferType};

// USB ids used by `inspect`. Every USB device has a vendor id (03f0 = HP/HyperX) and a product id.
const VID: u16 = 0x03f0;
const PID: u16 = 0x098c; // HyperX DuoCast Controller

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
        ["inspect"] => inspect().map_err(|e| e.to_string()),
        ["watch"] => watch(),
        ["meeting", file] => meeting(file, 60),
        ["meeting", file, n] => n.parse().map_err(|_| "strength must be a number from 0 to 100".to_string()).and_then(|n| meeting(file, n)),
        ["devices"] => devices(),
        ["meeting-run"] => run_live(60, None),
        ["meeting-run", n] => n.parse().map_err(|_| "strength must be a number from 0 to 100".to_string()).and_then(|n| run_live(n, None)),
        ["meeting-run", n, out] => n.parse().map_err(|_| "strength must be a number from 0 to 100".to_string()).and_then(|n| run_live(n, Some(out))),
        ["audio"] => audio(None, None),
        ["gain", v] => v.parse().map_err(|_| "gain must be 0 to 100".to_string()).and_then(|v: f32| audio(Some(v / 100.0), None)),
        // `m @ ("on" | "off")` matches only those two words and keeps the one that matched in `m`.
        ["mute", m @ ("on" | "off")] => audio(None, Some(m == "on")),
        _ => Err("usage: micdeck inspect | devices | watch | audio | meeting <file.wav> [strength 0-100] | meeting-run [strength 0-100] [output device name] | gain <0-100> | mute on|off".into()),
    };
    // Print errors on stderr (not stdout) so they do not mix with a command's normal output.
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
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
fn meeting(file: &str, strength: u8) -> Result<(), String> {
    use micdeck_core::meeting::*;
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

/// Clean the DuoCast's audio live and play it into an output device (default: the current output, e.g. your
/// headphones; later a virtual mic such as "BlackHole 2ch"). Ctrl-C to stop.
fn run_live(strength: u8, output: Option<&str>) -> Result<(), String> {
    use micdeck_core::audio::*;
    let input = find_input("duocast")?.ok_or("DuoCast audio device not found")?;
    // Use the output the user named, or fall back to wherever macOS is currently playing sound.
    let output = match output {
        Some(name) => find_output(name)?.ok_or(format!("no output device matching \"{name}\""))?,
        None => default_output()?,
    };
    println!("{} -> {}, strength {strength}. Use headphones. Ctrl-C to stop.", input.name, output.name);
    micdeck_core::stream::run(&input, &output, strength)
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

/// Print the DuoCast Controller's USB layout: its interfaces, their endpoints, and the HID report
/// descriptors. This is how we found out where the mic's buttons and lighting commands live.
fn inspect() -> Result<(), rusb::Error> {
    // `let ... else` takes the device if found; otherwise the `else` block runs and must leave the function.
    let Some(dev) = rusb::devices()?
        .iter()
        .find(|d| d.device_descriptor().is_ok_and(|x| x.vendor_id() == VID && x.product_id() == PID))
    else {
        eprintln!("device {VID:04x}:{PID:04x} not found");
        std::process::exit(1);
    };

    // Reading descriptors needs no open handle, so no special permissions.
    let cfg = dev.active_config_descriptor()?;
    for iface in cfg.interfaces() {
        for d in iface.descriptors() {
            println!(
                "interface {} alt {}: class {:#04x} subclass {:#04x} protocol {:#04x}",
                d.interface_number(), d.setting_number(), d.class_code(), d.sub_class_code(), d.protocol_code()
            );
            for e in d.endpoint_descriptors() {
                let dir = if e.direction() == Direction::In { "IN" } else { "OUT" };
                let kind = match e.transfer_type() {
                    TransferType::Control => "control",
                    TransferType::Isochronous => "isochronous",
                    TransferType::Bulk => "bulk",
                    TransferType::Interrupt => "interrupt",
                };
                println!("  endpoint {:#04x} {dir} {kind} max_packet {}", e.address(), e.max_packet_size());
            }
        }
    }

    // The HID report descriptor is a small table in the device that says what each report's bytes mean.
    // We ask for it with the standard USB request GET_DESCRIPTOR (0x06), type 0x22 = HID report, sent to
    // each interface (0x81 means: device-to-host, standard request, addressed to an interface).
    // This needs an open handle to the device, which macOS allows even though its own driver owns the mic.
    let handle = dev.open()?;
    for iface in 0..cfg.num_interfaces() {
        let mut buf = [0u8; 1024];
        match handle.read_control(0x81, 0x06, 0x2200, iface as u16, &mut buf, Duration::from_secs(1)) {
            Ok(n) => {
                print!("report descriptor, interface {iface} ({n} bytes):");
                // Print each byte as two hex digits; `buf[..n]` is only the part the device filled in.
                buf[..n].iter().for_each(|b| print!(" {b:02x}"));
                println!();
            }
            Err(e) => eprintln!("report descriptor, interface {iface}: {e}"),
        }
    }
    Ok(())
}
