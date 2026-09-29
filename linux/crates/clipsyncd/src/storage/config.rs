//! `config.toml`: settings the user edits.

use std::net::SocketAddr;
use std::path::Path;

use anyhow::{Context, Result, bail};
use clipsync_core::identity::{MAX_NAME_LEN, is_valid_name};
use serde::Deserialize;

/// Default TCP port (spec §3).
pub const DEFAULT_PORT: u16 = 47823;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Name shown to other devices (1–64 bytes).
    pub name: String,
    /// TCP port to listen on; 0 lets the system choose.
    pub port: u16,
    /// Addresses to dial besides the ones found with mDNS.
    pub peers: Vec<SocketAddr>,
}

impl Config {
    /// Reads `path`; a missing file means every default, with `default_name` as the name.
    pub fn load(path: &Path, default_name: impl FnOnce() -> String) -> Result<Self> {
        let file: File = match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).with_context(|| format!("{} is not valid", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => File::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let name = file.name.unwrap_or_else(default_name);
        if !is_valid_name(&name) {
            bail!("{}: name must be 1-{MAX_NAME_LEN} bytes, got {}", path.display(), name.len());
        }
        let peers = file
            .peers
            .iter()
            .map(|p| p.parse().with_context(|| format!("{}: peer {p:?} is not an ip:port address", path.display())))
            .collect::<Result<_>>()?;
        Ok(Config { name, port: file.port.unwrap_or(DEFAULT_PORT), peers })
    }
}

/// The file as written; every key is optional.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    name: Option<String>,
    port: Option<u16>,
    #[serde(default)]
    peers: Vec<String>,
}

/// The machine's hostname, cut to a valid device name.
pub fn default_device_name() -> String {
    let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    let mut name = hostname.trim().to_owned();
    while name.len() > MAX_NAME_LEN {
        name.pop();
    }
    if name.is_empty() { "clipsync".to_owned() } else { name }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(contents: Option<&str>) -> Result<Config> {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        if let Some(contents) = contents {
            std::fs::write(&path, contents).unwrap();
        }
        Config::load(&path, || "my-host".into())
    }

    #[test]
    fn a_missing_file_means_defaults() {
        assert_eq!(load(None).unwrap(), Config { name: "my-host".into(), port: DEFAULT_PORT, peers: vec![] });
    }

    #[test]
    fn reads_every_setting() {
        let config =
            load(Some("name = \"Meu PC\"\nport = 5000\npeers = [\"192.168.0.20:47823\", \"[fd00::2]:1\"]\n")).unwrap();
        assert_eq!(config.name, "Meu PC");
        assert_eq!(config.port, 5000);
        assert_eq!(config.peers, vec!["192.168.0.20:47823".parse().unwrap(), "[fd00::2]:1".parse().unwrap()]);
    }

    #[test]
    fn unset_settings_keep_their_defaults() {
        assert_eq!(load(Some("port = 1\n")).unwrap().name, "my-host");
    }

    #[test]
    fn rejects_invalid_names_unknown_keys_and_bad_addresses() {
        assert!(load(Some("name = \"\"\n")).is_err());
        assert!(load(Some(&format!("name = \"{}\"\n", "a".repeat(65)))).is_err());
        assert!(load(Some("nmae = \"typo\"\n")).is_err());
        assert!(load(Some("peers = [\"my-pc.local\"]\n")).is_err());
    }

    #[test]
    fn the_default_name_is_a_valid_device_name() {
        let name = default_device_name();
        assert!(clipsync_core::identity::is_valid_name(&name), "{name:?}");
    }
}
