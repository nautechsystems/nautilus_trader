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

//! Databento live feed handler.
//!
//! The feed handler runs a single async task per dataset. It receives
//! [`HandlerCommand`] messages over an unbounded channel and streams decoded
//! market data back as [`DatabentoMessage`]s on an unbounded tokio channel.
//!
//! The inner loop uses `tokio::select!` to concurrently await the next record
//! from the Databento gateway and the next command from the engine, giving
//! near-zero idle CPU and immediate command responsiveness.
//!
//! Heartbeat detection is delegated to the upstream `databento` client, which
//! returns `Error::HeartbeatTimeout` when no data arrives within
//! `heartbeat_interval + 5 s` (default 35 s). The handler treats this as a
//! connection error and enters the reconnection backoff loop.

use std::{
    fmt::{Debug, Display},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use ahash::{AHashMap, HashSet, HashSetExt};
use databento::{
    dbn::{self, PitSymbolMap, Record, SymbolIndex},
    live::Subscription,
};
use indexmap::IndexMap;
use nautilus_core::{
    AtomicMap, UnixNanos, consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Data, InstrumentStatus, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API},
    enums::RecordFlag,
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{Instrument, InstrumentAny},
    types::Currency,
};
use nautilus_network::backoff::ExponentialBackoff;
use time::OffsetDateTime;

use super::{
    decode::{
        decode_imbalance_msg, decode_statistics_msg, decode_status_msg, is_supported_stat_type,
    },
    types::{DatabentoImbalance, DatabentoStatistics, SubscriptionAckEvent},
};
use crate::{
    common::{Credential, build_publisher_venue_map, load_publishers},
    decode::{decode_instrument_def_msg, decode_record},
    symbology::{check_consistent_symbology, infer_symbology_type},
    types::PublisherId,
};

#[derive(Debug)]
pub enum HandlerCommand {
    Subscribe(Subscription),
    SetPricePrecision(Symbol, u8),
    Start,
    Close,
}

#[derive(Debug)]
pub enum DatabentoMessage {
    Data(Data),
    Instrument(Box<InstrumentAny>),
    Status(InstrumentStatus),
    Imbalance(DatabentoImbalance),
    Statistics(DatabentoStatistics),
    SubscriptionAck(SubscriptionAckEvent),
    Error(anyhow::Error),
    Close,
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.databento")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.databento")
)]
pub struct DatabentoLiveClient {
    credential: Credential,
    pub dataset: String,
    is_running: bool,
    is_closed: bool,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    cmd_rx: Option<tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>>,
    publisher_venue_map: IndexMap<PublisherId, Venue>,
    symbol_venue_map: Arc<AtomicMap<Symbol, Venue>>,
    use_exchange_as_venue: bool,
    bars_timestamp_on_close: bool,
    reconnect_timeout_mins: Option<u64>,
}

impl Debug for DatabentoLiveClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DatabentoLiveClient))
            .field("credential", &self.credential)
            .field("dataset", &self.dataset)
            .field("is_running", &self.is_running)
            .field("is_closed", &self.is_closed)
            .finish()
    }
}

