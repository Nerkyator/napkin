//! Hyprland scratchpads: when Napkin's window sits in a special workspace
//! (Omarchy moves it there with SUPER+ALT+S), showing and hiding Napkin means
//! showing and hiding that workspace, so the window stays in it.

use std::process::Command;

/// The special workspace that holds this window, such as `scratchpad`.
pub fn special_workspace() -> Option<String> {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let output = Command::new("hyprctl").args(["clients", "-j"]).output().ok()?;
    let clients: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let pid = i64::from(std::process::id());
    clients
        .as_array()?
        .iter()
        .find(|client| client["pid"].as_i64() == Some(pid))?["workspace"]["name"]
        .as_str()?
        .strip_prefix("special:")
        .map(str::to_string)
}

/// Shows the special workspace, or hides it when it is shown.
pub fn toggle_special(name: &str) {
    // Lua configuration first, as Omarchy writes it; the classic dispatcher otherwise.
    let lua = format!("hl.dsp.workspace.toggle_special(\"{name}\")");
    let done = Command::new("hyprctl")
        .args(["dispatch", &lua])
        .output()
        .is_ok_and(|out| out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "ok");
    if !done {
        let _ = Command::new("hyprctl").args(["dispatch", "togglespecialworkspace", name]).output();
    }
}

/// Hyprland's corner radius, which Omarchy sets to 0 for square windows.
pub fn rounding() -> Option<u32> {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let output = Command::new("hyprctl").args(["-j", "getoption", "decoration:rounding"]).output().ok()?;
    let option: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    option["int"].as_u64().and_then(|r| u32::try_from(r).ok())
}
