//! Time zones: IANA names, fixed abbreviations, explicit offsets and cities.
//! City names come from GeoNames (CC BY 4.0), bundled in
//! `data/cities.tsv` and loaded on first use.

use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zone {
    Named(Tz),
    Fixed(FixedOffset),
}

impl Zone {
    pub fn local(&self, utc: &DateTime<Utc>) -> NaiveDateTime {
        match self {
            Zone::Named(tz) => utc.with_timezone(tz).naive_local(),
            Zone::Fixed(offset) => utc.with_timezone(offset).naive_local(),
        }
    }

    pub fn to_utc(&self, local: &NaiveDateTime) -> Option<DateTime<Utc>> {
        match self {
            Zone::Named(tz) => tz.from_local_datetime(local).earliest().map(|t| t.with_timezone(&Utc)),
            Zone::Fixed(offset) => offset.from_local_datetime(local).single().map(|t| t.with_timezone(&Utc)),
        }
    }
}

/// Fixed offsets in minutes. Ambiguous abbreviations have a declared choice:
/// IST is India, CST is Central USA, BST is British Summer Time.
const ABBREVIATIONS: &[(&str, i32)] = &[
    ("UTC", 0), ("GMT", 0), ("WET", 0), ("WEST", 60), ("BST", 60), ("CET", 60), ("CEST", 120),
    ("EET", 120), ("EEST", 180), ("MSK", 180), ("TRT", 180), ("IST", 330), ("AST", -240),
    ("ADT", -180), ("NST", -210), ("NDT", -150), ("EST", -300), ("EDT", -240), ("CST", -360),
    ("CDT", -300), ("MST", -420), ("MDT", -360), ("PST", -480), ("PDT", -420), ("AKST", -540),
    ("AKDT", -480), ("HST", -600), ("ART", -180), ("BRT", -180), ("CLT", -240), ("COT", -300),
    ("PET", -300), ("WAT", 60), ("CAT", 120), ("EAT", 180), ("SAST", 120), ("GST", 240),
    ("IRST", 210), ("AFT", 270), ("PKT", 300), ("NPT", 345), ("ICT", 420), ("WIB", 420),
    ("SGT", 480), ("HKT", 480), ("PHT", 480), ("AWST", 480), ("JST", 540), ("KST", 540),
    ("ACST", 570), ("ACDT", 630), ("AEST", 600), ("AEDT", 660), ("NZST", 720), ("NZDT", 780),
];

pub fn offset(minutes: i32) -> Option<Zone> {
    FixedOffset::east_opt(minutes * 60).map(Zone::Fixed)
}

/// `PST`, `CET`: upper case only, so `cat` or `eat` stay words.
pub fn abbreviation(word: &str) -> Option<Zone> {
    let minutes = ABBREVIATIONS.iter().find(|(name, _)| *name == word)?.1;
    offset(minutes)
}

pub fn is_offset_base(word: &str) -> bool {
    matches!(word, "UTC" | "GMT")
}

/// IANA names such as `Europe/Rome`, any case.
pub fn iana(name: &str) -> Option<Zone> {
    static NAMES: OnceLock<HashMap<String, Tz>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        chrono_tz::TZ_VARIANTS.iter().map(|tz| (tz.name().to_lowercase(), *tz)).collect()
    });
    names.get(&name.to_lowercase()).copied().map(Zone::Named)
}

struct Cities {
    zones: Vec<Option<Tz>>,
    names: HashMap<&'static str, u16>,
}

const CITY_DATA: &str = include_str!("../data/cities.tsv");

fn cities() -> &'static Cities {
    static CITIES: OnceLock<Cities> = OnceLock::new();
    CITIES.get_or_init(|| {
        let mut lines = CITY_DATA.lines();
        let zones = lines
            .next()
            .unwrap_or_default()
            .split('\t')
            .map(|name| name.parse::<Tz>().ok())
            .collect();
        let mut names = HashMap::with_capacity(70_000);
        for line in lines {
            if let Some((name, index)) = line.split_once('\t') {
                if let Ok(index) = index.parse() {
                    names.insert(name, index);
                }
            }
        }
        Cities { zones, names }
    })
}

/// A city by name, any case; the most populous wins (`Paris` is in France).
pub fn city(name: &str) -> Option<Zone> {
    let db = cities();
    let index = *db.names.get(name.to_lowercase().as_str())?;
    db.zones.get(index as usize).copied().flatten().map(Zone::Named)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cities_resolve_to_the_most_populous() {
        assert_eq!(city("Paris"), Some(Zone::Named(chrono_tz::Europe::Paris)));
        assert_eq!(city("new york"), Some(Zone::Named(chrono_tz::America::New_York)));
        assert_eq!(city("Tokyo"), Some(Zone::Named(chrono_tz::Asia::Tokyo)));
        assert_eq!(iana("europe/rome"), Some(Zone::Named(chrono_tz::Europe::Rome)));
        assert!(abbreviation("cat").is_none());
    }
}
