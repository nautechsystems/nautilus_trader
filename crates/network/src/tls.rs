// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Wraps raw socket streams with TLS encryption and builds `rustls` client configurations from
//! certificate directories.

use std::{convert::TryFrom, fs::File, io::BufReader, path::Path, sync::Arc};

use nautilus_cryptography::{providers::install_cryptographic_provider, tls::create_tls_config};
use rustls::{
    ClientConfig,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName},
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::TlsConnector;
use tokio_tungstenite::{
    MaybeTlsStream,
    tungstenite::{Error, error::TlsError, handshake::client::Request, stream::Mode},
};

pub(crate) async fn tcp_tls<S>(
    request: &Request,
    mode: Mode,
    stream: S,
    connector: Option<Arc<ClientConfig>>,
) -> Result<MaybeTlsStream<S>, Error>
where
    S: 'static + AsyncRead + AsyncWrite + Send + Unpin,
    MaybeTlsStream<S>: Unpin,
{
    let domain = domain(request)?;

    wrap_stream(stream, domain, mode, connector).await
}

async fn wrap_stream<S>(
    socket: S,
    domain: String,
    mode: Mode,
    tls_config: Option<Arc<ClientConfig>>,
) -> Result<MaybeTlsStream<S>, Error>
where
    S: 'static + AsyncRead + AsyncWrite + Send + Unpin,
{
    match mode {
        Mode::Plain => Ok(MaybeTlsStream::Plain(socket)),
        Mode::Tls => {
            let config = tls_config.unwrap_or_else(create_tls_config);
            let domain = ServerName::try_from(domain.as_str())
                .map_err(|_| TlsError::InvalidDnsName)?
                .to_owned();
            let stream = TlsConnector::from(config).connect(domain, socket).await?;
            Ok(MaybeTlsStream::Rustls(stream))
        }
    }
}

/// Extracts the host name from the request URI.
///
/// # Errors
///
/// Returns an error if the request URI has no host component.
fn domain(request: &Request) -> Result<String, Error> {
    match request.uri().host() {
        // rustls expects IPv6 addresses without the surrounding [] brackets
        Some(d) if d.starts_with('[') && d.ends_with(']') => Ok(d[1..d.len() - 1].to_string()),
        Some(d) => Ok(d.to_string()),
        None => Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Request URI missing host component",
        ))),
    }
}

pub(crate) fn create_tls_config_from_certs_dir(
    certs_dir: &Path,
    require_client_auth: bool,
) -> anyhow::Result<rustls::ClientConfig> {
    install_cryptographic_provider();

    if !certs_dir.is_dir() {
        anyhow::bail!(
            "Certificate path is not a directory: {}",
            certs_dir.display()
        );
    }

    let mut all_certs: Vec<(std::path::PathBuf, Vec<CertificateDer<'static>>)> = Vec::new();
    let mut client_key = None;
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    // Sort entries for deterministic cert/key selection across platforms
    let mut entries: Vec<_> = std::fs::read_dir(certs_dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::path);

    for entry in entries {
        let path = entry.path();

        if client_key.is_none()
            && let Ok(key) = load_private_key(&path)
        {
            client_key = Some(key);
            // No early continue: a combined PEM carries the certificate alongside
            // the key, so this file is still scanned for certificates below.
        }

        if let Ok(certs) = load_certs(&path)
            && !certs.is_empty()
        {
            all_certs.push((path, certs));
        }
    }

    // If key found, find the matching client cert by trial validation
    let client_cert = if let Some(ref key) = client_key
        && !all_certs.is_empty()
    {
        let mut matched = None;

        for i in 0..all_certs.len() {
            let test_config = rustls::ClientConfig::builder()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_client_auth_cert(all_certs[i].1.clone(), key.clone_key());

            if test_config.is_ok() {
                let (path, cert) = all_certs.remove(i);
                log::debug!("Matched client certificate from {}", path.display());
                matched = Some(cert);
                break;
            }
        }

        if matched.is_none() {
            log::warn!(
                "Private key found but no matching client certificate in {}",
                certs_dir.display()
            );
        }
        matched
    } else {
        None
    };

    for (path, certs) in all_certs {
        for cert in certs {
            if let Err(e) = root_store.add(cert) {
                log::warn!("Invalid certificate in {}: {e}", path.display());
            }
        }
    }

    let builder = rustls::ClientConfig::builder().with_root_certificates(root_store);

    if let (Some(cert), Some(key)) = (client_cert, client_key) {
        return Ok(builder.with_client_auth_cert(cert, key)?);
    }

    if require_client_auth {
        anyhow::bail!(
            "Client certificate or private key missing in {} but client auth required",
            certs_dir.display(),
        );
    }

    log::debug!(
        "No TLS client certificate/key pair found in {}; proceeding without client authentication",
        certs_dir.display(),
    );

    Ok(builder.with_no_client_auth())
}

fn load_private_key(path: &Path) -> anyhow::Result<PrivateKeyDer<'static>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    if let Some(key) = rustls_pemfile::pkcs8_private_keys(&mut reader).find_map(Result::ok) {
        return Ok(key.into());
    }

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    if let Some(key) = rustls_pemfile::rsa_private_keys(&mut reader).find_map(Result::ok) {
        return Ok(key.into());
    }

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    if let Some(key) = rustls_pemfile::ec_private_keys(&mut reader).find_map(Result::ok) {
        return Ok(key.into());
    }

    anyhow::bail!("No valid private key found in {}", path.display());
}

