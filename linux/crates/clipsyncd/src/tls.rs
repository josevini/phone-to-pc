//! TLS 1.3 with mutual certificates and no CA (spec §4).
//!
//! Certificates are not validated against anything: a peer is whoever holds the
//! private key of the certificate it presents. The handshake signatures are still
//! verified, so the peer's device ID (the hash of its public key) is proven; the
//! engine then decides whether that device is trusted.

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clipsync_core::DeviceId;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    CertificateError, ClientConfig, CommonState, DigitallySignedStruct, DistinguishedName, Error, PeerIncompatible,
    ServerConfig, SignatureScheme,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::storage::Identity;
use crate::storage::identity::device_id_from_cert;

/// ALPN protocol ID (spec §4).
pub const ALPN: &[u8] = clipsync_core::discovery::ALPN.as_bytes();

/// Client and server configurations for one identity.
#[derive(Clone)]
pub struct Tls {
    pub client: Arc<ClientConfig>,
    pub server: Arc<ServerConfig>,
}

impl Tls {
    pub fn new(identity: &Identity) -> Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = Arc::new(AnyPeer::new());
        let cert = || vec![CertificateDer::from(identity.cert_der.clone())];
        let key = || PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_der.clone()));

        let mut client = ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .dangerous()
            .with_custom_certificate_verifier(verifier.clone())
            .with_client_auth_cert(cert(), key())?;
        client.alpn_protocols = vec![ALPN.to_vec()];

        let mut server = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_client_cert_verifier(verifier)
            .with_single_cert(cert(), key())?;
        server.alpn_protocols = vec![ALPN.to_vec()];

        Ok(Tls { client: Arc::new(client), server: Arc::new(server) })
    }

    /// Runs the client handshake and returns the stream with the peer's device ID.
    pub async fn connect<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        stream: S,
    ) -> Result<(tokio_rustls::client::TlsStream<S>, DeviceId)> {
        // Peers are identified by key, not by name: the name is never checked.
        let name = ServerName::try_from("clipsync").expect("valid DNS name");
        let stream = TlsConnector::from(self.client.clone()).connect(name, stream).await?;
        let peer = peer_id(stream.get_ref().1)?;
        Ok((stream, peer))
    }

    /// Runs the server handshake and returns the stream with the peer's device ID.
    pub async fn accept<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        stream: S,
    ) -> Result<(tokio_rustls::server::TlsStream<S>, DeviceId)> {
        let stream = TlsAcceptor::from(self.server.clone()).accept(stream).await?;
        let peer = peer_id(stream.get_ref().1)?;
        Ok((stream, peer))
    }
}

/// Checks the negotiated protocol and reads the peer's device ID from its certificate.
fn peer_id(state: &CommonState) -> Result<DeviceId> {
    if state.alpn_protocol() != Some(ALPN) {
        bail!("peer did not negotiate ALPN {}", String::from_utf8_lossy(ALPN));
    }
    let cert = state.peer_certificates().and_then(|c| c.first()).context("peer presented no certificate")?;
    device_id_from_cert(cert)
}

/// Accepts any well-formed certificate and verifies the handshake signature against its key.
#[derive(Debug)]
pub(crate) struct AnyPeer {
    algorithms: WebPkiSupportedAlgorithms,
}

impl AnyPeer {
    pub(crate) fn new() -> Self {
        let provider: CryptoProvider = rustls::crypto::ring::default_provider();
        AnyPeer { algorithms: provider.signature_verification_algorithms }
    }

    fn check(&self, cert: &CertificateDer<'_>) -> Result<(), Error> {
        device_id_from_cert(cert).map(|_| ()).map_err(|_| Error::InvalidCertificate(CertificateError::BadEncoding))
    }
}

impl ServerCertVerifier for AnyPeer {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        self.check(end_entity).map(|_| ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::PeerIncompatible(PeerIncompatible::Tls13RequiredForQuic))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

impl ClientCertVerifier for AnyPeer {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        self.check(end_entity).map(|_| ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::PeerIncompatible(PeerIncompatible::Tls13RequiredForQuic))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn identity() -> (tempfile::TempDir, Identity) {
        let dir = tempfile::tempdir().unwrap();
        let identity = Identity::load_or_create(dir.path()).unwrap();
        (dir, identity)
    }

