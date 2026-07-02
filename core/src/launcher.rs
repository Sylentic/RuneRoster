//! Spawns RuneLite for a given profile + selected character.
//!
//! Uses pieces of RuneLite's own (documented, or discovered by reading its launcher source)
//! launch contract:
//! - the `--profile=<name>` client argument, which loads/creates an isolated RuneLite
//!   settings profile (see the [RuneLite Launcher Configuration wiki](https://github.com/runelite/runelite/wiki/RuneLite-Launcher-Configuration)).
//!   **Must be delivered via the `RUNELITE_ARGS` environment variable, not a raw CLI arg**
//!   — confirmed by reading `runelite/launcher`'s `Launcher.java`: its own `OptionParser`
//!   only recognizes a fixed set of top-level flags (`--debug`, `--configure`, etc.) and
//!   silently discards anything else passed directly on the command line. The actual
//!   client argument list (`getClientArgs`) is built only from the launcher's persisted
//!   config file *or* `System.getenv("RUNELITE_ARGS")` (space-separated, appended as-is) —
//!   the latter is what this module uses, since it needs no GUI-config-file writing.
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

/// Builds the `Command` that `launch` would spawn, without actually spawning it — split out
/// so the env vars/args RuneLite receives can be asserted on in tests.
fn build_command(target: LaunchTarget, profile: &Profile, session: &LaunchSession) -> Command {
    let mut command = target.into_command();
    command
        // Forces the launcher's REFLECT launch mode instead of the AUTO default (which
        // picks FORK when running RuneLite.exe directly). FORK re-spawns the launcher as a
        // child process with the already-computed client args embedded on its command
        // line, while that child *also* inherits RUNELITE_ARGS from the environment — its
        // own client-arg computation then merges both sources, duplicating --profile= and
        // crashing jopt-simple (`MultipleArgumentsForOptionException`), confirmed live.
        // REFLECT runs the client in the same process via reflection, no forking, no
        // duplication. This is a real, recognized top-level launcher flag (unlike
        // --profile=, which must go through RUNELITE_ARGS — see module docs above).
        .arg("--launch-mode=REFLECT")
        .env("RUNELITE_ARGS", format!("--profile={}", profile.id))
        .env("JX_SESSION_ID", &session.session_id)
        .env("JX_CHARACTER_ID", &session.account_id)
        .env("JX_DISPLAY_NAME", &session.display_name);
    command
}

/// Spawns RuneLite for `profile`, logged directly into `session`'s character.
pub fn launch(
    target: LaunchTarget,
    profile: &Profile,
    session: &LaunchSession,
) -> Result<(), LaunchError> {
    build_command(target, profile, session).spawn()?;
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

    #[test]
    fn build_command_sets_runelite_args_and_jx_env_vars() {
        let profile = Profile::new("Main");
        let session = LaunchSession {
            session_id: "sess-123".into(),
            account_id: "acct-456".into(),
            display_name: "Zezima".into(),
        };
        let target = LaunchTarget::WindowsExe(PathBuf::from(r"C:\RuneLite\RuneLite.exe"));
        let command = build_command(target, &profile, &session);

        let envs: std::collections::HashMap<_, _> = command
            .get_envs()
            .filter_map(|(k, v)| Some((k.to_str()?, v?.to_str()?)))
            .collect();

        assert_eq!(
            envs.get("RUNELITE_ARGS").copied(),
            Some(format!("--profile={}", profile.id)).as_deref()
        );
        assert_eq!(envs.get("JX_SESSION_ID").copied(), Some("sess-123"));
        assert_eq!(envs.get("JX_CHARACTER_ID").copied(), Some("acct-456"));
        assert_eq!(envs.get("JX_DISPLAY_NAME").copied(), Some("Zezima"));
        // --profile= must NOT be passed as a raw CLI arg — RuneLite's launcher silently
        // discards unrecognized top-level options instead of forwarding them to the
        // client. --launch-mode=REFLECT IS a recognized top-level flag, forcing REFLECT
        // mode to avoid FORK mode's client-arg duplication bug (see build_command docs).
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, vec!["--launch-mode=REFLECT"]);
    }
}
