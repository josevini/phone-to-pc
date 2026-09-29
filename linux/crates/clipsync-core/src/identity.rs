use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hex::{Hex32, HexError};

/// Maximum device name length in bytes of UTF-8 (spec §2).
pub const MAX_NAME_LEN: usize = 64;

/// A device's identity: SHA-256 of its public key's SubjectPublicKeyInfo DER (spec §2).
///
/// Ordering is by raw bytes, which is the order spec §6 and §7.1 compare with.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(pub Hex32);

impl DeviceId {
    pub fn from_spki_der(spki: &[u8]) -> Self {
        Self(crate::hex::Hex(Sha256::digest(spki).into()))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_bytes()
    }

    /// First 8 hex characters, for display (spec §2).
    pub fn short(&self) -> String {
        self.to_string()[..8].to_owned()
    }
}

impl FromStr for DeviceId {
    type Err = HexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceId({})", self.short())
    }
}

/// Whether `name` is an acceptable device name (1–64 bytes).
pub fn is_valid_name(name: &str) -> bool {
    (1..=MAX_NAME_LEN).contains(&name.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_form_and_debug_show_the_first_8_hex_chars() {
        let id: DeviceId = "86224755c0ff3b3b412a5da3ef12466cb12c48326e3454c02687f2cc88771027".parse().unwrap();
        assert_eq!(id.short(), "86224755");
        assert_eq!(format!("{id:?}"), "DeviceId(86224755)");
    }
}
