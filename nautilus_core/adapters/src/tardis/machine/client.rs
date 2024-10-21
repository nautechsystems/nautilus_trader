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

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use futures_util::{pin_mut, Stream, StreamExt};
use nautilus_model::data::Data;

use super::{
    enums::WsMessage, replay_normalized, stream_normalized, Error, ReplayNormalizedRequestOptions,
    StreamNormalizedRequestOptions,
};
use crate::tardis::machine::{enums::Exchange, parse::parse_tardis_ws_message};

// pub type Result<T> = std::result::Result<T, Error>;

pub struct TardisInstrument {
    pub symbol: String,
    pub exchange: Exchange,
}

/// Provides a client for connecting to a [Tardis Machine Server](https://docs.tardis.dev/api/tardis-machine).
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct TardisClient {
    pub base_url: String,
    pub replay_signal: Arc<AtomicBool>,
    pub stream_signals: HashMap<TardisInstrument, Arc<AtomicBool>>,
}

impl TardisClient {
    /// Creates a new [`TardisClient`] instance.
    pub fn new(base_url: impl ToString) -> Self {
        Self {
            base_url: base_url.to_string(),
            replay_signal: Arc::new(AtomicBool::new(false)),
            stream_signals: HashMap::new(),
        }
    }

    pub fn close(&mut self) {
        self.replay_signal.store(true, Ordering::Relaxed);

        for signal in self.stream_signals.values() {
            signal.store(true, Ordering::Relaxed);
        }
        tracing::info!("All signals set to true, shutting down");
    }

    pub async fn replay(
        &self,
        options: Vec<ReplayNormalizedRequestOptions>,
    ) -> impl Stream<Item = Data> {
        let stream = replay_normalized(&self.base_url, options, self.replay_signal.clone())
            .await
            .expect("Failed to connect to WebSocket");

        // We use Box::pin to heap-allocate the stream and ensure it implements
        // Unpin for safe async handling across lifetimes.
        handle_ws_stream(Box::pin(stream))
    }

    pub async fn stream(
        &self,
        options: Vec<StreamNormalizedRequestOptions>,
    ) -> impl Stream<Item = Data> {
        let stream = stream_normalized(&self.base_url, options, self.replay_signal.clone())
            .await
            .expect("Failed to connect to WebSocket");

        // We use Box::pin to heap-allocate the stream and ensure it implements
        // Unpin for safe async handling across lifetimes.
        handle_ws_stream(Box::pin(stream))
    }
}

fn handle_ws_stream<S>(stream: S) -> impl Stream<Item = Data>
where
    S: Stream<Item = Result<WsMessage, Error>> + Unpin,
{
    async_stream::stream! {
        pin_mut!(stream);
        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    if let Some(data) = parse_tardis_ws_message(msg, 0, 0) {
                        yield data;
                    } else {
                        continue;
                    }
                }
                Err(e) => {
                    tracing::error!("Error in WebSocket stream: {e:?}");
                    break;
                }
            }
        }
    }
}
