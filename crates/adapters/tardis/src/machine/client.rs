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

use std::{
    collections::HashMap,
    env,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use futures_util::{Stream, StreamExt, pin_mut};
use nautilus_model::data::Data;
use ustr::Ustr;

use super::{
    Error,
    message::WsMessage,
    replay_normalized, stream_normalized,
    types::{
        InstrumentMiniInfo, ReplayNormalizedRequestOptions, StreamNormalizedRequestOptions,
        TardisInstrumentKey,
    },
};
use crate::machine::parse::parse_tardis_ws_message;

/// Provides a client for connecting to a [Tardis Machine Server](https://docs.tardis.dev/api/tardis-machine).
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
#[derive(Debug, Clone)]
pub struct TardisMachineClient {
    pub base_url: String,
    pub replay_signal: Arc<AtomicBool>,
    pub stream_signal: Arc<AtomicBool>,
    pub instruments: HashMap<TardisInstrumentKey, Arc<InstrumentMiniInfo>>,
    pub normalize_symbols: bool,
}

impl TardisMachineClient {
    /// Creates a new [`TardisMachineClient`] instance.
    pub fn new(base_url: Option<&str>, normalize_symbols: bool) -> anyhow::Result<Self> {
        let base_url = base_url
            .map(ToString::to_string)
            .or_else(|| env::var("TARDIS_MACHINE_WS_URL").ok())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Tardis Machine `base_url` must be provided or set in the 'TARDIS_MACHINE_WS_URL' environment variable"
                )
            })?;

        Ok(Self {
            base_url,
            replay_signal: Arc::new(AtomicBool::new(false)),
            stream_signal: Arc::new(AtomicBool::new(false)),
            instruments: HashMap::new(),
            normalize_symbols,
        })
    }

    pub fn add_instrument_info(&mut self, info: InstrumentMiniInfo) {
        let key = info.as_tardis_instrument_key();
        self.instruments.insert(key, Arc::new(info));
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.replay_signal.load(Ordering::Relaxed) && self.stream_signal.load(Ordering::Relaxed)
    }

    pub fn close(&mut self) {
        tracing::debug!("Closing");

        self.replay_signal.store(true, Ordering::Relaxed);
        self.stream_signal.store(true, Ordering::Relaxed);

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
        instrument: InstrumentMiniInfo,
        options: Vec<StreamNormalizedRequestOptions>,
    ) -> impl Stream<Item = Data> {
        let stream = stream_normalized(&self.base_url, options, self.stream_signal.clone())
            .await
            .expect("Failed to connect to WebSocket");

        // We use Box::pin to heap-allocate the stream and ensure it implements
        // Unpin for safe async handling across lifetimes.
        handle_ws_stream(Box::pin(stream), Some(Arc::new(instrument)), None)
    }
}

fn handle_ws_stream<S>(
    stream: S,
    instrument: Option<Arc<InstrumentMiniInfo>>,
    instrument_map: Option<HashMap<TardisInstrumentKey, Arc<InstrumentMiniInfo>>>,
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
                    let info = instrument.clone().or_else(|| {
                        instrument_map
                            .as_ref()
                            .and_then(|map| determine_instrument_info(&msg, map))
                    });

                    if let Some(info) = info {
                        if let Some(data) = parse_tardis_ws_message(msg, info) {
                            yield data;
                        }
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
    instrument_map: &HashMap<TardisInstrumentKey, Arc<InstrumentMiniInfo>>,
) -> Option<Arc<InstrumentMiniInfo>> {
    let key = match msg {
        WsMessage::BookChange(msg) => {
            TardisInstrumentKey::new(Ustr::from(&msg.symbol), msg.exchange.clone())
        }
        WsMessage::BookSnapshot(msg) => {
            TardisInstrumentKey::new(Ustr::from(&msg.symbol), msg.exchange.clone())
        }
        WsMessage::Trade(msg) => {
            TardisInstrumentKey::new(Ustr::from(&msg.symbol), msg.exchange.clone())
        }
        WsMessage::TradeBar(msg) => {
            TardisInstrumentKey::new(Ustr::from(&msg.symbol), msg.exchange.clone())
        }
        WsMessage::DerivativeTicker(_) => return None,
        WsMessage::Disconnect(_) => return None,
    };
    if let Some(inst) = instrument_map.get(&key) {
        Some(inst.clone())
    } else {
        tracing::error!("Instrument definition info not available for {key:?}");
        None
    }
}
