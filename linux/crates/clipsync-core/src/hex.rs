use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// Fixed-size bytes carried as a hex string on the wire (spec §2, §5).
///
/// Parsing accepts either case; formatting is always lowercase.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hex<const N: usize>(pub [u8; N]);

/// A random 16-byte value (clip IDs, pairing tokens).
pub type Hex16 = Hex<16>;
/// A 32-byte value (SHA-256 digests, SAS nonces).
pub type Hex32 = Hex<32>;

impl<const N: usize> Hex<N> {
    /// Fresh random bytes from the OS-seeded thread RNG.
    pub fn random() -> Self {
        let mut bytes = [0u8; N];
        rand::fill(&mut bytes[..]);
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; N] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("expected {expected} hex characters")]
pub struct HexError {
    pub expected: usize,
}

impl<const N: usize> FromStr for Hex<N> {
    type Err = HexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0u8; N];
        hex::decode_to_slice(s, &mut bytes).map_err(|_| HexError { expected: N * 2 })?;
        Ok(Self(bytes))
    }
}

impl<const N: usize> fmt::Display for Hex<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl<const N: usize> fmt::Debug for Hex<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl<const N: usize> Serialize for Hex<N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de, const N: usize> Deserialize<'de> for Hex<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HexVisitor<const N: usize>;

        impl<const N: usize> Visitor<'_> for HexVisitor<N> {
            type Value = Hex<N>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a string of {} hex characters", N * 2)
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                v.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(HexVisitor::<N>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_either_case_and_formats_lowercase() {
        let h: Hex<2> = "aBcD".parse().unwrap();
        assert_eq!(h.0, [0xab, 0xcd]);
        assert_eq!(h.to_string(), "abcd");
    }

    #[test]
    fn rejects_wrong_length_and_non_hex() {
        assert!("abc".parse::<Hex<2>>().is_err());
        assert!("abcdef".parse::<Hex<2>>().is_err());
        assert!("zzzz".parse::<Hex<2>>().is_err());
    }

    #[test]
    fn debug_is_plain_hex() {
        assert_eq!(format!("{:?}", Hex([0xab, 0x01])), "ab01");
    }

    #[test]
    fn deserialize_error_names_the_expected_length() {
        let err = serde_json::from_str::<Hex<2>>("12").unwrap_err().to_string();
        assert!(err.contains("a string of 4 hex characters"), "{err}");
    }
}
