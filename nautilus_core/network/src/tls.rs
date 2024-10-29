// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{
    tungstenite::{handshake::client::Request, stream::Mode, Error},
    MaybeTlsStream,
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
        use rustls::pki_types::ServerName;
        pub use rustls::ClientConfig;
        use tokio::io::{AsyncRead, AsyncWrite};
        use tokio_rustls::TlsConnector as TokioTlsConnector;
        use tokio_tungstenite::{
            tungstenite::{error::TlsError, stream::Mode, Error},
            MaybeTlsStream,
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
            tungstenite::{
                error::{Error, UrlError},
                stream::Mode,
            },
            MaybeTlsStream,
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

fn domain(request: &Request) -> Result<String, Error> {
    match request.uri().host() {
        // rustls expects IPv6 addresses without the surrounding [] brackets
        Some(d) if d.starts_with('[') && d.ends_with(']') => Ok(d[1..d.len() - 1].to_string()),
        Some(d) => Ok(d.to_string()),
        None => panic!("No host name"),
    }
}
