//! Golden test files: a sheet whose lines end with `=>`
//! and the result expected, character for character. `—` means no result.
//!
//! Header lines set the context: `@now`, `@tz`, `@rates`, `@precision`,
//! `@format en|it`, `@clock 24|12`, `@rates-on DATE FILE` for past rates.
//! `@total` gives the expected grand total.

use crate::num::{self, NumberFormat};
use crate::zones::{self, Zone};
use crate::{Clock, Context, Rates, evaluate};
use chrono::{DateTime, NaiveDate};
use std::path::Path;
use std::sync::Arc;

pub struct Golden {
    pub context: Context,
    pub sheet: String,
    /// Expected text per sheet line, with the line number in the file.
    pub expected: Vec<Option<(usize, String)>>,
    pub total: Option<String>,
}

#[derive(Debug)]
pub struct Mismatch {
    pub line: usize,
    pub input: String,
    pub expected: String,
    pub actual: String,
}

pub const NO_RESULT: &str = "—";

pub fn default_context() -> Context {
    let now = DateTime::parse_from_rfc3339("2026-10-14T12:00:00+02:00").expect("valid date").to_utc();
    Context::new(now, Zone::Named(chrono_tz::Europe::Rome))
}

pub fn parse(text: &str, dir: &Path) -> Result<Golden, String> {
    let mut context = default_context();
    let mut sheet = Vec::new();
    let mut expected = Vec::new();
    let mut total = None;
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        if let Some(directive) = raw.strip_prefix('@') {
            let (key, value) = directive.split_once(' ').unwrap_or((directive, ""));
            let value = value.trim();
            let bad = |what: &str| format!("line {line_no}: invalid {what} `{value}`");
            match key {
                "now" => {
                    context.now = DateTime::parse_from_rfc3339(value).map_err(|_| bad("@now"))?.to_utc();
                }
                "tz" => context.zone = zones::iana(value).ok_or_else(|| bad("@tz"))?,
                "rates" => {
                    let json = std::fs::read_to_string(dir.join(value)).map_err(|e| format!("{value}: {e}"))?;
                    context.rates = Some(Arc::new(Rates::from_json(&json).map_err(|e| format!("{value}: {e}"))?));
                }
                "rates-on" => {
                    let (day, file) = value.split_once(' ').ok_or_else(|| bad("@rates-on"))?;
                    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").map_err(|_| bad("@rates-on"))?;
                    let file = file.trim();
                    let json = std::fs::read_to_string(dir.join(file)).map_err(|e| format!("{file}: {e}"))?;
                    let rates = Rates::from_json(&json).map_err(|e| format!("{file}: {e}"))?;
                    context.history.insert(date, Arc::new(rates));
                }
                "precision" => context.precision = value.parse().map_err(|_| bad("@precision"))?,
                "clock" => {
                    context.clock = match value {
                        "24" => Clock::TwentyFourHour,
                        "12" => Clock::TwelveHour,
                        _ => return Err(bad("@clock")),
                    }
                }
                "format" => {
                    context.format = match value {
                        "en" => NumberFormat::English,
                        "it" => NumberFormat::Italian,
                        _ => return Err(bad("@format")),
                    }
                }
                "ppi" => context.ppi = num::parse(value).ok_or_else(|| bad("@ppi"))?,
                "total" => total = Some(value.to_string()),
                _ => return Err(format!("line {line_no}: unknown directive @{key}")),
            }
            continue;
        }
        match raw.split_once("=>") {
            Some((input, result)) => {
                sheet.push(input.trim_end().to_string());
                expected.push(Some((line_no, result.trim().to_string())));
            }
            None => {
                sheet.push(raw.trim_end().to_string());
                expected.push(None);
            }
        }
    }
    Ok(Golden { context, sheet: sheet.join("\n"), expected, total })
}

pub fn run(golden: &Golden) -> Vec<Mismatch> {
    let result = evaluate(&golden.sheet, &golden.context);
    let inputs: Vec<&str> = golden.sheet.split('\n').collect();
    let mut mismatches = Vec::new();
    for (i, line) in result.lines.iter().enumerate() {
        if let Some(Some((line_no, expected))) = golden.expected.get(i) {
            let actual = line.text.clone().unwrap_or_else(|| NO_RESULT.to_string());
            if &actual != expected {
                mismatches.push(Mismatch {
                    line: *line_no,
                    input: inputs[i].to_string(),
                    expected: expected.clone(),
                    actual,
                });
            }
        }
    }
    if let Some(expected) = &golden.total {
        let actual = result.total.clone().unwrap_or_else(|| NO_RESULT.to_string());
        if &actual != expected {
            mismatches.push(Mismatch { line: 0, input: "@total".into(), expected: expected.clone(), actual });
        }
    }
    mismatches
}
