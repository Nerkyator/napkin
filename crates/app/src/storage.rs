//! The last sheet, kept between launches.

use crate::i18n::tr;
use std::path::PathBuf;

/// The first sheet, in the interface language. The calculations stay in
/// English, the language every sheet is written in.
pub fn welcome() -> String {
    welcome_in(tr)
}

fn welcome_in(tr: impl Fn(&str) -> String) -> String {
    format!(
        "# {}\n// {}\n20 USD in EUR\n2 m + 35 cm\ntoday + 10 days\ntime in Tokyo\n// {}\n{}: €850\nprice = $40 - 15%\nprice * 3\n",
        tr("Welcome to Napkin"),
        tr("One calculation per line; results appear on the right. Click one to copy it."),
        tr("A label ends with a colon; a variable, set with =, can be reused below."),
        tr("Rent"),
    )
}

pub fn path() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("napkin").join("sheet.txt"))
}

pub fn load() -> Option<String> {
    std::fs::read_to_string(path()?).ok()
}

pub fn save(text: &str) -> std::io::Result<()> {
    let path = path().ok_or_else(|| std::io::Error::other("no data directory"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("txt.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, &path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use napkin_engine::LineKind;

    /// A translation that turned a comment or the label into a calculation
    /// would greet a new user with errors.
    #[test]
    fn the_first_sheet_reads_the_same_in_every_language() {
        let ctx = napkin_engine::Context::new(chrono::Utc::now(), napkin_engine::Zone::Named(chrono_tz::UTC));
        for code in crate::i18n::languages() {
            let sheet = napkin_engine::evaluate(&welcome_in(|english| crate::i18n::tr_in(code, english)), &ctx);
            let kinds: Vec<LineKind> = sheet.lines.iter().map(|line| line.kind).collect();
            assert_eq!(&kinds[..2], [LineKind::Header, LineKind::Comment], "{code}");
            assert_eq!(kinds[6], LineKind::Comment, "{code}");
            let results: Vec<Option<&str>> = sheet.lines[7..10].iter().map(|line| line.text.as_deref()).collect();
            assert_eq!(results, [Some("€850.00"), Some("$34.00"), Some("$102.00")], "{code}");
        }
    }
}
