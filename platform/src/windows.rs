//! Windows-specific RuneLite discovery.

use std::path::PathBuf;

/// Attempts to locate an installed RuneLite launcher executable in the standard Windows
/// install location (`%LOCALAPPDATA%\RuneLite\RuneLite.exe`, where the official installer
/// puts it). Returns `None` if not found there — the UI should let the user browse for the
/// executable manually in that case, rather than assuming this is exhaustive.
pub fn find_runelite() -> Option<PathBuf> {
    let local_appdata = std::env::var_os("LOCALAPPDATA")?;
    let candidate = PathBuf::from(local_appdata)
        .join("RuneLite")
        .join("RuneLite.exe");
    candidate.exists().then_some(candidate)
}
