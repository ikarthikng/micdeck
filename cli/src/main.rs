use std::time::Duration;
use rusb::{Direction, TransferType};

const VID: u16 = 0x03f0;
const PID: u16 = 0x098c; // HyperX DuoCast Controller

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args[..] {
        ["inspect"] => inspect().map_err(|e| e.to_string()),
        ["watch"] => watch(),
        ["audio"] => audio(None, None),
        ["gain", v] => v.parse().map_err(|_| "gain must be 0 to 100".to_string()).and_then(|v: f32| audio(Some(v / 100.0), None)),
        ["mute", m @ ("on" | "off")] => audio(None, Some(m == "on")),
        _ => Err("usage: micdeck inspect | watch | audio | gain <0-100> | mute on|off".into()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// Show (and optionally change) the DuoCast's recording gain and mute.
fn audio(gain: Option<f32>, mute: Option<bool>) -> Result<(), String> {
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

/// Print "muted" / "unmuted" whenever the mic's mute button is pressed. Ctrl-C to stop.
fn watch() -> Result<(), String> {
    println!("listening; press the mic's mute button (Ctrl-C to stop)");
    micdeck_core::hid::watch(micdeck_duocast::VENDOR_ID, micdeck_duocast::AUDIO_PRODUCT_ID, |page, usage, v| {
        if let Some(m) = micdeck_duocast::muted_from_hid(page, usage, v) {
            println!("{}", if m { "muted" } else { "unmuted" });
        }
    })
}

fn inspect() -> Result<(), rusb::Error> {
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

    // HID report descriptor: standard GET_DESCRIPTOR (type 0x22) sent to each interface.
    // Needs an open handle; macOS may refuse if it blocks access to HID devices.
    let handle = dev.open()?;
    for iface in 0..cfg.num_interfaces() {
        let mut buf = [0u8; 1024];
        match handle.read_control(0x81, 0x06, 0x2200, iface as u16, &mut buf, Duration::from_secs(1)) {
            Ok(n) => {
                print!("report descriptor, interface {iface} ({n} bytes):");
                buf[..n].iter().for_each(|b| print!(" {b:02x}"));
                println!();
            }
            Err(e) => eprintln!("report descriptor, interface {iface}: {e}"),
        }
    }
    Ok(())
}
