//! HyperX DuoCast driver.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// USB vendor id (03f0 is HP, which owns HyperX).
pub const VENDOR_ID: u16 = 0x03f0;
/// The mic's audio device. Its HID interface carries the mute button (the 0x098c Controller does not).
pub const AUDIO_PRODUCT_ID: u16 = 0x0a8c;

/// The Controller, the DuoCast's second USB device. Its first HID interface takes the lighting commands.
pub const CONTROLLER_PRODUCT_ID: u16 = 0x098c;

/// The two 64-byte packets that make the mic show these colors, one for each of its two lights (zones).
/// Decoded from our own capture of NGENUITY (see "Lighting protocol" in CLAUDE.md):
///  - the first is a fixed header, `04 f2 ...` with a 1 in byte 8;
///  - the second is 14 entries of 4 bytes, `81 red green blue`, then zeros. Only the first two entries carry
///    a color, one per zone (in every frame of our captures, even the effects); the other twelve are
///    `81 00 00 00` (off).
pub fn color_packets(zones: [[u8; 3]; 2]) -> [[u8; 64]; 2] {
    let mut header = [0u8; 64];
    header[..9].copy_from_slice(&[0x04, 0xf2, 0, 0, 0, 0, 0, 0, 0x01]);
    let mut colors = [0u8; 64];
    // `chunks_exact_mut(4)` walks the packet 4 bytes at a time; `take(14)` stops after the 14 entries.
    for (i, entry) in colors.chunks_exact_mut(4).take(14).enumerate() {
        entry[0] = 0x81;
        if let Some(rgb) = zones.get(i) {
            entry[1..].copy_from_slice(rgb);
        }
    }
    [header, colors]
}

/// The HyperX DuoCast, as a `Device` the rest of MicDeck can control.
pub struct DuoCast;

/// Open the Controller over plain USB. The lighting interface has no input endpoint, so macOS never turns it
/// into a HID device and the HID layer cannot reach it. But every USB device answers requests on its
/// control endpoint, and that is how NGENUITY talks to it too.
fn open() -> Result<rusb::DeviceHandle<rusb::GlobalContext>, String> {
    rusb::open_device_with_vid_pid(VENDOR_ID, CONTROLLER_PRODUCT_ID).ok_or_else(|| "DuoCast Controller (03f0:098c) not found; is it plugged in?".into())
}

/// Send one color update (the two packets, one color per zone) to an open Controller.
fn send(handle: &rusb::DeviceHandle<rusb::GlobalContext>, zones: [[u8; 3]; 2]) -> Result<(), String> {
    for packet in color_packets(zones) {
        // HID SET_REPORT, the request NGENUITY sends: 0x21 = host-to-device, class request, to an
        // interface; 0x09 = SET_REPORT; 0x0300 = report type 3 (feature) with report id 0; 0 = interface 0.
        handle
            .write_control(0x21, 0x09, 0x0300, 0, &packet, Duration::from_secs(1))
            .map_err(|e| format!("could not send the color to the mic: {e}"))?;
    }
    Ok(())
}

impl DuoCast {
    /// Keep the mic showing `rgb` on both lights until `stop` is set to true.
    pub fn hold_color(&self, stop: &AtomicBool, rgb: [u8; 3]) -> Result<(), String> {
        self.play(stop, |_| [rgb, rgb])
    }

    /// Play lighting until `stop` is set to true (a command-line tool that stops with Ctrl-C passes a flag that
    /// never changes; the app sets it from another thread). `colors_at(t)` says what the two lights should show `t`
    /// seconds after the start (an effect, for instance). The mic treats a color update as a live preview and
    /// returns to its own default soon after the updates stop, so we send one about 21 times a second (every
    /// 47 ms), the rate NGENUITY uses in our capture.
    ///
    /// If the mic is unplugged while playing, it waits and carries on when the mic is back. A mic that is
    /// missing right at the start is an error instead. Progress messages go to stderr.
    pub fn play(&self, stop: &AtomicBool, mut colors_at: impl FnMut(f32) -> [[u8; 3]; 2]) -> Result<(), String> {
        let start = std::time::Instant::now();
        let mut first_try = true;
        loop {
            match open() {
                Ok(handle) => {
                    first_try = false;
                    while !stop.load(Ordering::Relaxed) {
                        if let Err(e) = send(&handle, colors_at(start.elapsed().as_secs_f32())) {
                            eprintln!("lost the mic ({e}); waiting for it to come back");
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(47));
                    }
                }
                Err(e) if first_try => return Err(e),
                Err(_) => {} // not back yet
            }
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}

impl micdeck_core::Device for DuoCast {
    fn name(&self) -> &str {
        "HyperX DuoCast"
    }

    fn capabilities(&self) -> &[micdeck_core::Capability] {
        &[micdeck_core::Capability::Color]
    }

    /// Show a color once. It is a live preview, so the mic goes back to its own default after a moment;
    /// use `DuoCast::hold_color` to keep it.
    fn set_color(&mut self, rgb: [u8; 3]) -> Result<(), String> {
        send(&open()?, [rgb, rgb])
    }
}

/// A HID "usage" is a pair of numbers that name a control: a page (the category) and a usage within it.
/// Page 0xffc1 is vendor-defined, meaning HyperX picked these numbers themselves.
///
/// Decode a HID value from the mic into "is it muted now?". None for anything else.
/// Seen on hardware: usage page 0xffc1, usage 0xf0, value 6 = unmuted, 262 (0x106) = muted.
pub fn muted_from_hid(page: u32, usage: u32, value: isize) -> Option<bool> {
    // `then_some` gives `Some(..)` when the condition is true and `None` otherwise. `value & 0x100` looks at
    // one bit of the number: set (262 = 0x106) means muted, clear (6 = 0x006) means unmuted.
    (page == 0xffc1 && usage == 0xf0).then_some(value & 0x100 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turn a hex string like "81 ff" or "81ff" into bytes, zero-padded to a full 64-byte packet.
    fn packet(hex: &str) -> [u8; 64] {
        let hex: String = hex.split_whitespace().collect();
        let mut out = [0u8; 64];
        for (i, byte) in out.iter_mut().enumerate().take(hex.len() / 2) {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    // The expected bytes below are the packets NGENUITY really sent in our capture, so these tests fail
    // if our packets ever differ from the real ones.
    #[test]
    fn color_packets_match_what_ngenuity_sent() {
        let ff0000 = "81ff000081ff0000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000";
        let blue = "810068ff810068ff 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000";
        let header = packet("04f2000000000000 01");
        assert_eq!(color_packets([[0xff, 0, 0]; 2]), [header, packet(ff0000)]);
        assert_eq!(color_packets([[0x00, 0x68, 0xff]; 2]), [header, packet(blue)]);
    }

    #[test]
    fn each_zone_gets_its_own_color() {
        let both = "81010203 81040506 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000 81000000";
        assert_eq!(color_packets([[1, 2, 3], [4, 5, 6]])[1], packet(both));
    }

    #[test]
    fn decodes_mute_button() {
        assert_eq!(muted_from_hid(0xffc1, 0xf0, 6), Some(false));
        assert_eq!(muted_from_hid(0xffc1, 0xf0, 262), Some(true));
        assert_eq!(muted_from_hid(0x0001, 0x30, 262), None);
    }
}
