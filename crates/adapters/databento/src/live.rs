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
    sync::{Arc, RwLock},
    time::Duration as StdDuration,
};

use ahash::{AHashMap, HashSet, HashSetExt};
use databento::{
    dbn::{self, PitSymbolMap, Record, SymbolIndex},
    live::Subscription,
};
use indexmap::IndexMap;
use nautilus_core::{UnixNanos, consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime};
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
#[allow(
    clippy::large_enum_variant,
    reason = "TODO: Optimize this (largest variant 1096 vs 80 bytes)"
)]
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
/// [`LiveCommand`] messages are received synchronously across a channel,
/// decoded records are sent asynchronously on a tokio channel as [`LiveMessage`]s
/// back to a message processing task.
///
/// # Crash Policy
///
/// This handler intentionally crashes on catastrophic feed issues rather than
/// attempting recovery. If excessive buffering occurs (indicating severe feed
/// misbehavior), the process will run out of memory and terminate. This is by
/// design - such scenarios indicate fundamental problems that require external
/// intervention.
#[derive(Debug)]
pub struct DatabentoFeedHandler {
    key: String,
    dataset: String,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<LiveCommand>,
    msg_tx: tokio::sync::mpsc::Sender<LiveMessage>,
    publisher_venue_map: IndexMap<PublisherId, Venue>,
    symbol_venue_map: Arc<RwLock<AHashMap<Symbol, Venue>>>,
    replay: bool,
    use_exchange_as_venue: bool,
    bars_timestamp_on_close: bool,
}

