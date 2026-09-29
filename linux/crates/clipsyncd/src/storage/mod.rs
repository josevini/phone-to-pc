//! Files the daemon keeps between runs: identity, configuration and state.

pub mod config;
pub mod identity;
pub mod state;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub use config::Config;
pub use identity::Identity;
pub use state::State;

/// Where clipsync keeps its files, following the XDG base directory spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    /// `$XDG_DATA_HOME/clipsync`: identity and state.
    pub data: PathBuf,
    /// `$XDG_CONFIG_HOME/clipsync`: `config.toml`.
    pub config: PathBuf,
    /// `$XDG_RUNTIME_DIR`: the control socket.
    pub runtime: PathBuf,
}

impl Dirs {
    pub fn from_env() -> Result<Self> {
        Self::from_vars(|name| std::env::var_os(name).map(PathBuf::from))
    }

    /// Resolves the directories from environment variables read through `var`.
    pub fn from_vars(var: impl Fn(&str) -> Option<PathBuf>) -> Result<Self> {
        let absolute = |name: &str| var(name).filter(|p| p.is_absolute());
        let home = || absolute("HOME").context("HOME is not set to an absolute path");
        let data = match absolute("XDG_DATA_HOME") {
            Some(dir) => dir,
            None => home()?.join(".local/share"),
        };
        let config = match absolute("XDG_CONFIG_HOME") {
            Some(dir) => dir,
            None => home()?.join(".config"),
        };
        let Some(runtime) = absolute("XDG_RUNTIME_DIR") else {
            bail!("XDG_RUNTIME_DIR is not set; clipsync needs a user session");
        };
        Ok(Dirs { data: data.join("clipsync"), config: config.join("clipsync"), runtime })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    pub fn state_file(&self) -> PathBuf {
        self.data.join("state.json")
    }

    pub fn socket(&self) -> PathBuf {
        self.runtime.join("clipsync.sock")
    }
}

/// Creates `dir` (and its parents) readable only by the user.
pub(crate) fn create_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("creating {}", dir.display()))
}

/// Writes `contents` to `path` atomically (temporary file + rename), readable only by the user.
pub(crate) fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let dir = path.parent().context("path has no parent directory")?;
    create_private_dir(dir)?;
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("writing {}", tmp.display()))?;
    file.write_all(contents)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<PathBuf> + 'a {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| PathBuf::from(v))
    }

    #[test]
    fn uses_xdg_variables_when_set() {
        let dirs = Dirs::from_vars(vars(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "/d"),
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]))
        .unwrap();
        assert_eq!(dirs.data, PathBuf::from("/d/clipsync"));
        assert_eq!(dirs.config_file(), PathBuf::from("/c/clipsync/config.toml"));
        assert_eq!(dirs.state_file(), PathBuf::from("/d/clipsync/state.json"));
        assert_eq!(dirs.socket(), PathBuf::from("/run/user/1000/clipsync.sock"));
    }

    #[test]
    fn falls_back_to_home_and_ignores_relative_paths() {
        let dirs = Dirs::from_vars(vars(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "relative"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]))
        .unwrap();
        assert_eq!(dirs.data, PathBuf::from("/home/u/.local/share/clipsync"));
        assert_eq!(dirs.config, PathBuf::from("/home/u/.config/clipsync"));
    }

    #[test]
    fn requires_a_runtime_dir_and_a_home() {
        assert!(Dirs::from_vars(vars(&[("HOME", "/home/u")])).is_err());
        assert!(Dirs::from_vars(vars(&[("XDG_RUNTIME_DIR", "/run/user/1000")])).is_err());
    }

    #[test]
    fn private_files_are_replaced_atomically_with_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested/file.json");
        write_private(&path, b"one").unwrap();
        write_private(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(!path.with_extension("tmp").exists());
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }
}
