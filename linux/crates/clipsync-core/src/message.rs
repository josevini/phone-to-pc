//! Wire messages (spec §5).

use serde::{Deserialize, Serialize};

use crate::clip::{MAX_TEXT_LEN, text_digest};
use crate::hex::{Hex16, Hex32};
use crate::identity::{DeviceId, is_valid_name};

/// Protocol version implemented by this crate.
pub const PROTO_VERSION: u32 = 1;

/// Every `type` this version understands. Anything else is ignored (spec §4.1).
pub const KNOWN_TYPES: &[&str] = &[
    "hello",
    "ping",
    "pong",
    "unpair",
    "error",
    "clip",
    "ack",
    "pair_request",
    "pair_nonce",
    "pair_reveal",
    "pair_result",
];

// Field-less messages are empty struct variants rather than unit variants so
// that unknown fields on them are ignored like on every other message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Hello(Hello),
    Ping {},
    Pong {},
    Unpair {},
    Error(ErrorMsg),
    Clip(Clip),
    Ack(Ack),
    PairRequest(PairRequest),
    PairNonce(PairNonce),
    PairReveal(PairNonce),
    PairResult(PairResult),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub proto: u32,
    pub id: DeviceId,
    pub name: String,
    pub platform: String,
    pub caps: Vec<String>,
    /// The sender's Lamport counter (spec §6).
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorMsg {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Error codes of spec §5.1.
pub mod error_code {
    pub const UNSUPPORTED_VERSION: &str = "unsupported_version";
    pub const IDENTITY_MISMATCH: &str = "identity_mismatch";
    pub const NOT_PAIRED: &str = "not_paired";
    pub const PAIRING_CLOSED: &str = "pairing_closed";
    pub const BAD_TOKEN: &str = "bad_token";
    pub const PROTOCOL_ERROR: &str = "protocol_error";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clip {
    pub id: Hex16,
    pub origin: DeviceId,
    pub seq: u64,
    /// Unix time in milliseconds; informative only.
    pub ts: u64,
    pub mime: String,
    pub text: String,
    pub sha256: Hex32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    pub id: Hex16,
    pub applied: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairMethod {
    Token,
    Sas,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    pub method: PairMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<Hex16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<Hex32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairNonce {
    pub nonce: Hex32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Message {
    /// Checks the rules of spec §5 that the type system does not already enforce.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Message::Hello(h) if !is_valid_name(&h.name) => {
                Err(format!("hello.name must be 1-64 bytes, got {}", h.name.len()))
            }
            Message::Clip(c) => c.validate(),
            Message::PairRequest(r) => match r.method {
                PairMethod::Token if r.token.is_none() => Err("pair_request token without token".into()),
                PairMethod::Sas if r.commit.is_none() => Err("pair_request sas without commit".into()),
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }
}

impl Clip {
    fn validate(&self) -> Result<(), String> {
        if self.seq == 0 {
            return Err("clip.seq must be >= 1".into());
        }
        if self.text.is_empty() {
            return Err("clip.text must not be empty".into());
        }
        if self.text.len() > MAX_TEXT_LEN {
            return Err(format!("clip.text is {} bytes, limit is {MAX_TEXT_LEN}", self.text.len()));
        }
        if text_digest(&self.text) != self.sha256 {
            return Err("clip.sha256 does not match text".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::TEXT_MIME;
    use crate::hex::Hex;

    #[test]
    fn rejects_clip_text_over_1_mib() {
        let text = "a".repeat(MAX_TEXT_LEN + 1);
        let clip = Clip {
            id: Hex([0; 16]),
            origin: DeviceId(Hex([1; 32])),
            seq: 1,
            ts: 0,
            mime: TEXT_MIME.into(),
            sha256: text_digest(&text),
            text,
        };
        let err = Message::Clip(clip).validate().unwrap_err();
        assert!(err.contains("limit is 1048576"), "{err}");
    }
}
