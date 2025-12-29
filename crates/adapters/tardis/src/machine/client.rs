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
        ReplayNormalizedRequestOptions, StreamNormalizedRequestOptions, TardisInstrumentKey,
        TardisInstrumentMiniInfo,
    },
};
use crate::{config::BookSnapshotOutput, machine::parse::parse_tardis_ws_message};

/// Provides a client for connecting to a [Tardis Machine Server](https://docs.tardis.dev/api/tardis-machine).
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.tardis")
)]
#[derive(Debug, Clone)]
pub struct TardisMachineClient {
    pub base_url: String,
    pub replay_signal: Arc<AtomicBool>,
    pub stream_signal: Arc<AtomicBool>,
    pub instruments: HashMap<TardisInstrumentKey, Arc<TardisInstrumentMiniInfo>>,
    pub normalize_symbols: bool,
    pub book_snapshot_output: BookSnapshotOutput,
}

impl TardisMachineClient {
    /// Creates a new [`TardisMachineClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if `base_url` is not provided and `TARDIS_MACHINE_WS_URL` env var is missing.
    pub fn new(
        base_url: Option<&str>,
        normalize_symbols: bool,
        book_snapshot_output: BookSnapshotOutput,
    ) -> anyhow::Result<Self> {
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
            book_snapshot_output,
        })
    }

    pub fn add_instrument_info(&mut self, info: TardisInstrumentMiniInfo) {
        let key = info.as_tardis_instrument_key();
        self.instruments.insert(key, Arc::new(info));
    }

    /// Returns `true` if `close()` has been called.
    ///
    /// This checks that both replay and stream signals have been set,
    /// which only occurs when `close()` is explicitly called.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        // Use Acquire ordering to synchronize with Release stores in close()
        self.replay_signal.load(Ordering::Acquire) && self.stream_signal.load(Ordering::Acquire)
    }

    pub fn close(&mut self) {
        tracing::debug!("Closing");

        // Use Release ordering to ensure visibility to Acquire loads in is_closed()
        self.replay_signal.store(true, Ordering::Release);
        self.stream_signal.store(true, Ordering::Release);

        tracing::debug!("Closed");
    }

    /// Connects to the Tardis Machine replay WebSocket and yields parsed `Data` items.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection cannot be established.
    pub async fn replay(
        &self,
        options: Vec<ReplayNormalizedRequestOptions>,
    ) -> Result<impl Stream<Item = Result<Data, Error>>, Error> {
        let stream = replay_normalized(&self.base_url, options, self.replay_signal.clone()).await?;

        // We use Box::pin to heap-allocate the stream and ensure it implements
        // Unpin for safe async handling across lifetimes.
        Ok(handle_ws_stream(
            Box::pin(stream),
            None,
            Some(self.instruments.clone()),
            self.book_snapshot_output.clone(),
        ))
    }

    /// Connects to the Tardis Machine stream WebSocket for a single instrument and yields parsed `Data` items.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection cannot be established.
    pub async fn stream(
        &self,
        instrument: TardisInstrumentMiniInfo,
        options: Vec<StreamNormalizedRequestOptions>,
    ) -> Result<impl Stream<Item = Result<Data, Error>>, Error> {
        let stream = stream_normalized(&self.base_url, options, self.stream_signal.clone()).await?;

        // We use Box::pin to heap-allocate the stream and ensure it implements
        // Unpin for safe async handling across lifetimes.
        Ok(handle_ws_stream(
            Box::pin(stream),
            Some(Arc::new(instrument)),
            None,
            self.book_snapshot_output.clone(),
        ))
    }
}

fn handle_ws_stream<S>(
    stream: S,
    instrument: Option<Arc<TardisInstrumentMiniInfo>>,
    instrument_map: Option<HashMap<TardisInstrumentKey, Arc<TardisInstrumentMiniInfo>>>,
    book_snapshot_output: BookSnapshotOutput,
) -> impl Stream<Item = Result<Data, Error>>
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
                    if matches!(msg, WsMessage::Disconnect(_)) {
                        tracing::debug!("Received disconnect message: {msg:?}");
                        continue;
                    }

                    let info = instrument.clone().or_else(|| {
                        instrument_map
                            .as_ref()
                            .and_then(|map| determine_instrument_info(&msg, map))
                    });

                    if let Some(info) = info {
                        if let Some(data) = parse_tardis_ws_message(msg, info, &book_snapshot_output) {
                            yield Ok(data);
                        }
                    } else {
                        tracing::error!("Missing instrument info for message: {msg:?}");
                        yield Err(Error::ConnectionClosed {
                            reason: "Missing instrument definition info".to_string()
                        });
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Error in WebSocket stream: {e:?}");
                    yield Err(e);
                    break;
                }
            }
        }
    }
}

pub fn determine_instrument_info(
    msg: &WsMessage,
    instrument_map: &HashMap<TardisInstrumentKey, Arc<TardisInstrumentMiniInfo>>,
) -> Option<Arc<TardisInstrumentMiniInfo>> {
    let key = match msg {
        WsMessage::BookChange(msg) => {
            TardisInstrumentKey::new(Ustr::from(&msg.symbol), msg.exchange)
        }
        WsMessage::BookSnapshot(msg) => {
            TardisInstrumentKey::new(Ustr::from(&msg.symbol), msg.exchange)
        }
        WsMessage::Trade(msg) => TardisInstrumentKey::new(Ustr::from(&msg.symbol), msg.exchange),
        WsMessage::TradeBar(msg) => TardisInstrumentKey::new(Ustr::from(&msg.symbol), msg.exchange),
        WsMessage::DerivativeTicker(msg) => {
            TardisInstrumentKey::new(Ustr::from(&msg.symbol), msg.exchange)
        }
        WsMessage::Disconnect(_) => return None,
    };
    if let Some(inst) = instrument_map.get(&key) {
        Some(inst.clone())
    } else {
        tracing::error!("Instrument definition info not available for {key:?}");
        None
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_is_closed_initial_state() {
        let client = TardisMachineClient::new(
            Some("ws://localhost:8001"),
            false,
            BookSnapshotOutput::Deltas,
        )
        .unwrap();
        // Initially neither signal is set, so is_closed should be false
        assert!(!client.is_closed());
    }

    #[rstest]
    fn test_is_closed_after_close() {
        let mut client = TardisMachineClient::new(
            Some("ws://localhost:8001"),
            false,
            BookSnapshotOutput::Deltas,
        )
        .unwrap();
        client.close();
        // After close(), both signals are set, so is_closed should be true
        assert!(client.is_closed());
    }

    #[rstest]
    fn test_is_closed_partial_signal() {
        let client = TardisMachineClient::new(
            Some("ws://localhost:8001"),
            false,
            BookSnapshotOutput::Deltas,
        )
        .unwrap();
        // Set only one signal - is_closed should still be false
        // (since close() wasn't called, which sets both)
        client.replay_signal.store(true, Ordering::Release);
        assert!(!client.is_closed());

        client.stream_signal.store(true, Ordering::Release);
        // Now both are set, so is_closed should be true
        assert!(client.is_closed());
    }
}
