//! The interface language: the one picked under Omarchy's Setup > Language,
//! followed while Napkin runs, or outside Omarchy the system's.
//!
//! Catalogs are flat JSON objects from the English text to its translation,
//! one per Omarchy language code (`i18n/it.json`, `i18n/pt-BR.json`), compiled
//! into the binary; `i18n/strings.txt` lists the English. A string missing
//! from a catalog shows in English. What the user writes stays English: the
//! keywords, units and functions of a sheet are the same in every language.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

macro_rules! catalogs {
    ($($code:literal),* $(,)?) => {
        &[$(($code, include_str!(concat!("../i18n/", $code, ".json")))),*]
    };
}

/// Every language Omarchy offers but English, the language of the source.
const CATALOGS: &[(&str, &str)] = catalogs![
    "ar", "bg", "bn", "ca", "cs", "da", "de", "el", "es-MX", "et", "fa", "fi", "fil", "fr", "ga", "he", "hi",
    "hr", "hu", "id", "is", "it", "ja", "ko", "lt", "lv", "ms", "nl", "no", "pl", "pt", "pt-BR", "ro", "ru",
    "si", "sk", "sl", "sr", "sv", "sw", "ta", "th", "tr", "uk", "ur", "uz", "vi", "zh-CN", "zh-TW",
];

/// The languages written right to left, from Omarchy's registry.
const RIGHT_TO_LEFT: &[&str] = &["ar", "fa", "he", "ur"];

/// The languages that write a decimal comma, from Omarchy's registry, and
/// Spanish outside Mexico for system locales.
const DECIMAL_COMMA: &[&str] = &[
    "bg", "ca", "cs", "da", "de", "el", "es", "et", "fi", "fr", "hr", "hu", "id", "is", "it", "lt", "lv", "nl",
    "no", "pl", "pt", "pt-BR", "ro", "ru", "sk", "sl", "sr", "sv", "tr", "uk", "uz", "vi",
];

static CURRENT: RwLock<Option<Language>> = RwLock::new(None);

/// Where Omarchy keeps the language picked under Setup > Language, found the
/// way `omarchy-language` finds it, test override included.
pub fn language_file() -> Option<PathBuf> {
    match std::env::var_os("OMARCHY_LANGUAGE_FILE") {
        Some(path) if !path.is_empty() => Some(path.into()),
        _ => dirs::config_dir().map(|dir| dir.join("omarchy").join("language")),
    }
}

/// The code on the first line of Omarchy's language file, or None without
/// the file. Anything but a code means English, as it does for Omarchy.
pub fn read_language(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let code = text.lines().next().unwrap_or_default().trim();
    let (language, region) = code.split_once('-').unwrap_or((code, ""));
    let valid = (2..=3).contains(&language.len())
        && language.bytes().all(|b| b.is_ascii_lowercase())
        && (region.is_empty() && !code.ends_with('-')
            || region.len() == 2 && region.bytes().all(|b| b.is_ascii_alphabetic()));
    Some(if valid { code.to_string() } else { "en".to_string() })
}

/// A POSIX locale as an Omarchy code: `pt_BR.UTF-8` is `pt-BR`, `nb_NO` is
/// `no`, and `C` is English. `LANGUAGE` lists several; the first counts.
pub fn from_locale(locale: &str) -> String {
    let first = locale.split(':').next().unwrap_or_default();
    let name = first.split(['.', '@']).next().unwrap_or_default();
    let (language, region) = name.split_once('_').unwrap_or((name, ""));
    let language = match language.to_ascii_lowercase().as_str() {
        "" | "c" | "posix" => return "en".to_string(),
        "nb" | "nn" => "no".to_string(),
        other => other.to_string(),
    };
    if region.is_empty() { language } else { format!("{language}-{}", region.to_ascii_uppercase()) }
}

/// The language of the system's messages, for when Omarchy has none set.
fn system_language() -> String {
    let locale = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    from_locale(&locale)
}

/// Starts GTK and libadwaita, whose own strings (Cut, Paste, the dialogs'
/// buttons) come from gettext, in Omarchy's language even where the session's
/// `LANG` has not caught up with it: gettext reads `LANGUAGE` first. Those
/// strings keep it until Napkin restarts, as Omarchy says of running apps.
///
/// # Safety
///
/// Sets an environment variable, so no other thread may be running.
pub unsafe fn export_to_gettext() {
    let Some(code) = language_file().and_then(|path| read_language(&path)) else { return };
    let gettext = if code == "no" { "nb".to_string() } else { code.replace('-', "_") };
    // SAFETY: the caller guarantees this is the only thread.
    unsafe { std::env::set_var("LANGUAGE", gettext) };
}

