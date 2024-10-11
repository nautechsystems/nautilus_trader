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

#![allow(dead_code)] // Use for initial development

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::{self};

use super::enums::Exchange;

/// The options that can be specified for calling Tardis Machine Server's replay-normalized.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayNormalizedRequestOptions {
    /// Requested [`Exchange`].
    pub exchange: Exchange,
    /// Optional symbols of requested historical data feed.
    /// Use /exchanges/:exchange HTTP API to get allowed symbols for requested exchange.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub symbols: Option<Vec<String>>,
    /// Replay period start date (UTC) in a ISO 8601 format, e.g., 2019-04-01.
    pub from: NaiveDate,
    /// Replay period start date (UTC) in a ISO 8601 format, e.g., 2019-04-02.
    pub to: NaiveDate,
    /// Array of normalized [data types](https://docs.tardis.dev/api/tardis-machine#normalized-data-types)
    /// for which real-time data will be provided.
    pub data_types: Vec<String>,
    /// When set to true, sends also disconnect messages that mark events when real-time WebSocket
    /// connection that was used to collect the historical data got disconnected.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub with_disconnect_messages: Option<bool>,
}

/// The options that can be specified for calling Tardis Machine Server's stream-normalized.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamNormalizedRequestOptions {
    /// Requested [`Exchange`].
    pub exchange: Exchange,
    /// Optional symbols of requested real-time data feed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub symbols: Option<Vec<String>>,
    /// Array of normalized [data types](https://docs.tardis.dev/api/tardis-machine#normalized-data-types)
    /// for which real-time data will be provided.
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

/// Provides a client for connecting to a [Tardis Machine Server](https://docs.tardis.dev/api/tardis-machine).
pub struct TardisClient {
    url: String,
}

impl TardisClient {
    /// Creates a new [`Client`] instance.
    pub fn new(url: impl ToString) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    // pub async fn replay_normalized(
    //     &self,
    //     options: Vec<ReplayNormalizedRequestOptions>,
    // ) -> Result<impl Stream<Item = Result<WsMessage>>> {
    //     if options.len() == 0 {
    //         return Err(Error::EmptyOptions);
    //     }
    //
    //     let options = serde_json::to_string(&options)?;
    //     let url = format!(
    //         "{}/ws-replay-normalized?options={}",
    //         &self.url,
    //         urlencoding::encode(&options)
    //     );
    //
    //     // let url = "ws://localhost:8001/ws-replay"
    //     let url = "ws://localhost:8001/ws-replay?exchange=bitmex&from=2019-10-01&to=2019-10-02";
    //
    //     tracing::info!("[replay_normalized] url to tardis {url}");
    // }
    //
    // pub async fn stream_normalized(
    //     &self,
    //     options: Vec<StreamNormalizedRequestOptions>,
    // ) -> Result<impl Stream<Item = Result<WsMessage>>> {
    //     if options.len() == 0 {
    //         return Err(Error::EmptyOptions);
    //     }
    //
    //     let options = serde_json::to_string(&options)?;
    //     let url = format!(
    //         "{}/ws-stream-normalized?options={}",
    //         &self.url,
    //         urlencoding::encode(&options)
    //     );
    //
    //     tracing::info!("[stream_normalized] url to tardis {url}");
    // }
}
