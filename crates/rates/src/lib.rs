//! Exchange rates from the network, kept in a cache.
//!
//! Traditional currencies come from open.er-api.com (ExchangeRate-API, which
//! asks for attribution), with Frankfurter (ECB data) as a fallback.
//! Cryptocurrencies come from CoinGecko. The engine never calls this crate:
//! the app hands it a fixed copy of the rates.

use napkin_engine::{Rates, currency, units, zones};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Shown in the app: the open.er-api.com terms ask for it.
pub const ATTRIBUTION: &str = "Rates by Exchange Rate API, ECB data via Frankfurter, crypto prices by CoinGecko";
pub const ATTRIBUTION_URL: &str = "https://www.exchangerate-api.com";

/// Traditional currencies are checked every 6 hours, cryptocurrencies every hour.
pub const FIAT_INTERVAL: i64 = 6 * 3600;
pub const CRYPTO_INTERVAL: i64 = 3600;

const ER_API: &str = "https://open.er-api.com/v6/latest/USD";
const FRANKFURTER: &str = "https://api.frankfurter.dev/v1/latest?base=USD";
const COINGECKO: &str =
    "https://api.coingecko.com/api/v3/coins/markets?vs_currency=usd&order=market_cap_desc&per_page=250&page=1";
const USER_AGENT: &str = concat!("Napkin/", env!("CARGO_PKG_VERSION"));

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// What the cache file holds: the rates and when each part was last fetched.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    pub rates: Rates,
    /// Unix time of the last successful download of traditional currencies.
    #[serde(default)]
    pub fiat_checked: i64,
    /// Unix time of the last successful download of cryptocurrencies.
    #[serde(default)]
    pub crypto_checked: i64,
}

impl Cache {
    pub fn is_empty(&self) -> bool {
        self.rates.rates.is_empty()
    }
}

pub fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("napkin").join("rates.json"))
}

/// The cache as last saved; read at startup, before any request.
pub fn load_cache() -> Option<Cache> {
    let text = std::fs::read_to_string(cache_path()?).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_cache(cache: &Cache) -> Result<(), Error> {
    let path = cache_path().ok_or_else(|| Error("no cache directory".into()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error(e.to_string()))?;
    }
    let text = serde_json::to_string(cache).map_err(|e| Error(e.to_string()))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text).map_err(|e| Error(e.to_string()))?;
    std::fs::rename(&temp, &path).map_err(|e| Error(e.to_string()))
}

fn get(url: &str) -> Result<String, Error> {
    let config = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| Error(format!("{url}: {e}")))?;
    response.body_mut().read_to_string().map_err(|e| Error(format!("{url}: {e}")))
}

/// Traditional currencies: the value of 1 USD in each.
pub struct Fiat {
    pub date: String,
    pub timestamp: i64,
    pub rates: HashMap<String, f64>,
}

#[derive(Deserialize)]
struct ErApi {
    result: String,
    time_last_update_unix: i64,
    rates: HashMap<String, f64>,
}

#[derive(Deserialize)]
struct Frankfurter {
    date: String,
    rates: HashMap<String, f64>,
}

#[derive(Deserialize)]
struct Coin {
    symbol: String,
    current_price: Option<f64>,
}

