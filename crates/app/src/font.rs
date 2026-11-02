//! The editor's font: fontconfig's `monospace`, which `omarchy font set`
//! changes for the terminal and the shell, so Napkin follows it too.
//! libadwaita would otherwise take GNOME's monospace setting, which Omarchy
//! leaves at Adwaita Mono.

use std::path::PathBuf;
use std::process::Command;

/// The family fontconfig gives for `monospace`, read the way
/// `omarchy font current` reads it. A child process sees a changed
/// configuration, which this process's own fontconfig would not.
pub fn monospace() -> Option<String> {
    let output = Command::new("fc-match").args(["monospace", "-f", "%{family}"]).output().ok()?;
    let families = String::from_utf8(output.stdout).ok()?;
    let family = families.split(',').next()?.trim();
    (!family.is_empty()).then(|| family.to_string())
}

/// The user's fontconfig file, which `omarchy font set` rewrites.
pub fn user_config() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("fontconfig").join("fonts.conf"))
}

/// A CSS `font-family` value: the family by name, then the generic one.
pub fn css_family(family: Option<&str>) -> String {
    match family {
        Some(name) => format!("\"{}\", monospace", name.replace('\\', "\\\\").replace('"', "\\\"")),
        None => "monospace".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_names_are_quoted() {
        assert_eq!(css_family(Some("JetBrainsMono Nerd Font")), "\"JetBrainsMono Nerd Font\", monospace");
        assert_eq!(css_family(Some("Odd \"Mono\"")), "\"Odd \\\"Mono\\\"\", monospace");
        assert_eq!(css_family(None), "monospace");
    }
}
