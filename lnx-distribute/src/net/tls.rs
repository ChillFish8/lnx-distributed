use std::path::Path;
use std::sync::Arc;

use quinn::{ClientConfig, IdleTimeout, ServerConfig, VarInt};
use rustls::{Certificate, PrivateKey};
use tokio::fs;

use crate::{Error, Result};

/// Reads a cert file and generates the rustls cert from the content.
pub(crate) async fn read_cert(cert: &Path) -> Result<Certificate> {
    let cert = fs::read(cert).await.map_err(Error::TlsFileError)?;

    let cert = Certificate(cert);

    Ok(cert)
}

/// Reads a key file and generates the rustls key from the content.
pub(crate) async fn read_key(key: &Path) -> Result<PrivateKey> {
    let key = fs::read(key).await.map_err(Error::TlsFileError)?;

    let key = PrivateKey(key);

    Ok(key)
}

/// Produces a insecure client that skips server verification.
///
/// This is not recommended for production use.
pub(crate) fn get_insecure_client_config() -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    let mut cfg = ClientConfig::new(Arc::new(crypto));

    Arc::get_mut(&mut cfg.transport)
        .unwrap()
        .max_idle_timeout(Some(IdleTimeout::from(VarInt::from_u32(120_000))));

    cfg
}

/// Produces a secure client that takes a cert and key.
///
/// This is recommended for production use.
pub(crate) fn get_secure_client_config(cert: Certificate) -> Result<ClientConfig> {
    let mut certs = rustls::RootCertStore::empty();
    certs.add(&cert).map_err(|_| {
        Error::TlsError(rustls::Error::General("invalid cert provided".to_string()))
    })?;

    let crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(certs)
        .with_no_client_auth();

    let mut cfg = ClientConfig::new(Arc::new(crypto));

    Arc::get_mut(&mut cfg.transport)
        .unwrap()
        .max_idle_timeout(Some(IdleTimeout::from(VarInt::from_u32(120_000))));

    Ok(cfg)
}

/// Produces a insecure server that uses a self-signed certificate.
///
/// This is designed to work only with the insecure client that doesn't
/// verify the cert. Hence why we generate a completely irrelevant cert.
pub(crate) fn get_insecure_server_config() -> Result<ServerConfig> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert_der = cert.serialize_der().unwrap();
    let key = cert.serialize_private_key_der();
    let key = PrivateKey(key);
    let cert_chain = vec![rustls::Certificate(cert_der)];

    let mut server_config = ServerConfig::with_single_cert(cert_chain, key)?;
    Arc::get_mut(&mut server_config.transport)
        .unwrap()
        .max_concurrent_uni_streams(0_u8.into())
        .max_idle_timeout(Some(IdleTimeout::from(VarInt::from_u32(120_000))));

    Ok(server_config)
}

/// Produces a secure server that uses a given (cert, key) pair.
///
/// This is designed to work only with the insecure client that doesn't
/// verify the cert. Hence why we generate a completely irrelevant cert.
pub(crate) fn get_secure_server_config(
    cert: Certificate,
    key: PrivateKey,
) -> Result<ServerConfig> {
    let mut server_config = ServerConfig::with_single_cert(vec![cert], key)?;
    Arc::get_mut(&mut server_config.transport)
        .unwrap()
        .max_concurrent_uni_streams(0_u8.into())
        .max_idle_timeout(Some(IdleTimeout::from(VarInt::from_u32(120_000))));

    Ok(server_config)
}

struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> core::result::Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}
