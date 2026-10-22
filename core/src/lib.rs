//! Shared driver interface. A mic driver declares what it supports; frontends adapt.

/// Something a mic can do. A driver lists the capabilities its mic really has, and a frontend (CLI or app)
/// shows only those controls, so one app can serve many different mics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Color,
    Brightness,
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

#[cfg(target_os = "macos")]
pub mod audio;

pub mod fake;

#[cfg(target_os = "macos")]
pub mod hid;

pub mod meeting;

#[cfg(target_os = "macos")]
pub mod stream;
