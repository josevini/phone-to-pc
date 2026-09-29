//! The device's key pair and self-signed certificate (spec §2).

use std::path::Path;

use anyhow::{Context, Result};
use clipsync_core::DeviceId;
use rcgen::{CertificateParams, DnType, KeyPair, PKCS_ECDSA_P256_SHA256, PublicKeyData};

use super::{create_private_dir, write_private};

const KEY_FILE: &str = "identity.key";
const CERT_FILE: &str = "identity.crt";

/// The local device's TLS identity.
#[derive(Clone)]
pub struct Identity {
    pub id: DeviceId,
    /// Self-signed X.509 certificate, DER.
    pub cert_der: Vec<u8>,
    /// EC P-256 private key, PKCS#8 DER.
    pub key_der: Vec<u8>,
}

impl Identity {
    /// Loads the identity from `dir`, creating it on first run.
    ///
    /// The identity is the key: if the certificate is missing or does not match the key,
    /// a new certificate is issued for the same key, so the device ID never changes.
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        create_private_dir(dir)?;
        let key_path = dir.join(KEY_FILE);
        let key = if key_path.exists() {
            let pem = std::fs::read_to_string(&key_path).with_context(|| format!("reading {}", key_path.display()))?;
            KeyPair::from_pem(&pem).with_context(|| format!("{} is not a valid private key", key_path.display()))?
        } else {
            let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
            write_private(&key_path, key.serialize_pem().as_bytes())?;
            key
        };
        let id = DeviceId::from_spki_der(&key.subject_public_key_info());

        let cert_path = dir.join(CERT_FILE);
        let existing = std::fs::read_to_string(&cert_path)
            .ok()
            .and_then(|text| pem::parse(text).ok())
            .map(|p| p.into_contents())
            .filter(|der| device_id_from_cert(der).is_ok_and(|cert_id| cert_id == id));
        let cert_der = match existing {
            Some(der) => der,
            None => {
                let cert = self_signed(&key)?;
                write_private(&cert_path, cert.pem().as_bytes())?;
                cert.der().to_vec()
            }
        };
        Ok(Identity { id, cert_der, key_der: key.serialize_der() })
    }
}

/// Device ID of the certificate's owner: SHA-256 of its SubjectPublicKeyInfo.
pub fn device_id_from_cert(cert_der: &[u8]) -> Result<DeviceId> {
    let (_, cert) = x509_parser::parse_x509_certificate(cert_der).context("not an X.509 certificate")?;
    Ok(DeviceId::from_spki_der(cert.tbs_certificate.subject_pki.raw))
}

/// Peers ignore everything in the certificate but the public key (spec §2).
fn self_signed(key: &KeyPair) -> Result<rcgen::Certificate> {
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.distinguished_name.push(DnType::CommonName, "clipsync");
    params.not_before = rcgen::date_time_ymd(2000, 1, 1);
    params.not_after = rcgen::date_time_ymd(9999, 12, 31);
    Ok(params.self_signed(key)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Certificate whose device ID was computed by an independent implementation of the spec.
    const KNOWN_CERT: &str = "-----BEGIN CERTIFICATE-----
MIIBfjCCASOgAwIBAgIUZj4BFs+ijOLxWQK54HKusq6OsiowCgYIKoZIzj0EAwIw
EzERMA8GA1UEAwwIY2xpcHN5bmMwIBcNMjYwOTI5MTQzMjA3WhgPMjEyNjA5MDUx
NDMyMDdaMBMxETAPBgNVBAMMCGNsaXBzeW5jMFkwEwYHKoZIzj0CAQYIKoZIzj0D
AQcDQgAEGoZmtUX/T2107qpMKrDLFRTiVjFJlBOgecPsEbX4uFG2zrUcBxD3hABm
S5EpxLUHBCIx5V/DVGB6CYINCupxhqNTMFEwHQYDVR0OBBYEFJJQS7uVizz4YjVx
C61emjq+gJGsMB8GA1UdIwQYMBaAFJJQS7uVizz4YjVxC61emjq+gJGsMA8GA1Ud
EwEB/wQFMAMBAf8wCgYIKoZIzj0EAwIDSQAwRgIhAMUPnExnGectIWBwaAHFWJ4p
Z2DK/hzE7XBZOWwWXzqKAiEA+rAcdI4kLN1wrkOZwwBCdDng9S3xC89zyKjaaonP
r1s=
-----END CERTIFICATE-----";

    #[test]
    fn device_id_is_the_hash_of_the_certificate_public_key() {
        let der = pem::parse(KNOWN_CERT).unwrap().into_contents();
        assert_eq!(
            device_id_from_cert(&der).unwrap().to_string(),
            "86224755c0ff3b3b412a5da3ef12466cb12c48326e3454c02687f2cc88771027"
        );
    }

    #[test]
    fn rejects_something_that_is_not_a_certificate() {
        assert!(device_id_from_cert(b"not a certificate").is_err());
    }

    #[test]
    fn is_created_once_and_reloaded_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("clipsync");
        let first = Identity::load_or_create(&dir).unwrap();
        let second = Identity::load_or_create(&dir).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.cert_der, second.cert_der);
        assert_eq!(first.id, device_id_from_cert(&first.cert_der).unwrap());
        let mode = std::fs::metadata(dir.join("identity.key")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn different_devices_get_different_ids() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        assert_ne!(Identity::load_or_create(a.path()).unwrap().id, Identity::load_or_create(b.path()).unwrap().id);
    }

    #[test]
    fn a_certificate_that_does_not_match_the_key_is_reissued_for_the_same_id() {
        let (mine, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let original = Identity::load_or_create(mine.path()).unwrap();
        Identity::load_or_create(other.path()).unwrap();
        std::fs::copy(other.path().join("identity.crt"), mine.path().join("identity.crt")).unwrap();
        let reloaded = Identity::load_or_create(mine.path()).unwrap();
        assert_eq!(reloaded.id, original.id);
        assert_eq!(device_id_from_cert(&reloaded.cert_der).unwrap(), original.id);
    }

    #[test]
    fn a_damaged_key_is_an_error_not_a_new_identity() {
        let tmp = tempfile::tempdir().unwrap();
        Identity::load_or_create(tmp.path()).unwrap();
        std::fs::write(tmp.path().join("identity.key"), "garbage").unwrap();
        assert!(Identity::load_or_create(tmp.path()).is_err());
    }
}
