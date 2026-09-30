//! `state.json`: what the daemon remembers on its own — paired devices, the Lamport counter and whether sharing is
//! paused.

use std::path::Path;

use anyhow::{Context, Result};
use clipsync_core::DeviceId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub lamport: u64,
    pub paired: Vec<PairedRecord>,
    #[serde(default)]
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedRecord {
    pub id: DeviceId,
    pub name: String,
}

impl State {
    /// Reads `path`; a missing file means a fresh state.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("{} is damaged", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(self).expect("state always serialises");
        super::write_private(path, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipsync_core::Hex;

    #[test]
    fn a_missing_file_is_a_fresh_state() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(State::load(&tmp.path().join("state.json")).unwrap(), State::default());
    }

    #[test]
    fn saves_and_reloads() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("data/state.json");
        let state = State {
            lamport: 42,
            paired: vec![PairedRecord { id: DeviceId(Hex([7; 32])), name: "phone".into() }],
            paused: true,
        };
        state.save(&path).unwrap();
        assert_eq!(State::load(&path).unwrap(), state);
    }

    #[test]
    fn a_file_without_the_paused_flag_is_not_paused() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.json");
        std::fs::write(&path, r#"{"lamport":3,"paired":[]}"#).unwrap();
        assert_eq!(State::load(&path).unwrap(), State { lamport: 3, paired: vec![], paused: false });
    }

    #[test]
    fn a_damaged_file_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.json");
        std::fs::write(&path, "{").unwrap();
        assert!(State::load(&path).is_err());
    }
}