fn load_certs(path: &Path) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader)
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(certs)
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use rstest::rstest;
    use rustls::{
        ClientConnection, Connection, ServerConnection,
        pki_types::{PrivatePkcs8KeyDer, ServerName},
        server::WebPkiClientVerifier,
    };

    use super::*;

    // Test certificates generated with:
    // openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem -days 3650 -nodes
    const TEST_CERT: &str = "-----BEGIN CERTIFICATE-----
MIIDCTCCAfGgAwIBAgIUXzkvs6Ax5p8YYbc6KPC4x1sZuqgwDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MDEwODIzNTYxMVoXDTM2MDEw
NjIzNTYxMVowFDESMBAGA1UEAwwJbG9jYWxob3N0MIIBIjANBgkqhkiG9w0BAQEF
AAOCAQ8AMIIBCgKCAQEAsa03TrY+zTXNonP40Fa8Ui9y6WMG8KmclvHl6nDLxiXb
CwxDHRCP2g7ThaWrqUaise1/K4LA5yH1+l4qUZ3MmpLo5f4RgyzgOc9OPoRT/weh
O78G+6+O82MCYxGUMDAya6Q6k7Zvc/HfdoUJhkDpiWVBQpWOH+kpM5O084MRGucn
AdhbuPVo/V5w9++td1rUcv75NhGxI47A/yy/ZffCRklnh+M8YejjwRJI14uhAAnO
h6el8A9Qwgb2nuyUg7pAKenkIuYFMidqnCwEAcE9ix0re+A+H11MqWVIUeHW6fI2
gfv9FWkZDka/76YAuCe2eLZ6WR6ubk3wcSuqdx898wIDAQABo1MwUTAdBgNVHQ4E
FgQUew+Y/26vcPPfyLkqc7pGMvOlNigwHwYDVR0jBBgwFoAUew+Y/26vcPPfyLkq
c7pGMvOlNigwDwYDVR0TAQH/BAUwAwEB/zANBgkqhkiG9w0BAQsFAAOCAQEATTd1
Jsi3yi3MXf3GPAohdbVBdOixQj5/m8Ne/w3YtYBtUhViAiXxYyDPZeKmRd35dxyr
0Mb0NT6TAitchhKnHej4tQrco6Ou/cBUX5Wp5AmCXqCbG8st/iFUnfuxZ8khdVx9
nLkvYWLN+KVV8rAs+dYnHhWZhXaso28/1XP81iT27uXMlUv0LsTXn0+EsA5q1fSE
+6vX6mRHix+Y5FOuBTN5WpdJSA6ReBnIwikMq4r5oZw7uvnv0boMCrc/Ob/OLEBO
p7IFiQUGnQjf+3/xxKYEB9X8RiWFAeL73HRQDZNoAxcavPgUD2zir7W18phYC0RB
QnLUubWTCa8z45k3oQ==
-----END CERTIFICATE-----";

    // Second test CA certificate (CN=test-ca-2, different key material)
    const TEST_CA_CERT_2: &str = "-----BEGIN CERTIFICATE-----
