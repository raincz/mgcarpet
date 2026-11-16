//! Where the game keeps its four kinds of files, and the environment
//! variables that move them (player ruling 2026-11-16 — the whole
//! scheme, on purpose):
//!
//! | Variable         | What                                   | Default    |
//! |------------------|----------------------------------------|------------|
//! | `MGC_CONFIG_DIR` | `mgcarpet.json` + `mgcarpet.json.defaults` | `.`    |
//! | `MGC_SAVES_DIR`  | save slots, netplay history            | `saves`    |
//! | `MGC_GAMEDATA`   | the original game data                 | `gamedata` |
//! | `MGC_BAKED_DIR`  | the baked packages (regenerable)       | `baked`    |
//!
//! A value is a path, used AS GIVEN: no `$VAR`, no `~`, nothing
//! expanded. Absolute is absolute; relative is relative to the working
//! directory, exactly like the defaults (the campaign launchers pin
//! that to their own folder on a double-click, and a dev run from the
//! repository root finds its data there). An unset or empty variable
//! means the default. Packagers wanting an XDG layout set these in a
//! wrapper (or a Flatpak/Snap manifest, a Docker image…); the game
//! itself resolves nothing.

use std::path::PathBuf;

/// A variable's path, or `default` when it is unset or empty.
fn from_env(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

/// The player's config file (`--config` overrides it).
pub fn config_file() -> PathBuf {
    from_env("MGC_CONFIG_DIR", ".").join(crate::config::DEFAULT_PATH)
}

/// The saves tree.
pub fn saves_dir() -> PathBuf {
    from_env("MGC_SAVES_DIR", "saves")
}

/// The original game data (the config's `gamedata` key outranks it).
pub fn gamedata_dir() -> PathBuf {
    from_env("MGC_GAMEDATA", "gamedata")
}

/// The baked packages.
pub fn baked_dir() -> PathBuf {
    from_env("MGC_BAKED_DIR", "baked")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set means used as given — absolute or relative, nothing
    /// expanded; unset or empty means the default.
    #[test]
    fn env_values_are_used_as_given() {
        // A variable only this test touches.
        let var = "MGC_PATHS_TEST_ONLY";
        // SAFETY: no other test reads or writes this variable.
        unsafe { std::env::remove_var(var) };
        assert_eq!(from_env(var, "baked"), PathBuf::from("baked"));
        unsafe { std::env::set_var(var, "") };
        assert_eq!(
            from_env(var, "baked"),
            PathBuf::from("baked"),
            "empty = unset"
        );
        unsafe { std::env::set_var(var, "/var/cache/mgcarpet") };
        assert_eq!(from_env(var, "baked"), PathBuf::from("/var/cache/mgcarpet"));
        unsafe { std::env::set_var(var, "$HOME/~x") };
        assert_eq!(
            from_env(var, "baked"),
            PathBuf::from("$HOME/~x"),
            "no expansion"
        );
        unsafe { std::env::remove_var(var) };
    }
}
