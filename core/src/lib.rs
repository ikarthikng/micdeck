//! Shared driver interface. A mic driver declares what it supports; frontends adapt.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Color,
    Brightness,
}

pub trait Device {
    fn name(&self) -> &str;
    fn capabilities(&self) -> &[Capability];
    fn set_color(&mut self, _rgb: [u8; 3]) -> Result<(), String> {
        Err("color not supported".into())
    }
}

#[cfg(target_os = "macos")]
pub mod audio;

pub mod fake;

#[cfg(target_os = "macos")]
pub mod hid;
