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
    sync::{Arc, RwLock},
};

use ahash::{HashSet, HashSetExt};
use databento::{
    dbn::{self, PitSymbolMap, Record, SymbolIndex, VersionUpgradePolicy},
    live::Subscription,
};
use indexmap::IndexMap;
use nautilus_core::{UnixNanos, python::to_pyruntime_err, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{Data, InstrumentStatus, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API},
    enums::RecordFlag,
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::InstrumentAny,
};
use tokio::{sync::mpsc::error::TryRecvError, time::Duration};

use super::{
    decode::{decode_imbalance_msg, decode_statistics_msg, decode_status_msg},
    types::{DatabentoImbalance, DatabentoStatistics},
};
use crate::{
    decode::{decode_instrument_def_msg, decode_record},
    types::PublisherId,
};

#[derive(Debug)]
pub enum LiveCommand {
    Subscribe(Subscription),
    Start,
    Close,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // TODO: Optimize this (largest variant 1096 vs 80 bytes)
pub enum LiveMessage {
    Data(Data),
    Instrument(InstrumentAny),
    Status(InstrumentStatus),
    Imbalance(DatabentoImbalance),
    Statistics(DatabentoStatistics),
    Error(anyhow::Error),
    Close,
}

/// Handles a raw TCP data feed from the Databento LSG for a single dataset.
///
/// [`LiveCommand`] messages are recieved synchronously across a channel,
/// decoded records are sent asynchronously on a tokio channel as [`LiveMessage`]s
/// back to a message processing task.
#[derive(Debug)]
pub struct DatabentoFeedHandler {
    key: String,
    dataset: String,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<LiveCommand>,
    msg_tx: tokio::sync::mpsc::Sender<LiveMessage>,
    publisher_venue_map: IndexMap<PublisherId, Venue>,
    symbol_venue_map: Arc<RwLock<HashMap<Symbol, Venue>>>,
    replay: bool,
    use_exchange_as_venue: bool,
}

impl DatabentoFeedHandler {
    /// Creates a new [`DatabentoFeedHandler`] instance.
    #[must_use]
    pub const fn new(
        key: String,
        dataset: String,
        rx: tokio::sync::mpsc::UnboundedReceiver<LiveCommand>,
        tx: tokio::sync::mpsc::Sender<LiveMessage>,
        publisher_venue_map: IndexMap<PublisherId, Venue>,
        symbol_venue_map: Arc<RwLock<HashMap<Symbol, Venue>>>,
        use_exchange_as_venue: bool,
    ) -> Self {
        Self {
            key,
            dataset,
            cmd_rx: rx,
            msg_tx: tx,
            publisher_venue_map,
            symbol_venue_map,
            replay: false,
            use_exchange_as_venue,
        }
    }

    /// Run the feed handler to begin listening for commands and processing messages.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Running feed handler");
        let clock = get_atomic_clock_realtime();
        let mut symbol_map = PitSymbolMap::new();
        let mut instrument_id_map: HashMap<u32, InstrumentId> = HashMap::new();

        let mut buffering_start = None;
        let mut buffered_deltas: HashMap<InstrumentId, Vec<OrderBookDelta>> = HashMap::new();
        let mut deltas_count = 0_u64;
        let timeout = Duration::from_secs(5); // Hard-coded timeout for now

        let result = tokio::time::timeout(
            timeout,
            databento::LiveClient::builder()
                .key(self.key.clone())?
                .dataset(self.dataset.clone())
                .upgrade_policy(VersionUpgradePolicy::UpgradeToV2)
                .build(),
        )
        .await?;
        tracing::info!("Connected");

        let mut client = if let Ok(client) = result {
            client
        } else {
            self.msg_tx.send(LiveMessage::Close).await?;
            self.cmd_rx.close();
            anyhow::bail!("Timeout connecting to LSG");
        };

        // Timeout awaiting the next record before checking for a command
        let timeout = Duration::from_millis(10);

        // Flag to control whether to continue to await next record
        let mut running = false;

