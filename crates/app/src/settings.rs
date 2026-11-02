//! Preferences, saved in `~/.config/napkin/settings.toml`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Omarchy,
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    English,
    Italian,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Clock {
    #[serde(rename = "24h")]
    TwentyFourHour,
    #[serde(rename = "12h")]
    TwelveHour,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Decimals shown, 0 to 10.
    pub precision: u32,
    /// Text size in points, 10 to 32.
    pub font_size: u32,
    pub theme: Theme,
    pub format: Format,
    pub clock: Clock,
    /// Initial pixels per inch for CSS units.
    pub ppi: u32,
    /// Initial font size in pixels for `em`.
    pub em: u32,
    pub update_rates: bool,
    pub autostart: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            precision: 2,
            font_size: 14,
            theme: if crate::theme::omarchy_available() { Theme::Omarchy } else { Theme::System },
            format: system_format(),
            clock: Clock::TwentyFourHour,
            ppi: 96,
            em: 16,
            update_rates: true,
            autostart: false,
        }
    }
}

/// The decimal comma when the language picked in Omarchy writes one, or
/// without it the system's number locale; the decimal point otherwise.
fn system_format() -> Format {
    let language = crate::i18n::language_file().and_then(|path| crate::i18n::read_language(&path)).unwrap_or_else(|| {
        let locale = ["LC_ALL", "LC_NUMERIC", "LANG"]
            .iter()
            .filter_map(|key| std::env::var(key).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        crate::i18n::from_locale(&locale)
    });
    if crate::i18n::decimal_comma(&language) { Format::Italian } else { Format::English }
}

pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("napkin").join("settings.toml"))
}

pub fn load() -> Settings {
    let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else {
        return Settings::default();
    };
    let mut settings: Settings = toml::from_str(&text).unwrap_or_default();
    settings.precision = settings.precision.min(10);
    settings.font_size = settings.font_size.clamp(10, 32);
    settings.ppi = settings.ppi.max(1);
    settings.em = settings.em.max(1);
    settings
}

pub fn save(settings: &Settings) {
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = toml::to_string(settings) {
        let _ = std::fs::write(path, text);
    }
}
