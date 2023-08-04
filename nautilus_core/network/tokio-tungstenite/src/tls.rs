//! Connection helper.
use tokio::io::{AsyncRead, AsyncWrite};

use tungstenite::{
    client::uri_mode,
    error::Error,
    handshake::client::{Request, Response},
    protocol::WebSocketConfig,
    stream::Mode,
};

use crate::{client_async_with_config, IntoClientRequest, WebSocketStream};

pub use crate::stream::MaybeTlsStream;

/// A connector that can be used when establishing connections, allowing to control whether
/// `native-tls` or `rustls` is used to create a TLS connection. Or TLS can be disabled with the
/// `Plain` variant.
#[non_exhaustive]
#[derive(Clone)]
pub enum Connector {
    /// Plain (non-TLS) connector.
    Plain,
    /// `native-tls` TLS connector.
    #[cfg(feature = "native-tls")]
    NativeTls(native_tls_crate::TlsConnector),
    /// `rustls` TLS connector.
    #[cfg(feature = "__rustls-tls")]
    Rustls(std::sync::Arc<rustls::ClientConfig>),
}

/// Encrypt a stream usin Tls
pub mod encryption {
    /// Use native-tls implementaiton to encrypt
    #[cfg(feature = "native-tls")]
    pub mod native_tls {
        use native_tls_crate::TlsConnector;
        use tokio_native_tls::TlsConnector as TokioTlsConnector;

        use tokio::io::{AsyncRead, AsyncWrite};

        use tungstenite::{error::TlsError, stream::Mode, Error};

        use crate::stream::MaybeTlsStream;

        /// Wraps the stream with tls encryption
        pub async fn wrap_stream<S>(
            socket: S,
            domain: String,
            mode: Mode,
            tls_connector: Option<TlsConnector>,
        ) -> Result<MaybeTlsStream<S>, Error>
        where
            S: 'static + AsyncRead + AsyncWrite + Send + Unpin,
        {
            match mode {
                Mode::Plain => Ok(MaybeTlsStream::Plain(socket)),
                Mode::Tls => {
                    let try_connector = tls_connector.map_or_else(TlsConnector::new, Ok);
                    let connector = try_connector.map_err(TlsError::Native)?;
                    let stream = TokioTlsConnector::from(connector);
                    let connected = stream.connect(&domain, socket).await;
                    match connected {
                        Err(e) => Err(Error::Tls(e.into())),
                        Ok(s) => Ok(MaybeTlsStream::NativeTls(s)),
                    }
                }
            }
        }
    }

    /// Use rust-tls implementation to encrypt
    #[cfg(feature = "__rustls-tls")]
    pub mod rustls {
        pub use rustls::ClientConfig;
        use rustls::{RootCertStore, ServerName};
        use tokio_rustls::TlsConnector as TokioTlsConnector;

        use std::{convert::TryFrom, sync::Arc};
        use tokio::io::{AsyncRead, AsyncWrite};

        use tungstenite::{error::TlsError, stream::Mode, Error};

        use crate::stream::MaybeTlsStream;

        /// Wraps the stream with rust-tls encryption
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
                        None => {
                            #[allow(unused_mut)]
                            let mut root_store = RootCertStore::empty();
                            #[cfg(feature = "rustls-tls-native-roots")]
                            {
                                let native_certs = rustls_native_certs::load_native_certs()?;
                                let der_certs: Vec<Vec<u8>> =
                                    native_certs.into_iter().map(|cert| cert.0).collect();
                                let total_number = der_certs.len();
                                let (number_added, number_ignored) =
                                    root_store.add_parsable_certificates(&der_certs);
                                log::debug!("Added {number_added}/{total_number} native root certificates (ignored {number_ignored})");
                            }
                            #[cfg(feature = "rustls-tls-webpki-roots")]
                            {
                                root_store.add_trust_anchors(
                                    webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
                                        rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
                                            ta.subject,
                                            ta.spki,
                                            ta.name_constraints,
                                        )
                                    })
                                );
                            }

                            Arc::new(
                                ClientConfig::builder()
                                    .with_safe_defaults()
                                    .with_root_certificates(root_store)
                                    .with_no_client_auth(),
                            )
                        }
                    };
                    let domain = ServerName::try_from(domain.as_str())
                        .map_err(|_| TlsError::InvalidDnsName)?;
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

    /// Does not encrypt stream
    pub mod plain {
        use tokio::io::{AsyncRead, AsyncWrite};

        use tungstenite::{
            error::{Error, UrlError},
            stream::Mode,
        };

        use crate::stream::MaybeTlsStream;

        /// Keeps the stream unwrapped
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

/// Creates a `WebSocket` handshake from a request and a stream,
/// upgrading the stream to TLS if required.
#[cfg(any(feature = "native-tls", feature = "__rustls-tls"))]
pub async fn client_async_tls<R, S>(
    request: R,
    stream: S,
) -> Result<(WebSocketStream<MaybeTlsStream<S>>, Response), Error>
where
    R: IntoClientRequest + Unpin,
    S: 'static + AsyncRead + AsyncWrite + Send + Unpin,
    MaybeTlsStream<S>: Unpin,
{
    client_async_tls_with_config(request, stream, None, None).await
}

/// Given a domain and mode
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
    #[cfg(any(feature = "native-tls", feature = "__rustls-tls"))]
    let domain = crate::domain(request)?;

    match connector {
        Some(conn) => match conn {
            #[cfg(feature = "native-tls")]
            Connector::NativeTls(conn) => {
                self::encryption::native_tls::wrap_stream(stream, domain, mode, Some(conn)).await
            }
            #[cfg(feature = "__rustls-tls")]
            Connector::Rustls(conn) => {
                self::encryption::rustls::wrap_stream(stream, domain, mode, Some(conn)).await
            }
            Connector::Plain => self::encryption::plain::wrap_stream(stream, mode).await,
        },
        None => {
            #[cfg(feature = "native-tls")]
            {
                self::encryption::native_tls::wrap_stream(stream, domain, mode, None).await
            }
            #[cfg(all(feature = "__rustls-tls", not(feature = "native-tls")))]
            {
                self::encryption::rustls::wrap_stream(stream, domain, mode, None).await
            }
            #[cfg(not(any(feature = "native-tls", feature = "__rustls-tls")))]
            {
                self::encryption::plain::wrap_stream(stream, mode).await
            }
        }
    }
}

/// The same as `client_async_tls()` but the one can specify a websocket configuration,
/// and an optional connector. If no connector is specified, a default one will
/// be created.
///
/// Please refer to `client_async_tls()` for more details.
pub async fn client_async_tls_with_config<R, S>(
    request: R,
    stream: S,
    config: Option<WebSocketConfig>,
    connector: Option<Connector>,
) -> Result<(WebSocketStream<MaybeTlsStream<S>>, Response), Error>
where
    R: IntoClientRequest + Unpin,
    S: 'static + AsyncRead + AsyncWrite + Send + Unpin,
    MaybeTlsStream<S>: Unpin,
{
    let request = request.into_client_request()?;

    // Make sure we check domain and mode first. URL must be valid.
    let mode = uri_mode(request.uri())?;

    let stream = tcp_tls(&request, mode, stream, connector).await?;
    client_async_with_config(request, stream, config).await
}