        loop {
            if self.msg_tx.is_closed() {
                tracing::debug!("Message channel was closed: stopping");
                break;
            }

            match self.cmd_rx.try_recv() {
                Ok(cmd) => {
                    tracing::debug!("Received command: {cmd:?}");
                    match cmd {
                        LiveCommand::Subscribe(sub) => {
                            if !self.replay & sub.start.is_some() {
                                self.replay = true;
                            }
                            client.subscribe(sub).await.map_err(to_pyruntime_err)?;
                        }
                        LiveCommand::Start => {
                            buffering_start = if self.replay {
                                Some(clock.get_time_ns())
                            } else {
                                None
                            };
                            client.start().await.map_err(to_pyruntime_err)?;
                            running = true;
                            tracing::debug!("Started");
                        }
                        LiveCommand::Close => {
                            self.msg_tx.send(LiveMessage::Close).await?;
                            if running {
                                client.close().await.map_err(to_pyruntime_err)?;
                                tracing::debug!("Closed inner client");
                            }
                            break;
                        }
                    }
                }
                Err(TryRecvError::Empty) => {} // No command yet
                Err(TryRecvError::Disconnected) => {
                    tracing::debug!("Disconnected");
                    break;
                }
            }

            if !running {
                continue;
            }

            // Await the next record with a timeout
            let result = tokio::time::timeout(timeout, client.next_record()).await;
            let record_opt = match result {
                Ok(record_opt) => record_opt,
                Err(_) => continue, // Timeout
            };

            let record = match record_opt {
                Ok(Some(record)) => record,
                Ok(None) => break, // Session ended normally
                Err(e) => {
                    // Fail the session entirely for now. Consider refining
                    // this strategy to handle specific errors more gracefully.
                    self.send_msg(LiveMessage::Error(anyhow::anyhow!(e))).await;
                    break;
                }
            };

            let ts_init = clock.get_time_ns();
            let mut initialized_books = HashSet::new();

            // Decode record
            if let Some(msg) = record.get::<dbn::ErrorMsg>() {
                handle_error_msg(msg);
            } else if let Some(msg) = record.get::<dbn::SystemMsg>() {
                handle_system_msg(msg);
            } else if let Some(msg) = record.get::<dbn::SymbolMappingMsg>() {
                // Remove instrument ID index as the raw symbol may have changed
                instrument_id_map.remove(&msg.hd.instrument_id);
                handle_symbol_mapping_msg(msg, &mut symbol_map, &mut instrument_id_map);
            } else if let Some(msg) = record.get::<dbn::InstrumentDefMsg>() {
                if self.use_exchange_as_venue {
                    update_instrument_id_map_with_exchange(
                        &symbol_map,
                        &self.symbol_venue_map,
                        &mut instrument_id_map,
                        msg.hd.instrument_id,
                        msg.exchange()?,
                    );
                }
                let data = handle_instrument_def_msg(
                    msg,
                    &record,
                    &symbol_map,
                    &self.publisher_venue_map,
                    &self.symbol_venue_map.read().unwrap(),
                    &mut instrument_id_map,
                    ts_init,
                )?;
                self.send_msg(LiveMessage::Instrument(data)).await;
            } else if let Some(msg) = record.get::<dbn::StatusMsg>() {
                let data = handle_status_msg(
                    msg,
                    &record,
                    &symbol_map,
                    &self.publisher_venue_map,
                    &self.symbol_venue_map.read().unwrap(),
                    &mut instrument_id_map,
                    ts_init,
                )?;
                self.send_msg(LiveMessage::Status(data)).await;
            } else if let Some(msg) = record.get::<dbn::ImbalanceMsg>() {
                let data = handle_imbalance_msg(
                    msg,
                    &record,
                    &symbol_map,
                    &self.publisher_venue_map,
                    &self.symbol_venue_map.read().unwrap(),
                    &mut instrument_id_map,
                    ts_init,
                )?;
                self.send_msg(LiveMessage::Imbalance(data)).await;
            } else if let Some(msg) = record.get::<dbn::StatMsg>() {
                let data = handle_statistics_msg(
                    msg,
                    &record,
                    &symbol_map,
                    &self.publisher_venue_map,
                    &self.symbol_venue_map.read().unwrap(),
                    &mut instrument_id_map,
                    ts_init,
                )?;
                self.send_msg(LiveMessage::Statistics(data)).await;
            } else {
                let (mut data1, data2) = match handle_record(
                    record,
                    &symbol_map,
                    &self.publisher_venue_map,
                    &self.symbol_venue_map.read().unwrap(),
                    &mut instrument_id_map,
                    ts_init,
                    &initialized_books,
                ) {
                    Ok(decoded) => decoded,
                    Err(e) => {
                        tracing::error!("Error decoding record: {e}");
                        continue;
                    }
                };

                if let Some(msg) = record.get::<dbn::MboMsg>() {
                    // Check if should mark book initialized
                    if let Some(Data::Delta(delta)) = &data1 {
                        initialized_books.insert(delta.instrument_id);
                    } else {
                        continue; // No delta yet
                    }

                    if let Data::Delta(delta) = data1.clone().expect("MBO should decode a delta") {
                        let buffer = buffered_deltas.entry(delta.instrument_id).or_default();
                        buffer.push(delta);

                        deltas_count += 1;
                        tracing::trace!(
                            "Buffering delta: {deltas_count} {} {buffering_start:?} flags={}",
                            delta.ts_event,
                            msg.flags.raw(),
                        );

                        // Check if last message in the book event
                        if !RecordFlag::F_LAST.matches(msg.flags.raw()) {
                            continue; // NOT last message
                        }

                        // Check if snapshot
                        if RecordFlag::F_SNAPSHOT.matches(msg.flags.raw()) {
                            continue; // Buffer snapshot
                        }

                        // Check if buffering a replay
                        if let Some(start_ns) = buffering_start {
                            if delta.ts_event <= start_ns {
                                continue; // Continue buffering replay
                            }
                            buffering_start = None;
                        }

                        // SAFETY: We can guarantee a deltas vec exists
                        let buffer = buffered_deltas.remove(&delta.instrument_id).unwrap();
                        let deltas = OrderBookDeltas::new(delta.instrument_id, buffer);
                        let deltas = OrderBookDeltas_API::new(deltas);
                        data1 = Some(Data::Deltas(deltas));
                    }
                }

                if let Some(data) = data1 {
                    self.send_msg(LiveMessage::Data(data)).await;
                }

                if let Some(data) = data2 {
                    self.send_msg(LiveMessage::Data(data)).await;
                }
            }
        }

