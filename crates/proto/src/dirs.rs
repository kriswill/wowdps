//! The XDG base directories, resolved in one place: where the per-machine
//! caches, the history store, the daemon's state and the config live.
//!
//! Each base follows the XDG Base Directory spec: its variable
//! (`$XDG_DATA_HOME`, `$XDG_CACHE_HOME`, `$XDG_STATE_HOME`,
//! `$XDG_CONFIG_HOME`) when it holds an absolute path, else `$HOME` and the
//! spec's default under it (`.local/share`, `.cache`, `.local/state`,
//! `.config`). Two readings the spec leaves to the reader are decided
//! here: a variable set to the EMPTY string counts as unset (the spec's
//! own words for an unset or empty one), and a RELATIVE value is ignored
//! as the spec says to ("consider the path invalid and ignore it"), so the
//! default applies — never a path that moves with the working directory.
//! `$HOME` is taken as given when it is set and not empty. With neither
//! the variable nor `$HOME` there is no directory, and the answer is
//! `None`: a caller says what it could not find rather than writing beside
//! itself.
//!
//! [`Base::resolve`] is the rule as a pure function of the two values, for
//! tests that must not touch the process environment (it is global, and a
//! test that sets it races every other test reading it); the functions
//! below read `std::env` and hand it the values.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// One XDG base directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// `$XDG_DATA_HOME`, else `~/.local/share`: the generated caches
    /// (icons, talents, floors, portraits) and the history store.
    Data,
    /// `$XDG_CACHE_HOME`, else `~/.cache`: what can be thrown away and
    /// rebuilt (the daemon's index checkpoints, decoded game tables).
    Cache,
    /// `$XDG_STATE_HOME`, else `~/.local/state`: logs and working state.
    State,
    /// `$XDG_CONFIG_HOME`, else `~/.config`: `config.toml`.
    Config,
}

impl Base {
    /// The environment variable that names it.
    pub const fn var(self) -> &'static str {
        match self {
            Base::Data => "XDG_DATA_HOME",
            Base::Cache => "XDG_CACHE_HOME",
            Base::State => "XDG_STATE_HOME",
            Base::Config => "XDG_CONFIG_HOME",
        }
    }

    /// Its default, relative to `$HOME`.
    pub const fn fallback(self) -> &'static str {
        match self {
            Base::Data => ".local/share",
            Base::Cache => ".cache",
            Base::State => ".local/state",
            Base::Config => ".config",
        }
    }

    /// The directory, from the variable's value and `$HOME`'s (either
    /// unset as `None`): the rule in the module docs, touching nothing.
    pub fn resolve(self, var: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
        let set = |v: Option<&OsStr>| v.filter(|v| !v.is_empty()).map(PathBuf::from);
        set(var)
            .filter(|p| p.is_absolute())
            .or_else(|| set(home).map(|h| h.join(self.fallback())))
    }

    /// The directory, as this process's environment says.
    pub fn dir(self) -> Option<PathBuf> {
        let var = std::env::var_os(self.var());
        let home = std::env::var_os("HOME");
        self.resolve(var.as_deref(), home.as_deref())
    }
}

/// `$XDG_DATA_HOME`, else `~/.local/share`.
pub fn data_home() -> Option<PathBuf> {
    Base::Data.dir()
}

/// `$XDG_CACHE_HOME`, else `~/.cache`.
pub fn cache_home() -> Option<PathBuf> {
    Base::Cache.dir()
}

/// `$XDG_STATE_HOME`, else `~/.local/state`.
pub fn state_home() -> Option<PathBuf> {
    Base::State.dir()
}

/// `$XDG_CONFIG_HOME`, else `~/.config`.
pub fn config_home() -> Option<PathBuf> {
    Base::Config.dir()
}

/// `rel` under the data home's `wowdps/`: every per-machine cache the
/// generators write (`talents.json`, `class-icons.bin`, `floors/`,
/// `portraits/` …) and the design shots beside them.
pub fn data_path(rel: impl AsRef<Path>) -> Option<PathBuf> {
    data_home().map(|d| d.join("wowdps").join(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(s: &str) -> Option<&OsStr> {
        Some(OsStr::new(s))
    }

    #[test]
    fn a_set_absolute_variable_wins() {
        assert_eq!(
            Base::Data.resolve(os("/xdg/data"), os("/home/k")),
            Some(PathBuf::from("/xdg/data"))
        );
        assert_eq!(
            Base::Config.resolve(os("/xdg/config/"), None),
            Some(PathBuf::from("/xdg/config/"))
        );
    }

    #[test]
    fn each_base_falls_back_under_home() {
        for (base, want) in [
            (Base::Data, "/h/.local/share"),
            (Base::Cache, "/h/.cache"),
            (Base::State, "/h/.local/state"),
            (Base::Config, "/h/.config"),
        ] {
            assert_eq!(base.resolve(None, os("/h")), Some(PathBuf::from(want)));
        }
    }

    /// The spec: an empty variable is unset, and a relative one is
    /// invalid and ignored, so the default under `$HOME` applies.
    #[test]
    fn an_empty_or_relative_variable_counts_as_unset() {
        for var in ["", "relative/data", "./data", "~/data"] {
            assert_eq!(
                Base::Data.resolve(os(var), os("/h")),
                Some(PathBuf::from("/h/.local/share")),
                "{var:?}"
            );
        }
        assert_eq!(Base::State.resolve(os(""), None), None);
        assert_eq!(Base::State.resolve(os("rel"), None), None);
    }

    #[test]
    fn no_home_and_no_variable_is_no_directory() {
        assert_eq!(Base::Cache.resolve(None, None), None);
        assert_eq!(Base::Cache.resolve(None, os("")), None);
    }

    #[test]
    fn the_variables_are_the_specs() {
        assert_eq!(Base::Data.var(), "XDG_DATA_HOME");
        assert_eq!(Base::Cache.var(), "XDG_CACHE_HOME");
        assert_eq!(Base::State.var(), "XDG_STATE_HOME");
        assert_eq!(Base::Config.var(), "XDG_CONFIG_HOME");
    }
}
