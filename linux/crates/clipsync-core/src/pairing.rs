//! Pairing primitives: SAS codes (spec §7.3) and pairing URIs (spec §8).

use std::net::SocketAddr;

use sha2::{Digest, Sha256};
use url::Url;

use crate::hex::{Hex, Hex16, Hex32};
use crate::identity::{DeviceId, is_valid_name};

const COMMIT_LABEL: &[u8] = b"clipsync-commit-v1";
const SAS_LABEL: &[u8] = b"clipsync-sas-v1";

/// Commitment the dialer sends in `pair_request` before revealing its nonce.
pub fn sas_commit(nonce_dialer: &Hex32) -> Hex32 {
    Hex(Sha256::new().chain_update(COMMIT_LABEL).chain_update(nonce_dialer.0).finalize().into())
}

/// The 6-digit code both users compare, as a number in `0..1_000_000`.
pub fn sas_code(id_dialer: &DeviceId, id_acceptor: &DeviceId, nonce_dialer: &Hex32, nonce_acceptor: &Hex32) -> u32 {
    let h = Sha256::new()
        .chain_update(SAS_LABEL)
        .chain_update(id_dialer.as_bytes())
        .chain_update(id_acceptor.as_bytes())
        .chain_update(nonce_dialer.0)
        .chain_update(nonce_acceptor.0)
        .finalize();
    u32::from_be_bytes([h[0], h[1], h[2], h[3]]) % 1_000_000
}

/// Formats a SAS code for display: `037 725`.
pub fn format_sas(code: u32) -> String {
    let digits = format!("{code:06}");
    format!("{} {}", &digits[..3], &digits[3..])
}

const URI_SCHEME: &str = "clipsync";
const URI_ACTION: &str = "pair";

/// Contents of the QR code an acceptor shows (spec §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairUri {
    pub id: DeviceId,
    pub name: String,
    pub addrs: Vec<SocketAddr>,
    pub token: Hex16,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UriError {
    #[error("invalid pairing URI: {0}")]
    Invalid(String),
    #[error("unsupported pairing URI version {0}")]
    UnsupportedVersion(String),
}

impl UriError {
    /// The outcome category of spec §8.
    pub fn code(&self) -> &'static str {
        match self {
            UriError::Invalid(_) => "invalid_uri",
            UriError::UnsupportedVersion(_) => "unsupported_version",
        }
    }
}

impl PairUri {
    pub fn parse(uri: &str) -> Result<Self, UriError> {
        let invalid = |msg: &str| UriError::Invalid(msg.to_owned());
        let url = Url::parse(uri).map_err(|e| UriError::Invalid(e.to_string()))?;
        if url.scheme() != URI_SCHEME || url.host_str() != Some(URI_ACTION) || !matches!(url.path(), "" | "/") {
            return Err(invalid("expected clipsync://pair"));
        }

        let (mut v, mut id, mut name, mut token) = (None, None, None, None);
        let mut addrs = Vec::new();
        for (key, value) in url.query_pairs() {
            let slot = match key.as_ref() {
                "v" => &mut v,
                "id" => &mut id,
                "name" => &mut name,
                "token" => &mut token,
                "addr" => {
                    let addr: SocketAddr = value.parse().map_err(|_| invalid("addr must be ip:port"))?;
                    if addr.port() == 0 {
                        return Err(invalid("addr port must not be 0"));
                    }
                    addrs.push(addr);
                    continue;
                }
                _ => continue,
            };
            if slot.replace(value.into_owned()).is_some() {
                return Err(UriError::Invalid(format!("duplicate parameter `{key}`")));
            }
        }

        match v.as_deref() {
            Some("1") => {}
            Some(other) => return Err(UriError::UnsupportedVersion(other.to_owned())),
            None => return Err(invalid("missing v")),
        }
        let id = id.ok_or_else(|| invalid("missing id"))?.parse().map_err(|_| invalid("bad id"))?;
        let name = name.filter(|n| is_valid_name(n)).ok_or_else(|| invalid("name must be 1-64 bytes"))?;
        let token = token.ok_or_else(|| invalid("missing token"))?.parse().map_err(|_| invalid("bad token"))?;
        if addrs.is_empty() {
            return Err(invalid("missing addr"));
        }
        Ok(Self { id, name, addrs, token })
    }

    pub fn to_uri(&self) -> String {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("v", "1");
        query.append_pair("id", &self.id.to_string());
        query.append_pair("name", &self.name);
        for addr in &self.addrs {
            query.append_pair("addr", &addr.to_string());
        }
        query.append_pair("token", &self.token.to_string());
        format!("{URI_SCHEME}://{URI_ACTION}?{}", query.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_round_trips() {
        let uri = PairUri {
            id: DeviceId(Hex32::random()),
            name: "Meu PC + notebook".into(),
            addrs: vec!["192.168.0.10:47823".parse().unwrap(), "[fd00::2]:5000".parse().unwrap()],
            token: Hex16::random(),
        };
        assert_eq!(PairUri::parse(&uri.to_uri()), Ok(uri));
    }

    #[test]
    fn sas_is_formatted_with_leading_zeros() {
        assert_eq!(format_sas(37_725), "037 725");
    }
}