MIIDCTCCAfGgAwIBAgIUdVEP5pTvhV0TAFlTYkuV0cSQVowwDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJdGVzdC1jYS0yMB4XDTI2MDEwOTAwMDgyNFoXDTM2MDEw
NzAwMDgyNFowFDESMBAGA1UEAwwJdGVzdC1jYS0yMIIBIjANBgkqhkiG9w0BAQEF
AAOCAQ8AMIIBCgKCAQEAtU4t5l7XTH5+NSxwweWmW3iWmIb1H/FpmN53SWFShKS4
yhSiWLBT6SiPArsKFFaQkFM04oLhYQD1V0sL0SlabkRfKbYvXJ1x2gc0UCJWbV0e
0WfVc0fEyjpOnX0+EAKWqQl671UZzbt+lVNj9LIMNsglTRgbFK/CtxKu10eyYK8k
/bFVUpHoacIaEWFk0bbhLS4IO2xfKDEcf29gTUs9wAsYlSOaR+gVlLr0fs7v02tM
Ex7Idkgo43D3tQlL0wqEU5T5+QzqSY3BbMfzySr4I+T1t0Q4WY7F3GrlvbC7zMCW
DBzQ9Gt6MMKf7qqdSsS4YFKGP20kccn3hlXsM3gXnwIDAQABo1MwUTAdBgNVHQ4E
FgQUM+3XKol4ODEuqJWJKN7oh3uKihQwHwYDVR0jBBgwFoAUM+3XKol4ODEuqJWJ
KN7oh3uKihQwDwYDVR0TAQH/BAUwAwEB/zANBgkqhkiG9w0BAQsFAAOCAQEATFm/
ufbzleLM2258Pd/wJUxE/Bs4zPrXpi0aHfVFbakvRoOJvcpuQi8vGaVPApriQhp/
8u94E8Owhi+bqZzcjXBg8d4wRGGeG2WdZ1ROEpu7uHHNGuXP12ndz/LnZUMtTD7H
R/mOrHN4JnUw91q5QdKxbsHGHR+pFl662Yc7pewJ8FloxoFxD6igZG/1TdpdK4ii
1bBxQD0CS9mD0tD2CXi/mFwbLTsY4qpoOT1TJJJcq/MldTcWAVEJpJ9UhblDtSy+
zhxL/14wqaVBwUW6/RNRr9hz6MkFFC8Uced5obScy8kOI0bMbeIC4ftNGG9pUdms
3BSW8BRUdXasnBkWIg==
-----END CERTIFICATE-----";

    struct TestIdentity {
        key_pem: String,
        cert_pem: String,
        key_der: PrivateKeyDer<'static>,
        cert_der: CertificateDer<'static>,
    }

    fn generate_identity() -> TestIdentity {
        let key_pair = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec!["localhost".to_string()])
            .unwrap()
            .self_signed(&key_pair)
            .unwrap();
        TestIdentity {
            key_pem: key_pair.serialize_pem(),
            cert_pem: cert.pem(),
            key_der: PrivatePkcs8KeyDer::from(key_pair.serialize_der()).into(),
            cert_der: cert.der().clone(),
        }
    }

    #[rstest]
    #[case::combined(true)]
    #[case::separate(false)]
    fn test_client_auth_files_complete_mutual_tls_handshake(#[case] combined: bool) {
        let client_identity = generate_identity();
        let server_identity = generate_identity();
        let temp_dir = tempfile::tempdir().unwrap();

        if combined {
            std::fs::write(
                temp_dir.path().join("client.pem"),
                format!("{}{}", client_identity.key_pem, client_identity.cert_pem),
            )
            .unwrap();
        } else {
            std::fs::write(
                temp_dir.path().join("client-key.pem"),
                &client_identity.key_pem,
            )
            .unwrap();
            std::fs::write(
                temp_dir.path().join("client-cert.pem"),
                &client_identity.cert_pem,
            )
            .unwrap();
        }
        std::fs::write(
            temp_dir.path().join("server.pem"),
            &server_identity.cert_pem,
        )
        .unwrap();

        let client_config = create_tls_config_from_certs_dir(temp_dir.path(), true).unwrap();

        assert_mutual_tls_handshake(client_config, &client_identity, server_identity);
    }

    #[rstest]
    fn test_ca_only_directory_succeeds() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ca1_path = temp_dir.path().join("ca1.pem");
        let ca2_path = temp_dir.path().join("ca2.pem");
        std::fs::write(&ca1_path, TEST_CERT).unwrap();
        std::fs::write(&ca2_path, TEST_CA_CERT_2).unwrap();

        let result = create_tls_config_from_certs_dir(temp_dir.path(), false);

        let config = result.unwrap();
        assert!(!config.client_auth_cert_resolver.has_certs());
    }

    #[rstest]
    fn test_ca_only_directory_fails_when_client_auth_required() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ca_path = temp_dir.path().join("ca.pem");
        std::fs::write(&ca_path, TEST_CERT).unwrap();

        let result = create_tls_config_from_certs_dir(temp_dir.path(), true);

        assert!(
            result.is_err(),
            "Should fail when client auth required but no key present"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("client auth required"),
            "Error should mention client auth required: {err_msg}"
        );
    }

    #[rstest]
    fn test_empty_directory_succeeds_without_client_auth() {
        let temp_dir = tempfile::tempdir().unwrap();

        let result = create_tls_config_from_certs_dir(temp_dir.path(), false);

        assert!(
            result.is_ok(),
            "Empty directory should succeed without client auth: {:?}",
            result.err()
        );
    }

    #[rstest]
    fn test_not_a_directory_fails() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("not_a_dir.txt");
        std::fs::write(&file_path, "test").unwrap();

        let result = create_tls_config_from_certs_dir(&file_path, false);
        assert!(result.is_err(), "Non-directory path should fail");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not a directory"),
            "Error should mention not a directory: {err_msg}"
        );
    }

    #[rstest]
    fn test_invalid_cert_file_ignored() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ca_path = temp_dir.path().join("ca.pem");
        let invalid_path = temp_dir.path().join("invalid.pem");
        std::fs::write(&ca_path, TEST_CERT).unwrap();
        std::fs::write(&invalid_path, "not a valid certificate").unwrap();

        let result = create_tls_config_from_certs_dir(temp_dir.path(), false);
        assert!(
            result.is_ok(),
            "Should succeed ignoring invalid cert file: {:?}",
            result.err()
        );
    }

    fn assert_mutual_tls_handshake(
        client_config: rustls::ClientConfig,
        client_identity: &TestIdentity,
        server_identity: TestIdentity,
    ) {
        let mut client_roots = rustls::RootCertStore::empty();
        client_roots.add(client_identity.cert_der.clone()).unwrap();
        let verifier = WebPkiClientVerifier::builder(Arc::new(client_roots))
            .build()
            .unwrap();
        let server_config = rustls::ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                vec![server_identity.cert_der],
                server_identity.key_der.clone_key(),
            )
            .unwrap();
        let server_name = ServerName::try_from("localhost").unwrap();
        let mut client = Connection::Client(
            ClientConnection::new(Arc::new(client_config), server_name).unwrap(),
        );
        let mut server =
            Connection::Server(ServerConnection::new(Arc::new(server_config)).unwrap());

        for _ in 0..10 {
            transfer_tls(&mut client, &mut server);
            transfer_tls(&mut server, &mut client);
            if !client.is_handshaking() && !server.is_handshaking() {
                break;
            }
        }

        assert!(!client.is_handshaking());
        assert!(!server.is_handshaking());
        assert_eq!(
            server.peer_certificates().unwrap(),
            std::slice::from_ref(&client_identity.cert_der)
        );
    }

    fn transfer_tls(from: &mut Connection, to: &mut Connection) {
        let mut bytes = Vec::new();
        from.write_tls(&mut bytes).unwrap();
        if bytes.is_empty() {
            return;
        }

        to.read_tls(&mut Cursor::new(bytes)).unwrap();
        to.process_new_packets().unwrap();
    }
}
