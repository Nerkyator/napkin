//! Currencies, cryptocurrencies and exchange rates.

use crate::num::{self, Num};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// ISO 4217 codes in use and their minor units.
const ISO_4217: &[(&str, u32)] = &[
    ("AED", 2), ("AFN", 2), ("ALL", 2), ("AMD", 2), ("ANG", 2), ("AOA", 2), ("ARS", 2), ("AUD", 2),
    ("AWG", 2), ("AZN", 2), ("BAM", 2), ("BBD", 2), ("BDT", 2), ("BGN", 2), ("BHD", 3), ("BIF", 0),
    ("BMD", 2), ("BND", 2), ("BOB", 2), ("BRL", 2), ("BSD", 2), ("BTN", 2), ("BWP", 2), ("BYN", 2),
    ("BZD", 2), ("CAD", 2), ("CDF", 2), ("CHF", 2), ("CLP", 0), ("CNY", 2), ("COP", 2), ("CRC", 2),
    ("CUP", 2), ("CVE", 2), ("CZK", 2), ("DJF", 0), ("DKK", 2), ("DOP", 2), ("DZD", 2), ("EGP", 2),
    ("ERN", 2), ("ETB", 2), ("EUR", 2), ("FJD", 2), ("FKP", 2), ("GBP", 2), ("GEL", 2), ("GHS", 2),
    ("GIP", 2), ("GMD", 2), ("GNF", 0), ("GTQ", 2), ("GYD", 2), ("HKD", 2), ("HNL", 2), ("HTG", 2),
    ("HUF", 2), ("IDR", 2), ("ILS", 2), ("INR", 2), ("IQD", 3), ("IRR", 2), ("ISK", 0), ("JMD", 2),
    ("JOD", 3), ("JPY", 0), ("KES", 2), ("KGS", 2), ("KHR", 2), ("KMF", 0), ("KPW", 2), ("KRW", 0),
    ("KWD", 3), ("KYD", 2), ("KZT", 2), ("LAK", 2), ("LBP", 2), ("LKR", 2), ("LRD", 2), ("LSL", 2),
    ("LYD", 3), ("MAD", 2), ("MDL", 2), ("MGA", 2), ("MKD", 2), ("MMK", 2), ("MNT", 2), ("MOP", 2),
    ("MRU", 2), ("MUR", 2), ("MVR", 2), ("MWK", 2), ("MXN", 2), ("MYR", 2), ("MZN", 2), ("NAD", 2),
    ("NGN", 2), ("NIO", 2), ("NOK", 2), ("NPR", 2), ("NZD", 2), ("OMR", 3), ("PAB", 2), ("PEN", 2),
    ("PGK", 2), ("PHP", 2), ("PKR", 2), ("PLN", 2), ("PYG", 0), ("QAR", 2), ("RON", 2), ("RSD", 2),
    ("RUB", 2), ("RWF", 0), ("SAR", 2), ("SBD", 2), ("SCR", 2), ("SDG", 2), ("SEK", 2), ("SGD", 2),
    ("SHP", 2), ("SLE", 2), ("SOS", 2), ("SRD", 2), ("SSP", 2), ("STN", 2), ("SVC", 2), ("SYP", 2),
    ("SZL", 2), ("THB", 2), ("TJS", 2), ("TMT", 2), ("TND", 3), ("TOP", 2), ("TRY", 2), ("TTD", 2),
    ("TWD", 2), ("TZS", 2), ("UAH", 2), ("UGX", 0), ("USD", 2), ("UYU", 2), ("UZS", 2), ("VES", 2),
    ("VND", 0), ("VUV", 0), ("WST", 2), ("XAF", 0), ("XCD", 2), ("XCG", 2), ("XOF", 0), ("XPF", 0),
    ("YER", 2), ("ZAR", 2), ("ZMW", 2), ("ZWG", 2),
];

/// Codes that are also English words or units: accepted in upper case only.
const UPPER_ONLY: &[&str] = &["CUP", "ALL", "TOP", "TRY", "PEN", "MAD", "GEL", "BOB"];

/// Well-known tickers, so `1 BTC` is read even before rates arrive.
const CRYPTO: &[&str] = &[
    "BTC", "ETH", "USDT", "BNB", "SOL", "XRP", "USDC", "DOGE", "ADA", "TRX", "TON", "AVAX", "SHIB",
    "DOT", "LINK", "BCH", "LTC", "XLM", "XMR", "ATOM", "UNI", "ETC", "NEAR", "APT", "SUI", "PEPE",
    "POL", "MATIC", "FIL", "ICP", "HBAR", "ARB", "VET", "ALGO", "AAVE",
];

/// Symbols written before or after the amount.
pub fn symbol_code(symbol: &str) -> Option<&'static str> {
    Some(match symbol {
        "$" => "USD",
        "€" => "EUR",
        "£" => "GBP",
        "¥" => "JPY",
        "₹" => "INR",
        "₩" => "KRW",
        "₺" => "TRY",
        "₴" => "UAH",
        "₪" => "ILS",
        "฿" => "THB",
        "₽" => "RUB",
        "R$" => "BRL",
        "C$" => "CAD",
        "A$" => "AUD",
        _ => return None,
    })
}

