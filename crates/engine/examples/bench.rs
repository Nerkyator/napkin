//! Times a 500-line sheet (it should stay under 20 ms): `cargo run --release --example bench`.

use napkin_engine::golden;
use std::time::Instant;

fn main() {
    let lines = [
        "Rent: €850", "Bills: $120 in EUR", "Food: €240 + 15%", "14 inches in cm", "2 m + 35 cm",
        "today + 10 days", "sqrt 25 + 4", "w = $45", "w times 3 - 12%", "8% of what is 10 EUR",
        "0xFF + 0b101", "1 GiB in MB", "100 °F in °C", "time in Tokyo", "2^64", "sum",
        "(1 + 2)(3 + 4)", "round(1 year in weeks)", "3pm London - 3pm Tokyo", "",
    ];
    let sheet: Vec<&str> = lines.iter().cycle().take(500).copied().collect();
    let text = sheet.join("\n");
    let mut ctx = golden::default_context();
    let rates = std::fs::read_to_string("crates/engine/tests/golden/fixtures/rates.json").expect("fixture");
    ctx.rates = Some(std::sync::Arc::new(napkin_engine::Rates::from_json(&rates).expect("valid rates")));
    napkin_engine::evaluate(&text, &ctx); // warm up: city database and unit catalog
    let runs = 20;
    let start = Instant::now();
    for _ in 0..runs {
        napkin_engine::evaluate(&text, &ctx);
    }
    println!("500 lines: {:.2} ms per evaluation", start.elapsed().as_secs_f64() * 1000.0 / runs as f64);
}