/// The language to show now: Omarchy's, or the system's without it.
pub fn wanted() -> String {
    language_file().and_then(|path| read_language(&path)).unwrap_or_else(system_language)
}

/// `pt` for `pt-BR`.
fn base(code: &str) -> &str {
    code.split('-').next().unwrap_or(code)
}

/// Looks a language up in a table by its code, then by its base language.
fn listed(table: &[&str], code: &str) -> bool {
    table.contains(&code) || table.contains(&base(code))
}

pub fn right_to_left(code: &str) -> bool {
    listed(RIGHT_TO_LEFT, code)
}

pub fn decimal_comma(code: &str) -> bool {
    code != "es-MX" && listed(DECIMAL_COMMA, code)
}

/// The language the interface is in.
pub fn current() -> String {
    CURRENT.read().ok().and_then(|current| current.as_ref().map(|l| l.code.clone())).unwrap_or_else(|| "en".into())
}

/// Puts the interface in a language; the next strings asked for take it.
pub fn set(code: &str) {
    let language = Language::new(code, CATALOGS);
    if let Ok(mut current) = CURRENT.write() {
        *current = Some(language);
    }
}

/// A string from `strings.txt` in the interface language.
pub fn tr(english: &str) -> String {
    CURRENT.read().ok().and_then(|current| current.as_ref().map(|l| l.tr(english))).unwrap_or_else(|| english.into())
}

/// A message from the engine in the interface language.
pub fn tr_message(message: &str) -> String {
    CURRENT
        .read()
        .ok()
        .and_then(|current| current.as_ref().map(|l| l.tr_message(message)))
        .unwrap_or_else(|| message.into())
}

/// Every language with a catalog.
#[cfg(test)]
pub fn languages() -> impl Iterator<Item = &'static str> {
    CATALOGS.iter().map(|(code, _)| *code)
}

/// A string in a given language, leaving the interface's alone.
#[cfg(test)]
pub fn tr_in(code: &str, english: &str) -> String {
    Language::new(code, CATALOGS).tr(english)
}

struct Language {
    code: String,
    strings: HashMap<String, String>,
    /// The entries with placeholders, in a fixed order, for engine messages.
    templates: Vec<(String, String)>,
}

impl Language {
    /// The base language's catalog, then the regional one over it (`pt`,
    /// then `pt-BR`), so a missing string falls back toward English one step
    /// at a time. A language with only regional catalogs, such as Spanish
    /// for a system in `es_ES`, takes the first.
    fn new(code: &str, catalogs: &[(&str, &str)]) -> Language {
        let find = |wanted: &str| catalogs.iter().find(|(c, _)| *c == wanted).map(|(_, text)| *text);
        let prefix = format!("{}-", base(code));
        let first_regional = || catalogs.iter().find(|(c, _)| c.starts_with(&prefix)).map(|(_, text)| *text);
        let general = find(base(code)).or_else(first_regional);
        let regional = if base(code) == code { None } else { find(code) };
        let mut strings = HashMap::new();
        for text in [general, regional].into_iter().flatten() {
            match serde_json::from_str::<HashMap<String, String>>(text) {
                Ok(catalog) => strings.extend(catalog.into_iter().filter(|(_, value)| !value.is_empty())),
                Err(e) => eprintln!("napkin: the {code} catalog is broken: {e}"),
            }
        }
        let mut templates: Vec<(String, String)> =
            strings.iter().filter(|(key, _)| key.contains('{')).map(|(k, v)| (k.clone(), v.clone())).collect();
        templates.sort();
        Language { code: code.to_string(), strings, templates }
    }

    fn tr(&self, english: &str) -> String {
        self.strings.get(english).cloned().unwrap_or_else(|| english.to_string())
    }

    /// Messages that carry a value, such as `no exchange rate for XYZ`, match
    /// their catalog entry (`no exchange rate for {from}`) around it.
    fn tr_message(&self, message: &str) -> String {
        if let Some(translation) = self.strings.get(message) {
            return translation.clone();
        }
        self.templates
            .iter()
            .find_map(|(key, translation)| {
                let values = fill(key, message)?;
                let filled = values.iter().fold(translation.clone(), |text, (name, value)| {
                    text.replace(&format!("{{{name}}}"), value)
                });
                Some(filled)
            })
            .unwrap_or_else(|| message.to_string())
    }
}

