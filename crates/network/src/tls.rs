// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
        pub use rustls::ClientConfig;
        use rustls::pki_types::ServerName;
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
                    let config = match tls_connector {
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
    if !certs_dir.is_dir() {
        anyhow::bail!("Certificate path is not a directory: {certs_dir:?}");
    }

    let mut client_cert = None;
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
            if client_cert.is_none() {
                client_cert = Some(certs);
            } else {
                for cert in certs {
                    if let Err(e) = root_store.add(cert) {
                        eprintln!("Warning: Invalid certificate in {path:?}: {e}");
                    }
                }
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

    tracing::warn!(
        "No TLS client certificate/key found in {:?}; proceeding without client authentication",
        certs_dir
    );

    Ok(builder.with_no_client_auth())
}

fn load_private_key(path: &Path) -> anyhow::Result<PrivateKeyDer<'static>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let pkcs8_keys: Vec<_> = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .filter_map(std::result::Result::ok)
        .collect();

    if let Some(key) = pkcs8_keys.into_iter().next() {
        return Ok(key.into());
    }

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let rsa_keys: Vec<_> = rustls_pemfile::rsa_private_keys(&mut reader)
        .filter_map(std::result::Result::ok)
        .collect();

    if let Some(key) = rsa_keys.into_iter().next() {
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
