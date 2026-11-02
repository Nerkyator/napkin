//! Start at login through XDG autostart, which uwsm runs on Omarchy.

use crate::APP_ID;
use std::path::PathBuf;

pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("autostart").join(format!("{APP_ID}.desktop")))
}

/// The entry written before Napkin took its short name.
fn legacy_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("autostart").join("io.github.nerkyator.Napkin.desktop"))
}

/// Moves an entry left under the old name to the new one, keeping its `Exec`
/// line, so login keeps starting the same binary once.
pub fn migrate() {
    let (Some(legacy), Some(path)) = (legacy_path(), path()) else { return };
    let Ok(entry) = std::fs::read_to_string(&legacy) else { return };
    let entry: String =
        entry.lines().map(|l| if l.starts_with("Icon=") { format!("Icon={APP_ID}\n") } else { format!("{l}\n") }).collect();
    if let Err(e) = std::fs::write(&path, entry).and_then(|()| std::fs::remove_file(&legacy)) {
        eprintln!("napkin: cannot move the autostart entry: {e}");
    }
}

pub fn set(enabled: bool) -> std::io::Result<()> {
    let Some(path) = path() else { return Ok(()) };
    write_entry(&path, enabled)?;
    // The old entry goes only once the new one is in place.
    if let Some(legacy) = legacy_path().filter(|p| p.exists()) {
        std::fs::remove_file(legacy)?;
    }
    Ok(())
}

fn write_entry(path: &std::path::Path, enabled: bool) -> std::io::Result<()> {
    if !enabled {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Napkin\nComment=Natural-language calculator\nExec={} --hidden\nIcon={APP_ID}\nTerminal=false\n",
        exe.display()
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, entry)
}