/// Symbol shown before the amount; the other currencies show their code after it.
fn display_symbol(code: &str) -> Option<&'static str> {
    Some(match code {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" => "¥",
        _ => return None,
    })
}

/// English names written as one word.
fn name_code(lower: &str) -> Option<&'static str> {
    Some(match lower {
        "dollar" | "dollars" => "USD",
        "euro" | "euros" => "EUR",
        "yen" => "JPY",
        "rouble" | "roubles" | "ruble" | "rubles" => "RUB",
        "yuan" | "renminbi" => "CNY",
        "rupee" | "rupees" => "INR",
        "bitcoin" | "bitcoins" => "BTC",
        "ethereum" | "ether" => "ETH",
        "solana" => "SOL",
        "dogecoin" => "DOGE",
        "litecoin" => "LTC",
        "ripple" => "XRP",
        "cardano" => "ADA",
        "tether" => "USDT",
        _ => return None,
    })
}

/// English names written as two words.
fn phrase_code(first: &str, second: &str) -> Option<&'static str> {
    Some(match (first, second) {
        ("pound" | "pounds", "sterling") => "GBP",
        ("swiss", "franc" | "francs") => "CHF",
        ("canadian", "dollar" | "dollars") => "CAD",
        ("australian", "dollar" | "dollars") => "AUD",
        _ => return None,
    })
}

pub fn iso_places(code: &str) -> Option<u32> {
    ISO_4217.iter().find(|(c, _)| *c == code).map(|(_, places)| *places)
}

/// Exchange rates handed to the engine: the value of 1 USD in each currency.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Rates {
    /// Day the data refers to, `YYYY-MM-DD`.
    #[serde(default)]
    pub date: String,
    /// Unix time of the data, when known more precisely than `date`.
    #[serde(default)]
    pub timestamp: Option<i64>,
    pub rates: HashMap<String, f64>,
    /// Tickers that are cryptocurrencies.
    #[serde(default)]
    pub crypto: Vec<String>,
}

impl Rates {
    pub fn from_json(text: &str) -> Result<Rates, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn usd_rate(&self, code: &str) -> Option<Num> {
        if code == "USD" {
            return Some(num::one());
        }
        let rate = *self.rates.get(code)?;
        if rate <= 0.0 {
            return None;
        }
        num::from_f64(rate)
    }

    pub fn is_crypto(&self, code: &str) -> bool {
        self.crypto.iter().any(|c| c == code)
    }

    /// Unix time the data refers to.
    pub fn data_time(&self) -> Option<i64> {
        if let Some(ts) = self.timestamp {
            return Some(ts);
        }
        let date = chrono::NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").ok()?;
        Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
    }
}

pub fn is_crypto(code: &str, rates: Option<&Rates>) -> bool {
    CRYPTO.contains(&code) || rates.is_some_and(|r| r.is_crypto(code))
}

/// A currency written as one word: `EUR`, `eur`, `euros`, `BTC`, `bitcoin`.
pub fn word_code(word: &str, rates: Option<&Rates>) -> Option<String> {
    let is_upper = word.chars().all(|c| c.is_ascii_uppercase());
    if is_upper && word.len() == 3 && iso_places(word).is_some() {
        return Some(word.to_string());
    }
    if is_upper && (2..=6).contains(&word.len()) && is_crypto(word, rates) {
        return Some(word.to_string());
    }
    let is_lower = word.chars().all(|c| c.is_ascii_lowercase());
    if is_lower && word.len() == 3 {
        let upper = word.to_ascii_uppercase();
        if iso_places(&upper).is_some() && !UPPER_ONLY.contains(&upper.as_str()) {
            return Some(upper);
        }
    }
    name_code(&word.to_lowercase()).map(str::to_string)
}

/// A currency written as two words: `pound sterling`, `swiss francs`.
pub fn phrase(first: &str, second: &str) -> Option<&'static str> {
    phrase_code(&first.to_lowercase(), &second.to_lowercase())
}

/// Money as shown in the results column.
pub fn format_money(amount: &Num, code: &str, crypto: bool, format: num::NumberFormat) -> String {
    if crypto {
        return format!("{} {code}", num::format_number(amount, 8, format));
    }
    let places = iso_places(code).unwrap_or(2);
    let text = num::fixed(amount, places, format);
    match display_symbol(code) {
        Some(symbol) => match text.strip_prefix('-') {
            Some(positive) => format!("-{symbol}{positive}"),
            None => format!("{symbol}{text}"),
        },
        None => format!("{text} {code}"),
    }
}

/// Words that may not be used as variable names because they are currencies.
pub fn is_currency_word(word: &str) -> bool {
    word_code(word, None).is_some()
}
