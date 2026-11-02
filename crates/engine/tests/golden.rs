//! Runs every `tests/golden/*.golden` file.

use napkin_engine::golden;
use std::path::Path;

#[test]
fn golden_files() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("golden directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "golden"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no golden files found");

    let mut failures = Vec::new();
    let mut checked = 0;
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(path).expect("readable golden file");
        let file = golden::parse(&text, &dir).unwrap_or_else(|e| panic!("{name}: {e}"));
        checked += file.expected.iter().flatten().count();
        for m in golden::run(&file) {
            failures.push(format!("{name}:{}: `{}` => expected `{}`, got `{}`", m.line, m.input.trim(), m.expected, m.actual));
        }
    }
    assert!(failures.is_empty(), "{} of {checked} checks failed:\n{}", failures.len(), failures.join("\n"));
}