impl DatabentoLiveClient {
    /// Creates a new [`DatabentoLiveClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if reading or parsing the publishers file fails.
    pub fn new(
        key: String,
        dataset: String,
        publishers_filepath: PathBuf,
        use_exchange_as_venue: bool,
        bars_timestamp_on_close: Option<bool>,
        reconnect_timeout_mins: Option<i64>,
    ) -> anyhow::Result<Self> {
        let publishers = load_publishers(publishers_filepath)?;
        let publisher_venue_map = build_publisher_venue_map(&publishers);

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        let reconnect_timeout_mins = reconnect_timeout_mins
            .and_then(|mins| if mins >= 0 { Some(mins as u64) } else { None });

        Ok(Self {
            credential: Credential::new(key),
            dataset,
            cmd_tx,
            cmd_rx: Some(cmd_rx),
            is_running: false,
            is_closed: false,
            publisher_venue_map,
            symbol_venue_map: Arc::new(AtomicMap::new()),
            use_exchange_as_venue,
            bars_timestamp_on_close: bars_timestamp_on_close.unwrap_or(true),
            reconnect_timeout_mins,
        })
    }

    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.is_running
    }

    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.is_closed
    }

    /// Subscribes to Databento live data for the requested instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if symbology, schema, timestamp, or precision inputs are invalid,
    /// or if the command cannot be sent to the feed handler.
    #[expect(clippy::needless_pass_by_value)]
    pub fn subscribe(
        &mut self,
        schema: String,
        instrument_ids: Vec<InstrumentId>,
        start: Option<u64>,
        snapshot: Option<bool>,
        price_precisions: Option<Vec<Option<u8>>>,
        stype_in: Option<String>,
    ) -> anyhow::Result<()> {
        if let Some(precisions) = &price_precisions
            && precisions.len() != instrument_ids.len()
        {
            anyhow::bail!(
                "`price_precisions` length ({}) must match `instrument_ids` length ({})",
                precisions.len(),
                instrument_ids.len()
            );
        }

        let symbols: Vec<String> = instrument_ids
            .iter()
            .map(|id| id.symbol.to_string())
            .collect();
        let first_symbol = symbols
            .first()
            .ok_or_else(|| anyhow::anyhow!("No symbols provided"))?;
        let stype_in = match stype_in {
            Some(stype_in) => dbn::SType::from_str(&stype_in)?,
            None => infer_symbology_type(first_symbol),
        };
        let symbols: Vec<&str> = symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(symbols.as_slice())?;
        let mut sub = Subscription::builder()
            .symbols(symbols)
            .schema(dbn::Schema::from_str(&schema)?)
            .stype_in(stype_in)
            .build();

        if let Some(start) = start {
            sub.start = Some(OffsetDateTime::from_unix_timestamp_nanos(i128::from(
                start,
            ))?);
        }
        sub.use_snapshot = snapshot.unwrap_or(false);

        self.symbol_venue_map.rcu(|m| {
            for id in &instrument_ids {
                m.entry(id.symbol).or_insert(id.venue);
            }
        });

        if let Some(precisions) = price_precisions {
            for (instrument_id, precision) in instrument_ids.iter().zip(precisions) {
                if let Some(precision) = precision {
                    self.send_command(HandlerCommand::SetPricePrecision(
                        instrument_id.symbol,
                        precision,
                    ))?;
                }
            }
        }

        self.send_command(HandlerCommand::Subscribe(sub))
    }

    /// Starts the live feed handler and returns its message receiver.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is already closed, already running, or cannot start.
    pub fn start(
        &mut self,
    ) -> anyhow::Result<(
        DatabentoFeedHandler,
        tokio::sync::mpsc::UnboundedReceiver<DatabentoMessage>,
    )> {
        if self.is_closed {
            anyhow::bail!("Client already closed");
        }

        if self.is_running {
            anyhow::bail!("Client already running");
        }

        log::debug!("Starting client");

        let (msg_tx, msg_rx) = tokio::sync::mpsc::unbounded_channel::<DatabentoMessage>();
        let cmd_rx = self
            .cmd_rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("Command receiver already taken"))?;

        let feed_handler = DatabentoFeedHandler::new(
            self.credential.clone(),
            self.dataset.clone(),
            cmd_rx,
            msg_tx,
            self.publisher_venue_map.clone(),
            self.symbol_venue_map.clone(),
            self.use_exchange_as_venue,
            self.bars_timestamp_on_close,
            self.reconnect_timeout_mins,
        );

        self.send_command(HandlerCommand::Start)?;
        self.is_running = true;

        Ok((feed_handler, msg_rx))
    }

    /// Closes the live client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client was never started, is already closed, or cannot send
    /// the close command to the feed handler.
    pub fn close(&mut self) -> anyhow::Result<()> {
        if !self.is_running {
            anyhow::bail!("Client never started");
        }

        if self.is_closed {
            anyhow::bail!("Client already closed");
        }

        log::debug!("Closing client");

        if !self.cmd_tx.is_closed() {
            self.send_command(HandlerCommand::Close)?;
        }

        self.is_running = false;
        self.is_closed = true;

        Ok(())
    }

    fn send_command(&self, cmd: HandlerCommand) -> anyhow::Result<()> {
        self.cmd_tx.send(cmd).map_err(|e| {
            anyhow::Error::new(CommandSendError {
                details: e.to_string(),
            })
        })
    }
}

#[cfg(any(test, feature = "python"))]
pub(crate) fn is_command_send_error(error: &anyhow::Error) -> bool {
    error.is::<CommandSendError>()
}

#[derive(Debug)]
struct CommandSendError {
    details: String,
}

impl Display for CommandSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Failed to send command to Databento feed handler: {}",
            self.details
        )
    }
}

impl std::error::Error for CommandSendError {}

/// Handles a raw TCP data feed from the Databento LSG for a single dataset.
///
/// [`HandlerCommand`] messages are received synchronously across a channel,
/// decoded records are sent asynchronously on a tokio channel as [`DatabentoMessage`]s
/// back to a message processing task.
///
/// # Crash Policy
///
/// This handler intentionally avoids applying downstream backpressure to the
/// live feed. If decoded output cannot be drained, memory pressure is the hard
/// failure mode instead of arbitrary queue limits or delayed market data.
pub struct DatabentoFeedHandler {
    credential: Credential,
    dataset: String,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    msg_tx: tokio::sync::mpsc::UnboundedSender<DatabentoMessage>,
    publisher_venue_map: IndexMap<PublisherId, Venue>,
    symbol_venue_map: Arc<AtomicMap<Symbol, Venue>>,
    replay: bool,
    use_exchange_as_venue: bool,
    bars_timestamp_on_close: bool,
    reconnect_timeout_mins: Option<u64>,
    backoff: ExponentialBackoff,
    subscriptions: Vec<Subscription>,
    buffered_commands: Vec<HandlerCommand>,
    price_precision_overrides: AHashMap<Symbol, u8>,
    gateway_addr: Option<String>,
    success_threshold: Duration,
}

impl Debug for DatabentoFeedHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DatabentoFeedHandler))
            .field("credential", &self.credential)
            .field("dataset", &self.dataset)
            .field("replay", &self.replay)
            .field("reconnect_timeout_mins", &self.reconnect_timeout_mins)
            .field("subscriptions", &self.subscriptions.len())
            .finish()
    }
}

impl DatabentoFeedHandler {
    /// Creates a new [`DatabentoFeedHandler`] instance.
    ///
    /// # Panics
    ///
    /// Panics if exponential backoff creation fails (should never happen with valid hardcoded parameters).
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        credential: Credential,
        dataset: String,
        rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        tx: tokio::sync::mpsc::UnboundedSender<DatabentoMessage>,
        publisher_venue_map: IndexMap<PublisherId, Venue>,
        symbol_venue_map: Arc<AtomicMap<Symbol, Venue>>,
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

        let backoff = ExponentialBackoff::new(Duration::from_secs(1), delay_max, 2.0, 1000, false)
            .expect("hardcoded backoff parameters are valid");

