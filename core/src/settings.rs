//! Saved settings, kept in a small text file so the CLI and the future app share them.
//!
//! The file is `~/Library/Application Support/MicDeck/settings.txt`, one `key=value` per line. Blank lines
//! and lines starting with `#` are ignored, and so are keys this version does not know (a newer MicDeck may
//! add some). A missing file simply means "all defaults".

use std::path::PathBuf;

/// The settings MicDeck remembers between runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// How hard meeting mode removes background sound, 0-100.
    pub strength: u8,
    /// Part of the name of the device meeting mode plays into, such as "BlackHole".
    pub output: String,
}

/// The names of all settings, in the order they are written to the file.
const KEYS: [&str; 2] = ["strength", "output"];

impl Default for Settings {
    fn default() -> Self {
        Self { strength: 60, output: "BlackHole".into() }
    }
}

impl Settings {
    /// Where the settings file lives (the standard place for an app's files on macOS).
    pub fn path() -> Result<PathBuf, String> {
        let home = std::env::var_os("HOME").ok_or("the HOME environment variable is not set")?;
        Ok(PathBuf::from(home).join("Library/Application Support/MicDeck/settings.txt"))
    }

    /// Read the saved settings; defaults if nothing has been saved yet.
    pub fn load() -> Result<Self, String> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Write the settings, creating the folder the first time.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path()?;
        let write = || -> std::io::Result<()> {
            // `parent()` is the folder part of the path; `unwrap` is safe because the path always has one.
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(&path, self.to_text())
        };
        write().map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Turn the text of a settings file into `Settings`, starting from the defaults.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut settings = Self::default();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Split at the FIRST `=`, so a value may itself contain `=`.
            let (key, value) = line.split_once('=').ok_or_else(|| format!("line {}: expected key=value", number + 1))?;
            let key = key.trim();
            if KEYS.contains(&key) {
                settings.set(key, value.trim()).map_err(|e| format!("line {}: {e}", number + 1))?;
            }
        }
        Ok(settings)
    }

    /// Change one setting by name, checking the value.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "strength" => self.strength = value.parse().ok().filter(|&n| n <= 100).ok_or("strength must be a number from 0 to 100")?,
            "output" if value.is_empty() => return Err("output must not be empty".into()),
            "output" => self.output = value.to_string(),
            _ => return Err(format!("unknown setting \"{key}\" (known settings: {})", KEYS.join(", "))),
        }
        Ok(())
    }

    /// The text that `save` writes and `parse` reads back.
    pub fn to_text(&self) -> String {
        format!("strength={}\noutput={}\n", self.strength, self.output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_means_defaults() {
        assert_eq!(Settings::parse("").unwrap(), Settings::default());
    }

    #[test]
    fn reads_values_and_skips_comments_blank_lines_and_unknown_keys() {
        let s = Settings::parse("# my setup\n\nstrength = 80\nfuture_key=1\noutput=USB Advanced Audio Device\n").unwrap();
        assert_eq!(s, Settings { strength: 80, output: "USB Advanced Audio Device".into() });
    }

    #[test]
    fn rejects_bad_values_and_names_the_line() {
        assert!(Settings::parse("strength=101").unwrap_err().contains("line 1"));
        assert!(Settings::parse("strength=abc").is_err());
        assert!(Settings::parse("\noutput=").unwrap_err().contains("line 2"));
        assert!(Settings::parse("just text").is_err());
    }

    #[test]
    fn set_checks_the_key_and_round_trips_through_text() {
        let mut s = Settings::default();
        s.set("strength", "75").unwrap();
        s.set("output", "AirPods").unwrap();
        assert!(s.set("volume", "5").is_err());
        assert_eq!(Settings::parse(&s.to_text()).unwrap(), s);
    }
}
