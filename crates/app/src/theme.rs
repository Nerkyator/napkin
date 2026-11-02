//! Colours: the active Omarchy theme when there is one, libadwaita otherwise.
//! Omarchy sets only light or dark for GTK apps, so the
//! palette is read from `~/.local/state/omarchy/current/theme/colors.toml`.

use gtk::gdk;
use std::path::PathBuf;

pub fn omarchy_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".local/state/omarchy/current"))
}

pub fn omarchy_colors() -> Option<PathBuf> {
    omarchy_dir().map(|dir| dir.join("theme").join("colors.toml"))
}

pub fn omarchy_available() -> bool {
    omarchy_colors().is_some_and(|path| path.exists())
}

/// The colours of an Omarchy theme.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub selection: String,
    pub muted: String,
    pub cyan: String,
    pub magenta: String,
    pub blue: String,
    pub yellow: String,
    /// Popovers and cards.
    pub surface: String,
    /// Header bar.
    pub header: String,
}

fn valid(color: &str) -> bool {
    gdk::RGBA::parse(color).is_ok()
}

pub fn load_omarchy() -> Option<Palette> {
    let text = std::fs::read_to_string(omarchy_colors()?).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    let get = |keys: &[&str]| {
        keys.iter()
            .filter_map(|key| table.get(*key).and_then(|v| v.as_str()))
            .find(|value| valid(value))
            .map(str::to_string)
    };
    let background = get(&["background", "color0"])?;
    let foreground = get(&["foreground", "color7"])?;
    let or_fg = |value: Option<String>| value.unwrap_or_else(|| foreground.clone());
    Some(Palette {
        dark: table.get("mode").and_then(|v| v.as_str()) != Some("light"),
        accent: or_fg(get(&["accent", "blue", "color4"])),
        selection: or_fg(get(&["selection", "selection_background", "color8"])),
        muted: or_fg(get(&["muted", "bright_black", "color8"])),
        cyan: or_fg(get(&["cyan", "color6"])),
        magenta: or_fg(get(&["magenta", "color5"])),
        blue: or_fg(get(&["blue", "color4"])),
        yellow: or_fg(get(&["yellow", "color3"])),
        surface: get(&["lighter_background"]).unwrap_or_else(|| background.clone()),
        header: get(&["dark_background"]).unwrap_or_else(|| background.clone()),
        foreground: foreground.clone(),
        background,
    })
}

/// libadwaita colour variables set from an Omarchy palette.
pub fn css(p: &Palette) -> String {
    format!(
        ":root {{
  --window-bg-color: {bg}; --window-fg-color: {fg};
  --view-bg-color: {bg}; --view-fg-color: {fg};
  --headerbar-bg-color: {header}; --headerbar-fg-color: {fg}; --headerbar-backdrop-color: {header};
  --popover-bg-color: {surface}; --popover-fg-color: {fg};
  --dialog-bg-color: {bg}; --dialog-fg-color: {fg};
  --card-bg-color: {surface}; --card-fg-color: {fg};
  --sidebar-bg-color: {header}; --sidebar-fg-color: {fg};
  --accent-bg-color: {accent}; --accent-fg-color: {bg}; --accent-color: {accent};
}}
textview.napkin text selection {{ background-color: {selection}; }}
",
        bg = p.background,
        fg = p.foreground,
        header = p.header,
        surface = p.surface,
        accent = p.accent,
        selection = p.selection,
    )
}

/// Colours of the editor: syntax highlighting and results.
#[derive(Clone, Debug, PartialEq)]
pub struct Colors {
    pub unit: gdk::RGBA,
    pub keyword: gdk::RGBA,
    pub function: gdk::RGBA,
    pub variable: gdk::RGBA,
    pub comment: gdk::RGBA,
    pub label: gdk::RGBA,
    pub result: gdk::RGBA,
    pub dim: gdk::RGBA,
    pub hover: gdk::RGBA,
}

fn rgba(color: &str) -> gdk::RGBA {
    gdk::RGBA::parse(color).unwrap_or(gdk::RGBA::BLACK)
}

fn with_alpha(color: gdk::RGBA, alpha: f32) -> gdk::RGBA {
    gdk::RGBA::new(color.red(), color.green(), color.blue(), alpha)
}

