//! Lighting effects: functions that say what color each light should show at a given moment.
//!
//! They are plain math with no hardware involved, so they are easy to test. A driver calls `frame` about
//! 21 times a second and sends the colors to the mic. Breathing and Cycle copy measurements of NGENUITY's
//! effects (see "Lighting protocol" in CLAUDE.md); Confetti, Sun and Twilight are our own take on how those
//! effects look, not exact copies.

use std::f32::consts::PI;

/// The effects MicDeck can play, named like the ones on NGENUITY's Light Sync page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// One steady color.
    Solid,
    /// The chosen color swells from black to full brightness and back.
    Breathing,
    /// The color runs round the whole color wheel.
    Cycle,
    /// Soft random-looking colors that drift, a different one on each light.
    Confetti,
    /// Warm orange that flickers.
    Sun,
    /// A cool light and a warm light that trade places slowly.
    Twilight,
}

/// Every effect with the name used on the command line.
const NAMES: [(&str, Effect); 6] = [
    ("solid", Effect::Solid),
    ("breathing", Effect::Breathing),
    ("cycle", Effect::Cycle),
    ("confetti", Effect::Confetti),
    ("sun", Effect::Sun),
    ("twilight", Effect::Twilight),
];

impl Effect {
    /// Look an effect up by its name (`"breathing"`).
    pub fn parse(name: &str) -> Option<Effect> {
        NAMES.iter().find(|(n, _)| *n == name).map(|&(_, effect)| effect)
    }

    /// The names of all effects, for help and error messages.
    pub fn names() -> String {
        NAMES.map(|(name, _)| name).join(", ")
    }
}

/// Multiply a color by a number (0.0 = black, 1.0 = unchanged).
fn scale(color: [u8; 3], amount: f32) -> [u8; 3] {
    color.map(|c| (c as f32 * amount.clamp(0.0, 1.0)).round() as u8)
}

/// The fully saturated, full-brightness color at a position on the color wheel, in degrees
/// (0 = red, 120 = green, 240 = blue).
fn wheel(degrees: f32) -> [u8; 3] {
    let sector = degrees.rem_euclid(360.0) / 60.0; // which sixth of the wheel we are in, with a fraction
    let rising = 1.0 - ((sector % 2.0) - 1.0).abs(); // the middle colour channel's level inside that sixth
    let (r, g, b) = match sector as u32 {
        0 => (1.0, rising, 0.0),
        1 => (rising, 1.0, 0.0),
        2 => (0.0, 1.0, rising),
        3 => (0.0, rising, 1.0),
        4 => (rising, 0.0, 1.0),
        _ => (1.0, 0.0, rising),
    };
    [r, g, b].map(|level: f32| (level * 255.0).round() as u8)
}

/// The color of one light (`zone`) at `t` seconds after the effect started. `base` is the color the user
/// chose; Solid and Breathing use it, the others make their own colors.
fn color(effect: Effect, t: f32, base: [u8; 3], zone: usize) -> [u8; 3] {
    match effect {
        Effect::Solid => base,
        // Measured on NGENUITY: the brightness follows |sin|, peaking every 2.28 s and touching black in between.
        Effect::Breathing => scale(base, (PI * t / 2.28).sin().abs()),
        // Measured on NGENUITY: the hue turns 90 degrees a second, so a full circle takes 4 s.
        Effect::Cycle => wheel(90.0 * t),
        Effect::Confetti => {
            // Each of the three channels of each light wobbles up and down at its own slow speed, so the
            // lights wander through soft colors and never match each other. The numbers are arbitrary.
            let mut rgb = [0u8; 3];
            for (channel, out) in rgb.iter_mut().enumerate() {
                let period = 4.0 + channel as f32 * 1.3 + zone as f32 * 2.1;
                let wave = 0.5 + 0.5 * (2.0 * PI * t / period + (zone * 3 + channel) as f32 * 1.7).sin();
                *out = ((0.12 + 0.76 * wave) * 255.0).round() as u8;
            }
            rgb
        }
        // NGENUITY's Sun flickers between about ff7c00 and ffa300; we do the same once a second.
        Effect::Sun => [255, (144.0 + 20.0 * (2.0 * PI * t).sin()).round() as u8, 0],
        Effect::Twilight => {
            // A dim slate blue and a warm gold (both taken from NGENUITY's Twilight) fade in and out in
            // opposite turns, ten seconds per round trip.
            let colors = [[0x2a, 0x49, 0x57], [0xee, 0xae, 0x3d]];
            let level = 0.5 + 0.5 * (2.0 * PI * t / 10.0 + zone as f32 * PI).sin();
            scale(colors[zone % 2], level)
        }
    }
}

/// The colors of all `zones` lights at `t` seconds. A DuoCast has two.
pub fn frame(effect: Effect, t: f32, base: [u8; 3], zones: usize) -> Vec<[u8; 3]> {
    (0..zones).map(|zone| color(effect, t, base, zone)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 3] = [255, 0, 0];

    #[test]
    fn solid_is_always_the_chosen_color() {
        assert_eq!(frame(Effect::Solid, 3.7, [1, 2, 3], 2), vec![[1, 2, 3]; 2]);
    }

    #[test]
    fn breathing_swells_from_black_to_full_and_back() {
        assert_eq!(color(Effect::Breathing, 0.0, RED, 0), [0, 0, 0]); // dark at the start of a breath
        assert_eq!(color(Effect::Breathing, 1.14, RED, 0), RED); // full at the peak (half of 2.28 s)
        assert!(color(Effect::Breathing, 2.28, RED, 0)[0] < 5); // dark again after one breath
    }

    #[test]
    fn cycle_goes_round_the_wheel_in_four_seconds() {
        assert_eq!(color(Effect::Cycle, 0.0, RED, 0), [255, 0, 0]); // red
        assert_eq!(color(Effect::Cycle, 4.0 / 3.0, RED, 0), [0, 255, 0]); // 120 degrees on: green
        assert_eq!(color(Effect::Cycle, 8.0 / 3.0, RED, 0), [0, 0, 255]); // 240 degrees on: blue
        assert_eq!(color(Effect::Cycle, 4.0, RED, 0), [255, 0, 0]); // back to red
    }

    #[test]
    fn confetti_lights_differ_and_twilight_lights_differ() {
        for effect in [Effect::Confetti, Effect::Twilight] {
            let f = frame(effect, 2.5, RED, 2);
            assert_ne!(f[0], f[1], "{effect:?}");
        }
    }

    #[test]
    fn effects_can_be_found_by_name() {
        assert_eq!(Effect::parse("breathing"), Some(Effect::Breathing));
        assert_eq!(Effect::parse("disco"), None);
        assert!(Effect::names().contains("twilight"));
    }
}