impl DatabentoFeedHandler {
    /// Creates a new [`DatabentoFeedHandler`] instance.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        key: String,
        dataset: String,
        rx: tokio::sync::mpsc::UnboundedReceiver<LiveCommand>,
        tx: tokio::sync::mpsc::Sender<LiveMessage>,
        publisher_venue_map: IndexMap<PublisherId, Venue>,
        symbol_venue_map: Arc<RwLock<AHashMap<Symbol, Venue>>>,
        use_exchange_as_venue: bool,
        bars_timestamp_on_close: bool,
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
            bars_timestamp_on_close,
        }
    }

    /// Runs the feed handler main loop, processing commands and streaming market data.
    ///
    /// Establishes a connection to the Databento LSG, subscribes to requested data feeds,
    /// and continuously processes incoming market data messages until shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error if any client operation or message handling fails.
    #[allow(clippy::blocks_in_conditions)]
    pub async fn run(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Running feed handler");
        let clock = get_atomic_clock_realtime();
        let mut symbol_map = PitSymbolMap::new();
        let mut instrument_id_map: AHashMap<u32, InstrumentId> = AHashMap::new();

        let mut buffering_start = None;
        let mut buffered_deltas: AHashMap<InstrumentId, Vec<OrderBookDelta>> = AHashMap::new();
        let mut initialized_books = HashSet::new();
        let timeout = Duration::from_secs(5); // Hardcoded timeout for now

        let result = tokio::time::timeout(
            timeout,
            databento::LiveClient::builder()
                .user_agent_extension(NAUTILUS_USER_AGENT.into())
                .key(self.key.clone())?
                .dataset(self.dataset.clone())
                .build(),
        )
        .await?;

        tracing::info!("Connected");

        let mut client = match result {
            Ok(client) => client,
            Err(e) => {
                self.msg_tx.send(LiveMessage::Close).await?;
                self.cmd_rx.close();
                anyhow::bail!("Failed to connect to Databento LSG: {e}");
            }
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
                            if !self.replay && sub.start.is_some() {
                                self.replay = true;
                            }
                            client.subscribe(sub).await?;
                        }
                        LiveCommand::Start => {
                            buffering_start = if self.replay {
                                Some(clock.get_time_ns())
                            } else {
                                None
                            };
                            client.start().await?;
                            running = true;
                            tracing::debug!("Started");
                        }
                        LiveCommand::Close => {
                            self.msg_tx.send(LiveMessage::Close).await?;
                            if running {
                                client.close().await?;
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

            // Decode record
            if let Some(msg) = record.get::<dbn::ErrorMsg>() {
                handle_error_msg(msg);
            } else if let Some(msg) = record.get::<dbn::SystemMsg>() {
                handle_system_msg(msg);
            } else if let Some(msg) = record.get::<dbn::SymbolMappingMsg>() {
                // Remove instrument ID index as the raw symbol may have changed
                instrument_id_map.remove(&msg.hd.instrument_id);
                handle_symbol_mapping_msg(msg, &mut symbol_map, &mut instrument_id_map)?;
            } else if let Some(msg) = record.get::<dbn::InstrumentDefMsg>() {
                if self.use_exchange_as_venue {
                    let exchange = msg.exchange()?;
                    if !exchange.is_empty() {
                        update_instrument_id_map_with_exchange(
                            &symbol_map,
                            &self.symbol_venue_map,
                            &mut instrument_id_map,
                            msg.hd.instrument_id,
                            exchange,
                        )?;
                    }
                }
                let data = {
                    let sym_map = self.read_symbol_venue_map()?;
                    handle_instrument_def_msg(
                        msg,
                        &record,
                        &symbol_map,
                        &self.publisher_venue_map,
                        &sym_map,
                        &mut instrument_id_map,
                        ts_init,
                    )?
                };
                self.send_msg(LiveMessage::Instrument(data)).await;
            } else if let Some(msg) = record.get::<dbn::StatusMsg>() {
                let data = {
                    let sym_map = self.read_symbol_venue_map()?;
                    handle_status_msg(
                        msg,
                        &record,
                        &symbol_map,
                        &self.publisher_venue_map,
                        &sym_map,
                        &mut instrument_id_map,
                        ts_init,
                    )?
                };
                self.send_msg(LiveMessage::Status(data)).await;
            } else if let Some(msg) = record.get::<dbn::ImbalanceMsg>() {
                let data = {
                    let sym_map = self.read_symbol_venue_map()?;
                    handle_imbalance_msg(
                        msg,
                        &record,
                        &symbol_map,
                        &self.publisher_venue_map,
                        &sym_map,
                        &mut instrument_id_map,
                        ts_init,
                    )?
                };
                self.send_msg(LiveMessage::Imbalance(data)).await;
            } else if let Some(msg) = record.get::<dbn::StatMsg>() {
                let data = {
                    let sym_map = self.read_symbol_venue_map()?;
                    handle_statistics_msg(
                        msg,
                        &record,
                        &symbol_map,
                        &self.publisher_venue_map,
                        &sym_map,
                        &mut instrument_id_map,
                        ts_init,
                    )?
                };
                self.send_msg(LiveMessage::Statistics(data)).await;
            } else {
                // Decode a generic record with possible errors
                let (mut data1, data2) = match {
                    let sym_map = self.read_symbol_venue_map()?;
                    handle_record(
                        record,
                        &symbol_map,
                        &self.publisher_venue_map,
                        &sym_map,
                        &mut instrument_id_map,
                        ts_init,
                        &initialized_books,
                        self.bars_timestamp_on_close,
                    )
                } {
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

                    if let Some(Data::Delta(delta)) = &data1 {
                        let buffer = buffered_deltas.entry(delta.instrument_id).or_default();
                        buffer.push(*delta);

                        tracing::trace!(
                            "Buffering delta: {} {buffering_start:?} flags={}",
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
                        let buffer =
                            buffered_deltas
                                .remove(&delta.instrument_id)
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "Internal error: no buffered deltas for instrument {id}",
                                        id = delta.instrument_id
                                    )
                                })?;
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

    /// Sends a message to the message processing task.
    async fn send_msg(&mut self, msg: LiveMessage) {
        tracing::trace!("Sending {msg:?}");
        match self.msg_tx.send(msg).await {
            Ok(()) => {}
            Err(e) => tracing::error!("Error sending message: {e}"),
        }
    }

    /// Acquires a read lock on the symbol-venue map with exponential backoff and timeout.
    ///
    /// # Errors
    ///
    /// Returns an error if the read lock cannot be acquired within the deadline.
    fn read_symbol_venue_map(
        &self,
    ) -> anyhow::Result<std::sync::RwLockReadGuard<'_, AHashMap<Symbol, Venue>>> {
        // Try to acquire the lock with exponential backoff and deadline
        const MAX_WAIT_MS: u64 = 500; // Total maximum wait time
        const INITIAL_DELAY_MICROS: u64 = 10;
        const MAX_DELAY_MICROS: u64 = 1000;

        let deadline = std::time::Instant::now() + StdDuration::from_millis(MAX_WAIT_MS);
        let mut delay = INITIAL_DELAY_MICROS;

        loop {
            match self.symbol_venue_map.try_read() {
                Ok(guard) => return Ok(guard),
                Err(std::sync::TryLockError::WouldBlock) => {
                    if std::time::Instant::now() >= deadline {
                        break;
                    }

                    // Yield to other threads first
                    std::thread::yield_now();

                    // Then sleep with exponential backoff if still blocked
                    if std::time::Instant::now() < deadline {
                        let remaining = deadline - std::time::Instant::now();
                        let sleep_duration = StdDuration::from_micros(delay).min(remaining);
                        std::thread::sleep(sleep_duration);
                        // Exponential backoff with cap and jitter
                        delay = ((delay * 2) + delay / 4).min(MAX_DELAY_MICROS);
                    }
                }
                Err(std::sync::TryLockError::Poisoned(e)) => {
                    anyhow::bail!("symbol_venue_map lock poisoned: {e}");
                }
            }
        }

        anyhow::bail!(
            "Failed to acquire read lock on symbol_venue_map after {MAX_WAIT_MS}ms deadline"
        )
    }
}

/// Handles Databento error messages by logging them.
fn handle_error_msg(msg: &dbn::ErrorMsg) {
    tracing::error!("{msg:?}");
}

/// Handles Databento system messages by logging them.
fn handle_system_msg(msg: &dbn::SystemMsg) {
    tracing::info!("{msg:?}");
}

/// Handles symbol mapping messages and updates the instrument ID map.
///
/// # Errors
///
/// Returns an error if symbol mapping fails.
fn handle_symbol_mapping_msg(
    msg: &dbn::SymbolMappingMsg,
    symbol_map: &mut PitSymbolMap,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
) -> anyhow::Result<()> {
    symbol_map
        .on_symbol_mapping(msg)
        .map_err(|e| anyhow::anyhow!("on_symbol_mapping failed for {msg:?}: {e}"))?;
    instrument_id_map.remove(&msg.header().instrument_id);
    Ok(())
}

/// Updates the instrument ID map using exchange information from the symbol map.
fn update_instrument_id_map_with_exchange(
    symbol_map: &PitSymbolMap,
    symbol_venue_map: &RwLock<AHashMap<Symbol, Venue>>,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
    raw_instrument_id: u32,
    exchange: &str,
) -> anyhow::Result<InstrumentId> {
    let raw_symbol = symbol_map.get(raw_instrument_id).ok_or_else(|| {
        anyhow::anyhow!("Cannot resolve raw_symbol for instrument_id {raw_instrument_id}")
    })?;
    let symbol = Symbol::from(raw_symbol.as_str());
    let venue = Venue::from_code(exchange)
        .map_err(|e| anyhow::anyhow!("Invalid venue code '{exchange}': {e}"))?;
    let instrument_id = InstrumentId::new(symbol, venue);
    let mut map = symbol_venue_map
        .write()
        .map_err(|e| anyhow::anyhow!("symbol_venue_map lock poisoned: {e}"))?;
    map.entry(symbol).or_insert(venue);
    instrument_id_map.insert(raw_instrument_id, instrument_id);
    Ok(instrument_id)
}

fn update_instrument_id_map(
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AHashMap<Symbol, Venue>,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
) -> anyhow::Result<InstrumentId> {
    let header = record.header();

    // Check if instrument ID is already in the map
    if let Some(&instrument_id) = instrument_id_map.get(&header.instrument_id) {
        return Ok(instrument_id);
    }

    let raw_symbol = symbol_map.get_for_rec(record).ok_or_else(|| {
        anyhow::anyhow!(
            "Cannot resolve `raw_symbol` from `symbol_map` for instrument_id {}",
            header.instrument_id
        )
    })?;

    let symbol = Symbol::from_str_unchecked(raw_symbol);

    let publisher_id = header.publisher_id;
    let venue = if let Some(venue) = symbol_venue_map.get(&symbol) {
        *venue
    } else {
        let venue = publisher_venue_map
            .get(&publisher_id)
            .ok_or_else(|| anyhow::anyhow!("No venue found for `publisher_id` {publisher_id}"))?;
        *venue
    };
    let instrument_id = InstrumentId::new(symbol, venue);

    instrument_id_map.insert(header.instrument_id, instrument_id);
    Ok(instrument_id)
}

/// Handles instrument definition messages and decodes them into Nautilus instruments.
///
/// # Errors
///
/// Returns an error if instrument decoding fails.
fn handle_instrument_def_msg(
    msg: &dbn::InstrumentDefMsg,
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AHashMap<Symbol, Venue>,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = update_instrument_id_map(
        record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    )?;

    decode_instrument_def_msg(msg, instrument_id, Some(ts_init))
}

fn handle_status_msg(
    msg: &dbn::StatusMsg,
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AHashMap<Symbol, Venue>,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentStatus> {
    let instrument_id = update_instrument_id_map(
        record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    )?;

    decode_status_msg(msg, instrument_id, Some(ts_init))
}

fn handle_imbalance_msg(
    msg: &dbn::ImbalanceMsg,
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AHashMap<Symbol, Venue>,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
    ts_init: UnixNanos,
) -> anyhow::Result<DatabentoImbalance> {
    let instrument_id = update_instrument_id_map(
        record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    )?;

    let price_precision = 2; // Hardcoded for now

    decode_imbalance_msg(msg, instrument_id, price_precision, Some(ts_init))
}

fn handle_statistics_msg(
    msg: &dbn::StatMsg,
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AHashMap<Symbol, Venue>,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
    ts_init: UnixNanos,
) -> anyhow::Result<DatabentoStatistics> {
    let instrument_id = update_instrument_id_map(
        record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    )?;

    let price_precision = 2; // Hardcoded for now

    decode_statistics_msg(msg, instrument_id, price_precision, Some(ts_init))
}

#[allow(clippy::too_many_arguments)]
fn handle_record(
    record: dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AHashMap<Symbol, Venue>,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
    ts_init: UnixNanos,
    initialized_books: &HashSet<InstrumentId>,
    bars_timestamp_on_close: bool,
) -> anyhow::Result<(Option<Data>, Option<Data>)> {
    let instrument_id = update_instrument_id_map(
        &record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    )?;

    let price_precision = 2; // Hardcoded for now

    // For MBP-1 and quote-based schemas, always include trades since they're integral to the data
    // For MBO, only include trades after the book is initialized to maintain consistency
    let include_trades = if record.get::<dbn::Mbp1Msg>().is_some()
        || record.get::<dbn::TbboMsg>().is_some()
        || record.get::<dbn::Cmbp1Msg>().is_some()
    {
        true // These schemas include trade information directly
    } else {
        initialized_books.contains(&instrument_id) // MBO requires initialized book
    };

    decode_record(
        &record,
        instrument_id,
        price_precision,
        Some(ts_init),
        include_trades,
        bars_timestamp_on_close,
    )
}
