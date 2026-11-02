//! Downloads the rates once and prints a summary: `cargo run --example fetch`.

fn main() {
    let mut cache = napkin_rates::Cache::default();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    match napkin_rates::refresh(&mut cache, now) {
        Ok(changed) => println!(
            "changed: {changed}, date {}, {} rates, {} crypto, EUR {:?}, BTC {:?}",
            cache.rates.date,
            cache.rates.rates.len(),
            cache.rates.crypto.len(),
            cache.rates.rates.get("EUR"),
            cache.rates.rates.get("BTC")
        ),
        Err(e) => println!("error: {e}"),
    }
}
