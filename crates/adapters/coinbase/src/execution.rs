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

//! Live execution client implementation for the Coinbase Advanced Trade adapter.

use std::{
    collections::VecDeque,
    future::Future,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    live::{get_runtime, runner::get_exec_event_sender},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateFillReportsBuilder, GenerateOrderStatusReport, GenerateOrderStatusReports,
        GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
        GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder, SubmitOrder,
    },
};
use nautilus_core::{
    MUTEX_POISONED, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide, OrderStatus},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, TradeId, Venue, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money, Price, Quantity},
};
use nautilus_network::retry::RetryConfig;
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use crate::{
    common::{
        consts::COINBASE_VENUE,
        credential::CoinbaseCredential,
        enums::{CoinbaseProductType, CoinbaseWsChannel},
    },
    config::CoinbaseExecClientConfig,
    http::client::CoinbaseHttpClient,
    websocket::{
        client::CoinbaseWebSocketClient,
        handler::{NautilusWsMessage, UserOrderUpdate},
        parse::parse_ws_user_event_to_fill_report,
    },
};

// Coinbase does not publish a formal max for batch_cancel; conservative chunk
// size mirrors the 100 used by other adapters and keeps request bodies small.
const BATCH_CANCEL_CHUNK: usize = 100;

// Bounded LRU to drop replayed fills after reconnect. Size follows the
// pattern used elsewhere; keyed by (venue_order_id, trade_id) as owned strings
// so the global Ustr arena is not polluted with unique trade IDs.
const FILL_DEDUP_CAPACITY: usize = 10_000;

// Bounded LRU for per-order cumulative tracking. Terminal events drop entries
// eagerly; this cap also protects against orders that this client never
// observes a terminal status for (e.g. cancelled out-of-band).
const CUMULATIVE_STATE_CAPACITY: usize = 10_000;

// Coinbase spot account is ready as soon as the REST account state lands, but
// the engine registers it asynchronously; wait up to 30s for that to happen.
const ACCOUNT_REGISTERED_TIMEOUT_SECS: f64 = 30.0;

#[derive(Debug)]
struct FillDedup {
    seen: AHashMap<(String, String), ()>,
    order: VecDeque<(String, String)>,
    capacity: usize,
}

impl FillDedup {
    fn new(capacity: usize) -> Self {
        Self {
            seen: AHashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    // Returns true if the key is new (and inserts it); false when already seen.
    fn insert(&mut self, key: (String, String)) -> bool {
        if self.seen.contains_key(&key) {
            return false;
        }

        if self.order.len() >= self.capacity
            && let Some(oldest) = self.order.pop_front()
        {
            self.seen.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.seen.insert(key, ());
        true
    }
}

// Per-order cumulative state tracked across WS reconnects so that delta-based
// fill synthesis remains correct even when the feed handler is recreated.
// `avg_price` is Coinbase's cumulative weighted-average fill price; the exec
// client derives the per-fill price from the notional delta between successive
// cumulative states.
//
// `quantity` records the largest `cumulative_quantity + leaves_quantity` ever
// observed for the order. Coinbase zeroes `leaves_quantity` on terminal updates
// (REJECTED / CANCELLED / EXPIRED), so the OSR's quantity computed from
// cum+leaves on those events would collapse to filled_qty (or zero). Holding
// the max-observed total lets us restore the original order quantity before
// emitting the terminal report.
#[derive(Debug, Default, Clone)]
struct OrderCumulativeState {
    filled_qty: Option<Quantity>,
    total_fees: Decimal,
    avg_price: Decimal,
    quantity: Option<Quantity>,
}

// Bounded map for per-order cumulative tracking. Insertions track LRU order;
// when the live entry count reaches `capacity`, the oldest non-stale entry is
// evicted. Terminal events call `remove()` which clears the map entry; the
// matching deque slot becomes stale and is reclaimed during the next eviction
// pass (the deque is also trimmed if it grows beyond `2 * capacity`).
#[derive(Debug)]
struct CumulativeStateMap {
    map: AHashMap<String, OrderCumulativeState>,
    order: VecDeque<String>,
    capacity: usize,
}

impl CumulativeStateMap {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            map: AHashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn entry_or_default(&mut self, key: &str) -> &mut OrderCumulativeState {
        if self.map.contains_key(key) {
            // Hit: refresh recency so a long-lived order receiving updates
            // is not evicted by churn on other orders. O(n) lookup and
            // shift; tolerated because user-channel update volume is small
            // relative to capacity
            if let Some(pos) = self.order.iter().position(|k| k == key) {
                self.order.remove(pos);
            }
            self.order.push_back(key.to_string());
        } else {
            self.evict_until_capacity_or_empty();
            self.order.push_back(key.to_string());
            self.map
                .insert(key.to_string(), OrderCumulativeState::default());
        }
        self.map
            .get_mut(key)
            .expect("key was just inserted or confirmed present")
    }

    fn remove(&mut self, key: &str) {
        if self.map.remove(key).is_some() {
            // Drop the matching deque slot too. Without this, a later
            // re-insert of the same key would leave a stale slot ahead of
            // the new live one, and the eviction loop would pop the stale
            // slot and remove the live entry from the map
            self.order.retain(|k| k != key);
        }
    }

    fn evict_until_capacity_or_empty(&mut self) {
        // Evict the oldest live entries until we're under capacity. Stale
        // deque entries (already removed from the map) are skipped naturally
        // because removing a missing key is a no-op
        while self.map.len() >= self.capacity {
            match self.order.pop_front() {
                Some(oldest) => {
                    self.map.remove(&oldest);
                }
                None => break,
            }
        }

        // When the deque accumulates many stale entries (e.g. a long-lived
        // order at the front while later orders churn through terminal
        // events), compact in place: keep live entries in their original
        // order and drop the rest. Bounds memory without ever evicting live
        // state
        if self.order.len() > 2 * self.capacity {
            self.order.retain(|key| self.map.contains_key(key));
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }

    #[cfg(test)]
    fn get(&self, key: &str) -> Option<&OrderCumulativeState> {
        self.map.get(key)
    }

    #[cfg(test)]
    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

/// Live execution client for Coinbase Advanced Trade.
#[derive(Debug)]
pub struct CoinbaseExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: CoinbaseExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: CoinbaseHttpClient,
    ws_user: CoinbaseWebSocketClient,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    instruments_cache: Arc<AHashMap<String, InstrumentAny>>,
    fill_dedup: Arc<Mutex<FillDedup>>,
    cumulative_state: Arc<Mutex<CumulativeStateMap>>,
}

impl CoinbaseExecutionClient {
    /// Creates a new [`CoinbaseExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if credentials cannot be resolved or the underlying
    /// HTTP / WebSocket client cannot be constructed.
    pub fn new(
        core: ExecutionClientCore,
        config: CoinbaseExecClientConfig,
    ) -> anyhow::Result<Self> {
        let credential =
            CoinbaseCredential::resolve(config.api_key.as_deref(), config.api_secret.as_deref())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Coinbase credentials not available; set COINBASE_API_KEY and COINBASE_API_SECRET or pass them in the config"
                    )
                })?;

        let retry_config = RetryConfig {
            max_retries: config.max_retries,
            initial_delay_ms: config.retry_delay_initial_ms,
            max_delay_ms: config.retry_delay_max_ms,
            backoff_factor: 2.0,
            jitter_ms: 250,
            operation_timeout_ms: Some(60_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };

        let http_client = CoinbaseHttpClient::with_credentials(
            credential.clone(),
            config.environment,
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            Some(retry_config),
        )
        .map_err(|e| anyhow::anyhow!("Failed to create Coinbase HTTP client: {e}"))?;

        if let Some(ref url) = config.base_url_rest {
            http_client.set_base_url(url.clone());
        }

        let ws_url = config.ws_url();
        let ws_user =
            CoinbaseWebSocketClient::with_credential(&ws_url, credential, config.transport_backend);

        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            None,
        );

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_user,
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
            instruments_cache: Arc::new(AHashMap::new()),
            fill_dedup: Arc::new(Mutex::new(FillDedup::new(FILL_DEDUP_CAPACITY))),
            cumulative_state: Arc::new(Mutex::new(CumulativeStateMap::with_capacity(
                CUMULATIVE_STATE_CAPACITY,
            ))),
        })
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
            }
        });

        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        tasks.retain(|h| !h.is_finished());
        tasks.push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    // Returns true when the exec client was created with a Margin account,
    // indicating it should handle CFM-backed derivatives traffic.
    fn is_margin(&self) -> bool {
        self.core.account_type == AccountType::Margin
    }

    // Returns true when the instrument resides in the connect-time bootstrap
    // cache. For the Cash (spot) factory this gates spot-only traffic; for the
    // Margin factory the cache contains CFM perp + future products.
    fn is_instrument_cached(&self, instrument_id: &InstrumentId) -> bool {
        self.instruments_cache
            .contains_key(instrument_id.symbol.as_str())
    }

    // Polls the cache until the account is registered or the timeout is hit.
    async fn await_account_registered(&self, timeout_secs: f64) -> anyhow::Result<()> {
        let account_id = self.core.account_id;

        if self.core.cache().account(&account_id).is_some() {
            log::info!("Account {account_id} registered");
            return Ok(());
        }

        let start = Instant::now();
        let timeout = Duration::from_secs_f64(timeout_secs);
        let interval = Duration::from_millis(10);

        loop {
            tokio::time::sleep(interval).await;

            if self.core.cache().account(&account_id).is_some() {
                log::info!("Account {account_id} registered");
                return Ok(());
            }

            if start.elapsed() >= timeout {
                anyhow::bail!(
                    "Timeout waiting for account {account_id} to be registered after {timeout_secs}s"
                );
            }
        }
    }
}