fn date_of(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0).map(|t| t.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

fn fetch_er_api() -> Result<Fiat, Error> {
    let data: ErApi = serde_json::from_str(&get(ER_API)?).map_err(|e| Error(format!("open.er-api.com: {e}")))?;
    if data.result != "success" {
        return Err(Error(format!("open.er-api.com: {}", data.result)));
    }
    Ok(Fiat { date: date_of(data.time_last_update_unix), timestamp: data.time_last_update_unix, rates: data.rates })
}

fn fetch_frankfurter() -> Result<Fiat, Error> {
    let data: Frankfurter =
        serde_json::from_str(&get(FRANKFURTER)?).map_err(|e| Error(format!("frankfurter: {e}")))?;
    let timestamp = chrono::NaiveDate::parse_from_str(&data.date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(16, 0, 0))
        .map(|t| t.and_utc().timestamp())
        .unwrap_or_default();
    Ok(Fiat { date: data.date, timestamp, rates: data.rates })
}

/// open.er-api.com first, Frankfurter when it fails.
pub fn fetch_fiat() -> Result<Fiat, Error> {
    fetch_er_api().or_else(|first| fetch_frankfurter().map_err(|second| Error(format!("{first}; {second}"))))
}

/// The 250 largest cryptocurrencies: the value of 1 USD in each.
/// Tickers that read as a currency, a unit or a time zone are left out,
/// so `5 GB` stays gigabytes.
pub fn fetch_crypto() -> Result<HashMap<String, f64>, Error> {
    let coins: Vec<Coin> = serde_json::from_str(&get(COINGECKO)?).map_err(|e| Error(format!("coingecko: {e}")))?;
    let mut out = HashMap::new();
    for coin in coins {
        let ticker = coin.symbol.to_uppercase();
        let price = coin.current_price.unwrap_or_default();
        let clashes = currency::iso_places(&ticker).is_some()
            || units::is_unit_word(&ticker)
            || zones::abbreviation(&ticker).is_some();
        if price > 0.0 && !clashes && ticker.chars().all(|c| c.is_ascii_alphanumeric()) {
            out.entry(ticker).or_insert(1.0 / price);
        }
    }
    Ok(out)
}

/// The rates of a past day: ECB data via Frankfurter, about 30 currencies and
/// no cryptocurrencies. For a weekend or a holiday it gives the last working day.
pub fn fetch_on(date: chrono::NaiveDate) -> Result<Rates, Error> {
    let url = format!("https://api.frankfurter.dev/v1/{}?base=USD", date.format("%Y-%m-%d"));
    let data: Frankfurter = serde_json::from_str(&get(&url)?).map_err(|e| Error(format!("frankfurter: {e}")))?;
    Ok(Rates { date: data.date, timestamp: None, rates: data.rates, crypto: Vec::new() })
}

pub fn history_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("napkin").join("history.json"))
}

/// Past rates already downloaded, by day.
pub fn load_history() -> HashMap<chrono::NaiveDate, Rates> {
    let Some(text) = history_path().and_then(|path| std::fs::read_to_string(path).ok()) else {
        return HashMap::new();
    };
    let days: HashMap<String, Rates> = serde_json::from_str(&text).unwrap_or_default();
    days.into_iter()
        .filter_map(|(day, rates)| Some((chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d").ok()?, rates)))
        .collect()
}

pub fn save_history(history: &HashMap<chrono::NaiveDate, Rates>) -> Result<(), Error> {
    let path = history_path().ok_or_else(|| Error("no cache directory".into()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error(e.to_string()))?;
    }
    let days: HashMap<String, &Rates> =
        history.iter().map(|(day, rates)| (day.format("%Y-%m-%d").to_string(), rates)).collect();
    let text = serde_json::to_string(&days).map_err(|e| Error(e.to_string()))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text).map_err(|e| Error(e.to_string()))?;
    std::fs::rename(&temp, &path).map_err(|e| Error(e.to_string()))
}

/// Brings the cache up to date, downloading only what is due.
/// Returns whether anything changed.
pub fn refresh(cache: &mut Cache, now: i64) -> Result<bool, Error> {
    let mut changed = false;
    let mut errors = Vec::new();
    if now - cache.fiat_checked >= FIAT_INTERVAL {
        match fetch_fiat() {
            Ok(fiat) => {
                let crypto = &cache.rates.crypto;
                cache.rates.rates.retain(|code, _| crypto.contains(code));
                cache.rates.rates.extend(fiat.rates);
                cache.rates.date = fiat.date;
                cache.rates.timestamp = Some(fiat.timestamp);
                cache.fiat_checked = now;
                changed = true;
            }
            Err(e) => errors.push(e.0),
        }
    }
    if now - cache.crypto_checked >= CRYPTO_INTERVAL {
        match fetch_crypto() {
            Ok(coins) => {
                for old in std::mem::take(&mut cache.rates.crypto) {
                    cache.rates.rates.remove(&old);
                }
                let mut tickers: Vec<String> = coins.keys().cloned().collect();
                tickers.sort();
                cache.rates.rates.extend(coins);
                cache.rates.crypto = tickers;
                cache.crypto_checked = now;
                changed = true;
            }
            Err(e) => errors.push(e.0),
        }
    }
    if !changed && !errors.is_empty() {
        return Err(Error(errors.join("; ")));
    }
    Ok(changed)
}
