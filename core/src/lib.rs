//! Shared driver interface. A mic driver declares what it supports; frontends adapt.

/// Something a mic can do. A driver lists the capabilities its mic really has, and a frontend (CLI or app)
/// shows only those controls, so one app can serve many different mics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Color,
}

/// What every mic driver must provide. A "trait" is Rust's way of describing a set of methods that different
/// types can each implement, so the rest of the program can work with any mic without knowing which one.
pub trait Device {
    fn name(&self) -> &str;
    fn capabilities(&self) -> &[Capability];
    /// Set the light color as [red, green, blue], each 0 to 255. This default says "not supported"; a driver
    /// whose mic has lights replaces it. The underscore in `_rgb` tells Rust the unused name is deliberate.
    fn set_color(&mut self, _rgb: [u8; 3]) -> Result<(), String> {
        Err("color not supported".into())
    }
}

/// Dim a color: `percent` 100 leaves it as is, 50 makes it half as bright, 0 turns it off (black). The mic has
/// no brightness command of its own (NGENUITY just sends smaller color numbers), so brightness is applied here.
// ponytail: plain linear scaling; LEDs look non-linear to the eye, so low values may seem brighter than the
// number suggests. Add a gamma curve if that bothers anyone.
pub fn with_brightness(rgb: [u8; 3], percent: u8) -> [u8; 3] {
    let percent = percent.min(100) as u32;
    // Multiply, then divide by 100 with +50 so the result is rounded to the nearest number instead of rounded down.
    rgb.map(|channel| ((channel as u32 * percent + 50) / 100) as u8)
}

#[cfg(target_os = "macos")]
pub mod audio;

pub mod fake;

#[cfg(target_os = "macos")]
pub mod hid;

pub mod meeting;

#[cfg(target_os = "macos")]
pub mod stream;

#[cfg(test)]
mod tests {
    use super::with_brightness;

    #[test]
    fn brightness_scales_each_channel() {
        assert_eq!(with_brightness([255, 128, 0], 100), [255, 128, 0]); // unchanged
        assert_eq!(with_brightness([255, 128, 0], 50), [128, 64, 0]); // half, rounded to nearest
        assert_eq!(with_brightness([255, 128, 0], 0), [0, 0, 0]); // off
        assert_eq!(with_brightness([255, 128, 0], 200), [255, 128, 0]); // over 100 counts as 100
    }
}

pub mod settings;

pub mod effects;