// Converts a UnixNanos to a UTC chrono::DateTime; returns an error when the
// nanosecond value is out of range.
fn unix_nanos_to_utc(ts: UnixNanos) -> anyhow::Result<chrono::DateTime<chrono::Utc>> {
    let secs = (ts.as_u64() / 1_000_000_000) as i64;
    let nanos = (ts.as_u64() % 1_000_000_000) as u32;
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos)
        .ok_or_else(|| anyhow::anyhow!("UnixNanos {ts} is out of range for chrono::DateTime"))
}

#[async_trait(?Send)]
impl ExecutionClient for CoinbaseExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *COINBASE_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account(&self.core.account_id).cloned()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        // If the underlying WS is still alive from a prior stop() that did not
        // explicitly disconnect, tear it down before reconnecting. The
        // in-handler signal path can race with the Disconnect command, leaving
        // the inner connection_mode stale even after disconnect().await, so
        // we rebuild the client outright to guarantee clean cmd_tx/out_rx
        // pairs and a fresh signal.
        if self.ws_user.is_active() || self.ws_user.is_reconnecting() {
            log::info!("Tearing down stale user WS before reconnect");
            self.ws_user.disconnect().await;
            // Abort any prior consumer task; the rebuilt ws_user gets a fresh
            // out_rx so the previous task is otherwise leaked.
            if let Some(handle) = self.ws_stream_handle.take() {
                handle.abort();
            }
            let credential = CoinbaseCredential::resolve(
                self.config.api_key.as_deref(),
                self.config.api_secret.as_deref(),
            )
            .ok_or_else(|| anyhow::anyhow!("Coinbase credentials unavailable for WS reset"))?;
            self.ws_user = CoinbaseWebSocketClient::with_credential(
                &self.config.ws_url(),
                credential,
                self.config.transport_backend,
            );
        }

        if self.core.instruments_initialized() {
            // Instruments were loaded externally; still propagate the cached
            // set to the WS client on reconnect scenarios.
            let cached: Vec<InstrumentAny> = self.instruments_cache.values().cloned().collect();
            if !cached.is_empty() {
                self.ws_user.initialize_instruments(cached).await;
            }
        } else {
            // The Cash (spot) factory loads only spot products; the Margin
            // (derivatives) factory loads the futures universe so CFM perps
            // and dated futures can be reconciled. Mixing the two through a
            // single client is intentionally unsupported, so each factory
            // picks one branch.
            let instruments = if self.is_margin() {
                self.http_client
                    .request_instruments(Some(CoinbaseProductType::Future))
                    .await
                    .context("failed to load Coinbase futures instruments")?
            } else {
                self.http_client
                    .request_instruments(Some(CoinbaseProductType::Spot))
                    .await
                    .context("failed to load Coinbase instruments")?
            };

            let product_kind = if self.is_margin() { "futures" } else { "spot" };

            if instruments.is_empty() {
                log::warn!("Coinbase instrument bootstrap returned no {product_kind} instruments");
            } else {
                log::info!(
                    "Coinbase exec client loaded {} {product_kind} instruments",
                    instruments.len()
                );
            }

            let mut map: AHashMap<String, InstrumentAny> =
                AHashMap::with_capacity(instruments.len());
            for inst in &instruments {
                map.insert(inst.id().symbol.as_str().to_string(), inst.clone());
            }
            self.instruments_cache = Arc::new(map);

            // Propagate to the WS client so the feed handler can resolve
            // user-channel product IDs to cached instruments.
            self.ws_user.initialize_instruments(instruments).await;

            self.core.set_instruments_initialized();
        }

        self.ws_user.set_account_id(self.core.account_id).await;
        self.ws_user.connect().await?;

        // Subscribe to the user channel (product-agnostic). User channel with
        // an empty product list returns events for all products.
        self.ws_user
            .subscribe(CoinbaseWsChannel::User, &[])
            .await
            .context("failed to subscribe to Coinbase user channel")?;

        if self.is_margin() {
            self.ws_user
                .subscribe(CoinbaseWsChannel::FuturesBalanceSummary, &[])
                .await
                .context("failed to subscribe to Coinbase futures_balance_summary channel")?;
        }

        if let Some(mut rx) = self.ws_user.take_out_rx() {
            let fill_dedup = Arc::clone(&self.fill_dedup);
            let cumulative_state = Arc::clone(&self.cumulative_state);
            let emitter = self.emitter.clone();
            let http_client = self.http_client.clone();
            let account_id = self.core.account_id;
            let clock = self.clock;
            let is_margin = self.is_margin();

            let handle = get_runtime().spawn(async move {
                while let Some(message) = rx.recv().await {
                    match message {
                        NautilusWsMessage::UserOrder(carrier) => {
                            handle_user_order_update(
                                *carrier,
                                &emitter,
                                &fill_dedup,
                                &cumulative_state,
                            );
                        }
                        NautilusWsMessage::FuturesBalanceSummary(summary) => {
                            let ts = clock.get_time_ns();
                            match crate::http::parse::parse_ws_cfm_account_state(
                                &summary, account_id, ts, ts,
                            ) {
                                Ok(state) => emitter.send_account_state(state),
                                Err(e) => log::warn!(
                                    "Failed to parse futures_balance_summary into AccountState: {e}"
                                ),
                            }
                        }
                        NautilusWsMessage::Reconnected => {
                            log::info!("Coinbase user WebSocket reconnected");
                            // Re-fetch account state so any balance change
                            // during the disconnect window is picked up. The
                            // margin flavor targets the CFM summary so the
                            // account type matches the registered Margin
                            // account.
                            let refresh = if is_margin {
                                http_client.request_cfm_account_state(account_id).await
                            } else {
                                http_client.request_account_state(account_id).await
                            };

                            match refresh {
                                Ok(state) => emitter.send_account_state(state),
                                Err(e) => {
                                    log::warn!("Failed to refresh account state on reconnect: {e}");
                                }
                            }
                        }
                        NautilusWsMessage::Error(err) => {
                            log::warn!("Coinbase user WebSocket error: {err}");
                        }
                        _ => {}
                    }
                }
            });
            self.ws_stream_handle = Some(handle);
        }

        let account_state = if self.is_margin() {
            self.http_client
                .request_cfm_account_state(self.core.account_id)
                .await
                .context("failed to request Coinbase CFM account state")?
        } else {
            self.http_client
                .request_account_state(self.core.account_id)
                .await
                .context("failed to request Coinbase account state")?
        };

        if !account_state.balances.is_empty() {
            log::info!(
                "Received account state with {} balance(s)",
                account_state.balances.len()
            );
        }
        self.emitter.send_account_state(account_state);

        self.await_account_registered(ACCOUNT_REGISTERED_TIMEOUT_SECS)
            .await?;

        self.core.set_connected();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        self.abort_pending_tasks();
        self.ws_user.disconnect().await;

        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }

        self.core.set_disconnected();
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, environment={:?}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.config.environment,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        self.core.set_stopped();
        self.core.set_disconnected();

        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();
        let is_margin = self.is_margin();

        self.spawn_task("query_account", async move {
            let account_state = if is_margin {
                http_client
                    .request_cfm_account_state(account_id)
                    .await
                    .context("failed to request Coinbase CFM account state")?
            } else {
                http_client
                    .request_account_state(account_id)
                    .await
                    .context("failed to request Coinbase account state")?
            };
            emitter.send_account_state(account_state);
            Ok(())
        });
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();
        let client_order_id = Some(cmd.client_order_id);
        let venue_order_id = cmd.venue_order_id;

        self.spawn_task("query_order", async move {
            match http_client
                .request_order_status_report(account_id, client_order_id, venue_order_id)
                .await
            {
                Ok(report) => emitter.send_order_status_report(report),
                Err(e) => log::warn!("Failed to query order: {e}"),
            }
            Ok(())
        });

        Ok(())
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let report = self
            .http_client
            .request_order_status_report(
                self.core.account_id,
                cmd.client_order_id,
                cmd.venue_order_id,
            )
            .await
            .ok();

        // Filter reports to instruments this client bootstrapped. A Cash
        // client drops derivatives reports (and vice-versa) so mixed activity
        // on the same venue account does not poison the engine state
        // associated with either exec client.
        Ok(report.filter(|r| self.is_instrument_cached(&r.instrument_id)))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let start = cmd.start.map(unix_nanos_to_utc).transpose()?;
        let end = cmd.end.map(unix_nanos_to_utc).transpose()?;

        let mut reports = self
            .http_client
            .request_order_status_reports(
                self.core.account_id,
                cmd.instrument_id,
                cmd.open_only,
                start,
                end,
                None,
            )
            .await?;

        let before = reports.len();
        reports.retain(|r| self.is_instrument_cached(&r.instrument_id));
        if reports.len() != before {
            let scope = if self.is_margin() {
                "non-futures"
            } else {
                "non-spot"
            };
            log::debug!("Filtered {} {scope} order reports", before - reports.len());
        }
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let start = cmd.start.map(unix_nanos_to_utc).transpose()?;
        let end = cmd.end.map(unix_nanos_to_utc).transpose()?;

        let mut reports = self
            .http_client
            .request_fill_reports(
                self.core.account_id,
                cmd.instrument_id,
                cmd.venue_order_id,
                start,
                end,
                None,
            )
            .await?;

        let before = reports.len();
        reports.retain(|r| self.is_instrument_cached(&r.instrument_id));
        if reports.len() != before {
            let scope = if self.is_margin() {
                "non-futures"
            } else {
                "non-spot"
            };
            log::debug!("Filtered {} {scope} fill reports", before - reports.len());
        }
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        // Coinbase spot has no positions.
        if !self.is_margin() {
            return Ok(Vec::new());
        }

        // Errors propagate (matching `generate_order_status_reports` /
        // `generate_fill_reports`) so `generate_mass_status` and the live
        // manager's reconciliation path see venue failures rather than
        // receive a silently-empty report set.
        if let Some(instrument_id) = cmd.instrument_id {
            let report = self
                .http_client
                .request_position_status_report(self.core.account_id, instrument_id)
                .await
                .with_context(|| format!("failed to request CFM position for {instrument_id}"))?;
            Ok(report.map(|r| vec![r]).unwrap_or_default())
        } else {
            self.http_client
                .request_position_status_reports(self.core.account_id)
                .await
                .context("failed to request CFM positions")
        }
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = self.clock.get_time_ns();
        let start = lookback_mins.map(|mins| {
            let lookback_ns = mins * 60 * 1_000_000_000;
            UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns))
        });

        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_now)
            .open_only(false)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let fill_cmd = GenerateFillReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let position_cmd = GeneratePositionStatusReportsBuilder::default()
            .ts_init(ts_now)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let (order_reports, fill_reports, position_reports) = tokio::try_join!(
            self.generate_order_status_reports(&order_cmd),
            self.generate_fill_reports(fill_cmd),
            self.generate_position_status_reports(&position_cmd),
        )?;

        log::info!("Received {} OrderStatusReports", order_reports.len());
        log::info!("Received {} FillReports", fill_reports.len());
        log::info!("Received {} PositionReports", position_reports.len());

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *COINBASE_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            let order = cache
                .order(&cmd.client_order_id)
                .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?;

            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                return Ok(());
            }

            order.clone()
        };

        // The connect-time bootstrap caches only the product family this
        // client was configured for (Cash -> spot, Margin -> futures). An
        // instrument outside that family is either not loaded yet or lives on
        // the other venue scope, so deny instead of forwarding to the venue
        // where the account type cannot reconcile the order's state.
        let instrument_id = order.instrument_id();
        let symbol_key = instrument_id.symbol.as_str();
        if !self.instruments_cache.contains_key(symbol_key) {
            let scope = if self.is_margin() {
                "a Coinbase futures / perpetual product"
            } else {
                "a Coinbase spot product"
            };
            self.emitter.emit_order_denied(
                &order,
                &format!(
                    "Instrument {} is not {scope} in this client's bootstrap cache",
                    order.instrument_id()
                ),
            );
            return Ok(());
        }

        log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
        self.emitter.emit_order_submitted(&order);

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let strategy_id = order.strategy_id();
        let client_order_id = order.client_order_id();
        let side = order.order_side();
        let order_type = order.order_type();
        let quantity = order.quantity();
        let time_in_force = order.time_in_force();
        let price = order.price();
        let trigger_price = order.trigger_price();
        let expire_time = order.expire_time();
        let post_only = order.is_post_only();
        let is_quote_quantity = order.is_quote_quantity();
        let reduce_only = order.is_reduce_only();
        let (leverage, margin_type) = if self.core.account_type == AccountType::Margin {
            (
                self.config.default_leverage,
                self.config.default_margin_type,
            )
        } else {
            (None, None)
        };
        let retail_portfolio_id = self.config.retail_portfolio_id.clone();

        self.spawn_task("submit_order", async move {
            let result = http_client
                .submit_order(
                    client_order_id,
                    instrument_id,
                    side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    expire_time,
                    post_only,
                    is_quote_quantity,
                    leverage,
                    margin_type,
                    reduce_only,
                    retail_portfolio_id,
                )
                .await;

            match result {
                Ok(response) => {
                    if response.success {
                        let venue_id = response
                            .success_response
                            .as_ref()
                            .map(|s| s.order_id.clone())
                            .unwrap_or(response.order_id);

                        if venue_id.is_empty() {
                            log::warn!(
                                "Submit succeeded but no order_id returned for {client_order_id}"
                            );
                        } else {
                            let venue_order_id = VenueOrderId::new(&venue_id);
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_accepted(&order, venue_order_id, ts_event);
                        }
                    } else {
                        let reason = response.error_response.as_ref().map_or_else(
                            || response.failure_reason.clone(),
                            |e| format!("{}: {}", e.error, e.message),
                        );
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            &format!("submit-order-rejected: {reason}"),
                            ts_event,
                            false,
                        );
                    }
                }
                Err(e) => {
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        &format!("submit-order-error: {e}"),
                        ts_event,
                        false,
                    );
                    anyhow::bail!("submit order failed: {e}");
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let ts_event = self.clock.get_time_ns();

        let Some(venue_order_id) = cmd.venue_order_id else {
            self.emitter.emit_order_modify_rejected_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                None,
                "modify-order requires venue_order_id",
                ts_event,
            );
            return Ok(());
        };

        if cmd.price.is_none() && cmd.quantity.is_none() && cmd.trigger_price.is_none() {
            self.emitter.emit_order_modify_rejected_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                Some(venue_order_id),
                "modify-order requires price, quantity, or trigger_price",
                ts_event,
            );
            return Ok(());
        }

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let price = cmd.price;
        let quantity = cmd.quantity;
        let trigger_price = cmd.trigger_price;

        self.spawn_task("modify_order", async move {
            let result = http_client
                .modify_order(venue_order_id, price, quantity, trigger_price)
                .await;

            match result {
                Ok(resp) => {
                    if !resp.success {
                        let reason = resp
                            .errors
                            .iter()
                            .map(|e| {
                                if e.edit_failure_reason.is_empty() {
                                    e.preview_failure_reason.clone()
                                } else {
                                    e.edit_failure_reason.clone()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(",");
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_modify_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Some(venue_order_id),
                            &format!("modify-order-rejected: {reason}"),
                            ts_event,
                        );
                    }
                }
                Err(e) => {
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_modify_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Some(venue_order_id),
                        &format!("modify-order-error: {e}"),
                        ts_event,
                    );
                    anyhow::bail!("modify order failed: {e}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let ts_event = self.clock.get_time_ns();

        let Some(venue_order_id) = cmd.venue_order_id else {
            self.emitter.emit_order_cancel_rejected_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                None,
                "cancel-order requires venue_order_id",
                ts_event,
            );
            return Ok(());
        };

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;

        self.spawn_task("cancel_order", async move {
            match http_client.cancel_orders(&[venue_order_id]).await {
                Ok(resp) => {
                    if let Some(result) = resp.results.first()
                        && !result.success
                    {
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_cancel_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Some(venue_order_id),
                            &format!("cancel-order-rejected: {}", result.failure_reason),
                            ts_event,
                        );
                    }
                }
                Err(e) => {
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Some(venue_order_id),
                        &format!("cancel-order-error: {e}"),
                        ts_event,
                    );
                    anyhow::bail!("cancel order failed: {e}");
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let instrument_id = cmd.instrument_id;
        let side_filter = cmd.order_side;
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let strategy_id = cmd.strategy_id;

        self.spawn_task("cancel_all_orders", async move {
            // Coinbase's `order_status=OPEN` filter excludes PENDING / QUEUED
            // orders that were submitted very recently and are still cancelable.
            // Fetch all reports and filter to any open status locally so a cancel-
            // all issued right after submission does not leave working orders behind.
            let reports = http_client
                .request_order_status_reports(
                    account_id,
                    Some(instrument_id),
                    false,
                    None,
                    None,
                    None,
                )
                .await
                .context("failed to list orders for cancel_all")?;

            // Filter to statuses that are safe to cancel and to the requested
            // side since Coinbase's batch-cancel endpoint has no side parameter.
            //
            // We can't use `OrderStatus::is_cancellable()` because it excludes
            // `Submitted`, but Coinbase's `PENDING` / `QUEUED` map to `Submitted`
            // and are still cancelable. We can't use `is_open()` either because
            // it includes `PendingCancel`, and re-cancelling a `CANCEL_QUEUED`
            // order risks `CancelRejected` flipping the order back to its prior
            // working status.
            let filtered: Vec<(Option<ClientOrderId>, VenueOrderId)> = reports
                .into_iter()
                .filter(|r| {
                    matches!(
                        r.order_status,
                        OrderStatus::Submitted
                            | OrderStatus::Accepted
                            | OrderStatus::Triggered
                            | OrderStatus::PendingUpdate
                            | OrderStatus::PartiallyFilled
                    )
                })
                .filter(|r| side_filter == OrderSide::NoOrderSide || r.order_side == side_filter)
                .map(|r| (r.client_order_id, r.venue_order_id))
                .collect();

            if filtered.is_empty() {
                return Ok(());
            }

            for chunk in filtered.chunks(BATCH_CANCEL_CHUNK) {
                let venue_ids: Vec<VenueOrderId> = chunk.iter().map(|(_, v)| *v).collect();
                match http_client.cancel_orders(&venue_ids).await {
                    Ok(resp) => {
                        for result in &resp.results {
                            if result.success {
                                continue;
                            }
                            let matching = chunk
                                .iter()
                                .find(|(_, vid)| vid.as_str() == result.order_id);
                            if let Some((cid_opt, vid)) = matching
                                && let Some(cid) = cid_opt
                            {
                                let ts_event = clock.get_time_ns();
                                emitter.emit_order_cancel_rejected_event(
                                    strategy_id,
                                    instrument_id,
                                    *cid,
                                    Some(*vid),
                                    &format!("cancel-all-rejected: {}", result.failure_reason),
                                    ts_event,
                                );
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to cancel chunk for {instrument_id}: {e}");
                        let ts_event = clock.get_time_ns();

                        for (cid_opt, vid) in chunk {
                            if let Some(cid) = cid_opt {
                                emitter.emit_order_cancel_rejected_event(
                                    strategy_id,
                                    instrument_id,
                                    *cid,
                                    Some(*vid),
                                    &format!("cancel-all-error: {e}"),
                                    ts_event,
                                );
                            }
                        }
                    }
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        if cmd.cancels.is_empty() {
            return Ok(());
        }

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;

        // Build parallel vectors so we can report per-order failures.
        let entries: Vec<(ClientOrderId, Option<VenueOrderId>)> = cmd
            .cancels
            .iter()
            .map(|c| (c.client_order_id, c.venue_order_id))
            .collect();

        self.spawn_task("batch_cancel_orders", async move {
            let venue_order_ids: Vec<VenueOrderId> =
                entries.iter().filter_map(|(_, v)| *v).collect();

            for (cid, vid_opt) in &entries {
                if vid_opt.is_none() {
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        *cid,
                        None,
                        "batch-cancel requires venue_order_id",
                        ts_event,
                    );
                }
            }

            for chunk in venue_order_ids.chunks(BATCH_CANCEL_CHUNK) {
                match http_client.cancel_orders(chunk).await {
                    Ok(resp) => {
                        for result in &resp.results {
                            if !result.success {
                                let vid = VenueOrderId::new(&result.order_id);
                                let matching = entries
                                    .iter()
                                    .find(|(_, v)| {
                                        v.is_some_and(|id| id.as_str() == result.order_id)
                                    })
                                    .map(|(cid, _)| *cid);
                                if let Some(cid) = matching {
                                    let ts_event = clock.get_time_ns();
                                    emitter.emit_order_cancel_rejected_event(
                                        strategy_id,
                                        instrument_id,
                                        cid,
                                        Some(vid),
                                        &format!(
                                            "batch-cancel-rejected: {}",
                                            result.failure_reason
                                        ),
                                        ts_event,
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("batch_cancel chunk failed: {e}");
                        let ts_event = clock.get_time_ns();

                        for vid in chunk {
                            let matching = entries
                                .iter()
                                .find(|(_, v)| v.is_some_and(|id| id == *vid))
                                .map(|(cid, _)| *cid);
                            if let Some(cid) = matching {
                                emitter.emit_order_cancel_rejected_event(
                                    strategy_id,
                                    instrument_id,
                                    cid,
                                    Some(*vid),
                                    &format!("batch-cancel-error: {e}"),
                                    ts_event,
                                );
                            }
                        }
                    }
                }
            }
            Ok(())
        });

        Ok(())
    }
}

// Processes a single user-channel order update: emits the status report,
// synthesizes a FillReport from the cumulative delta, and deduplicates
// replayed fills by (venue_order_id, trade_id).
fn handle_user_order_update(
    carrier: UserOrderUpdate,
    emitter: &ExecutionEventEmitter,
    fill_dedup: &Arc<Mutex<FillDedup>>,
    cumulative_state: &Arc<Mutex<CumulativeStateMap>>,
) {
    let UserOrderUpdate {
        mut report,
        update,
        instrument,
        is_snapshot,
        ts_event,
        ts_init,
    } = carrier;

    let size_precision = instrument.size_precision();

    let cumulative_qty = if update.cumulative_quantity.is_empty() {
        Quantity::zero(size_precision)
    } else {
        match crate::http::parse::parse_quantity(&update.cumulative_quantity, size_precision) {
            Ok(q) => q,
            Err(e) => {
                log::warn!(
                    "Failed to parse cumulative_quantity for order {}: {e}",
                    update.order_id
                );
                return;
            }
        }
    };

    let cumulative_fees = if update.total_fees.is_empty() {
        Decimal::ZERO
    } else {
        match Decimal::from_str(&update.total_fees) {
            Ok(d) => d,
            Err(e) => {
                log::warn!(
                    "Failed to parse total_fees for order {}: {e}",
                    update.order_id
                );
                return;
            }
        }
    };

    let cumulative_avg = if update.avg_price.is_empty() {
        Decimal::ZERO
    } else {
        match Decimal::from_str(&update.avg_price) {
            Ok(d) => d,
            Err(e) => {
                log::warn!(
                    "Failed to parse avg_price for order {}: {e}",
                    update.order_id
                );
                return;
            }
        }
    };
    let order_id = update.order_id.clone();

    let is_terminal = update.status.is_terminal();

    // Snapshot previous state under lock; update immediately to avoid races
    // between concurrent handler tasks for the same order.
    let (delta_qty, delta_fees, last_fill_price_decimal, restored_quantity) = {
        let mut state = cumulative_state.lock().expect(MUTEX_POISONED);
        let entry = state.entry_or_default(&order_id);
        let prev_qty = entry
            .filled_qty
            .unwrap_or_else(|| Quantity::zero(size_precision));
        let prev_fees = entry.total_fees;
        let prev_avg = entry.avg_price;

        // Track the max-observed total quantity. The freshly-built report has
        // quantity = cum+leaves which is correct for working orders; on
        // terminal events Coinbase zeroes leaves_quantity, so we use the
        // stored max instead.
        let observed_quantity = report.quantity;
        let stored_quantity = match entry.quantity {
            Some(q) if q >= observed_quantity => q,
            _ => observed_quantity,
        };
        entry.quantity = Some(stored_quantity);

        // Snapshots restate the cumulative state of pre-existing open orders.
        // Treat them as the new baseline (so subsequent updates compute correct
        // deltas) but never synthesize a fill from them.
        if is_snapshot {
            entry.filled_qty = Some(cumulative_qty);
            entry.total_fees = cumulative_fees;
            entry.avg_price = cumulative_avg;

            if is_terminal {
                state.remove(&order_id);
            }
            (
                Quantity::zero(size_precision),
                Decimal::ZERO,
                Decimal::ZERO,
                stored_quantity,
            )
        } else {
            let delta_qty = if cumulative_qty > prev_qty {
                cumulative_qty - prev_qty
            } else {
                Quantity::zero(size_precision)
            };
            let delta_fees = cumulative_fees - prev_fees;

            // Derive per-fill price from the cumulative notional delta:
            //   last_px = (avg_now * qty_now - avg_prev * qty_prev) / delta_qty
            // Falls back to the cumulative avg on the first fill (where
            // delta_qty equals qty_now and prev_notional is zero).
            let last_fill_price_decimal = if delta_qty.is_positive() {
                let now_notional = cumulative_avg * cumulative_qty.as_decimal();
                let prev_notional = prev_avg * prev_qty.as_decimal();
                let delta_notional = now_notional - prev_notional;
                let delta_qty_dec = delta_qty.as_decimal();
                if delta_qty_dec.is_zero() {
                    cumulative_avg
                } else {
                    delta_notional / delta_qty_dec
                }
            } else {
                Decimal::ZERO
            };

            entry.filled_qty = Some(cumulative_qty);
            entry.total_fees = cumulative_fees;
            entry.avg_price = cumulative_avg;

            if is_terminal {
                state.remove(&order_id);
            }

            (
                delta_qty,
                delta_fees,
                last_fill_price_decimal,
                stored_quantity,
            )
        }
    };

    // Restore the original order quantity on terminal events when the venue's
    // zeroed leaves_quantity would otherwise collapse the report to filled_qty.
    if is_terminal && report.quantity < restored_quantity {
        report.quantity = restored_quantity;
    }

    emitter.send_order_status_report(*report);

    if !delta_qty.is_positive() || !last_fill_price_decimal.is_sign_positive() {
        return;
    }

    if last_fill_price_decimal.is_zero() {
        return;
    }

    let price_precision = instrument.price_precision();
    let last_px = match Price::from_decimal_dp(last_fill_price_decimal, price_precision) {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "Failed to build Price from derived last_fill={last_fill_price_decimal} at precision {price_precision} for order {}: {e}",
                update.order_id
            );
            return;
        }
    };

    let trade_id = TradeId::new(format!("{}-{}", update.order_id, cumulative_qty));
    let trade_id_str = trade_id.as_str().to_string();

    let is_new = {
        let mut dedup = fill_dedup.lock().expect(MUTEX_POISONED);
        dedup.insert((update.order_id.clone(), trade_id_str))
    };

    if !is_new {
        log::debug!(
            "Dropping duplicate fill venue_order_id={}, trade_id={}",
            update.order_id,
            trade_id,
        );
        return;
    }

    let commission_currency = instrument.quote_currency();
    let commission = match Money::from_decimal(delta_fees, commission_currency) {
        Ok(m) => m,
        Err(e) => {
            log::warn!(
                "Failed to build commission Money for order {}: {e}",
                update.order_id
            );
            return;
        }
    };

    let report = parse_ws_user_event_to_fill_report(
        &update,
        delta_qty,
        last_px,
        commission,
        trade_id,
        &instrument,
        emitter.account_id(),
        ts_event,
        ts_init,
    );
    emitter.send_fill_report(report);
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::{ExecutionEvent, ExecutionReport};
    use nautilus_model::{
        enums::AccountType,
        identifiers::{Symbol, TraderId},
        instruments::CurrencyPair,
        types::Currency,
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::{
            CoinbaseContractExpiryType, CoinbaseOrderSide as CbSide,
            CoinbaseOrderStatus as CbStatus, CoinbaseOrderType as CbType,
            CoinbaseProductType as CbProductType, CoinbaseRiskManagedBy,
            CoinbaseTimeInForce as CbTif, CoinbaseTriggerStatus,
        },
        websocket::messages::WsOrderUpdate,
    };

    #[rstest]
    fn test_fill_dedup_rejects_duplicates() {
        let mut dedup = FillDedup::new(4);
        let key = ("venue-1".to_string(), "trade-1".to_string());
        assert!(dedup.insert(key.clone()));
        assert!(!dedup.insert(key));
    }

    #[rstest]
    fn test_fill_dedup_evicts_oldest_when_full() {
        let mut dedup = FillDedup::new(2);
        assert!(dedup.insert(("v".to_string(), "t1".to_string())));
        assert!(dedup.insert(("v".to_string(), "t2".to_string())));
        // Insert a third; oldest (t1) should be evicted so re-insertion succeeds.
        assert!(dedup.insert(("v".to_string(), "t3".to_string())));
        assert!(dedup.insert(("v".to_string(), "t1".to_string())));
    }

    #[rstest]
    fn test_cumulative_state_evicts_oldest_at_capacity() {
        let mut state = CumulativeStateMap::with_capacity(2);
        state.entry_or_default("a");
        state.entry_or_default("b");
        // Capacity reached; inserting a third evicts "a"
        state.entry_or_default("c");
        assert_eq!(state.len(), 2);
        assert!(state.map.contains_key("b"));
        assert!(state.map.contains_key("c"));
        assert!(!state.map.contains_key("a"));
    }

    #[rstest]
    fn test_cumulative_state_remove_drops_entry_and_allows_reinsert() {
        let mut state = CumulativeStateMap::with_capacity(2);
        state.entry_or_default("a");
        state.entry_or_default("b");
        state.remove("a");
        // After remove, the next insert should fit without evicting "b"
        state.entry_or_default("c");
        assert_eq!(state.len(), 2);
        assert!(state.map.contains_key("b"));
        assert!(state.map.contains_key("c"));
    }

    #[rstest]
    fn test_cumulative_state_remove_and_reinsert_does_not_evict_live_state() {
        // Codex repro: remove() must purge stale deque slots so a later
        // re-insert of the same key cannot have the eviction loop pop the
        // stale slot and remove the now-live entry.
        let mut state = CumulativeStateMap::with_capacity(2);
        state.entry_or_default("a");
        state.remove("a");
        state.entry_or_default("b");
        state.entry_or_default("a");
        // With the bug, inserting "c" pops the stale "a" slot at the front
        // and removes the live "a" entry from the map; the live "b" should
        // be evicted instead because it is now the oldest live entry.
        state.entry_or_default("c");
        assert_eq!(state.len(), 2);
        assert!(
            state.map.contains_key("a"),
            "re-inserted live key must survive"
        );
        assert!(state.map.contains_key("c"));
        assert!(!state.map.contains_key("b"));
    }

    #[rstest]
    fn test_cumulative_state_hit_refreshes_lru_recency() {
        // A repeat access to an existing key must move it to the back of the
        // eviction queue so a hot order receiving many updates is not evicted
        // by churn on other orders.
        let mut state = CumulativeStateMap::with_capacity(2);
        state.entry_or_default("a");
        state.entry_or_default("b");
        // Re-access "a": without the LRU refresh this is a no-op and the
        // next insert evicts "a"; with the refresh it should evict "b".
        state.entry_or_default("a");
        state.entry_or_default("c");
        assert_eq!(state.len(), 2);
        assert!(
            state.map.contains_key("a"),
            "recently-accessed key must survive eviction"
        );
        assert!(state.map.contains_key("c"));
        assert!(!state.map.contains_key("b"));
    }

    #[rstest]
    fn test_cumulative_state_preserves_live_entry_when_trimming_stale() {
        // A long-lived order at the front of the deque must survive any number
        // of terminal events on later orders, and the deque must stay bounded
        // (compacted) so memory does not grow without bound under high churn.
        let mut state = CumulativeStateMap::with_capacity(2);
        state.entry_or_default("live");
        // Churn far beyond 2*capacity to force the deque-compaction path.
        for i in 0..50 {
            let key = format!("t{i}");
            state.entry_or_default(&key);
            state.remove(&key);
        }
        assert!(
            state.map.contains_key("live"),
            "live entry must survive stale-trim cycles"
        );
        assert_eq!(state.len(), 1);
        assert!(
            state.order.len() <= 2 * state.capacity,
            "deque must remain bounded after compaction (was {})",
            state.order.len(),
        );
        // The live key must remain reachable through the deque so future
        // eviction can find and (correctly) evict it. A bug that drops live
        // keys from the deque would let the map grow past capacity on the
        // next series of inserts.
        assert!(
            state.order.iter().any(|k| k == "live"),
            "live key must remain in the deque, was: {:?}",
            state.order,
        );
        // Drive eviction past capacity to confirm the live key still
        // participates in LRU. With capacity=2, "live" plus two new keys
        // means the next insert must evict the next-oldest live key
        // ("live"), not silently grow the map.
        state.entry_or_default("a");
        state.entry_or_default("b");
        state.entry_or_default("c");
        assert_eq!(state.len(), state.capacity);
        assert!(
            !state.map.contains_key("live"),
            "live key should have been evicted in LRU order once capacity demanded it"
        );
    }

    fn test_instrument() -> InstrumentAny {
        let instrument_id =
            InstrumentId::new(Symbol::new("BTC-USD"), Venue::new(Ustr::from("COINBASE")));
        InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            Symbol::new("BTC-USD"),
            Currency::get_or_create_crypto("BTC"),
            Currency::get_or_create_crypto("USD"),
            2,
            8,
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            Some(Quantity::from("0.00000001")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    fn make_emitter() -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut emitter = ExecutionEventEmitter::new(
            get_atomic_clock_realtime(),
            TraderId::from("TRADER-001"),
            AccountId::new("COINBASE-001"),
            AccountType::Cash,
            None,
        );
        emitter.set_sender(tx);
        (emitter, rx)
    }

    fn make_user_order_update(
        cumulative: &str,
        leaves: &str,
        avg_price: &str,
        total_fees: &str,
        status: CbStatus,
    ) -> WsOrderUpdate {
        WsOrderUpdate {
            order_id: "venue-1".to_string(),
            client_order_id: "client-1".to_string(),
            contract_expiry_type: CoinbaseContractExpiryType::Unknown,
            cumulative_quantity: cumulative.to_string(),
            leaves_quantity: leaves.to_string(),
            avg_price: avg_price.to_string(),
            total_fees: total_fees.to_string(),
            status,
            product_id: Ustr::from("BTC-USD"),
            product_type: CbProductType::Spot,
            creation_time: String::new(),
            order_side: CbSide::Buy,
            order_type: CbType::Limit,
            risk_managed_by: CoinbaseRiskManagedBy::Unknown,
            time_in_force: CbTif::GoodUntilCancelled,
            trigger_status: CoinbaseTriggerStatus::InvalidOrderType,
            cancel_reason: String::new(),
            reject_reason: String::new(),
            total_value_after_fees: String::new(),
        }
    }

    fn make_carrier(update: WsOrderUpdate) -> UserOrderUpdate {
        make_carrier_with_kind(update, false)
    }

    fn make_carrier_with_kind(update: WsOrderUpdate, is_snapshot: bool) -> UserOrderUpdate {
        let instrument = test_instrument();
        let report = crate::websocket::parse::parse_ws_user_event_to_order_status_report(
            &update,
            &instrument,
            AccountId::new("COINBASE-001"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        UserOrderUpdate {
            report: Box::new(report),
            update: Box::new(update),
            instrument,
            is_snapshot,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    fn drain_fill_reports(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> Vec<FillReport> {
        let mut reports = Vec::new();

        while let Ok(event) = rx.try_recv() {
            if let ExecutionEvent::Report(ExecutionReport::Fill(report)) = event {
                reports.push(*report);
            }
        }
        reports
    }

    #[rstest]
    fn test_handle_user_order_update_emits_status_report_and_no_fill_when_zero_filled() {
        let (emitter, mut rx) = make_emitter();
        let dedup = Arc::new(Mutex::new(FillDedup::new(64)));
        let state = Arc::new(Mutex::new(CumulativeStateMap::with_capacity(
            CUMULATIVE_STATE_CAPACITY,
        )));

        // Open with no fills yet.
        let update = make_user_order_update("0", "1.0", "0", "0", CbStatus::Open);
        handle_user_order_update(make_carrier(update), &emitter, &dedup, &state);

        // Status report emitted, no fill report.
        let mut got_status = false;
        let mut got_fill = false;

        while let Ok(event) = rx.try_recv() {
            match event {
                ExecutionEvent::Report(ExecutionReport::Order(_)) => got_status = true,
                ExecutionEvent::Report(ExecutionReport::Fill(_)) => got_fill = true,
                _ => {}
            }
        }
        assert!(got_status);
        assert!(!got_fill);
    }

    #[rstest]
    fn test_handle_user_order_update_synthesizes_per_fill_price_from_notional_delta() {
        let (emitter, mut rx) = make_emitter();
        let dedup = Arc::new(Mutex::new(FillDedup::new(64)));
        let state = Arc::new(Mutex::new(CumulativeStateMap::with_capacity(
            CUMULATIVE_STATE_CAPACITY,
        )));

        // First partial: 0.5 @ 100, total_fees=0.05.
        let update_1 = make_user_order_update("0.5", "0.5", "100.00", "0.05", CbStatus::Open);
        handle_user_order_update(make_carrier(update_1), &emitter, &dedup, &state);

        // Second partial: cumulative 1.0 @ 110, total_fees=0.15.
        // delta_qty = 0.5; per_fill_px = (110*1.0 - 100*0.5) / 0.5 = 120.
        // delta_fees = 0.10.
        let update_2 = make_user_order_update("1.0", "0", "110.00", "0.15", CbStatus::Filled);
        handle_user_order_update(make_carrier(update_2), &emitter, &dedup, &state);

        let fills = drain_fill_reports(&mut rx);
        assert_eq!(fills.len(), 2);

        // First synthesized fill mirrors the first partial.
        assert_eq!(fills[0].last_qty, Quantity::from("0.50000000"));
        assert_eq!(fills[0].last_px, Price::from("100.00"));
        assert_eq!(fills[0].commission.as_decimal().to_string(), "0.05");

        // Second synthesized fill is per-fill price (120), not cumulative avg (110).
        assert_eq!(fills[1].last_qty, Quantity::from("0.50000000"));
        assert_eq!(fills[1].last_px, Price::from("120.00"));
        assert_eq!(fills[1].commission.as_decimal().to_string(), "0.10");
    }

    #[rstest]
    fn test_handle_user_order_update_drops_replayed_fills() {
        let (emitter, mut rx) = make_emitter();
        let dedup = Arc::new(Mutex::new(FillDedup::new(64)));
        let state = Arc::new(Mutex::new(CumulativeStateMap::with_capacity(
            CUMULATIVE_STATE_CAPACITY,
        )));

        let update = make_user_order_update("0.5", "0.5", "100.00", "0.05", CbStatus::Open);
        handle_user_order_update(make_carrier(update.clone()), &emitter, &dedup, &state);

        // Simulate a WS reconnect that wipes the cumulative state, then replays
        // the same cumulative=0.5 snapshot. The fill_dedup must drop the
        // synthesized fill because the trade_id matches the prior emission.
        {
            let mut s = state.lock().unwrap();
            s.clear();
        }
        handle_user_order_update(make_carrier(update), &emitter, &dedup, &state);

        let fills = drain_fill_reports(&mut rx);
        assert_eq!(fills.len(), 1, "replay should be deduplicated");
    }

    #[rstest]
    fn test_handle_user_order_update_clears_state_on_terminal_status() {
        let (emitter, mut rx) = make_emitter();
        let dedup = Arc::new(Mutex::new(FillDedup::new(64)));
        let state = Arc::new(Mutex::new(CumulativeStateMap::with_capacity(
            CUMULATIVE_STATE_CAPACITY,
        )));

        let update = make_user_order_update("1.0", "0", "100.00", "0.10", CbStatus::Filled);
        handle_user_order_update(make_carrier(update), &emitter, &dedup, &state);

        // Drain emitted events.
        let _ = drain_fill_reports(&mut rx);

        let s = state.lock().unwrap();
        assert!(
            s.get("venue-1").is_none(),
            "terminal status should remove cumulative state entry"
        );
    }

    #[rstest]
    fn test_handle_user_order_update_skips_when_avg_price_nonpositive() {
        let (emitter, mut rx) = make_emitter();
        let dedup = Arc::new(Mutex::new(FillDedup::new(64)));
        let state = Arc::new(Mutex::new(CumulativeStateMap::with_capacity(
            CUMULATIVE_STATE_CAPACITY,
        )));

        // cumulative_quantity > 0 but avg_price = 0 (defensive: should not emit fill).
        let update = make_user_order_update("0.5", "0.5", "0", "0", CbStatus::Open);
        handle_user_order_update(make_carrier(update), &emitter, &dedup, &state);

        let fills = drain_fill_reports(&mut rx);
        assert!(
            fills.is_empty(),
            "non-positive avg_price should not emit a fill"
        );
    }

    #[rstest]
    fn test_handle_user_order_update_snapshot_does_not_synthesize_fill() {
        let (emitter, mut rx) = make_emitter();
        let dedup = Arc::new(Mutex::new(FillDedup::new(64)));
        let state = Arc::new(Mutex::new(CumulativeStateMap::with_capacity(
            CUMULATIVE_STATE_CAPACITY,
        )));

        // Cold-start snapshot: order was already partially filled before we
        // subscribed. Cumulative_quantity > 0 with positive avg_price would
        // normally synthesize a fill, but the snapshot flag must suppress it.
        let update = make_user_order_update("0.5", "0.5", "100.00", "0.05", CbStatus::Open);
        handle_user_order_update(
            make_carrier_with_kind(update, true),
            &emitter,
            &dedup,
            &state,
        );

        let fills = drain_fill_reports(&mut rx);
        assert!(
            fills.is_empty(),
            "snapshot must not synthesize a fill from pre-existing cumulative state"
        );

        // The snapshot must seed cumulative_state so that the next live update
        // computes a correct delta.
        let s = state.lock().unwrap();
        let entry = s.get("venue-1").expect("snapshot should seed state");
        assert_eq!(entry.filled_qty.unwrap(), Quantity::from("0.50000000"));
    }

    #[rstest]
    fn test_handle_user_order_update_snapshot_then_update_synthesizes_only_delta() {
        let (emitter, mut rx) = make_emitter();
        let dedup = Arc::new(Mutex::new(FillDedup::new(64)));
        let state = Arc::new(Mutex::new(CumulativeStateMap::with_capacity(
            CUMULATIVE_STATE_CAPACITY,
        )));

        // Cold-start snapshot at cumulative=0.5.
        let snap = make_user_order_update("0.5", "0.5", "100.00", "0.05", CbStatus::Open);
        handle_user_order_update(make_carrier_with_kind(snap, true), &emitter, &dedup, &state);

        // Subsequent live update at cumulative=1.0 should emit a single fill
        // for the 0.5 delta only, not the full cumulative.
        let live = make_user_order_update("1.0", "0", "110.00", "0.15", CbStatus::Filled);
        handle_user_order_update(make_carrier(live), &emitter, &dedup, &state);

        let fills = drain_fill_reports(&mut rx);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].last_qty, Quantity::from("0.50000000"));
        // Per-fill price derived from notional delta: (110*1.0 - 100*0.5) / 0.5 = 120.
        assert_eq!(fills[0].last_px, Price::from("120.00"));
        // delta_fees = 0.10.
        assert_eq!(fills[0].commission.as_decimal().to_string(), "0.10");
    }

    #[rstest]
    fn test_handle_user_order_update_terminal_restores_original_quantity() {
        use nautilus_common::messages::{ExecutionEvent, ExecutionReport};

        let (emitter, mut rx) = make_emitter();
        let dedup = Arc::new(Mutex::new(FillDedup::new(64)));
        let state = Arc::new(Mutex::new(CumulativeStateMap::with_capacity(
            CUMULATIVE_STATE_CAPACITY,
        )));

        // Live partial: cumulative=0, leaves=1.0 (full size 1.0 working).
        let working = make_user_order_update("0", "1.0", "0", "0", CbStatus::Open);
        handle_user_order_update(make_carrier(working), &emitter, &dedup, &state);
        // Drain the open report.
        while rx.try_recv().is_ok() {}

        // Cancellation: venue zeroes leaves_quantity. cum+leaves would be 0,
        // but the report's quantity must stay 1.0 (the original order size).
        let cancelled = make_user_order_update("0", "0", "0", "0", CbStatus::Cancelled);
        handle_user_order_update(make_carrier(cancelled), &emitter, &dedup, &state);

        let mut got_terminal_report: Option<OrderStatusReport> = None;

        while let Ok(event) = rx.try_recv() {
            if let ExecutionEvent::Report(ExecutionReport::Order(r)) = event {
                got_terminal_report = Some(*r);
            }
        }
        let report = got_terminal_report.expect("terminal report emitted");
        assert_eq!(
            report.quantity,
            Quantity::from("1.00000000"),
            "terminal report must restore the original order quantity"
        );
    }
}
