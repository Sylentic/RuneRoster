//! Placeholder entry point.
//!
//! This is deliberately not the real UI yet — the custom iced UI design is a later step
//! (see `rs-launcher-plan.md`). For now this just proves the workspace wires together:
//! it loads (or creates) the profile registry from disk and lists what's there, so
//! `runeroster-core` and `runeroster-platform` can be exercised end-to-end from a binary.

use runeroster_core::accounts::ProfileRegistry;

fn profiles_path() -> std::path::PathBuf {
    let base = dirs_next_config_dir();
    base.join("RuneRoster").join("profiles.json")
}

/// Minimal `%APPDATA%`-based config dir lookup (Windows-only for v1, matching
/// `runeroster-platform`'s scope). Kept local/tiny rather than pulling in a whole crate
/// for one env var lookup.
fn dirs_next_config_dir() -> std::path::PathBuf {
    std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn main() {
    let path = profiles_path();
    let registry = ProfileRegistry::load(&path).expect("failed to load profile registry");

    println!("RuneRoster — profiles file: {}", path.display());
    if registry.profiles.is_empty() {
        println!("No profiles yet.");
    } else {
        for profile in &registry.profiles {
            println!("- {} ({})", profile.display_name, profile.id);
        }
    }

    match runeroster_platform::find_runelite() {
        Some(path) => println!("Found RuneLite at: {}", path.display()),
        None => println!("RuneLite not found in the default install location."),
    }
}
