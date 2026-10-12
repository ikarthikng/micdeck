//! HyperX DuoCast driver.

pub const VENDOR_ID: u16 = 0x03f0;
/// The mic's audio device. Its HID interface carries the mute button (the 0x098c Controller does not).
pub const AUDIO_PRODUCT_ID: u16 = 0x0a8c;

/// Decode a HID value from the mic into "is it muted now?". None for anything else.
/// Seen on hardware: usage page 0xffc1, usage 0xf0, value 6 = unmuted, 262 (0x106) = muted.
pub fn muted_from_hid(page: u32, usage: u32, value: isize) -> Option<bool> {
    (page == 0xffc1 && usage == 0xf0).then_some(value & 0x100 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_mute_button() {
        assert_eq!(muted_from_hid(0xffc1, 0xf0, 6), Some(false));
        assert_eq!(muted_from_hid(0xffc1, 0xf0, 262), Some(true));
        assert_eq!(muted_from_hid(0x0001, 0x30, 262), None);
    }
}