/// Contrast for comments and labels: WCAG AA for body text. Omarchy's `muted`
/// is meant for borders and falls well short of it in most themes.
const DIM_CONTRAST: f64 = 4.5;

/// Contrast for highlighted words and results: enough to fix the few theme
/// colours that cannot be read, without changing the others.
const TEXT_CONTRAST: f64 = 3.0;

/// Relative luminance, as WCAG defines it.
fn luminance(c: &gdk::RGBA) -> f64 {
    let linear = |v: f32| {
        let v = f64::from(v);
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(c.red()) + 0.7152 * linear(c.green()) + 0.0722 * linear(c.blue())
}

fn contrast(a: &gdk::RGBA, b: &gdk::RGBA) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn mix(a: &gdk::RGBA, b: &gdk::RGBA, t: f32) -> gdk::RGBA {
    let m = |x: f32, y: f32| x + (y - x) * t;
    gdk::RGBA::new(m(a.red(), b.red()), m(a.green(), b.green()), m(a.blue(), b.blue()), 1.0)
}

/// `color`, moved toward the foreground just enough to read on the background.
fn readable(color: &str, p: &Palette, min: f64) -> gdk::RGBA {
    let (color, fg, bg) = (rgba(color), rgba(&p.foreground), rgba(&p.background));
    (0..=20).map(|step| mix(&color, &fg, step as f32 / 20.0)).find(|c| contrast(c, &bg) >= min).unwrap_or(fg)
}

/// From the Omarchy palette, or a built-in one for light and dark with the
/// system accent for results.
pub fn colors(palette: Option<&Palette>, dark: bool, accent: gdk::RGBA) -> Colors {
    if let Some(p) = palette {
        let dim = readable(&p.muted, p, DIM_CONTRAST);
        return Colors {
            unit: readable(&p.cyan, p, TEXT_CONTRAST),
            keyword: readable(&p.magenta, p, TEXT_CONTRAST),
            function: readable(&p.blue, p, TEXT_CONTRAST),
            variable: readable(&p.yellow, p, TEXT_CONTRAST),
            comment: dim,
            label: dim,
            result: readable(&p.accent, p, TEXT_CONTRAST),
            dim,
            hover: with_alpha(rgba(&p.selection), 0.9),
        };
    }
    let pick = |dark_color: &str, light_color: &str| rgba(if dark { dark_color } else { light_color });
    Colors {
        unit: pick("#5fd7c4", "#0f7b6c"),
        keyword: pick("#dc8add", "#9141ac"),
        function: pick("#78aeed", "#1c71d8"),
        variable: pick("#f6d32d", "#986a44"),
        comment: pick("#9a9996", "#77767b"),
        label: pick("#9a9996", "#77767b"),
        result: accent,
        dim: pick("#9a9996", "#77767b"),
        hover: with_alpha(accent, 0.18),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Matte Black, where `muted` is #333333 on #121212.
    fn matte_black() -> Palette {
        let c = |s: &str| s.to_string();
        Palette {
            dark: true,
            background: c("#121212"),
            foreground: c("#bebebe"),
            accent: c("#e68e0d"),
            selection: c("#2a2a2a"),
            muted: c("#333333"),
            cyan: c("#bebebe"),
            magenta: c("#D35F5F"),
            blue: c("#e68e0d"),
            yellow: c("#b91c1c"),
            surface: c("#1e1e1e"),
            header: c("#0d0d0d"),
        }
    }

    #[test]
    fn dim_text_reads_on_the_background() {
        let p = matte_black();
        let colors = colors(Some(&p), true, gdk::RGBA::BLACK);
        let bg = rgba(&p.background);
        assert!(contrast(&colors.comment, &bg) >= DIM_CONTRAST);
        // Still dimmer than the text itself.
        assert!(contrast(&colors.comment, &bg) < contrast(&rgba(&p.foreground), &bg));
    }

    #[test]
    fn readable_colours_stay_as_they_are() {
        let p = matte_black();
        assert_eq!(readable(&p.accent, &p, TEXT_CONTRAST), rgba(&p.accent));
        assert_ne!(readable(&p.yellow, &p, TEXT_CONTRAST), rgba(&p.yellow));
    }
}
