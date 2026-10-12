//! A pretend mic for tests and for building frontends without hardware.

use crate::{Capability, Device};

pub struct FakeMic {
    caps: Vec<Capability>,
    /// The last color it was told to show.
    pub color: Option<[u8; 3]>,
}

impl FakeMic {
    pub fn new(caps: &[Capability]) -> Self {
        Self { caps: caps.to_vec(), color: None }
    }
}

impl Device for FakeMic {
    fn name(&self) -> &str {
        "Fake Mic"
    }

    fn capabilities(&self) -> &[Capability] {
        &self.caps
    }

    fn set_color(&mut self, rgb: [u8; 3]) -> Result<(), String> {
        if !self.caps.contains(&Capability::Color) {
            return Err("color not supported".into());
        }
        self.color = Some(rgb);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_is_stored_when_supported() {
        let mut mic = FakeMic::new(&[Capability::Color]);
        mic.set_color([255, 0, 0]).unwrap();
        assert_eq!(mic.color, Some([255, 0, 0]));
    }

    #[test]
    fn color_is_rejected_when_unsupported() {
        let mut mic = FakeMic::new(&[]);
        assert!(mic.set_color([255, 0, 0]).is_err());
        assert_eq!(mic.color, None);
    }
}