        Self {
            credential,
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
            price_precision_overrides: AHashMap::new(),
            gateway_addr: None,
            success_threshold: Duration::from_secs(60),
        }
    }

    /// Sets a custom gateway address, overriding the default Databento LSG endpoint.
    #[must_use]
    pub fn with_gateway_addr(mut self, addr: String) -> Self {
        self.gateway_addr = Some(addr);
        self
    }

    /// Sets the duration a session must run before it counts as successful.
    ///
    /// A successful session resets the reconnection backoff cycle.
    /// Defaults to 60 seconds.
    #[must_use]
    pub fn with_success_threshold(mut self, threshold: Duration) -> Self {
        self.success_threshold = threshold;
        self
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
    pub async fn run(&mut self) -> anyhow::Result<()> {
        log::debug!("Running feed handler");

        let mut reconnect_start: Option<tokio::time::Instant> = None;
        let mut attempt = 0;

        loop {
            attempt += 1;

            match self.run_session(attempt).await {
                Ok(ran_successfully) => {
                    if ran_successfully {
                        log::info!("Resetting reconnection cycle after successful session");
                        reconnect_start = None;
                        attempt = 0;
                        self.backoff.reset();
                    } else {
                        log::debug!("Session ended normally");
                        break Ok(());
                    }
                }
                Err(e) => {
                    let cycle_start = reconnect_start.get_or_insert_with(tokio::time::Instant::now);

                    if let Some(timeout_mins) = self.reconnect_timeout_mins {
                        let elapsed = cycle_start.elapsed();
                        let timeout = Duration::from_mins(timeout_mins);

                        if elapsed >= timeout {
                            log::error!("Giving up reconnection after {timeout_mins} minutes");
                            self.send_msg(DatabentoMessage::Error(anyhow::anyhow!(
                                "Reconnection timeout after {timeout_mins} minutes: {e}"
                            )));
                            break Err(e);
                        }
                    }

                    let delay = self.backoff.next_duration();

                    log::warn!(
                        "Connection lost (attempt {}): {}. Reconnecting in {}s...",
                        attempt,
                        e,
                        delay.as_secs()
                    );

                    let sleep = tokio::time::sleep(delay);
                    tokio::pin!(sleep);

                    loop {
                        tokio::select! {
                            () = &mut sleep => break,
                            cmd = self.cmd_rx.recv() => {
                                match cmd {
                                    Some(HandlerCommand::Close) => {
                                        log::debug!("Close received during backoff");
                                        return Ok(());
                                    }
                                    None => {
                                        log::debug!("Command channel closed during backoff");
                                        return Ok(());
                                    }
                                    Some(cmd) => {
                                        log::debug!("Buffering command received during backoff: {cmd:?}");
                                        self.buffered_commands.push(cmd);
                                    }
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
            log::info!("Reconnecting (attempt {attempt})...");
        }

        let session_start = tokio::time::Instant::now();
        let clock = get_atomic_clock_realtime();
        let mut symbol_map = PitSymbolMap::new();
        let mut instrument_id_map: AHashMap<u32, InstrumentId> = AHashMap::new();
        let mut instrument_def_price_precision_map: AHashMap<u32, u8> = AHashMap::new();
        let mut subscription_price_precision_map: AHashMap<u32, u8> = AHashMap::new();

        let mut buffering_start = None;
        let mut buffered_deltas: AHashMap<InstrumentId, Vec<OrderBookDelta>> = AHashMap::new();
        let mut initialized_books = HashSet::new();
        let timeout = Duration::from_secs(5); // Hardcoded timeout for now

        let gateway_addr = self.gateway_addr.clone();
        let api_key = self.credential.api_key().to_owned();
        let dataset = self.dataset.clone();

        let result = tokio::time::timeout(timeout, async move {
            let base = databento::LiveClient::builder();
            let base = if let Some(addr) = gateway_addr {
                base.addr(addr).await?
            } else {
                base
            };
            base.user_agent_extension(NAUTILUS_USER_AGENT.into())
                .key(api_key)?
                .dataset(dataset)
                .build()
                .await
        })
        .await?;

        let mut client = match result {
            Ok(client) => {
                if attempt > 1 {
                    log::info!("Reconnected successfully");
                } else {
                    log::info!("Connected");
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
            log::debug!(
                "Processing {} buffered commands",
                self.buffered_commands.len()
            );

            for cmd in self.buffered_commands.drain(..) {
                match cmd {
                    HandlerCommand::Subscribe(sub) => {
                        if !self.replay && sub.start.is_some() {
                            self.replay = true;
                        }
                        self.subscriptions.push(sub);
                    }
                    HandlerCommand::SetPricePrecision(symbol, precision) => {
                        self.price_precision_overrides.insert(symbol, precision);
                    }
                    HandlerCommand::Start => {
                        start_buffered = true;
                    }
                    HandlerCommand::Close => {
                        log::warn!("Close command was buffered, shutting down");
                        return Ok(false);
                    }
                }
            }
        }

        let mut running = false;

        if !self.subscriptions.is_empty() {
            log::info!(
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
            log::info!("Resubscription complete");
        } else if start_buffered {
            log::debug!("Starting session from buffered Start command");
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
                log::debug!("Message channel was closed: stopping");
                return Ok(false);
            }

            // Wait for either a command or a record. When the session has not
            // started yet (`!running`), only commands are awaited. Once running,
            // `next_record` is cancel-safe so `tokio::select!` can safely
            // race both futures.
            if !running {
                match self.cmd_rx.recv().await {
                    Some(HandlerCommand::Subscribe(sub)) => {
                        log::debug!("Received command: Subscribe");

                        if !self.replay && sub.start.is_some() {
                            self.replay = true;
                        }
                        client.subscribe(sub.clone()).await?;
                        let mut sub_for_reconnect = sub;
                        sub_for_reconnect.start = None;
                        self.subscriptions.push(sub_for_reconnect);
                        continue;
                    }
                    Some(HandlerCommand::SetPricePrecision(symbol, precision)) => {
                        log::debug!(
                            "Received command: SetPricePrecision for {symbol} to {precision}"
                        );
                        self.price_precision_overrides.insert(symbol, precision);
                        continue;
                    }
                    Some(HandlerCommand::Start) => {
                        log::debug!("Received command: Start");
                        buffering_start = if self.replay {
                            Some(clock.get_time_ns())
                        } else {
                            None
                        };
                        client.start().await?;
                        running = true;
                        continue;
                    }
                    Some(HandlerCommand::Close) => {
                        self.send_close_msg();
                        return Ok(false);
                    }
                    None => {
                        log::debug!("Command channel disconnected");
                        return Ok(false);
                    }
                }
            }

            let record_opt = tokio::select! {
                cmd = self.cmd_rx.recv() =>
                match cmd {
                    Some(HandlerCommand::Subscribe(sub)) => {
                        log::debug!("Received command: Subscribe");

                        if sub.start.is_some() {
                            self.replay = true;
                            log::error!(
                                "Ignoring `start` on {} subscribe, session already running, Databento drops replay anchors sent after session start",
                                self.dataset,
                            );
                        }
                        client.subscribe(sub.clone()).await?;
                        let mut sub_for_reconnect = sub;
                        sub_for_reconnect.start = None;
                        self.subscriptions.push(sub_for_reconnect);
                        continue;
                    }
                    Some(HandlerCommand::SetPricePrecision(symbol, precision)) => {
                        log::debug!(
                            "Received command: SetPricePrecision for {symbol} to {precision}"
                        );
                        self.price_precision_overrides.insert(symbol, precision);
                        continue;
                    }
                    Some(HandlerCommand::Start) => {
                        log::warn!("Received Start command but session already running");
                        continue;
                    }
                    Some(HandlerCommand::Close) => {
                        self.send_close_msg();
                        client.close().await?;
                        log::debug!("Closed inner client");
                        return Ok(false);
                    }
                    None => {
                        log::debug!("Command channel disconnected");
                        return Ok(false);
                    }
                },
                result = client.next_record() => result,
            };

            let record = match record_opt {
                Ok(Some(record)) => record,
                Ok(None) => {
                    if session_start.elapsed() >= self.success_threshold {
                        log::debug!("Session ended after successful run");
                        return Ok(true);
                    }
                    anyhow::bail!("Session ended by gateway");
                }
                Err(e) => {
                    if session_start.elapsed() >= self.success_threshold {
                        log::debug!("Connection error after successful run: {e}");
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
                if let Some(ack) = handle_system_msg(msg, ts_init) {
                    self.send_msg(DatabentoMessage::SubscriptionAck(ack));
                }
            } else if let Some(msg) = record.get::<dbn::SymbolMappingMsg>() {
                // Remove instrument ID index as the raw symbol may have changed
                instrument_id_map.remove(&msg.hd.instrument_id);
                instrument_def_price_precision_map.remove(&msg.hd.instrument_id);
                update_price_precision_map_with_symbol_mapping_msg(
                    msg,
                    &self.price_precision_overrides,
                    &mut subscription_price_precision_map,
                )?;
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
                let maybe_data = handle_instrument_def_msg(
                    msg,
                    &record,
                    &symbol_map,
                    &self.publisher_venue_map,
                    &self.symbol_venue_map,
                    &mut instrument_id_map,
                    ts_init,
                )?;

                if let Some(data) = maybe_data {
                    instrument_def_price_precision_map
                        .insert(msg.hd.instrument_id, data.price_precision());
                    self.send_msg(DatabentoMessage::Instrument(Box::new(data)));
                }
            } else if let Some(msg) = record.get::<dbn::StatusMsg>() {
                let data = handle_status_msg(
                    msg,
                    &record,
                    &symbol_map,
                    &self.publisher_venue_map,
                    &self.symbol_venue_map,
                    &mut instrument_id_map,
                    ts_init,
                )?;
                self.send_msg(DatabentoMessage::Status(data));
            } else if let Some(msg) = record.get::<dbn::ImbalanceMsg>() {
                let data = handle_imbalance_msg(
                    msg,
                    &record,
                    &symbol_map,
                    &self.publisher_venue_map,
                    &self.symbol_venue_map,
                    &mut instrument_id_map,
                    &instrument_def_price_precision_map,
                    &subscription_price_precision_map,
                    &self.price_precision_overrides,
                    ts_init,
                )?;
                self.send_msg(DatabentoMessage::Imbalance(data));
            } else if let Some(msg) = record.get::<dbn::StatMsg>() {
                let maybe_data = handle_statistics_msg(
                    msg,
                    &record,
                    &symbol_map,
                    &self.publisher_venue_map,
                    &self.symbol_venue_map,
                    &mut instrument_id_map,
                    &instrument_def_price_precision_map,
                    &subscription_price_precision_map,
                    &self.price_precision_overrides,
                    ts_init,
                )?;

                if let Some(data) = maybe_data {
                    self.send_msg(DatabentoMessage::Statistics(data));
                }
            } else {
                // Decode a generic record with possible errors
                let res = handle_record(
                    record,
                    &symbol_map,
                    &self.publisher_venue_map,
                    &self.symbol_venue_map,
                    &mut instrument_id_map,
                    &instrument_def_price_precision_map,
                    &subscription_price_precision_map,
                    &self.price_precision_overrides,
                    ts_init,
                    &initialized_books,
                    self.bars_timestamp_on_close,
                );
                let (mut data1, data2) = match res {
                    Ok(decoded) => decoded,
                    Err(e) => {
                        log::error!("Error decoding record: {e}");
                        continue;
                    }
                };

                if let Some(msg) = record.get::<dbn::MboMsg>() {
                    if let Some(Data::Delta(delta)) = &data1 {
                        initialized_books.insert(delta.instrument_id);
                    } else {
                        continue;
                    }

                    if let Some(Data::Delta(delta)) = &data1 {
                        log::trace!(
                            "Buffering delta: {} {buffering_start:?} flags={}",
                            delta.ts_event,
                            msg.flags.raw(),
                        );

                        match process_mbo_delta(
                            *delta,
                            msg.flags.raw(),
                            &mut buffering_start,
                            &mut buffered_deltas,
                        )? {
                            Some(deltas) => data1 = Some(Data::Deltas(deltas)),
                            None => continue,
                        }
                    }
                }

                if let Some(data) = data1 {
                    self.send_msg(DatabentoMessage::Data(data));
                }

                if let Some(data) = data2 {
                    self.send_msg(DatabentoMessage::Data(data));
                }
            }
        }
    }

    /// Sends a message to the message processing task.
    fn send_msg(&self, msg: DatabentoMessage) {
        log::trace!("Sending {msg:?}");
        match self.msg_tx.send(msg) {
            Ok(()) => {}
            Err(e) => log::error!("Error sending message: {e}"),
        }
    }

    fn send_close_msg(&self) {
        if let Err(e) = self.msg_tx.send(DatabentoMessage::Close) {
            log::debug!("Could not send close message: {e}");
        }
    }
}

/// Handles Databento error messages by logging them.
fn handle_error_msg(msg: &dbn::ErrorMsg) {
    log::error!("{msg:?}");
}

/// Handles Databento system messages, returning a subscription ack event if applicable.
fn handle_system_msg(msg: &dbn::SystemMsg, ts_received: UnixNanos) -> Option<SubscriptionAckEvent> {
    match msg.code() {
        Ok(dbn::SystemCode::SubscriptionAck) => {
            let message = msg.msg().unwrap_or("<invalid utf-8>");
            log::debug!("Subscription acknowledged: {message}");

            let schema = parse_ack_message(message);

            Some(SubscriptionAckEvent {
                schema,
                message: message.to_string(),
                ts_received,
            })
        }
        Ok(dbn::SystemCode::Heartbeat) => {
            log::trace!("Heartbeat received");
            None
        }
        Ok(dbn::SystemCode::SlowReaderWarning) => {
            let message = msg.msg().unwrap_or("<invalid utf-8>");
            log::warn!("Slow reader warning: {message}");
            None
        }
        Ok(dbn::SystemCode::ReplayCompleted) => {
            let message = msg.msg().unwrap_or("<invalid utf-8>");
            log::debug!("Replay completed: {message}");
            None
        }
        _ => {
            log::debug!("{msg:?}");
            None
        }
    }
}

/// Parses a subscription ack message to extract the schema.
fn parse_ack_message(message: &str) -> String {
    // Format: "Subscription request N for <schema> data succeeded"
    message
        .strip_prefix("Subscription request ")
        .and_then(|rest| rest.split_once(" for "))
        .and_then(|(_, after_num)| after_num.strip_suffix(" data succeeded"))
        .map(|schema| schema.trim().to_string())
        .unwrap_or_default()
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

fn update_price_precision_map_with_symbol_mapping_msg(
    msg: &dbn::SymbolMappingMsg,
    price_precision_overrides: &AHashMap<Symbol, u8>,
    subscription_price_precision_map: &mut AHashMap<u32, u8>,
) -> anyhow::Result<()> {
    subscription_price_precision_map.remove(&msg.hd.instrument_id);

    if price_precision_overrides.is_empty() {
        return Ok(());
    }

    let stype_in_symbol = msg
        .stype_in_symbol()
        .map_err(|e| anyhow::anyhow!("Error decoding `stype_in_symbol`: {e}"))?;
    let stype_out_symbol = msg
        .stype_out_symbol()
        .map_err(|e| anyhow::anyhow!("Error decoding `stype_out_symbol`: {e}"))?;

    let price_precision = [stype_in_symbol, stype_out_symbol]
        .into_iter()
        .find_map(|symbol| {
            price_precision_overrides
                .get(&Symbol::from_str_unchecked(symbol))
                .copied()
        });

    if let Some(price_precision) = price_precision {
        subscription_price_precision_map.insert(msg.hd.instrument_id, price_precision);
    }

    Ok(())
}

/// Updates the instrument ID map using exchange information from the symbol map.
fn update_instrument_id_map_with_exchange(
    symbol_map: &PitSymbolMap,
    symbol_venue_map: &AtomicMap<Symbol, Venue>,
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
    symbol_venue_map.rcu(|m| {
        m.entry(symbol).or_insert(venue);
    });
    instrument_id_map.insert(raw_instrument_id, instrument_id);
    Ok(instrument_id)
}

fn update_instrument_id_map(
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AtomicMap<Symbol, Venue>,
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
    let venue = if let Some(venue) = symbol_venue_map.get_cloned(&symbol) {
        venue
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
    symbol_venue_map: &AtomicMap<Symbol, Venue>,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<InstrumentAny>> {
    let instrument_id = update_instrument_id_map(
        record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    )?;

    decode_instrument_def_msg(msg, instrument_id, Some(ts_init), None)
}

fn handle_status_msg(
    msg: &dbn::StatusMsg,
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AtomicMap<Symbol, Venue>,
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

#[expect(clippy::too_many_arguments)]
fn handle_imbalance_msg(
    msg: &dbn::ImbalanceMsg,
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AtomicMap<Symbol, Venue>,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
    instrument_def_price_precision_map: &AHashMap<u32, u8>,
    subscription_price_precision_map: &AHashMap<u32, u8>,
    price_precision_overrides: &AHashMap<Symbol, u8>,
    ts_init: UnixNanos,
) -> anyhow::Result<DatabentoImbalance> {
    let instrument_id = update_instrument_id_map(
        record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    )?;

    let price_precision = resolve_price_precision(
        msg.hd.instrument_id,
        instrument_id,
        instrument_def_price_precision_map,
        subscription_price_precision_map,
        price_precision_overrides,
    );

    decode_imbalance_msg(msg, instrument_id, price_precision, Some(ts_init))
}

#[expect(clippy::too_many_arguments)]
fn handle_statistics_msg(
    msg: &dbn::StatMsg,
    record: &dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AtomicMap<Symbol, Venue>,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
    instrument_def_price_precision_map: &AHashMap<u32, u8>,
    subscription_price_precision_map: &AHashMap<u32, u8>,
    price_precision_overrides: &AHashMap<Symbol, u8>,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<DatabentoStatistics>> {
    // Precheck before symbol resolution so unmodeled types skip cleanly
    if !is_supported_stat_type(msg.stat_type) {
        log::warn!("Skipping unsupported `stat_type` {}", msg.stat_type);
        return Ok(None);
    }

    let instrument_id = update_instrument_id_map(
        record,
        symbol_map,
        publisher_venue_map,
        symbol_venue_map,
        instrument_id_map,
    )?;

    let price_precision = resolve_price_precision(
        msg.hd.instrument_id,
        instrument_id,
        instrument_def_price_precision_map,
        subscription_price_precision_map,
        price_precision_overrides,
    );

    decode_statistics_msg(msg, instrument_id, price_precision, Some(ts_init))
}

#[expect(clippy::too_many_arguments)]
fn handle_record(
    record: dbn::RecordRef,
    symbol_map: &PitSymbolMap,
    publisher_venue_map: &IndexMap<PublisherId, Venue>,
    symbol_venue_map: &AtomicMap<Symbol, Venue>,
    instrument_id_map: &mut AHashMap<u32, InstrumentId>,
    instrument_def_price_precision_map: &AHashMap<u32, u8>,
    subscription_price_precision_map: &AHashMap<u32, u8>,
    price_precision_overrides: &AHashMap<Symbol, u8>,
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

    let price_precision = resolve_price_precision(
        record.header().instrument_id,
        instrument_id,
        instrument_def_price_precision_map,
        subscription_price_precision_map,
        price_precision_overrides,
    );

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

fn resolve_price_precision(
    record_instrument_id: u32,
    instrument_id: InstrumentId,
    instrument_def_price_precision_map: &AHashMap<u32, u8>,
    subscription_price_precision_map: &AHashMap<u32, u8>,
    price_precision_overrides: &AHashMap<Symbol, u8>,
) -> u8 {
    instrument_def_price_precision_map
        .get(&record_instrument_id)
        .copied()
        .or_else(|| {
            subscription_price_precision_map
                .get(&record_instrument_id)
                .copied()
        })
        .or_else(|| {
            price_precision_overrides
                .get(&instrument_id.symbol)
                .copied()
        })
        .unwrap_or(Currency::USD().precision)
}

/// Processes an MBO delta through the buffering state machine.
///
/// Returns `Some(deltas)` when a complete batch is ready to emit (non-snapshot
/// F_LAST with replay buffering complete), or `None` when still accumulating.
fn process_mbo_delta(
    delta: OrderBookDelta,
    flags: u8,
    buffering_start: &mut Option<UnixNanos>,
    buffered_deltas: &mut AHashMap<InstrumentId, Vec<OrderBookDelta>>,
) -> anyhow::Result<Option<OrderBookDeltas_API>> {
    let is_last = RecordFlag::F_LAST.matches(flags);
    let is_snapshot = RecordFlag::F_SNAPSHOT.matches(flags);

    // Most live MBO events are single non-snapshot deltas, avoid map churn on that path
    if is_last
        && !is_snapshot
        && buffering_start.is_none()
        && !buffered_deltas.contains_key(&delta.instrument_id)
    {
        let deltas = OrderBookDeltas::new(delta.instrument_id, vec![delta]);
        return Ok(Some(OrderBookDeltas_API::new(deltas)));
    }

    let buffer = buffered_deltas.entry(delta.instrument_id).or_default();
    buffer.push(delta);

    if !is_last {
        return Ok(None);
    }

    if is_snapshot {
        return Ok(None);
    }

    if let Some(start_ns) = *buffering_start {
        if delta.ts_event <= start_ns {
            return Ok(None);
        }
        *buffering_start = None;
    }

    let buffer = buffered_deltas
        .remove(&delta.instrument_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Internal error: no buffered deltas for instrument {id}",
                id = delta.instrument_id
            )
        })?;
    let deltas = OrderBookDeltas::new(delta.instrument_id, buffer);
    Ok(Some(OrderBookDeltas_API::new(deltas)))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use databento::live::Subscription;
    use indexmap::IndexMap;
    use rstest::*;
    use time::macros::datetime;

    use super::*;

    fn create_test_handler(reconnect_timeout_mins: Option<u64>) -> DatabentoFeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (msg_tx, _msg_rx) = tokio::sync::mpsc::unbounded_channel();

        DatabentoFeedHandler::new(
            Credential::new("test_key"),
            "GLBX.MDP3".to_string(),
            cmd_rx,
            msg_tx,
            IndexMap::new(),
            Arc::new(AtomicMap::new()),
            false,
            false,
            reconnect_timeout_mins,
        )
    }

    fn create_test_client() -> DatabentoLiveClient {
        DatabentoLiveClient::new(
            "test-api-key".to_string(),
            "GLBX.MDP3".to_string(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json"),
            true,
            None,
            None,
        )
        .unwrap()
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
        assert_eq!(handler.credential.api_key(), "test_key");
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

    #[rstest]
    fn test_subscribe_uses_explicit_parent_stype() {
        let mut client = create_test_client();

        client
            .subscribe(
                "definition".to_string(),
                vec![InstrumentId::from("ES.FUT.GLBX")],
                None,
                None,
                None,
                Some("parent".to_string()),
            )
            .unwrap();

        let command = client.cmd_rx.as_mut().unwrap().try_recv().unwrap();
        match command {
            HandlerCommand::Subscribe(sub) => {
                assert_eq!(sub.schema, dbn::Schema::Definition);
                assert_eq!(sub.stype_in, dbn::SType::Parent);
                assert_eq!(sub.symbols.to_api_string(), "ES.FUT");
            }
            other => panic!("expected HandlerCommand::Subscribe, was {other:?}"),
        }
    }

    #[rstest]
    fn test_subscribe_rejects_invalid_stype() {
        let mut client = create_test_client();

        let err = client
            .subscribe(
                "definition".to_string(),
                vec![InstrumentId::from("ES.FUT.GLBX")],
                None,
                None,
                None,
                Some("not-a-stype".to_string()),
            )
            .unwrap_err();

        assert!(err.to_string().contains("not-a-stype"));
        assert!(!is_command_send_error(&err));
        assert!(matches!(
            client.cmd_rx.as_mut().unwrap().try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[rstest]
    fn test_subscribe_classifies_command_send_errors() {
        let mut client = create_test_client();
        client.cmd_rx = None;

        let err = client
            .subscribe(
                "definition".to_string(),
                vec![InstrumentId::from("ES.FUT.GLBX")],
                None,
                None,
                None,
                Some("parent".to_string()),
            )
            .unwrap_err();

        assert!(is_command_send_error(&err));
    }

    #[rstest]
    fn test_close_after_handler_exit_marks_closed() {
        let mut client = create_test_client();
        let (handler, _msg_rx) = client.start().unwrap();
        drop(handler);

        client.close().unwrap();

        assert!(!client.is_running());
        assert!(client.is_closed());
    }

    fn test_delta(instrument_id: InstrumentId, ts_event: u64) -> OrderBookDelta {
        OrderBookDelta::clear(instrument_id, 0, ts_event.into(), 0.into())
    }

    #[rstest]
    fn test_mbo_delta_without_f_last_buffers() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let delta = test_delta(instrument_id, 1_000_000_000);
        let mut buffering_start = None;
        let mut buffered = AHashMap::new();

        let result = process_mbo_delta(delta, 0, &mut buffering_start, &mut buffered).unwrap();

        assert!(result.is_none());
        assert_eq!(buffered[&instrument_id].len(), 1);
    }

    #[rstest]
    fn test_mbo_single_f_last_emits_without_buffering() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let ts_event = 1_000_000_000;
        let mut delta = test_delta(instrument_id, ts_event);
        delta.flags = 128;
        delta.sequence = 42;
        let mut buffering_start = None;
        let mut buffered = AHashMap::new();

        let result =
            process_mbo_delta(delta, delta.flags, &mut buffering_start, &mut buffered).unwrap();

        let emitted = result.expect("single F_LAST delta should emit");
        assert_eq!(emitted.instrument_id, instrument_id);
        assert_eq!(emitted.deltas.len(), 1);
        assert_eq!(emitted.flags, 128);
        assert_eq!(emitted.sequence, 42);
        assert_eq!(emitted.ts_event, UnixNanos::from(ts_event));
        assert_eq!(emitted.deltas[0].instrument_id, instrument_id);
        assert_eq!(emitted.deltas[0].flags, 128);
        assert_eq!(emitted.deltas[0].sequence, 42);
        assert_eq!(emitted.deltas[0].ts_event, UnixNanos::from(ts_event));
        assert!(buffering_start.is_none());
        assert!(buffered.is_empty());
    }

    #[rstest]
    fn test_mbo_delta_with_f_last_emits() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let mut buffering_start = None;
        let mut buffered = AHashMap::new();

        process_mbo_delta(
            test_delta(instrument_id, 1_000_000_000),
            0,
            &mut buffering_start,
            &mut buffered,
        )
        .unwrap();

        let result = process_mbo_delta(
            test_delta(instrument_id, 2_000_000_000),
            128, // F_LAST
            &mut buffering_start,
            &mut buffered,
        )
        .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().deltas.len(), 2);
        assert!(buffered.is_empty());
    }

    #[rstest]
    fn test_mbo_snapshot_with_f_last_buffers() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let mut buffering_start = None;
        let mut buffered = AHashMap::new();

        let result = process_mbo_delta(
            test_delta(instrument_id, 1_000_000_000),
            128 | 32, // F_LAST | F_SNAPSHOT
            &mut buffering_start,
            &mut buffered,
        )
        .unwrap();

        assert!(result.is_none());
        assert_eq!(buffered[&instrument_id].len(), 1);
    }

    #[rstest]
    fn test_mbo_replay_buffers_until_past_start() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let start_ns = 5_000_000_000u64;
        let mut buffering_start = Some(start_ns.into());
        let mut buffered = AHashMap::new();

        let result = process_mbo_delta(
            test_delta(instrument_id, 4_000_000_000),
            128, // F_LAST
            &mut buffering_start,
            &mut buffered,
        )
        .unwrap();
        assert!(result.is_none());

        let result = process_mbo_delta(
            test_delta(instrument_id, 5_000_000_000),
            128,
            &mut buffering_start,
            &mut buffered,
        )
        .unwrap();
        assert!(result.is_none());

        // Delta past start: emits and clears buffering_start
        let result = process_mbo_delta(
            test_delta(instrument_id, 6_000_000_000),
            128,
            &mut buffering_start,
            &mut buffered,
        )
        .unwrap();
        assert!(result.is_some());
        assert!(buffering_start.is_none());
    }

    #[rstest]
    fn test_mbo_multiple_deltas_accumulated() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let mut buffering_start = None;
        let mut buffered = AHashMap::new();

        for i in 0..5 {
            process_mbo_delta(
                test_delta(instrument_id, 1_000_000_000 + i),
                0,
                &mut buffering_start,
                &mut buffered,
            )
            .unwrap();
        }

        let result = process_mbo_delta(
            test_delta(instrument_id, 2_000_000_000),
            128,
            &mut buffering_start,
            &mut buffered,
        )
        .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().deltas.len(), 6);
    }

    #[rstest]
    fn test_mbo_multi_instrument_isolation() {
        let id_a = InstrumentId::from("ESM4.GLBX");
        let id_b = InstrumentId::from("NQM4.GLBX");
        let mut buffering_start = None;
        let mut buffered = AHashMap::new();

        process_mbo_delta(
            test_delta(id_a, 1_000_000_000),
            0,
            &mut buffering_start,
            &mut buffered,
        )
        .unwrap();
        process_mbo_delta(
            test_delta(id_b, 1_000_000_000),
            0,
            &mut buffering_start,
            &mut buffered,
        )
        .unwrap();

        // F_LAST for A: only A's deltas emitted, B remains
        let result = process_mbo_delta(
            test_delta(id_a, 2_000_000_000),
            128,
            &mut buffering_start,
            &mut buffered,
        )
        .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap().instrument_id, id_a);
        assert!(buffered.contains_key(&id_b));
        assert!(!buffered.contains_key(&id_a));
    }

    mod property_tests {
        use proptest::prelude::*;
        use rstest::rstest;

        use super::*;

        proptest! {
            #[rstest]
            fn mbo_buffering_conserves_deltas(
                num_non_last in 0usize..=20,
            ) {
                let instrument_id = InstrumentId::from("ESM4.GLBX");
                let mut buffering_start = None;
                let mut buffered = AHashMap::new();
                let total = num_non_last + 1;

                for i in 0..num_non_last {
                    let result = process_mbo_delta(
                        test_delta(instrument_id, 1_000_000_000 + i as u64),
                        0, // No F_LAST
                        &mut buffering_start,
                        &mut buffered,
                    ).unwrap();
                    prop_assert!(result.is_none());
                }

                let result = process_mbo_delta(
                    test_delta(instrument_id, 2_000_000_000),
                    128, // F_LAST
                    &mut buffering_start,
                    &mut buffered,
                ).unwrap();

                prop_assert!(result.is_some());
                let emitted = result.unwrap();
                prop_assert_eq!(emitted.deltas.len(), total);
                prop_assert!(buffered.is_empty());
            }

            #[rstest]
            fn mbo_snapshots_never_emit(
                num_snapshots in 1usize..=20,
            ) {
                let instrument_id = InstrumentId::from("ESM4.GLBX");
                let mut buffering_start = None;
                let mut buffered = AHashMap::new();

                for i in 0..num_snapshots {
                    let result = process_mbo_delta(
                        test_delta(instrument_id, 1_000_000_000 + i as u64),
                        128 | 32, // F_LAST | F_SNAPSHOT
                        &mut buffering_start,
                        &mut buffered,
                    ).unwrap();
                    prop_assert!(result.is_none());
                }

                prop_assert_eq!(buffered[&instrument_id].len(), num_snapshots);
            }

            #[rstest]
            fn mbo_replay_delays_emission(
                start_offset in 1u64..=100,
                num_before in 1usize..=10,
            ) {
                let instrument_id = InstrumentId::from("ESM4.GLBX");
                let start_ns = 1_000_000_000u64 * start_offset;
                let mut buffering_start = Some(start_ns.into());
                let mut buffered = AHashMap::new();

                for i in 0..num_before {
                    let ts = start_ns - (num_before as u64 - i as u64);
                    let result = process_mbo_delta(
                        test_delta(instrument_id, ts),
                        128, // F_LAST
                        &mut buffering_start,
                        &mut buffered,
                    ).unwrap();
                    prop_assert!(result.is_none());
                }

                let result = process_mbo_delta(
                    test_delta(instrument_id, start_ns + 1),
                    128,
                    &mut buffering_start,
                    &mut buffered,
                ).unwrap();

                prop_assert!(result.is_some());
                prop_assert!(buffering_start.is_none());
                let total = num_before + 1;
                prop_assert_eq!(result.unwrap().deltas.len(), total);
            }
        }
    }
}
