//! Prints each line of a golden file with its result: `cargo run --example sheet FILE`.

use napkin_engine::golden;
use std::path::Path;

fn main() {
    let path = std::env::args().nth(1).expect("golden file");
    let path = Path::new(&path);
    let text = std::fs::read_to_string(path).expect("readable file");
    let file = golden::parse(&text, path.parent().unwrap()).expect("valid golden file");
    let sheet = napkin_engine::evaluate(&file.sheet, &file.context);
    for (input, line) in file.sheet.split('\n').zip(&sheet.lines) {
        let shown = line.text.clone().unwrap_or_else(|| golden::NO_RESULT.into());
        let why = line.error.as_deref().map(|e| format!("   [{e}]")).unwrap_or_default();
        println!("{:<34} | {shown}{why}", input.trim());
    }
    println!("total: {:?}", sheet.total);
}
