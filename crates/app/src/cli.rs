//! The terminal command: `napkin eval` and `napkin test`.

use crate::context;
use napkin_engine::golden;
use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

pub const USAGE: &str = "\
Usage:
  napkin                  open the window (or bring it forward)
  napkin --toggle         show or hide the window
  napkin eval EXPRESSION  print the result of one line
  napkin eval             read a sheet from standard input, print one result per line
  napkin test FILE...     run golden test files
";

/// Evaluates with the cached rates and the system clock; never uses the network.
pub fn eval(args: &[String]) -> ExitCode {
    let text = if args.is_empty() {
        let mut input = String::new();
        if std::io::stdin().read_to_string(&mut input).is_err() {
            eprintln!("napkin: cannot read standard input");
            return ExitCode::FAILURE;
        }
        input.trim_end_matches('\n').to_string()
    } else {
        args.join(" ")
    };
    let settings = crate::settings::load();
    let rates = napkin_rates::load_cache().filter(|c| !c.is_empty()).map(|c| std::sync::Arc::new(c.rates));
    let ctx = context(&settings, rates, &crate::load_history());
    let sheet = napkin_engine::evaluate(&text, &ctx);
    for line in &sheet.lines {
        println!("{}", line.text.as_deref().unwrap_or(golden::NO_RESULT));
    }
    ExitCode::SUCCESS
}

pub fn test(files: &[String]) -> ExitCode {
    if files.is_empty() {
        eprintln!("napkin test: no files given");
        return ExitCode::FAILURE;
    }
    let mut failed = 0;
    let mut checked = 0;
    for file in files {
        let path = Path::new(file);
        let result = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| golden::parse(&text, path.parent().unwrap_or(Path::new("."))));
        let golden = match result {
            Ok(golden) => golden,
            Err(e) => {
                eprintln!("{file}: {e}");
                failed += 1;
                continue;
            }
        };
        checked += golden.expected.iter().flatten().count();
        for m in golden::run(&golden) {
            failed += 1;
            println!("{file}:{}: `{}` => expected `{}`, got `{}`", m.line, m.input.trim(), m.expected, m.actual);
        }
    }
    println!("{} checks, {failed} failed", checked);
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
