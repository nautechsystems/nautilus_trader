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

pub mod client;
pub mod message;
pub mod parse;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use async_stream::stream;
use chrono::NaiveDate;
use futures_util::{stream::SplitSink, SinkExt, Stream, StreamExt};
use message::WsMessage;
use nautilus_model::identifiers::InstrumentId;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{self, protocol::frame::coding::CloseCode},
    MaybeTlsStream, WebSocketStream,
};

use super::enums::Exchange;

pub use crate::tardis::machine::client::TardisMachineClient;

/// Instrument definition information necessary for stream parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct InstrumentMiniInfo {
    /// The instrument ID with optionally Nautilus normalized symbol.
    pub instrument_id: InstrumentId,
    /// The price precision for the instrument.
    pub price_precision: u8,
    /// The size precision for the instrument.
    pub size_precision: u8,
}

impl InstrumentMiniInfo {
    /// Creates a new [`InstrumentMiniInfo`] instance.
    #[must_use]
    pub const fn new(instrument_id: InstrumentId, price_precision: u8, size_precision: u8) -> Self {
        Self {
            instrument_id,
            price_precision,
            size_precision,
        }
    }
}

/// The options that can be specified for calling Tardis Machine Server's replay-normalized.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct ReplayNormalizedRequestOptions {
    /// Requested [`Exchange`].
    pub exchange: Exchange,
    /// Optional symbols of requested historical data feed.
    /// Use /exchanges/:exchange HTTP API to get allowed symbols for requested exchange.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub symbols: Option<Vec<String>>,
    /// Replay period start date (UTC) in a ISO 8601 format, e.g., 2019-10-01.
    pub from: NaiveDate,
    /// Replay period start date (UTC) in a ISO 8601 format, e.g., 2019-10-02.
    pub to: NaiveDate,
    /// Array of normalized [data types](https://docs.tardis.dev/api/tardis-machine#normalized-data-types)
    /// for which real-time data will be provided.
    #[serde(alias = "data_types")]
    pub data_types: Vec<String>,
    /// When set to true, sends also disconnect messages that mark events when real-time WebSocket
    /// connection that was used to collect the historical data got disconnected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    #[serde(alias = "with_disconnect_messages")]
    pub with_disconnect_messages: Option<bool>,
}

/// The options that can be specified for calling Tardis Machine Server's stream-normalized.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct StreamNormalizedRequestOptions {
    /// Requested [`Exchange`].
    pub exchange: Exchange,
    /// Optional symbols of requested real-time data feed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub symbols: Option<Vec<String>>,
    /// Array of normalized [data types](https://docs.tardis.dev/api/tardis-machine#normalized-data-types)
    /// for which real-time data will be provided.
    #[serde(alias = "data_types")]
    pub data_types: Vec<String>,
    /// When set to true, sends disconnect messages anytime underlying exchange real-time WebSocket
    /// connection(s) gets disconnected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub with_disconnect_messages: Option<bool>,
    /// Specifies time in milliseconds after which connection to real-time exchanges' WebSocket API
    /// is restarted if no message has been received.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, rename = "timeoutIntervalMS")]
    pub timeout_interval_ms: Option<u64>,
}

pub type Result<T> = std::result::Result<T, Error>;

/// The error that could happen while interacting with Tardis Machine Server.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An error that could happen when an empty options array was given.
    #[error("Options cannot be empty")]
    EmptyOptions,
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

    let plain_url = format!("{path}{options}");
    tracing::debug!("Connecting to {plain_url}");

    let url = format!("{path}{}", urlencoding::encode(&options));
    stream_from_websocket(base_url, &url, signal).await
}

pub async fn stream_normalized(
    base_url: &str,
    options: Vec<StreamNormalizedRequestOptions>,
    signal: Arc<AtomicBool>,
) -> Result<impl Stream<Item = Result<WsMessage>>> {
    if options.is_empty() {
        return Err(Error::EmptyOptions);
    }

    let path = format!("{base_url}/ws-stream-normalized?options=");
    let options = serde_json::to_string(&options)?;

    let plain_url = format!("{path}{options}");
    tracing::debug!("Connecting to {plain_url}");

    let url = format!("{path}{}", urlencoding::encode(&options));
    stream_from_websocket(base_url, &url, signal).await
}

async fn stream_from_websocket(
    base_url: &str,
    url: &str,
    signal: Arc<AtomicBool>,
) -> Result<impl Stream<Item = Result<WsMessage>>> {
    let (ws_stream, ws_resp) = connect_async(url).await?;

    handle_connection_response(ws_resp)?;
    tracing::info!("Connected to {base_url}");

    Ok(stream! {
        let (writer, mut reader) = ws_stream.split();
        tokio::spawn(heartbeat(writer));

        tracing::info!("Streaming from websocket...");

        loop {
            if signal.load(Ordering::Relaxed) {
                tracing::info!("Shutdown signal received");
                break;
            }

            match reader.next().await {
                Some(Ok(msg)) => match msg {
                    tungstenite::Message::Frame(_)
                    | tungstenite::Message::Binary(_)
                    | tungstenite::Message::Pong(_)
                    | tungstenite::Message::Ping(_) => {
                        tracing::trace!("Received {msg:?}");
                        continue; // Skip and continue to the next message
                    }
                    tungstenite::Message::Close(Some(frame)) => {
                        let reason = frame.reason.to_string();
                        if frame.code != CloseCode::Normal {
                            tracing::error!(
                                "Connection closed abnormally with code: {:?}, reason: {reason}",
                                frame.code
                            );
                            yield Err(Error::ConnectionClosed { reason });
                        } else {
                            tracing::debug!("Connection closed normally: {reason}");
                        }
                        break;
                    }
                    tungstenite::Message::Close(None) => {
                        tracing::error!("Connection closed without a frame");
                        yield Err(Error::ConnectionClosed {
                            reason: "No close frame provided".to_string()
                        });
                        break;
                    }
                    tungstenite::Message::Text(msg) => {
                        match serde_json::from_str::<WsMessage>(&msg) {
                            Ok(parsed_msg) => yield Ok(parsed_msg),
                            Err(e) => {
                                tracing::error!("Failed to deserialize message: {msg}. Error: {e}");
                                yield Err(Error::Deserialization(e));
                            }
                        }
                    }
                },
                Some(Err(e)) => {
                    tracing::error!("WebSocket error: {e}");
                    yield Err(Error::ConnectFailed(e));
                    break;
                }
                None => {
                    tracing::error!("Connection closed unexpectedly");
                    yield Err(Error::ConnectionClosed {
                        reason: "Unexpected connection close".to_string(),
                    });
                    break;
                }
            }
        }
    })
}

fn handle_connection_response(ws_resp: tungstenite::http::Response<Option<Vec<u8>>>) -> Result<()> {
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

async fn heartbeat(
    mut sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
) {
    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(10));
    let retry_interval = Duration::from_secs(1);

    loop {
        heartbeat_interval.tick().await;
        tracing::trace!("Sending PING");

        let mut count = 3;
        let mut retry_interval = tokio::time::interval(retry_interval);

        while count > 0 {
            retry_interval.tick().await;
            let _ = sender.send(tungstenite::Message::Ping(vec![])).await;
            count -= 1;
        }
    }
}
