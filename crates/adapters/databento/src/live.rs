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
use nautilus_network::backoff::ExponentialBackoff;
use tokio::{
    sync::mpsc::error::TryRecvError,
    time::{Duration, Instant},
};

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
    reconnect_timeout_mins: Option<u64>,
    backoff: ExponentialBackoff,
    subscriptions: Vec<Subscription>,
    buffered_commands: Vec<LiveCommand>,
}

impl DatabentoFeedHandler {
    /// Creates a new [`DatabentoFeedHandler`] instance.
    ///
    /// # Panics
    ///
    /// Panics if exponential backoff creation fails (should never happen with valid hardcoded parameters).
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: String,
        dataset: String,
        rx: tokio::sync::mpsc::UnboundedReceiver<LiveCommand>,
        tx: tokio::sync::mpsc::Sender<LiveMessage>,
        publisher_venue_map: IndexMap<PublisherId, Venue>,
        symbol_venue_map: Arc<RwLock<AHashMap<Symbol, Venue>>>,
        use_exchange_as_venue: bool,
        bars_timestamp_on_close: bool,
        reconnect_timeout_mins: Option<u64>,
    ) -> Self {
        // Choose max delay based on timeout configuration:
        // - With timeout: 60s max (quick recovery to reconnect within window)
        // - Without timeout (None): 600s max (patient recovery, respectful of infrastructure)
        let delay_max = if reconnect_timeout_mins.is_some() {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(600)
        };

        // SAFETY: Hardcoded parameters are all valid
        let backoff =
            ExponentialBackoff::new(Duration::from_secs(1), delay_max, 2.0, 1000, false).unwrap();

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
            reconnect_timeout_mins,
            backoff,
            subscriptions: Vec::new(),
            buffered_commands: Vec::new(),
        }
    }

    /// Runs the feed handler main loop, processing commands and streaming market data.
    ///
    /// Establishes a connection to the Databento LSG, subscribes to requested data feeds,
    /// and continuously processes incoming market data messages until shutdown.
    ///
    /// Implements automatic reconnection with exponential backoff (1s to 60s with jitter).
    /// Each successful session resets the reconnection cycle, giving the next disconnect
    /// a fresh timeout window. Gives up after `reconnect_timeout_mins` if configured.
    ///
    /// # Errors
    ///
    /// Returns an error if any client operation or message handling fails.
    #[allow(clippy::blocks_in_conditions)]
    pub async fn run(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Running feed handler");

        let mut reconnect_start: Option<Instant> = None;
        let mut attempt = 0;

        loop {
            attempt += 1;

            match self.run_session(attempt).await {
                Ok(ran_successfully) => {
                    if ran_successfully {
                        tracing::info!("Resetting reconnection cycle after successful session");
                        reconnect_start = None;
                        attempt = 0;
                        self.backoff.reset();
                        continue;
                    } else {
                        tracing::info!("Session ended normally");
                        break Ok(());
                    }
                }
                Err(e) => {
                    let cycle_start = reconnect_start.get_or_insert_with(Instant::now);

                    if let Some(timeout_mins) = self.reconnect_timeout_mins {
                        let elapsed = cycle_start.elapsed();
                        let timeout = Duration::from_secs(timeout_mins * 60);

                        if elapsed >= timeout {
                            tracing::error!(
                                "Giving up reconnection after {} minutes",
                                timeout_mins
                            );
                            self.send_msg(LiveMessage::Error(anyhow::anyhow!(
                                "Reconnection timeout after {timeout_mins} minutes: {e}"
                            )))
                            .await;
                            break Err(e);
                        }
                    }

                    let delay = self.backoff.next_duration();

                    tracing::warn!(
                        "Connection lost (attempt {}): {}. Reconnecting in {}s...",
                        attempt,
                        e,
                        delay.as_secs()
                    );

                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        cmd = self.cmd_rx.recv() => {
                            match cmd {
                                Some(LiveCommand::Close) => {
                                    tracing::info!("Close received during backoff");
                                    return Ok(());
                                }
                                None => {
                                    tracing::debug!("Command channel closed during backoff");
                                    return Ok(());
                                }
                                Some(cmd) => {
                                    tracing::debug!("Buffering command received during backoff: {:?}", cmd);
                                    self.buffered_commands.push(cmd);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Runs a single session, handling connection, subscriptions, and data streaming.
    ///
    /// Returns `Ok(bool)` where the bool indicates if the session ran successfully
    /// for a meaningful duration (true) or was intentionally closed (false).
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails, subscription fails, or data streaming encounters an error.
    async fn run_session(&mut self, attempt: usize) -> anyhow::Result<bool> {
        if attempt > 1 {
            tracing::info!("Reconnecting (attempt {})...", attempt);
        }

        let session_start = Instant::now();
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

        let mut client = match result {
            Ok(client) => {
                if attempt > 1 {
                    tracing::info!("Reconnected successfully");
                } else {
                    tracing::info!("Connected");
                }
                client
            }
            Err(e) => {
                anyhow::bail!("Failed to connect to Databento LSG: {e}");
            }
        };

        // Process any commands buffered during reconnection backoff
        let mut start_buffered = false;
        if !self.buffered_commands.is_empty() {
            tracing::info!(
                "Processing {} buffered commands",
                self.buffered_commands.len()
            );
            for cmd in self.buffered_commands.drain(..) {
                match cmd {
                    LiveCommand::Subscribe(sub) => {
                        if !self.replay && sub.start.is_some() {
                            self.replay = true;
                        }
                        self.subscriptions.push(sub);
                    }
                    LiveCommand::Start => {
                        start_buffered = true;
                    }
                    LiveCommand::Close => {
                        tracing::warn!("Close command was buffered, shutting down");
                        return Ok(false);
                    }
                }
            }
        }

        let timeout = Duration::from_millis(10);
        let mut running = false;

        if !self.subscriptions.is_empty() {
            tracing::info!(
                "Resubscribing to {} subscriptions",
                self.subscriptions.len()
            );
            for sub in self.subscriptions.clone() {
                client.subscribe(sub).await?;
            }
            // Strip start timestamps after successful subscription to avoid replaying history on future reconnects
            for sub in &mut self.subscriptions {
                sub.start = None;
            }
            client.start().await?;
            running = true;
            tracing::info!("Resubscription complete");
        } else if start_buffered {
            tracing::info!("Starting session from buffered Start command");
            buffering_start = if self.replay {
                Some(clock.get_time_ns())
            } else {
                None
            };
            client.start().await?;
            running = true;
        }

        loop {
            if self.msg_tx.is_closed() {
                tracing::debug!("Message channel was closed: stopping");
                return Ok(false);
            }

            match self.cmd_rx.try_recv() {
                Ok(cmd) => {
                    tracing::debug!("Received command: {cmd:?}");
                    match cmd {
                        LiveCommand::Subscribe(sub) => {
                            if !self.replay && sub.start.is_some() {
                                self.replay = true;
                            }
                            client.subscribe(sub.clone()).await?;
                            // Store without start to avoid replaying history on reconnect
                            let mut sub_for_reconnect = sub;
                            sub_for_reconnect.start = None;
                            self.subscriptions.push(sub_for_reconnect);
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
                            return Ok(false);
                        }
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    tracing::debug!("Command channel disconnected");
                    return Ok(false);
                }
            }

            if !running {
                continue;
            }

            let result = tokio::time::timeout(timeout, client.next_record()).await;
            let record_opt = match result {
                Ok(record_opt) => record_opt,
                Err(_) => continue,
            };

            let record = match record_opt {
                Ok(Some(record)) => record,
                Ok(None) => {
                    const SUCCESS_THRESHOLD: Duration = Duration::from_secs(60);
                    if session_start.elapsed() >= SUCCESS_THRESHOLD {
                        tracing::info!("Session ended after successful run");
                        return Ok(true);
                    }
                    anyhow::bail!("Session ended by gateway");
                }
                Err(e) => {
                    const SUCCESS_THRESHOLD: Duration = Duration::from_secs(60);
                    if session_start.elapsed() >= SUCCESS_THRESHOLD {
                        tracing::info!("Connection error after successful run: {e}");
                        return Ok(true);
                    }
                    anyhow::bail!("Connection error: {e}");
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
                let res = {
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
                };
                let (mut data1, data2) = match res {
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
    tracing::debug!("{msg:?}");
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use databento::live::Subscription;
    use indexmap::IndexMap;
    use rstest::*;
    use time::macros::datetime;

    use super::*;

    fn create_test_handler(reconnect_timeout_mins: Option<u64>) -> DatabentoFeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (msg_tx, _msg_rx) = tokio::sync::mpsc::channel(100);

        DatabentoFeedHandler::new(
            "test_key".to_string(),
            "GLBX.MDP3".to_string(),
            cmd_rx,
            msg_tx,
            IndexMap::new(),
            Arc::new(RwLock::new(AHashMap::new())),
            false,
            false,
            reconnect_timeout_mins,
        )
    }

    #[rstest]
    #[case(Some(10))]
    #[case(None)]
    fn test_backoff_initialization(#[case] reconnect_timeout_mins: Option<u64>) {
        let handler = create_test_handler(reconnect_timeout_mins);

        assert_eq!(handler.reconnect_timeout_mins, reconnect_timeout_mins);
        assert!(handler.subscriptions.is_empty());
        assert!(handler.buffered_commands.is_empty());
    }

    #[rstest]
    fn test_subscription_with_and_without_start() {
        let start_time = datetime!(2024-01-01 00:00:00 UTC);
        let sub_with_start = Subscription::builder()
            .symbols("ES.FUT")
            .schema(databento::dbn::Schema::Mbp1)
            .start(start_time)
            .build();

        let mut sub_without_start = sub_with_start.clone();
        sub_without_start.start = None;

        assert!(sub_with_start.start.is_some());
        assert!(sub_without_start.start.is_none());
        assert_eq!(sub_with_start.schema, sub_without_start.schema);
        assert_eq!(sub_with_start.symbols, sub_without_start.symbols);
    }

    #[rstest]
    fn test_handler_initialization_state() {
        let handler = create_test_handler(Some(10));

        assert!(!handler.replay);
        assert_eq!(handler.dataset, "GLBX.MDP3");
        assert_eq!(handler.key, "test_key");
        assert!(handler.subscriptions.is_empty());
        assert!(handler.buffered_commands.is_empty());
    }

    #[rstest]
    fn test_handler_with_no_timeout() {
        let handler = create_test_handler(None);

        assert_eq!(handler.reconnect_timeout_mins, None);
        assert!(!handler.replay);
    }

    #[rstest]
    fn test_handler_with_zero_timeout() {
        let handler = create_test_handler(Some(0));

        assert_eq!(handler.reconnect_timeout_mins, Some(0));
        assert!(!handler.replay);
    }
}
