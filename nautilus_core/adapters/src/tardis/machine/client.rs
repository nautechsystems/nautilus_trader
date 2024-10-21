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
use nautilus_model::{
    data::Data,
    identifiers::{InstrumentId, Symbol, Venue},
};

use super::{
    enums::Exchange, message::WsMessage, replay_normalized, stream_normalized, Error,
    ReplayNormalizedRequestOptions, StreamNormalizedRequestOptions, TardisInstrumentInfo,
};
use crate::tardis::machine::parse::parse_tardis_ws_message;

/// Provides a client for connecting to a [Tardis Machine Server](https://docs.tardis.dev/api/tardis-machine).
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct TardisClient {
    pub base_url: String,
    pub replay_signal: Arc<AtomicBool>,
    pub stream_signals: HashMap<TardisInstrumentInfo, Arc<AtomicBool>>,
    pub instruments: HashMap<InstrumentId, Arc<TardisInstrumentInfo>>,
}

impl TardisClient {
    /// Creates a new [`TardisClient`] instance.
    pub fn new(base_url: impl ToString) -> Self {
        Self {
            base_url: base_url.to_string(),
            replay_signal: Arc::new(AtomicBool::new(false)),
            stream_signals: HashMap::new(),
            instruments: HashMap::new(),
        }
    }

    pub fn add_instrument_info(&mut self, info: TardisInstrumentInfo) {
        self.instruments.insert(info.instrument_id, Arc::new(info));
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.replay_signal.load(Ordering::Relaxed)
    }

    pub fn close(&mut self) {
        tracing::debug!("Closing");

        self.replay_signal.store(true, Ordering::Relaxed);

        for signal in self.stream_signals.values() {
            signal.store(true, Ordering::Relaxed);
        }

        tracing::debug!("Closed");
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
        handle_ws_stream(Box::pin(stream), None, Some(self.instruments.clone()))
    }

    pub async fn stream(
        &self,
        instrument: TardisInstrumentInfo,
        options: Vec<StreamNormalizedRequestOptions>,
    ) -> impl Stream<Item = Data> {
        let stream = stream_normalized(&self.base_url, options, self.replay_signal.clone())
            .await
            .expect("Failed to connect to WebSocket");

        // We use Box::pin to heap-allocate the stream and ensure it implements
        // Unpin for safe async handling across lifetimes.
        handle_ws_stream(Box::pin(stream), Some(Arc::new(instrument)), None)
    }
}

fn handle_ws_stream<S>(
    stream: S,
    instrument: Option<Arc<TardisInstrumentInfo>>,
    instrument_map: Option<HashMap<InstrumentId, Arc<TardisInstrumentInfo>>>,
) -> impl Stream<Item = Data>
where
    S: Stream<Item = Result<WsMessage, Error>> + Unpin,
{
    assert!(
        instrument.is_some() || instrument_map.is_some(),
        "Either `instrument` or `instrument_map` must be provided"
    );

    async_stream::stream! {
        pin_mut!(stream);
        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    // TODO: This sequence needs optimizing
                    let info = if let Some(ref instrument) = instrument {
                        Some(instrument.clone())
                    } else {
                        instrument_map.as_ref().and_then(|map| determine_instrument_info(&msg, map))
                    };

                    if let Some(info) = info {
                        if let Some(data) = parse_tardis_ws_message(msg, info) {
                            yield data;
                        } else {
                            continue;  // Non-data message
                        }
                    } else {
                        continue;  // No instrument info
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

pub fn determine_instrument_info(
    msg: &WsMessage,
    instrument_map: &HashMap<InstrumentId, Arc<TardisInstrumentInfo>>,
) -> Option<Arc<TardisInstrumentInfo>> {
    let instrument_id = match msg {
        WsMessage::BookChange(msg) => parse_instrument_id_with_enum(&msg.symbol, &msg.exchange),
        WsMessage::BookSnapshot(msg) => parse_instrument_id_with_enum(&msg.symbol, &msg.exchange),
        WsMessage::Trade(msg) => parse_instrument_id_with_enum(&msg.symbol, &msg.exchange),
        WsMessage::Bar(msg) => parse_instrument_id_with_enum(&msg.symbol, &msg.exchange),
        WsMessage::DerivativeTicker(_) => return None,
        WsMessage::Disconnect(_) => return None,
    };
    match instrument_map.get(&instrument_id) {
        Some(instr) => Some(instr.clone().clone()),
        None => {
            tracing::error!("Instrument definition info not available for {instrument_id}");
            None
        }
    }
}

#[must_use]
fn parse_instrument_id_with_enum(symbol: &str, exchange: &Exchange) -> InstrumentId {
    InstrumentId::new(Symbol::from(symbol), Venue::from(exchange.as_venue_str()))
}
