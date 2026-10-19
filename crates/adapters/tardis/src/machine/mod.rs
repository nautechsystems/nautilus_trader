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

pub mod cache;
pub mod client;
pub mod message;
pub mod parse;
pub mod types;

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_stream::stream;
use futures_util::{Sink, SinkExt, Stream, StreamExt, pin_mut};
use message::{WsMessage, decode_ws_message};
use nautilus_core::{consts::NAUTILUS_USER_AGENT, string::urlencoding};
use nautilus_network::url_origin;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{self, client::IntoClientRequest, protocol::frame::coding::CloseCode},
};
use tokio_util::sync::CancellationToken;
use types::{ReplayNormalizedRequestOptions, StreamNormalizedRequestOptions};

use crate::common::enums::TardisExchange;
pub use crate::machine::client::TardisMachineClient;

pub type Result<T> = std::result::Result<T, Error>;

/// The error that could happen while interacting with Tardis Machine Server.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An error that could happen when an empty options array was given.
    #[error("Options cannot be empty")]
    EmptyOptions,
    /// An exchange supported for replay but not real-time streaming.
    #[error("Unsupported Tardis streaming exchange: {0} (historical-only; use replay options)")]
    UnsupportedStreamingExchange(TardisExchange),
    /// An error when failed to connect to Tardis' websocket connection.
    #[error("Failed to connect: {0}")]
    ConnectFailed(#[from] tungstenite::Error),
    /// An error when WS connection to the machine server was rejected.
    #[error("Connection rejected: {reason}")]
    ConnectRejected {
        /// The status code for the initial WS connection.
        status: tungstenite::http::StatusCode,
        /// The reason why the connection was rejected.
        reason: String,
    },
    /// An error where the websocket connection was closed unexpectedly by Tardis.
    #[error("Connection closed: {reason}")]
    ConnectionClosed {
        /// The reason why the connection was closed.
        reason: String,
    },
    /// An error when deserializing the response from Tardis.
    #[error("Failed to deserialize message: {0}")]
    Deserialization(#[from] serde_json::Error),
}

#[cfg(any(feature = "replay", feature = "python", test))]
impl Error {
    pub(crate) fn log_stream_error(&self) {
        log::error!("Tardis Machine stream error: {}", self.redacted_message());
    }

    fn redacted_message(&self) -> String {
        match self {
            Self::EmptyOptions | Self::UnsupportedStreamingExchange(_) => self.to_string(),
            Self::ConnectFailed(tungstenite::Error::Io(error)) => {
                format!("Failed to connect: I/O error: {:?}", error.kind())
            }
            Self::ConnectFailed(tungstenite::Error::Http(response)) => {
                format!(
                    "Failed to connect: HTTP status {}",
                    response.status().as_u16()
                )
            }
            Self::ConnectFailed(_) => "Failed to connect".to_string(),
            Self::ConnectRejected { status, .. } => {
                format!("Connection rejected with status {}", status.as_u16())
            }
            Self::ConnectionClosed { reason } if reason == MISSING_INSTRUMENT_INFO => {
                MISSING_INSTRUMENT_INFO.to_string()
            }
            Self::ConnectionClosed { .. } => "Connection closed".to_string(),
            Self::Deserialization(error) => format!(
                "Failed to deserialize message at line {}, column {}",
                error.line(),
                error.column()
            ),
        }
    }
}

pub(crate) const MISSING_INSTRUMENT_INFO: &str = "Missing instrument definition info";

/// Connects to the Tardis Machine WS replay endpoint and returns a stream of WebSocket messages.
///
/// # Errors
///
/// Returns an error if no options are provided or the connection fails.
pub async fn replay_normalized(
    base_url: &str,
    options: Vec<ReplayNormalizedRequestOptions>,
    signal: Arc<AtomicBool>,
) -> Result<impl Stream<Item = Result<WsMessage>>> {
    if options.is_empty() {
        return Err(Error::EmptyOptions);
    }

    let path = format!("{base_url}/ws-replay-normalized?options=");
    let options = serde_json::to_string(&options)?;

    log::debug!("Connecting to {}", url_origin(base_url));

    let url = format!("{path}{}", urlencoding::encode(&options));
    stream_from_websocket(base_url, url, signal).await
}

/// Connects to the Tardis Machine WS streaming endpoint and returns a stream of WebSocket messages.
///
/// # Errors
///
/// Returns an error if no options are provided, the exchange is BitMEX, or the connection fails.
pub async fn stream_normalized(
    base_url: &str,
    options: Vec<StreamNormalizedRequestOptions>,
    signal: Arc<AtomicBool>,
) -> Result<impl Stream<Item = Result<WsMessage>>> {
    validate_stream_options(&options)?;

    let path = format!("{base_url}/ws-stream-normalized?options=");
    let options = serde_json::to_string(&options)?;

    log::debug!("Connecting to {}", url_origin(base_url));

    let url = format!("{path}{}", urlencoding::encode(&options));
    stream_from_websocket(base_url, url, signal).await
}

pub(crate) fn validate_stream_options(options: &[StreamNormalizedRequestOptions]) -> Result<()> {
    if options.is_empty() {
        return Err(Error::EmptyOptions);
    }

    for option in options {
        if option.exchange == TardisExchange::Bitmex {
            return Err(Error::UnsupportedStreamingExchange(option.exchange));
        }
    }

    Ok(())
}

pub(crate) fn is_unsupported_streaming_error(reason: &str) -> bool {
    reason
        .strip_prefix("Error: ")
        .unwrap_or(reason)
        .starts_with("Real-time streaming is not supported for exchange ")
}

async fn stream_from_websocket(
    base_url: &str,
    url: String,
    signal: Arc<AtomicBool>,
) -> Result<impl Stream<Item = Result<WsMessage>>> {
    let mut request = url.into_client_request()?;
    request.headers_mut().insert(
        tungstenite::http::header::USER_AGENT,
        tungstenite::http::HeaderValue::from_static(NAUTILUS_USER_AGENT),
    );

    let (ws_stream, ws_resp) = connect_async(request).await?;

    handle_connection_response(&ws_resp)?;
    log::debug!("Connected to {}", url_origin(base_url));

    Ok(stream! {
        let (writer, mut reader) = ws_stream.split();
        let cancel = CancellationToken::new();
        let heartbeat = heartbeat(writer, cancel.child_token());
        pin_mut!(heartbeat);
        let mut heartbeat_active = true;
        let _cancel_heartbeat = cancel.drop_guard();

        // Timeout awaiting the next record before checking signal
        let timeout = Duration::from_millis(10);

        log::debug!("Streaming from websocket...");

        loop {
            if signal.load(Ordering::Relaxed) {
                log::debug!("Shutdown signal received");
                break;
            }

            let result = tokio::select! {
                result = tokio::time::timeout(timeout, reader.next()) => result,
                () = &mut heartbeat, if heartbeat_active => {
                    heartbeat_active = false;
                    continue;
                }
            };
            let msg = match result {
                Ok(msg) => msg,
                Err(_) => continue, // Timeout
            };

            match msg {
                Some(Ok(msg)) => match msg {
                    tungstenite::Message::Frame(_)
                    | tungstenite::Message::Binary(_)
                    | tungstenite::Message::Pong(_)
                    | tungstenite::Message::Ping(_) => {
                        log::trace!("Received WebSocket control or binary frame");
                    }
                    tungstenite::Message::Close(Some(frame)) => {
                        let reason = frame.reason.to_string();
                        if frame.code == CloseCode::Normal {
                            log::debug!("Connection closed normally");
                        } else {
                            log::warn!(
                                "Connection closed abnormally with code: {:?}", frame.code
                            );
                            yield Err(Error::ConnectionClosed { reason });
                        }
                        break;
                    }
                    tungstenite::Message::Close(None) => {
                        log::warn!("Connection closed without a frame");
                        yield Err(Error::ConnectionClosed {
                            reason: "No close frame provided".to_string()
                        });
                        break;
                    }
                    tungstenite::Message::Text(msg) => {
                        match decode_ws_message(&msg) {
                            Ok(parsed_msg) => yield Ok(parsed_msg),
                            Err(e) => {
                                log::error!("Failed to deserialize message at line {}, column {}", e.line(), e.column());
                                yield Err(Error::Deserialization(e));
                            }
                        }
                    }
                },
                Some(Err(e)) => {
                    log::warn!("WebSocket error");
                    yield Err(Error::ConnectFailed(e));
                    break;
                }
                None => {
                    log::warn!("Connection closed unexpectedly");
                    yield Err(Error::ConnectionClosed {
                        reason: "Unexpected connection close".to_string(),
                    });
                    break;
                }
            }
        }

        log::debug!("Shutdown stream");
    })
}

fn handle_connection_response(
    ws_resp: &tungstenite::http::Response<Option<Vec<u8>>>,
) -> Result<()> {
    if ws_resp.status() != tungstenite::http::StatusCode::SWITCHING_PROTOCOLS {
        return match ws_resp.body() {
            Some(resp) => Err(Error::ConnectRejected {
                status: ws_resp.status(),
                reason: String::from_utf8_lossy(resp).to_string(),
            }),
            None => Err(Error::ConnectRejected {
                status: ws_resp.status(),
                reason: "Unknown reason".to_string(),
            }),
        };
    }
    Ok(())
}

async fn heartbeat<S>(mut sender: S, cancel: CancellationToken)
where
    S: Sink<tungstenite::Message> + Unpin,
    S::Error: std::fmt::Display,
{
    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            _ = heartbeat_interval.tick() => {}
            () = cancel.cancelled() => break,
        }

        log::trace!("Sending PING");

        tokio::select! {
            result = sender.send(tungstenite::Message::Ping(vec![].into())) => {
                if result.is_err() {
                    log::debug!("Heartbeat send failed (connection closed)");
                    break;
                }
            }
            () = cancel.cancelled() => break,
        }
    }

    log::debug!("Heartbeat task exiting");
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
        time::Duration,
    };

    use futures_util::Sink;
    use rstest::rstest;
    use tokio_tungstenite::tungstenite::{self, Message};
    use tokio_util::sync::CancellationToken;

    use super::{
        Arc, AtomicBool, Error, MISSING_INSTRUMENT_INFO, StreamNormalizedRequestOptions,
        TardisExchange, heartbeat, stream_normalized,
    };

    #[allow(
        dead_code,
        reason = "shared capture also provides integration-only servers"
    )]
    mod client_logging {
        include!("../../../../network/tests/integration/common/client_logging.rs");
    }

    #[rstest]
    #[case(Error::EmptyOptions, "Options cannot be empty")]
    #[case(
        Error::UnsupportedStreamingExchange(TardisExchange::Bitmex),
        "Unsupported Tardis streaming exchange: bitmex (historical-only; use replay options)"
    )]
    #[case(
        Error::ConnectFailed(tungstenite::Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "SYN-secret"
        ))),
        "Failed to connect: I/O error: ConnectionRefused"
    )]
    #[case(
        Error::ConnectFailed(tungstenite::Error::ConnectionClosed),
        "Failed to connect"
    )]
    #[case(Error::ConnectFailed(tungstenite::Error::Http(Box::new(tungstenite::http::Response::builder().status(403).body(Some(b"SYN-secret".to_vec())).unwrap()))), "Failed to connect: HTTP status 403")]
    #[case(Error::ConnectRejected { status: tungstenite::http::StatusCode::UNAUTHORIZED, reason: "SYN-secret".to_string() }, "Connection rejected with status 401")]
    #[case(Error::ConnectionClosed { reason: "SYN-secret".to_string() }, "Connection closed")]
    #[case(Error::ConnectionClosed { reason: MISSING_INSTRUMENT_INFO.to_string() }, MISSING_INSTRUMENT_INFO)]
    #[case(Error::Deserialization(serde_json::from_str::<u64>("\"SYN-secret\"").unwrap_err()), "Failed to deserialize message at line 1, column 12")]
    #[tokio::test]
    async fn test_stream_error_logging_omits_payload(#[case] error: Error, #[case] message: &str) {
        let _capture = client_logging::capture_logs().await;
        error.log_stream_error();
        let messages = client_logging::messages();
        let relevant = messages
            .iter()
            .filter(|(_, value)| value.starts_with("Tardis Machine stream error:"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            relevant,
            vec![(
                log::Level::Error,
                format!("Tardis Machine stream error: {message}")
            )]
        );
        assert!(
            !messages
                .iter()
                .any(|(_, value)| value.contains("SYN-secret"))
        );
    }

    #[rstest]
    #[case(vec![TardisExchange::Bitmex])]
    #[case(vec![TardisExchange::Deribit, TardisExchange::Bitmex])]
    #[tokio::test]
    async fn test_stream_rejects_bitmex_before_connect(#[case] exchanges: Vec<TardisExchange>) {
        let options = exchanges
            .into_iter()
            .map(|exchange| StreamNormalizedRequestOptions {
                exchange,
                symbols: None,
                data_types: vec!["trade".to_string()],
                with_disconnect_messages: None,
                timeout_interval_ms: None,
            })
            .collect();

        let result =
            stream_normalized("not a URL", options, Arc::new(AtomicBool::new(false))).await;

        assert!(matches!(
            result,
            Err(Error::UnsupportedStreamingExchange(TardisExchange::Bitmex))
        ));
    }

    #[tokio::test]
    async fn test_stream_allows_deribit_to_reach_connect() {
        let options = vec![StreamNormalizedRequestOptions {
            exchange: TardisExchange::Deribit,
            symbols: None,
            data_types: vec!["book_change".to_string()],
            with_disconnect_messages: None,
            timeout_interval_ms: None,
        }];

        let result =
            stream_normalized("not a URL", options, Arc::new(AtomicBool::new(false))).await;

        assert!(matches!(result, Err(Error::ConnectFailed(_))));
    }

    struct StallSink;

    impl Sink<Message> for StallSink {
        type Error = Infallible;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }

        fn start_send(self: Pin<&mut Self>, _item: Message) -> Result<(), Self::Error> {
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_heartbeat_exits_on_cancel_during_stalled_send() {
        let cancel = CancellationToken::new();
        let task = tokio::spawn(heartbeat(StallSink, cancel.clone()));

        tokio::task::yield_now().await;
        cancel.cancel();

        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("heartbeat should exit after cancel during a stalled send")
            .unwrap();
    }
}
