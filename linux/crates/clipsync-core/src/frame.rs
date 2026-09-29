//! Length-prefixed JSON framing (spec §4.1).

use crate::message::{KNOWN_TYPES, Message};

/// Largest accepted payload, in bytes.
pub const MAX_FRAME_LEN: usize = 8 * 1024 * 1024;

const HEADER_LEN: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("empty frame")]
    EmptyFrame,
    #[error("frame of {0} bytes exceeds the {MAX_FRAME_LEN} byte limit")]
    FrameTooLarge(usize),
    #[error("payload is not valid UTF-8 JSON")]
    InvalidJson,
    #[error("invalid message: {0}")]
    InvalidMessage(String),
}

impl FrameError {
    /// The outcome category of spec §4.1.
    pub fn code(&self) -> &'static str {
        match self {
            FrameError::EmptyFrame => "empty_frame",
            FrameError::FrameTooLarge(_) => "frame_too_large",
            FrameError::InvalidJson => "invalid_json",
            FrameError::InvalidMessage(_) => "invalid_message",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoded {
    Message(Message),
    /// A well-formed message of a type this version does not know.
    Ignored {
        kind: String,
    },
}

/// Serialises `msg` into a complete frame, header included.
pub fn encode(msg: &Message) -> Vec<u8> {
    let payload = serde_json::to_vec(msg).expect("messages always serialise");
    debug_assert!(payload.len() <= MAX_FRAME_LEN, "text limit keeps frames under 8 MiB");
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&payload);
    out
}

/// Decodes one frame payload (without the length header).
pub fn decode_payload(payload: &[u8]) -> Result<Decoded, FrameError> {
    if payload.is_empty() {
        return Err(FrameError::EmptyFrame);
    }
    let value: serde_json::Value = serde_json::from_slice(payload).map_err(|_| FrameError::InvalidJson)?;
    let kind = match value.get("type") {
        _ if !value.is_object() => return Err(FrameError::InvalidMessage("payload is not an object".into())),
        Some(serde_json::Value::String(kind)) => kind.clone(),
        _ => return Err(FrameError::InvalidMessage("missing string field `type`".into())),
    };
    if !KNOWN_TYPES.contains(&kind.as_str()) {
        return Ok(Decoded::Ignored { kind });
    }
    let msg: Message = serde_json::from_value(value).map_err(|e| FrameError::InvalidMessage(e.to_string()))?;
    msg.validate().map_err(FrameError::InvalidMessage)?;
    Ok(Decoded::Message(msg))
}

/// Incremental decoder: feed it bytes as they arrive, pull complete frames out.
///
/// After an error the stream is unusable and the connection must be closed.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Returns the next complete frame, or `None` if more bytes are needed.
    pub fn next_frame(&mut self) -> Result<Option<Decoded>, FrameError> {
        let Some(header) = self.buf.first_chunk::<HEADER_LEN>() else {
            return Ok(None);
        };
        let len = u32::from_be_bytes(*header) as usize;
        if len == 0 {
            return Err(FrameError::EmptyFrame);
        }
        if len > MAX_FRAME_LEN {
            return Err(FrameError::FrameTooLarge(len));
        }
        if self.buf.len() < HEADER_LEN + len {
            return Ok(None);
        }
        let decoded = decode_payload(&self.buf[HEADER_LEN..HEADER_LEN + len]);
        self.buf.drain(..HEADER_LEN + len);
        decoded.map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_byte_by_byte_and_back_to_back() {
        let mut stream = encode(&Message::Ping {});
        stream.extend(encode(&Message::Pong {}));
        let mut dec = FrameDecoder::new();
        let mut out = Vec::new();
        for b in stream {
            dec.push(&[b]);
            while let Some(d) = dec.next_frame().unwrap() {
                out.push(d);
            }
        }
        assert_eq!(out, vec![Decoded::Message(Message::Ping {}), Decoded::Message(Message::Pong {})]);
    }

    #[test]
    fn rejects_empty_payload() {
        assert_eq!(decode_payload(&[]), Err(FrameError::EmptyFrame));
    }

    #[test]
    fn rejects_oversized_header_before_body_arrives() {
        let mut dec = FrameDecoder::new();
        dec.push(&((MAX_FRAME_LEN as u32) + 1).to_be_bytes());
        assert_eq!(dec.next_frame(), Err(FrameError::FrameTooLarge(MAX_FRAME_LEN + 1)));
    }
}
