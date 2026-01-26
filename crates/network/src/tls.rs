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

//! Module for wrapping raw socket streams with TLS encryption.

use std::{fs::File, io::BufReader, path::Path};

use nautilus_cryptography::providers::install_cryptographic_provider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{
    MaybeTlsStream,
    tungstenite::{Error, handshake::client::Request, stream::Mode},
};

/// A connector that can be used when establishing connections, allowing to control whether
/// `native-tls` or `rustls` is used to create a TLS connection. Or TLS can be disabled with the
/// `Plain` variant.
#[non_exhaustive]
#[derive(Clone)]
#[allow(dead_code)]
pub enum Connector {
    /// No TLS connection.
    Plain,
    /// TLS connection using `rustls`.
    Rustls(std::sync::Arc<rustls::ClientConfig>),
}

mod encryption {

    pub mod rustls {
        use std::{convert::TryFrom, sync::Arc};

        use nautilus_cryptography::tls::create_tls_config;
        use rustls::{ClientConfig, pki_types::ServerName};
        use tokio::io::{AsyncRead, AsyncWrite};
        use tokio_rustls::TlsConnector as TokioTlsConnector;
        use tokio_tungstenite::{
            MaybeTlsStream,
            tungstenite::{Error, error::TlsError, stream::Mode},
        };

        pub async fn wrap_stream<S>(
            socket: S,
            domain: String,
            mode: Mode,
            tls_connector: Option<Arc<ClientConfig>>,
        ) -> Result<MaybeTlsStream<S>, Error>
        where
            S: 'static + AsyncRead + AsyncWrite + Send + Unpin,
        {
            match mode {
                Mode::Plain => Ok(MaybeTlsStream::Plain(socket)),
                Mode::Tls => {
                    let config: Arc<ClientConfig> = match tls_connector {
                        Some(config) => config,
                        None => create_tls_config(),
                    };
                    let domain = ServerName::try_from(domain.as_str())
                        .map_err(|_| TlsError::InvalidDnsName)?
                        .to_owned();
                    let stream = TokioTlsConnector::from(config);
                    let connected = stream.connect(domain, socket).await;

                    match connected {
                        Err(e) => Err(Error::Io(e)),
                        Ok(s) => Ok(MaybeTlsStream::Rustls(s)),
                    }
                }
            }
        }
    }

    pub mod plain {
        use tokio::io::{AsyncRead, AsyncWrite};
        use tokio_tungstenite::{
            MaybeTlsStream,
            tungstenite::{
                error::{Error, UrlError},
                stream::Mode,
            },
        };

        pub async fn wrap_stream<S>(socket: S, mode: Mode) -> Result<MaybeTlsStream<S>, Error>
        where
            S: 'static + AsyncRead + AsyncWrite + Send + Unpin,
        {
            match mode {
                Mode::Plain => Ok(MaybeTlsStream::Plain(socket)),
                Mode::Tls => Err(Error::Url(UrlError::TlsFeatureNotEnabled)),
            }
        }
    }
}

pub async fn tcp_tls<S>(
    request: &Request,
    mode: Mode,
    stream: S,
    connector: Option<Connector>,
) -> Result<MaybeTlsStream<S>, Error>
where
    S: 'static + AsyncRead + AsyncWrite + Send + Unpin,
    MaybeTlsStream<S>: Unpin,
{
    let domain = domain(request)?;

    match connector {
        Some(conn) => match conn {
            Connector::Rustls(conn) => {
                self::encryption::rustls::wrap_stream(stream, domain, mode, Some(conn)).await
            }
            Connector::Plain => self::encryption::plain::wrap_stream(stream, mode).await,
        },
        None => self::encryption::rustls::wrap_stream(stream, domain, mode, None).await,
    }
}

/// Extracts the host name from the request URI.
///
/// # Errors
///
/// Returns an error if the request URI has no host component.
#[allow(clippy::result_large_err)]
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

pub fn create_tls_config_from_certs_dir(
    certs_dir: &Path,
    require_client_auth: bool,
) -> anyhow::Result<rustls::ClientConfig> {
    install_cryptographic_provider();

    if !certs_dir.is_dir() {
        anyhow::bail!("Certificate path is not a directory: {certs_dir:?}");
    }

    let mut all_certs: Vec<(std::path::PathBuf, Vec<CertificateDer<'static>>)> = Vec::new();
    let mut client_key = None;
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    for entry in std::fs::read_dir(certs_dir)? {
        let entry = entry?;
        let path = entry.path();

        if client_key.is_none()
            && let Ok(key) = load_private_key(&path)
        {
            client_key = Some(key);
            continue;
        }

        if let Ok(certs) = load_certs(&path)
            && !certs.is_empty()
        {
            all_certs.push((path, certs));
        }
    }

    // If key found, first cert becomes client cert; otherwise all certs are CA roots
    let client_cert = if client_key.is_some() && !all_certs.is_empty() {
        let (_, cert) = all_certs.remove(0);
        Some(cert)
    } else {
        None
    };

    for (path, certs) in all_certs {
        for cert in certs {
            if let Err(e) = root_store.add(cert) {
                log::warn!("Invalid certificate in {path:?}: {e}");
            }
        }
    }

    let builder = rustls::ClientConfig::builder().with_root_certificates(root_store);

    if let (Some(cert), Some(key)) = (client_cert, client_key) {
        return Ok(builder.with_client_auth_cert(cert, key)?);
    }

    if require_client_auth {
        anyhow::bail!(
            "Client certificate or private key missing in {certs_dir:?} but client auth required",
        );
    }

    log::debug!(
        "No TLS client certificate/key pair found in {certs_dir:?}; proceeding without client authentication"
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

    anyhow::bail!("No valid private key found in {path:?}");
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
    use rstest::rstest;

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

    #[rstest]
    fn test_ca_only_directory_succeeds() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ca1_path = temp_dir.path().join("ca1.pem");
        let ca2_path = temp_dir.path().join("ca2.pem");
        std::fs::write(&ca1_path, TEST_CERT).unwrap();
        std::fs::write(&ca2_path, TEST_CA_CERT_2).unwrap();

        let result = create_tls_config_from_certs_dir(temp_dir.path(), false);

        assert!(
            result.is_ok(),
            "CA-only directory should succeed: {:?}",
            result.err()
        );
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
}