        self.cmd_rx.close();
        tracing::debug!("Closed command receiver");

        Ok(())
    }

    async fn send_msg(&mut self, msg: LiveMessage) {
        tracing::trace!("Sending {msg:?}");
        match self.msg_tx.send(msg).await {
            Ok(()) => {}
            Err(e) => tracing::error!("Error sending message: {e}"),
        }
    }
}

fn handle_error_msg(msg: &dbn::ErrorMsg) {
    tracing::error!("{msg:?}");
}

fn handle_system_msg(msg: &dbn::SystemMsg) {
    tracing::info!("{msg:?}");
}

fn handle_symbol_mapping_msg(
    msg: &dbn::SymbolMappingMsg,
    symbol_map: &mut PitSymbolMap,
    instrument_id_map: &mut HashMap<u32, InstrumentId>,
) {
    // Update the symbol map
    symbol_map
        .on_symbol_mapping(msg)
        .unwrap_or_else(|_| panic!("Error updating `symbol_map` with {msg:?}"));

    // Remove current entry for instrument
    instrument_id_map.remove(&msg.header().instrument_id);
}

fn update_instrument_id_map_with_exchange(
    symbol_map: &PitSymbolMap,
    symbol_venue_map: &RwLock<HashMap<Symbol, Venue>>,
    instrument_id_map: &mut HashMap<u32, InstrumentId>,
    raw_instrument_id: u32,
    exchange: &str,
) -> InstrumentId {
    let raw_symbol = symbol_map
        .get(raw_instrument_id)
        .expect("Cannot resolve `raw_symbol` from `symbol_map`");
    let symbol = Symbol::from(raw_symbol.as_str());
    let venue = Venue::from(exchange);
    let instrument_id = InstrumentId::new(symbol, venue);
    symbol_venue_map
        .write()
        .unwrap()
        .entry(symbol)
        .or_insert(venue);
    instrument_id_map.insert(raw_instrument_id, instrument_id);
    instrument_id
}