/// The values a message gives the placeholders of a template, or None when
/// the message does not have the template's shape.
fn fill<'a, 'b>(template: &'a str, message: &'b str) -> Option<Vec<(&'a str, &'b str)>> {
    let (head, mut template) = template.split_once('{')?;
    let mut rest = message.strip_prefix(head)?;
    let mut values = Vec::new();
    loop {
        let (name, after) = template.split_once('}')?;
        let (literal, next) = after.split_once('{').map_or((after, None), |(l, n)| (l, Some(n)));
        let value = match next {
            None if literal.is_empty() => std::mem::take(&mut rest),
            None => std::mem::take(&mut rest).strip_suffix(literal)?,
            // Two placeholders side by side could split a value anywhere.
            Some(_) if literal.is_empty() => return None,
            Some(_) => {
                let (value, after) = rest.split_once(literal)?;
                rest = after;
                value
            }
        };
        if value.is_empty() {
            return None;
        }
        values.push((name, value));
        match next {
            Some(next) => template = next,
            None => return Some(values),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const SOURCE: &str = include_str!("../i18n/strings.txt");

    /// Every Rust file a translated string can come from.
    const CODE: &[&str] = &[
        include_str!("prefs.rs"),
        include_str!("storage.rs"),
        include_str!("window.rs"),
        include_str!("editor.rs"),
        include_str!("../../engine/src/eval.rs"),
        include_str!("../../engine/src/sheet.rs"),
        include_str!("../../rates/src/lib.rs"),
    ];

    fn source() -> BTreeSet<&'static str> {
        SOURCE.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).collect()
    }

    fn placeholders(text: &str) -> BTreeSet<&str> {
        text.split('{').skip(1).filter_map(|part| part.split_once('}').map(|(name, _)| name)).collect()
    }

    #[test]
    fn every_catalog_translates_every_string() {
        let source = source();
        let mut problems = Vec::new();
        for (code, text) in CATALOGS {
            let catalog: HashMap<String, String> = match serde_json::from_str(text) {
                Ok(catalog) => catalog,
                Err(e) => {
                    problems.push(format!("{code}: {e}"));
                    continue;
                }
            };
            for english in &source {
                match catalog.get(*english) {
                    None => problems.push(format!("{code}: missing `{english}`")),
                    Some(t) if t.trim().is_empty() => problems.push(format!("{code}: empty `{english}`")),
                    Some(t) if placeholders(t) != placeholders(english) => {
                        problems.push(format!("{code}: `{t}` does not keep the placeholders of `{english}`"))
                    }
                    Some(_) => {}
                }
            }
            for key in catalog.keys().filter(|key| !source.contains(key.as_str())) {
                problems.push(format!("{code}: `{key}` is not in strings.txt"));
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    /// A string changed in the code would silently show in English.
    #[test]
    fn every_listed_string_is_in_the_code() {
        let missing: Vec<_> =
            source().into_iter().filter(|s| !CODE.iter().any(|code| code.contains(&format!("\"{s}\"")))).collect();
        assert!(missing.is_empty(), "not in the code: {missing:?}");
    }

    #[test]
    fn every_translated_string_is_listed() {
        let source = source();
        let mut unlisted = Vec::new();
        for code in CODE {
            for (i, _) in code.match_indices("tr(\"") {
                if code[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let literal = &code[i + 4..];
                let literal = &literal[..literal.find('"').unwrap_or(0)];
                if !source.contains(literal) {
                    unlisted.push(literal);
                }
            }
        }
        assert!(unlisted.is_empty(), "not in strings.txt: {unlisted:?}");
    }

    #[test]
    fn the_language_file_holds_a_code() {
        let dir = std::env::temp_dir().join(format!("napkin-i18n-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("language");
        let read = |text: &str| {
            std::fs::write(&file, text).unwrap();
            read_language(&file).unwrap()
        };
        assert_eq!(read("it\n"), "it");
        assert_eq!(read("pt-BR\n"), "pt-BR");
        assert_eq!(read("zh-TW"), "zh-TW");
        assert_eq!(read(""), "en");
        assert_eq!(read("Italiano\n"), "en");
        assert_eq!(read("it-\n"), "en");
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(read_language(&file), None);
    }

    #[test]
    fn locales_become_omarchy_codes() {
        assert_eq!(from_locale("it_IT.UTF-8"), "it-IT");
        assert_eq!(from_locale("pt_BR.UTF-8"), "pt-BR");
        assert_eq!(from_locale("nb_NO.UTF-8"), "no-NO");
        assert_eq!(from_locale("de"), "de");
        assert_eq!(from_locale("sr_RS@latin"), "sr-RS");
        assert_eq!(from_locale("fr_CA:fr:en"), "fr-CA");
        assert_eq!(from_locale("C.UTF-8"), "en");
        assert_eq!(from_locale(""), "en");
    }

    #[test]
    fn regional_catalogs_lay_over_the_base_language() {
        let catalogs: &[(&str, &str)] = &[
            ("pt", r#"{"Total": "Total", "Copied": "Copiado", "Data": ""}"#),
            ("pt-BR", r#"{"Copied": "Copiada"}"#),
            ("es-MX", r#"{"Total": "Suma"}"#),
        ];
        let brazil = Language::new("pt-BR", catalogs);
        assert_eq!(brazil.tr("Copied"), "Copiada");
        assert_eq!(brazil.tr("Total"), "Total");
        assert_eq!(brazil.tr("Data"), "Data");
        assert_eq!(brazil.tr("Quit"), "Quit");
        assert_eq!(Language::new("pt-PT", catalogs).tr("Copied"), "Copiado");
        assert_eq!(Language::new("es-ES", catalogs).tr("Total"), "Suma");
        assert_eq!(Language::new("en", catalogs).tr("Total"), "Total");
    }

    #[test]
    fn engine_messages_keep_their_values() {
        let catalogs: &[(&str, &str)] = &[(
            "it",
            r#"{
                "division by zero": "divisione per zero",
                "no exchange rate for {from}": "nessun cambio per {from}",
                "{a} and {b} measure different things": "{a} e {b} misurano cose diverse",
                "{name} is a label, not a variable: to reuse it, write {name} = …": "{name} è un'etichetta: scrivi {name} = …"
            }"#,
        )];
        let italian = Language::new("it", catalogs);
        assert_eq!(italian.tr_message("division by zero"), "divisione per zero");
        assert_eq!(italian.tr_message("no exchange rate for XAU"), "nessun cambio per XAU");
        assert_eq!(italian.tr_message("kg and m measure different things"), "kg e m misurano cose diverse");
        assert_eq!(
            italian.tr_message("Rent is a label, not a variable: to reuse it, write Rent = …"),
            "Rent è un'etichetta: scrivi Rent = …"
        );
        assert_eq!(italian.tr_message("something new"), "something new");
    }

    /// Real failures, with and without a value in the message, reach every
    /// language: a reworded engine message would fall back to English.
    #[test]
    fn the_engine_explains_itself_in_every_language() {
        let ctx = napkin_engine::Context::new(chrono::Utc::now(), napkin_engine::Zone::Named(chrono_tz::UTC));
        let sheet = napkin_engine::evaluate("5 / 0\n3 kg + 2 m\n100 USD in EUR\nRent: 5\nRent * 2\nzz * 2", &ctx);
        let errors: Vec<&str> = sheet.lines.iter().filter_map(|line| line.error.as_deref()).collect();
        assert_eq!(errors.len(), 5, "{errors:?}");
        for (code, _) in CATALOGS {
            let language = Language::new(code, CATALOGS);
            for error in &errors {
                let translated = language.tr_message(error);
                assert_ne!(&translated, error, "{code}");
                assert!(!translated.contains('{'), "{code}: {translated}");
            }
        }
    }

    #[test]
    fn templates_need_their_whole_shape() {
        assert_eq!(fill("no rate for {code}", "no rate for XAU"), Some(vec![("code", "XAU")]));
        assert_eq!(fill("{a} and {b} differ", "m and kg differ"), Some(vec![("a", "m"), ("b", "kg")]));
        assert_eq!(fill("no rate for {code}", "no rate for "), None);
        assert_eq!(fill("no rate for {code}", "a rate for XAU"), None);
        assert_eq!(fill("{a}{b}", "mkg"), None);
        assert_eq!(fill("plain", "plain"), None);
    }

    #[test]
    fn languages_have_their_direction_and_decimal_mark() {
        assert!(right_to_left("ar") && right_to_left("he") && !right_to_left("it"));
        assert!(decimal_comma("it") && decimal_comma("de-AT") && decimal_comma("pt-BR") && decimal_comma("es-ES"));
        assert!(!decimal_comma("en") && !decimal_comma("es-MX") && !decimal_comma("ja") && !decimal_comma("fa"));
    }
}
