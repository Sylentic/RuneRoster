//! Reads/writes RuneLite's own per-profile plugin-config files directly.
//!
//! RuneLite stores each of its settings profiles as a plain `.properties` file under
//! `~/.runelite/profiles2/`, named `<profile-key>-<numeric-id>.properties` (the numeric id
//! is assigned by RuneLite itself when the profile is first created/launched, so it can't be
//! predicted ahead of time — files are located by matching the `<profile-key>-` prefix).
//! Profile identity (name/id) lives only in the filename and RuneLite's own `profiles.json`
//! manifest, not in the file's contents (confirmed by inspection — it's plain plugin config
//! `key=value` lines), so overwriting one profile's file contents with another's is safe and
//! doesn't corrupt profile identity.
//!
//! This is the first place RuneRoster reads/writes RuneLite's own private files directly
//! (everywhere else it only talks to RuneLite via env vars / CLI args) — more tightly
//! coupled to RuneLite's internal storage format than the rest of the app, but it's the
//! only realistic way to offer "copy my plugin settings to another profile" without an
//! upstream RuneLite feature for it. Only safe to call while RuneLite isn't running for
//! either profile involved (it does a plain file copy, not a live merge).

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum RuneLiteConfigError {
    #[error("failed to read RuneLite's profile directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("no RuneLite profile file found for \"{0}\" — has it been launched at least once?")]
    ProfileNotFound(String),
}

/// RuneLite's own profile-config directory (`~/.runelite/profiles2`), fixed by RuneLite
/// itself regardless of platform.
pub fn default_profiles_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".runelite").join("profiles2")
}

/// Finds the `.properties` file for a given profile key (e.g. `"default"` or a RuneRoster
/// profile UUID string) within `profiles_dir`, by matching the `<key>-` filename prefix.
fn find_profile_file(profiles_dir: &Path, profile_key: &str) -> Result<PathBuf, RuneLiteConfigError> {
    let prefix = format!("{profile_key}-");
    for entry in fs::read_dir(profiles_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(&prefix) && name.ends_with(".properties") {
            return Ok(entry.path());
        }
    }
    Err(RuneLiteConfigError::ProfileNotFound(profile_key.to_string()))
}

/// Copies `source_key`'s plugin config onto `dest_key`'s — the destination's existing
/// settings are fully overwritten. Both profiles must have been launched at least once
/// already (so RuneLite has created their `.properties` files).
pub fn copy_profile_settings(
    profiles_dir: &Path,
    source_key: &str,
    dest_key: &str,
) -> Result<(), RuneLiteConfigError> {
    let source_path = find_profile_file(profiles_dir, source_key)?;
    let dest_path = find_profile_file(profiles_dir, dest_key)?;
    fs::copy(&source_path, &dest_path)?;
    Ok(())
}

/// Convenience wrapper using RuneLite's real, default profile directory — copies the
/// `default` RuneLite profile's plugin config onto `dest_key`'s.
pub fn copy_profile_settings_from_default(dest_key: &str) -> Result<(), RuneLiteConfigError> {
    copy_profile_settings(&default_profiles_dir(), "default", dest_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_test_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("runeroster-profiles2-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copies_source_content_onto_destination() {
        let dir = make_test_dir();
        fs::write(dir.join("default-0.properties"), "plugin.setting=true\n").unwrap();
        fs::write(dir.join("abc123-999.properties"), "plugin.setting=false\n").unwrap();

        copy_profile_settings(&dir, "default", "abc123").unwrap();

        let dest_content = fs::read_to_string(dir.join("abc123-999.properties")).unwrap();
        assert_eq!(dest_content, "plugin.setting=true\n");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn errors_when_source_profile_missing() {
        let dir = make_test_dir();
        fs::write(dir.join("abc123-999.properties"), "x=1\n").unwrap();

        let result = copy_profile_settings(&dir, "default", "abc123");
        assert!(matches!(
            result,
            Err(RuneLiteConfigError::ProfileNotFound(key)) if key == "default"
        ));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn errors_when_destination_profile_missing() {
        let dir = make_test_dir();
        fs::write(dir.join("default-0.properties"), "x=1\n").unwrap();

        let result = copy_profile_settings(&dir, "default", "abc123");
        assert!(matches!(
            result,
            Err(RuneLiteConfigError::ProfileNotFound(key)) if key == "abc123"
        ));

        fs::remove_dir_all(&dir).ok();
    }
}