fn update_instrument_id_map(
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &HashMap<Symbol, Venue>,
    instrument_id_map: &mut HashMap<u32, InstrumentId>,
) -> InstrumentId {
    let header = record.header();

    // Check if instrument ID is already in the map
    if let Some(&instrument_id) = instrument_id_map.get(&header.instrument_id) {
        return instrument_id;
    }

    let raw_symbol = symbol_map
        .get_for_rec(record)
        .expect("Cannot resolve `raw_symbol` from `symbol_map`");

    let symbol = Symbol::from_str_unchecked(raw_symbol);

    let publisher_id = header.publisher_id;
    let venue = match symbol_venue_map.get(&symbol) {
        Some(venue) => venue,
        None => publisher_venue_map
            .get(&publisher_id)
            .unwrap_or_else(|| panic!("No venue found for `publisher_id` {publisher_id}")),
    };
    let instrument_id = InstrumentId::new(symbol, *venue);

    instrument_id_map.insert(header.instrument_id, instrument_id);
    instrument_id
}

fn handle_instrument_def_msg(
    msg: &dbn::InstrumentDefMsg,
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &HashMap<Symbol, Venue>,
    instrument_id_map: &mut HashMap<u32, InstrumentId>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = update_instrument_id_map(
        record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    );

    decode_instrument_def_msg(msg, instrument_id, ts_init)
}

fn handle_status_msg(
    msg: &dbn::StatusMsg,
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &HashMap<Symbol, Venue>,
    instrument_id_map: &mut HashMap<u32, InstrumentId>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentStatus> {
    let instrument_id = update_instrument_id_map(
        record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    );

    decode_status_msg(msg, instrument_id, ts_init)
}

fn handle_imbalance_msg(
    msg: &dbn::ImbalanceMsg,
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &HashMap<Symbol, Venue>,
    instrument_id_map: &mut HashMap<u32, InstrumentId>,
    ts_init: UnixNanos,
) -> anyhow::Result<DatabentoImbalance> {
    let instrument_id = update_instrument_id_map(
        record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    );

    let price_precision = 2; // Hard-coded for now

    decode_imbalance_msg(msg, instrument_id, price_precision, ts_init)
}

fn handle_statistics_msg(
    msg: &dbn::StatMsg,
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &HashMap<Symbol, Venue>,
    instrument_id_map: &mut HashMap<u32, InstrumentId>,
    ts_init: UnixNanos,
) -> anyhow::Result<DatabentoStatistics> {
    let instrument_id = update_instrument_id_map(
        record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    );

    let price_precision = 2; // Hard-coded for now

    decode_statistics_msg(msg, instrument_id, price_precision, ts_init)
}

fn handle_record(
    record: dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &HashMap<Symbol, Venue>,
    instrument_id_map: &mut HashMap<u32, InstrumentId>,
    ts_init: UnixNanos,
    initialized_books: &HashSet<InstrumentId>,
) -> anyhow::Result<(Option<Data>, Option<Data>)> {
    let instrument_id = update_instrument_id_map(
        &record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    );

    let price_precision = 2; // Hard-coded for now
    let include_trades = initialized_books.contains(&instrument_id);

    decode_record(
        &record,
        instrument_id,
        price_precision,
        Some(ts_init),
        include_trades,
    )
}
