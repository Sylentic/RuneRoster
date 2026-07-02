//! Spawns RuneLite for a given profile + selected character.
//!
//! Uses two pieces of RuneLite's own (documented) launch contract — not anything specific
//! to Bolt:
//! - the `--profile=<name>` client argument, which loads/creates an isolated RuneLite
//!   settings profile (see the [RuneLite Launcher Configuration wiki](https://github.com/runelite/runelite/wiki/RuneLite-Launcher-Configuration)).
//! - the `JX_SESSION_ID` / `JX_CHARACTER_ID` / `JX_DISPLAY_NAME` environment variables,
//!   which let a launcher start RuneLite directly logged into a specific Jagex character.

use std::path::PathBuf;
use std::process::Command;

use crate::accounts::Profile;
use crate::session::LaunchSession;

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("failed to spawn RuneLite: {0}")]
    Spawn(#[from] std::io::Error),
}

/// Where/how to invoke RuneLite. Kept as an enum (rather than hardcoding `java -jar` or an
/// `.exe` path) so this module stays OS-agnostic in shape, even though only the Windows path
/// is exercised for v1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTarget {
    /// Windows: the RuneLite launcher executable (bundles its own JRE).
    WindowsExe(PathBuf),
    /// Any platform with a JRE on `PATH`: `java -jar RuneLite.jar`.
    Jar(PathBuf),
}

impl LaunchTarget {
    fn into_command(self) -> Command {
        match self {
            LaunchTarget::WindowsExe(path) => Command::new(path),
            LaunchTarget::Jar(path) => {
                let mut cmd = Command::new("java");
                cmd.arg("-jar");
                cmd.arg(path);
                cmd
            }
        }
    }
}

/// Spawns RuneLite for `profile`, logged directly into `session`'s character.
pub fn launch(
    target: LaunchTarget,
    profile: &Profile,
    session: &LaunchSession,
) -> Result<(), LaunchError> {
    let mut command = target.into_command();
    command
        .arg(format!("--profile={}", profile.id))
        .env("JX_SESSION_ID", &session.session_id)
        .env("JX_CHARACTER_ID", &session.account_id)
        .env("JX_DISPLAY_NAME", &session.display_name)
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jar_target_builds_java_dash_jar_command() {
        let target = LaunchTarget::Jar(PathBuf::from("RuneLite.jar"));
        let command = target.into_command();
        assert_eq!(command.get_program(), "java");
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, vec!["-jar", "RuneLite.jar"]);
    }

    #[test]
    fn exe_target_runs_executable_directly() {
        let target = LaunchTarget::WindowsExe(PathBuf::from(r"C:\RuneLite\RuneLite.exe"));
        let command = target.into_command();
        assert_eq!(command.get_program(), r"C:\RuneLite\RuneLite.exe");
        assert_eq!(command.get_args().count(), 0);
    }
}