    #[tokio::test]
    async fn both_sides_learn_each_others_device_id_and_exchange_data() {
        let ((_d1, alice), (_d2, bob)) = (identity(), identity());
        let (alice_tls, bob_tls) = (Tls::new(&alice).unwrap(), Tls::new(&bob).unwrap());
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let (mut stream, peer) = bob_tls.accept(server_io).await.unwrap();
            let mut buf = [0u8; 5];
            stream.read_exact(&mut buf).await.unwrap();
            stream.write_all(b"world").await.unwrap();
            stream.flush().await.unwrap();
            (peer, buf)
        });
        let (mut stream, peer) = alice_tls.connect(client_io).await.unwrap();
        stream.write_all(b"hello").await.unwrap();
        stream.flush().await.unwrap();
        let mut buf = [0u8; 5];
        stream.read_exact(&mut buf).await.unwrap();
        let (server_saw, server_got) = server.await.unwrap();
        assert_eq!(peer, bob.id);
        assert_eq!(server_saw, alice.id);
        assert_eq!(&server_got, b"hello");
        assert_eq!(&buf, b"world");
    }

    #[tokio::test]
    async fn a_client_without_a_certificate_is_refused() {
        let (_d, bob) = identity();
        let bob_tls = Tls::new(&bob).unwrap();
        let anonymous = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyPeer::new()))
            .with_no_client_auth();
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move { bob_tls.accept(server_io).await.map(|(_, id)| id) });
        let connector = tokio_rustls::TlsConnector::from(Arc::new(anonymous));
        let name = ServerName::try_from("clipsync").unwrap();
        // Keep the client open until the server decides, so only its verdict can fail the handshake.
        let _client = connector.connect(name, client_io).await;
        assert!(server.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn a_client_that_negotiates_no_protocol_is_refused() {
        let ((_d1, alice), (_d2, bob)) = (identity(), identity());
        let mut other = Tls::new(&alice).unwrap();
        let mut client = (*other.client).clone();
        // No ALPN at all: rustls itself only rejects a mismatching one.
        client.alpn_protocols = vec![];
        other.client = Arc::new(client);
        let bob_tls = Tls::new(&bob).unwrap();
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move { bob_tls.accept(server_io).await.map(|(_, id)| id) });
        let _client = other.connect(client_io).await;
        assert!(server.await.unwrap().is_err());
    }

    /// Presents `cert` but signs with `key`: an impostor holding someone else's certificate.
    #[derive(Debug)]
    struct Impostor(Arc<rustls::sign::CertifiedKey>);

    impl rustls::client::ResolvesClientCert for Impostor {
        fn resolve(&self, _: &[&[u8]], _: &[SignatureScheme]) -> Option<Arc<rustls::sign::CertifiedKey>> {
            Some(self.0.clone())
        }

        fn has_certs(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn a_client_presenting_another_devices_certificate_is_refused() {
        let ((_d1, victim), (_d2, mallory), (_d3, bob)) = (identity(), identity(), identity());
        let provider = rustls::crypto::ring::default_provider();
        let key = provider
            .key_provider
            .load_private_key(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(mallory.key_der.clone())))
            .unwrap();
        let stolen = rustls::sign::CertifiedKey::new(vec![CertificateDer::from(victim.cert_der.clone())], key);
        let mut client = ClientConfig::builder_with_provider(Arc::new(provider))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyPeer::new()))
            .with_client_cert_resolver(Arc::new(Impostor(Arc::new(stolen))));
        client.alpn_protocols = vec![ALPN.to_vec()];

        let bob_tls = Tls::new(&bob).unwrap();
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move { bob_tls.accept(server_io).await.map(|(_, id)| id) });
        let connector = tokio_rustls::TlsConnector::from(Arc::new(client));
        // Keep the client open until the server decides, so only its verdict can fail the handshake.
        let _client = connector.connect(ServerName::try_from("clipsync").unwrap(), client_io).await;
        assert!(server.await.unwrap().is_err(), "the server must not accept the victim's identity");
    }

    #[derive(Debug)]
    struct ImpostorServer(Arc<rustls::sign::CertifiedKey>);

    impl rustls::server::ResolvesServerCert for ImpostorServer {
        fn resolve(&self, _: rustls::server::ClientHello<'_>) -> Option<Arc<rustls::sign::CertifiedKey>> {
            Some(self.0.clone())
        }
    }

    #[tokio::test]
    async fn a_server_presenting_another_devices_certificate_is_refused() {
        let ((_d1, victim), (_d2, mallory), (_d3, alice)) = (identity(), identity(), identity());
        let provider = rustls::crypto::ring::default_provider();
        let key = provider
            .key_provider
            .load_private_key(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(mallory.key_der.clone())))
            .unwrap();
        let stolen = rustls::sign::CertifiedKey::new(vec![CertificateDer::from(victim.cert_der.clone())], key);
        let mut server = ServerConfig::builder_with_provider(Arc::new(provider))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_client_cert_verifier(Arc::new(AnyPeer::new()))
            .with_cert_resolver(Arc::new(ImpostorServer(Arc::new(stolen))));
        server.alpn_protocols = vec![ALPN.to_vec()];

        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
        let server = tokio::spawn(async move { acceptor.accept(server_io).await.map(|_| ()) });
        let alice_tls = Tls::new(&alice).unwrap();
        let result = alice_tls.connect(client_io).await.map(|(_, id)| id);
        let _ = server.await;
        assert!(result.is_err(), "the client must not accept the victim's identity: {result:?}");
    }
}
