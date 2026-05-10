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

//! Live execution client implementation for the dYdX adapter.
//!
//! This module provides the execution client for submitting orders, cancellations,
//! and managing positions on dYdX v4.
//!
//! # Order Types
//!
//! dYdX supports the following order types:
//!
//! - **Market**: Execute immediately at best available price.
//! - **Limit**: Execute at specified price or better.
//! - **Stop Market**: Triggered when price crosses stop price, then executes as market order.
//! - **Stop Limit**: Triggered when price crosses stop price, then places limit order.
//! - **Take Profit Market**: Close position at profit target, executes as market order.
//! - **Take Profit Limit**: Close position at profit target, places limit order.
//!
//! See <https://docs.dydx.xyz/concepts/trading/orders#types> for details.
//!
//! # Order Lifetimes
//!
//! Orders can be short-term (expire by block height) or long-term/stateful (expire by timestamp).
//! Conditional orders (Stop/TakeProfit) are always stateful.
//!
//! See <https://docs.dydx.xyz/concepts/trading/orders#short-term-vs-long-term> for details.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use futures_util::{Stream, StreamExt, pin_mut};
use nautilus_common::{
    clients::ExecutionClient,
    live::{get_runtime, runner::get_exec_event_sender},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    MUTEX_POISONED, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{AccountState, OrderAccepted, OrderCanceled, OrderEventAny},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Symbol, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money},
};
use nautilus_network::retry::RetryConfig;
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use crate::{
    common::{
        consts::DYDX_VENUE,
        credential::{DydxCredential, credential_env_vars},
        instrument_cache::InstrumentCache,
        parse::nanos_to_secs_i64,
    },
    config::DydxAdapterConfig,
    execution::{
        broadcaster::TxBroadcaster,
        encoder::ClientOrderIdEncoder,
        order_builder::OrderMessageBuilder,
        tx_manager::TransactionManager,
        types::{LimitOrderParams, OrderContext},
    },
    grpc::{DydxGrpcClient, SHORT_TERM_ORDER_MAXIMUM_LIFETIME, types::ChainId},
    http::{
        client::DydxHttpClient,
        parse::{
            parse_account_state, parse_fill_report, parse_order_status_report,
            parse_position_status_report,
        },
    },
    websocket::{
        DydxWsDispatchState, OrderIdentity,
        client::DydxWebSocketClient,
        enums::DydxWsOutputMessage,
        fill_report_to_order_filled,
        parse::{parse_ws_fill_report, parse_ws_order_report, parse_ws_position_report},
    },
};

pub mod block_time;
pub mod broadcaster;
pub mod encoder;
pub mod order_builder;
pub mod submitter;
pub mod tx_manager;
pub mod types;
pub mod wallet;

use block_time::BlockTimeMonitor;

fn apply_avg_px_from_fills(order_reports: &mut [OrderStatusReport], fill_reports: &[FillReport]) {
    let mut totals: AHashMap<VenueOrderId, (Decimal, Decimal)> = AHashMap::new();
    for fill in fill_reports {
        let entry = totals.entry(fill.venue_order_id).or_default();
        let qty = fill.last_qty.as_decimal();
        entry.0 += fill.last_px.as_decimal() * qty;
        entry.1 += qty;
    }

    for report in order_reports {
        if let Some((notional, total_qty)) = totals.get(&report.venue_order_id)
            && !total_qty.is_zero()
        {
            report.avg_px = Some(notional / total_qty);
        }
    }
}

/// Live execution client for the dYdX v4 exchange adapter.
///
/// Supports Market, Limit, Stop Market, Stop Limit, Take Profit Market (MarketIfTouched),
/// and Take Profit Limit (LimitIfTouched) orders via gRPC. Trailing stops are NOT supported
/// by the dYdX v4 protocol. dYdX requires u32 client IDs - strings are hashed to fit.
///
/// # Architecture
///
/// The client follows a two-layer execution model:
/// 1. **Synchronous validation** - Immediate checks and event generation.
/// 2. **Async submission** - Non-blocking gRPC calls via `TransactionManager`, `TxBroadcaster`, and `OrderMessageBuilder`.
///
/// This matches the pattern used in OKX and other exchange adapters, ensuring
/// consistent behavior across the Nautilus ecosystem.
#[derive(Debug)]
pub struct DydxExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: DydxAdapterConfig,
    emitter: ExecutionEventEmitter,
    http_client: DydxHttpClient,
    ws_client: DydxWebSocketClient,
    grpc_client: Arc<tokio::sync::RwLock<Option<DydxGrpcClient>>>,
    instrument_cache: Arc<InstrumentCache>,
    block_time_monitor: Arc<BlockTimeMonitor>,
    oracle_prices: Arc<DashMap<InstrumentId, Decimal>>,
    encoder: Arc<ClientOrderIdEncoder>,
    dispatch_state: Arc<DydxWsDispatchState>,
    order_contexts: Arc<DashMap<u32, OrderContext>>,
    order_id_map: Arc<DashMap<String, (u32, u32)>>,
    wallet_address: String,
    subaccount_number: u32,
    tx_manager: Option<Arc<TransactionManager>>,
    broadcaster: Option<Arc<TxBroadcaster>>,
    order_builder: Option<Arc<OrderMessageBuilder>>,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl DydxExecutionClient {
    /// Creates a new [`DydxExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are not found or client fails to construct.
    pub fn new(
        core: ExecutionClientCore,
        config: DydxAdapterConfig,
        wallet_address: String,
        subaccount_number: u32,
    ) -> anyhow::Result<Self> {
        let trader_id = core.trader_id;
        let account_id = core.account_id;
        let clock = get_atomic_clock_realtime();
        let emitter =
            ExecutionEventEmitter::new(clock, trader_id, account_id, AccountType::Margin, None);

        let retry_config = RetryConfig {
            max_retries: config.max_retries,
            initial_delay_ms: config.retry_delay_initial_ms,
            max_delay_ms: config.retry_delay_max_ms,
            ..Default::default()
        };
        let http_client = DydxHttpClient::new(
            Some(config.base_url.clone()),
            config.timeout_secs,
            config.proxy_url.clone(),
            config.network,
            Some(retry_config),
        )?;

        // Share the HTTP client's instrument cache with WebSocket client
        let instrument_cache = http_client.instrument_cache().clone();

        // Use private WebSocket client for authenticated subaccount subscriptions
        let credential = DydxCredential::resolve(
            config.private_key.as_deref(),
            config.network,
            config.authenticator_ids.clone(),
        )?
        .ok_or_else(|| anyhow::anyhow!("Credentials required for execution client"))?;

        // Create WS client with shared instrument cache
        let ws_client = DydxWebSocketClient::new_private_with_cache(
            config.ws_url.clone(),
            credential,
            core.account_id,
            instrument_cache.clone(),
            Some(20),
            config.transport_backend,
            config.proxy_url.clone(),
        );

        let grpc_client = Arc::new(tokio::sync::RwLock::new(None));

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_client,
            grpc_client,
            instrument_cache,
            block_time_monitor: Arc::new(BlockTimeMonitor::new()),
            oracle_prices: Arc::new(DashMap::new()),
            encoder: Arc::new(ClientOrderIdEncoder::new()),
            dispatch_state: Arc::new(DydxWsDispatchState::default()),
            order_contexts: Arc::new(DashMap::new()),
            order_id_map: Arc::new(DashMap::new()),
            wallet_address,
            subaccount_number,
            tx_manager: None,
            broadcaster: None,
            order_builder: None,
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    fn resolve_private_key(config: &DydxAdapterConfig) -> anyhow::Result<String> {
        let (private_key_env, _) = credential_env_vars(config.network);

        // 1. Try private key from config
        if let Some(ref pk) = config.private_key
            && !pk.trim().is_empty()
        {
            return Ok(pk.clone());
        }

        // 2. Try private key from env var
        if let Some(pk) = std::env::var(private_key_env)
            .ok()
            .filter(|s| !s.trim().is_empty())
        {
            return Ok(pk);
        }

        anyhow::bail!("{private_key_env} not found in config or environment")
    }

    fn register_order_context(&self, client_id_u32: u32, context: OrderContext) {
        self.order_contexts.insert(client_id_u32, context);
    }

    fn get_order_context(&self, client_id_u32: u32) -> Option<OrderContext> {
        self.order_contexts
            .get(&client_id_u32)
            .map(|r| r.value().clone())
    }

    fn get_chain_id(&self) -> ChainId {
        self.config.get_chain_id()
    }

    fn spawn_ws_stream_handler(
        &mut self,
        stream: impl Stream<Item = DydxWsOutputMessage> + Send + 'static,
    ) {
        if self.ws_stream_handle.is_some() {
            return;
        }

        log::debug!("Starting execution WebSocket message processing task");

        // Clone data needed for account state parsing in spawned task
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let instrument_cache = self.instrument_cache.clone();
        let oracle_prices = self.oracle_prices.clone();
        let encoder = self.encoder.clone();
        let order_contexts = self.order_contexts.clone();
        let order_id_map = self.order_id_map.clone();
        let dispatch_state = self.dispatch_state.clone();
        let block_time_monitor = self.block_time_monitor.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let handle = get_runtime().spawn(async move {
            log::debug!("Execution WebSocket message loop started");

            // Cumulative fill totals per untracked order for avg_px computation
            let mut cum_fill_totals: AHashMap<VenueOrderId, (Decimal, Decimal)> =
                AHashMap::new();

            pin_mut!(stream);
            while let Some(msg) = stream.next().await {
                match msg {
                    DydxWsOutputMessage::SubaccountSubscribed(msg) => {
                        log::debug!("Parsing subaccount subscription with full context");

                        let inst_map = instrument_cache.to_instrument_id_map();

                        let oracle_map: std::collections::HashMap<_, _> = oracle_prices
                            .iter()
                            .map(|entry| (*entry.key(), *entry.value()))
                            .collect();

                        let ts_init = clock.get_time_ns();
                        let ts_event = ts_init;

                        if let Some(ref subaccount) = msg.contents.subaccount {
                        match parse_account_state(
                            subaccount,
                            account_id,
                            &inst_map,
                            &oracle_map,
                            ts_event,
                            ts_init,
                        ) {
                            Ok(account_state) => {
                                log::debug!(
                                    "Parsed account state: {} balance(s), {} margin(s)",
                                    account_state.balances.len(),
                                    account_state.margins.len()
                                );
                                emitter.send_account_state(account_state);
                            }
                            Err(e) => {
                                log::error!("Failed to parse account state: {e}");
                            }
                        }

                        if let Some(ref positions) =
                            subaccount.open_perpetual_positions
                        {
                            log::debug!(
                                "Parsing {} position(s) from subscription",
                                positions.len()
                            );

                            for (market, ws_position) in positions {
                                match parse_ws_position_report(
                                    ws_position,
                                    &instrument_cache,
                                    account_id,
                                    ts_init,
                                ) {
                                    Ok(report) => {
                                        log::debug!(
                                            "Parsed position report: {} {} {} {}",
                                            report.instrument_id,
                                            report.position_side,
                                            report.quantity,
                                            market
                                        );
                                        emitter.send_position_report(report);
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Failed to parse WebSocket position for {market}: {e}"
                                        );
                                    }
                                }
                            }
                        }
                        } else {
                            log::warn!("Subaccount subscription without initial state (new/empty subaccount)");

                            let currency = Currency::get_or_create_crypto_with_context("USDC", None);
                            let zero = Money::zero(currency);
                            let balance = AccountBalance::new_checked(zero, zero, zero)
                                .expect("zero balance should always be valid");
                            let account_state = AccountState::new(
                                account_id,
                                AccountType::Margin,
                                vec![balance],
                                vec![],
                                true,
                                UUID4::new(),
                                ts_init,
                                ts_init,
                                None,
                            );
                            emitter.send_account_state(account_state);
                        }
                    }
                    DydxWsOutputMessage::SubaccountsChannelData(data) => {
                        log::debug!(
                            "Processing subaccounts channel data (orders={:?}, fills={:?})",
                            data.contents.orders.as_ref().map(|o| o.len()),
                            data.contents.fills.as_ref().map(|f| f.len())
                        );
                        let ts_init = clock.get_time_ns();

                        let mut terminal_orders: Vec<(u32, u32, String)> = Vec::new();
                        let mut pending_order_reports = Vec::new();

                        // Phase 1: Parse orders and build order_id_map
                        if let Some(ref orders) = data.contents.orders {
                            for ws_order in orders {
                                log::debug!(
                                    "Parsing WS order: clob_pair_id={}, status={:?}, client_id={}",
                                    ws_order.clob_pair_id,
                                    ws_order.status,
                                    ws_order.client_id
                                );

                                if let Ok(client_id_u32) = ws_order.client_id.parse::<u32>() {
                                    let client_meta = ws_order.client_metadata
                                        .as_ref()
                                        .and_then(|s| s.parse::<u32>().ok())
                                        .unwrap_or(crate::grpc::DEFAULT_RUST_CLIENT_METADATA);
                                    order_id_map.insert(ws_order.id.clone(), (client_id_u32, client_meta));
                                }

                                match parse_ws_order_report(
                                    ws_order,
                                    &instrument_cache,
                                    &order_contexts,
                                    &encoder,
                                    account_id,
                                    ts_init,
                                ) {
                                    Ok(report) => {
                                        if !report.order_status.is_open()
                                            && let Ok(cid) = ws_order.client_id.parse::<u32>()
                                        {
                                            let meta = ws_order.client_metadata
                                                .as_ref()
                                                .and_then(|s| s.parse::<u32>().ok())
                                                .unwrap_or(crate::grpc::DEFAULT_RUST_CLIENT_METADATA);
                                            terminal_orders.push((cid, meta, ws_order.id.clone()));
                                        }
                                        log::debug!(
                                            "Parsed order report: {} {} {:?} qty={} client_order_id={:?}",
                                            report.instrument_id,
                                            report.order_side,
                                            report.order_status,
                                            report.quantity,
                                            report.client_order_id
                                        );
                                        pending_order_reports.push(report);
                                    }
                                    Err(e) => {
                                        log::error!("Failed to parse WebSocket order: {e}");
                                    }
                                }
                            }
                        }

                        // Phase 2: Process fills (sent before order status for correct reconciliation)
                        if let Some(ref fills) = data.contents.fills {
                            for ws_fill in fills {
                                match parse_ws_fill_report(
                                    ws_fill,
                                    &instrument_cache,
                                    &order_id_map,
                                    &order_contexts,
                                    &encoder,
                                    account_id,
                                    ts_init,
                                ) {
                                    Ok(report) => {
                                        log::debug!(
                                            "Parsed fill report: {} {} {} @ {} client_order_id={:?}",
                                            report.instrument_id,
                                            report.venue_order_id,
                                            report.last_qty,
                                            report.last_px,
                                            report.client_order_id
                                        );

                                        let identity = report.client_order_id.and_then(|cid| {
                                            dispatch_state.order_identities.get(&cid).map(|r| (cid, r.clone()))
                                        });

                                        if let Some((cid, ident)) = identity {
                                            // Tracked: synthesize OrderAccepted if not yet emitted
                                            if !dispatch_state.emitted_accepted.contains(&cid) {
                                                dispatch_state.insert_accepted(cid);
                                                let accepted = OrderAccepted::new(
                                                    trader_id,
                                                    ident.strategy_id,
                                                    ident.instrument_id,
                                                    cid,
                                                    report.venue_order_id,
                                                    account_id,
                                                    UUID4::new(),
                                                    ts_init,
                                                    ts_init,
                                                    false,
                                                );
                                                emitter.send_order_event(OrderEventAny::Accepted(accepted));
                                            }

                                            dispatch_state.insert_filled(cid);
                                            let instrument = instrument_cache.get(&report.instrument_id);
                                            let quote_currency = instrument
                                                .map_or_else(Currency::USD, |i: InstrumentAny| i.quote_currency());
                                            let filled = fill_report_to_order_filled(
                                                &report, trader_id, &ident, quote_currency,
                                            );
                                            emitter.send_order_event(OrderEventAny::Filled(filled));
                                        } else {
                                            // Untracked: track avg_px and emit report
                                            let entry = cum_fill_totals
                                                .entry(report.venue_order_id)
                                                .or_default();
                                            let qty = report.last_qty.as_decimal();
                                            entry.0 += report.last_px.as_decimal() * qty;
                                            entry.1 += qty;
                                            emitter.send_fill_report(report);
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("Failed to parse WebSocket fill: {e}");
                                    }
                                }
                            }
                        }

                        // Phase 3: Process order status updates
                        // Enrich untracked reports with avg_px from cumulative fills
                        for report in &mut pending_order_reports {
                            if let Some((notional, total_qty)) =
                                cum_fill_totals.get(&report.venue_order_id)
                                && !total_qty.is_zero()
                            {
                                report.avg_px = Some(notional / total_qty);
                            }
                        }

                        for report in pending_order_reports {
                            let identity = report.client_order_id.and_then(|cid| {
                                dispatch_state.order_identities.get(&cid).map(|r| (cid, r.clone()))
                            });

                            if let Some((cid, ident)) = identity {
                                // Tracked order: emit proper lifecycle events
                                match report.order_status {
                                    OrderStatus::Accepted => {
                                        if dispatch_state.emitted_accepted.contains(&cid)
                                            || dispatch_state.filled_orders.contains(&cid)
                                        {
                                            log::debug!("Skipping duplicate Accepted for {cid}");
                                            continue;
                                        }
                                        dispatch_state.insert_accepted(cid);
                                        let accepted = OrderAccepted::new(
                                            trader_id,
                                            ident.strategy_id,
                                            ident.instrument_id,
                                            cid,
                                            report.venue_order_id,
                                            account_id,
                                            UUID4::new(),
                                            report.ts_last,
                                            ts_init,
                                            false,
                                        );
                                        emitter.send_order_event(OrderEventAny::Accepted(accepted));
                                    }
                                    OrderStatus::Canceled => {
                                        // Synthesize Accepted if not yet emitted
                                        if !dispatch_state.emitted_accepted.contains(&cid) {
                                            dispatch_state.insert_accepted(cid);
                                            let accepted = OrderAccepted::new(
                                                trader_id,
                                                ident.strategy_id,
                                                ident.instrument_id,
                                                cid,
                                                report.venue_order_id,
                                                account_id,
                                                UUID4::new(),
                                                ts_init,
                                                ts_init,
                                                false,
                                            );
                                            emitter.send_order_event(OrderEventAny::Accepted(accepted));
                                        }
                                        let canceled = OrderCanceled::new(
                                            trader_id,
                                            ident.strategy_id,
                                            ident.instrument_id,
                                            cid,
                                            UUID4::new(),
                                            report.ts_last,
                                            ts_init,
                                            false,
                                            Some(report.venue_order_id),
                                            Some(account_id),
                                        );
                                        emitter.send_order_event(OrderEventAny::Canceled(canceled));
                                        dispatch_state.cleanup_terminal(&cid);
                                    }
                                    OrderStatus::Filled => {
                                        // Fills already emitted as OrderFilled in Phase 2
                                        dispatch_state.cleanup_terminal(&cid);
                                    }
                                    _ => {
                                        // PendingUpdate, PartiallyFilled, etc.
                                        emitter.send_order_status_report(report);
                                    }
                                }
                            } else {
                                // Untracked order: emit report for reconciliation
                                emitter.send_order_status_report(report);
                            }
                        }

                        // Phase 4: Cleanup terminal order tracking state
                        for (client_id, client_metadata, order_id) in terminal_orders {
                            order_contexts.remove(&client_id);
                            encoder.remove(client_id, client_metadata);
                            order_id_map.remove(&order_id);
                            cum_fill_totals.remove(&VenueOrderId::new(&order_id));
                        }
                    }
                    DydxWsOutputMessage::Markets(contents) => {
                        if let Some(ref oracle_map) = contents.oracle_prices {
                            for (symbol_str, oracle_market) in oracle_map {
                                let instrument_id = {
                                    let symbol = format!("{symbol_str}-PERP");
                                    InstrumentId::new(
                                        Symbol::new(&symbol),
                                        *crate::common::consts::DYDX_VENUE,
                                    )
                                };

                                if instrument_cache.get(&instrument_id).is_some()
                                    && let Ok(price_dec) = oracle_market.oracle_price.parse::<Decimal>()
                                {
                                    oracle_prices.insert(instrument_id, price_dec);
                                    log::trace!("Updated oracle price for {instrument_id}: {price_dec}");
                                }
                            }
                        }

                        if let Some(ref markets) = contents.markets {
                            for (symbol_str, market_data) in markets {
                                if let Some(oracle_price_str) = &market_data.oracle_price {
                                    let instrument_id = {
                                        let symbol = format!("{symbol_str}-PERP");
                                        InstrumentId::new(
                                            Symbol::new(&symbol),
                                            *crate::common::consts::DYDX_VENUE,
                                        )
                                    };

                                    if instrument_cache.get(&instrument_id).is_some()
                                        && let Ok(price_dec) = oracle_price_str.parse::<Decimal>()
                                    {
                                        oracle_prices.insert(instrument_id, price_dec);
                                    }
                                }
                            }
                        }
                    }
                    DydxWsOutputMessage::BlockHeight { height, time } => {
                        log::debug!("Block height update: {height} at {time}");
                        block_time_monitor.record_block(height, time);
                    }
                    DydxWsOutputMessage::Error(err) => {
                        log::error!("WebSocket error: {err:?}");
                    }
                    DydxWsOutputMessage::Reconnected => {
                        log::info!("WebSocket reconnected");
                    }
                    _ => {}
                }
            }
            log::debug!("WebSocket message processing task ended");
        });

        self.ws_stream_handle = Some(handle);
        log::info!("WebSocket stream handler started");
    }

    /// Marks instruments as initialized after HTTP client has fetched them.
    ///
    /// The instruments are stored in the shared `InstrumentCache` which is automatically
    /// populated by the HTTP client during `fetch_and_cache_instruments()`.
    fn mark_instruments_initialized(&self) {
        let count = self.instrument_cache.len();
        self.core.set_instruments_initialized();
        log::debug!("Instruments initialized: {count} instruments in shared cache");
    }

    fn get_instrument_by_market(&self, market: &str) -> Option<InstrumentAny> {
        self.instrument_cache.get_by_market(market)
    }

    fn get_instrument_by_clob_pair_id(&self, clob_pair_id: u32) -> Option<InstrumentAny> {
        let instrument = self.instrument_cache.get_by_clob_id(clob_pair_id);

        if instrument.is_none() {
            self.instrument_cache.log_missing_clob_pair_id(clob_pair_id);
        }

        instrument
    }

    /// Gets the execution components, returning an error if not initialized.
    ///
    /// This should only be called after `connect()` has completed.
    fn get_execution_components(
        &self,
    ) -> anyhow::Result<(
        Arc<TransactionManager>,
        Arc<TxBroadcaster>,
        Arc<OrderMessageBuilder>,
    )> {
        let tx_manager = self
            .tx_manager
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!("TransactionManager not initialized - call connect() first")
            })?
            .clone();
        let broadcaster = self
            .broadcaster
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TxBroadcaster not initialized - call connect() first"))?
            .clone();
        let order_builder = self
            .order_builder
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!("OrderMessageBuilder not initialized - call connect() first")
            })?
            .clone();
        Ok((tx_manager, broadcaster, order_builder))
    }

    fn spawn_task<F>(&self, label: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handle = get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{label}: {e:?}");
            }
        });

        self.pending_tasks
            .lock()
            .expect(MUTEX_POISONED)
            .push(handle);
    }

    /// Spawns an order submission task with error handling and rejection generation.
    ///
    /// If the submission fails, generates an `OrderRejected` event with the error details.
    fn spawn_order_task<F>(
        &self,
        label: &'static str,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        fut: F,
    ) where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let handle = get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                let error_msg = format!("{label} failed: {e:?}");
                log::error!("{error_msg}");

                let ts_event = clock.get_time_ns();
                emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &error_msg,
                    ts_event,
                    false,
                );
            }
        });

        self.pending_tasks
            .lock()
            .expect(MUTEX_POISONED)
            .push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut guard = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in guard.drain(..) {
            handle.abort();
        }
    }

    /// Sends an OrderModifyRejected event.
    fn send_modify_rejected(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
    ) {
        let ts_event = self.clock.get_time_ns();
        self.emitter.emit_order_modify_rejected_event(
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            reason,
            ts_event,
        );
    }

    /// Waits for the account to be registered in the cache.
    ///
    /// This method polls the cache until the account is registered, ensuring that
    /// execution state reconciliation can process fills correctly (fills require
    /// the account to be registered for portfolio updates).
    ///
    /// # Errors
    ///
    /// Returns an error if the account is not registered within the timeout period.
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

/// Broadcasts cancel orders with optimal partitioned strategy.
///
/// Partitions orders into short-term and long-term/conditional groups:
/// - Short-term → single `MsgBatchCancel` via `broadcast_short_term()`
/// - Long-term/conditional → batched `MsgCancelOrder` via `broadcast_with_retry()`
///
/// At most 2 gRPC calls regardless of order count or mix.
async fn broadcast_partitioned_cancels(
    orders: Vec<(InstrumentId, u32, u32)>,
    block_height: u32,
    tx_manager: Arc<TransactionManager>,
    broadcaster: Arc<TxBroadcaster>,
    order_builder: Arc<OrderMessageBuilder>,
) -> anyhow::Result<()> {
    if orders.is_empty() {
        return Ok(());
    }

    let (short_term_orders, long_term_orders): (Vec<_>, Vec<_>) = orders
        .into_iter()
        .partition(|(_, _, flags)| *flags == types::ORDER_FLAG_SHORT_TERM);

    // Cancel short-term orders with MsgBatchCancel (single gRPC call)
    if !short_term_orders.is_empty() {
        let st_pairs: Vec<_> = short_term_orders
            .iter()
            .map(|(inst_id, client_id, _)| (*inst_id, *client_id))
            .collect();

        log::debug!(
            "Batch cancelling {} short-term orders with MsgBatchCancel",
            st_pairs.len()
        );

        match order_builder.build_batch_cancel_short_term(&st_pairs, block_height) {
            Ok(msg) => {
                let operation = format!("BatchCancel {} short-term orders", st_pairs.len());
                match broadcaster
                    .broadcast_short_term(&tx_manager, vec![msg], &operation)
                    .await
                {
                    Ok(tx_hash) => {
                        log::debug!(
                            "Successfully batch cancelled {} short-term orders, tx_hash: {}",
                            st_pairs.len(),
                            tx_hash
                        );
                    }
                    Err(e) => {
                        log::error!("Short-term batch cancel failed: {e:?}");
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to build MsgBatchCancel: {e:?}");
            }
        }
    }

    // Cancel long-term/conditional orders with batched MsgCancelOrder (single gRPC call)
    if !long_term_orders.is_empty() {
        log::debug!(
            "Batch cancelling {} long-term orders",
            long_term_orders.len(),
        );

        match order_builder.build_cancel_orders_batch_with_flags(&long_term_orders, block_height) {
            Ok(cancel_msgs) => {
                let operation = format!("BatchCancel {} long-term orders", long_term_orders.len());
                match broadcaster
                    .broadcast_with_retry(&tx_manager, cancel_msgs, &operation)
                    .await
                {
                    Ok(tx_hash) => {
                        log::debug!(
                            "Successfully batch cancelled {} long-term orders, tx_hash: {}",
                            long_term_orders.len(),
                            tx_hash
                        );
                    }
                    Err(e) => {
                        log::error!("Long-term batch cancel failed: {e:?}");
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to build long-term cancel messages: {e:?}");
            }
        }
    }

    Ok(())
}

#[async_trait(?Send)]
impl ExecutionClient for DydxExecutionClient {
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
        *DYDX_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account(&self.core.account_id).cloned()
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

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            log::warn!("dYdX execution client already started");
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        log::info!("Starting dYdX execution client");
        self.core.set_started();
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            log::warn!("dYdX execution client not started");
            return Ok(());
        }

        log::info!("Stopping dYdX execution client");
        self.abort_pending_tasks();
        self.core.set_stopped();
        self.core.set_disconnected();
        Ok(())
    }

    /// Submits an order to dYdX via gRPC.
    ///
    /// dYdX requires u32 client IDs - Nautilus ClientOrderId strings are hashed to fit.
    ///
    /// Supported order types:
    /// - Market orders (short-term, IOC).
    /// - Limit orders (short-term or long-term based on TIF).
    /// - Stop Market orders (conditional, triggered at stop price).
    /// - Stop Limit orders (conditional, triggered at stop price, executed at limit).
    /// - Take Profit Market (MarketIfTouched - triggered at take profit price).
    /// - Take Profit Limit (LimitIfTouched - triggered at take profit price, executed at limit).
    ///
    /// Trailing stop orders are NOT supported by dYdX v4 protocol.
    ///
    /// Validates synchronously, generates OrderSubmitted event, then spawns async task for
    /// gRPC submission to avoid blocking. Unsupported order types generate OrderRejected.
    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        // Check connection status first (doesn't need order)
        if !self.is_connected() {
            let reason = "Cannot submit order: execution client not connected";
            log::error!("{reason}");
            anyhow::bail!(reason);
        }

        // Check block height is available for short-term orders
        let current_block = self.block_time_monitor.current_block_height();
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;

        let client_order_id = order.client_order_id();
        let instrument_id = order.instrument_id();
        let strategy_id = order.strategy_id();

        if current_block == 0 {
            let reason = "Block height not initialized";
            log::warn!("Cannot submit order {client_order_id}: {reason}");
            let ts_event = self.clock.get_time_ns();
            self.emitter.emit_order_rejected_event(
                strategy_id,
                instrument_id,
                client_order_id,
                reason,
                ts_event,
                false,
            );
            return Ok(());
        }

        // Check if order is already closed
        if order.is_closed() {
            log::warn!("Cannot submit closed order {client_order_id}");
            return Ok(());
        }

        // Reject unsupported order types
        match order.order_type() {
            OrderType::Market
            | OrderType::Limit
            | OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched => {}
            // Trailing stops not supported by dYdX v4 protocol
            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit => {
                let reason = "Trailing stop orders not supported by dYdX v4 protocol";
                log::error!("{reason}");
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    reason,
                    ts_event,
                    false,
                );
                return Ok(());
            }
            order_type => {
                let reason = format!("Order type {order_type:?} not supported by dYdX");
                log::error!("{reason}");
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &reason,
                    ts_event,
                    false,
                );
                return Ok(());
            }
        }

        self.emitter.emit_order_submitted(&order);

        // Get execution components (must be initialized after connect())
        let (tx_manager, broadcaster, order_builder) = match self.get_execution_components() {
            Ok(components) => components,
            Err(e) => {
                log::error!("Failed to get execution components: {e}");
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &e.to_string(),
                    ts_event,
                    false,
                );
                return Ok(());
            }
        };

        let block_height = self.block_time_monitor.current_block_height() as u32;

        // Generate client_order_id as (u32, u32) pair before async block (dYdX requires u32 client IDs)
        let encoded = match self.encoder.encode(client_order_id) {
            Ok(enc) => enc,
            Err(e) => {
                log::error!("Failed to generate client order ID: {e}");
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &e.to_string(),
                    ts_event,
                    false,
                );
                return Ok(());
            }
        };
        let client_id_u32 = encoded.client_id;
        let client_metadata = encoded.client_metadata;

        log::info!(
            "[SUBMIT_ORDER] Nautilus '{}' -> dYdX u32={} meta={:#x} | instrument={} side={:?} qty={} type={:?}",
            client_order_id,
            client_id_u32,
            client_metadata,
            instrument_id,
            order.order_side(),
            order.quantity(),
            order.order_type()
        );

        // Convert expire_time from nanoseconds to seconds if present
        let expire_time = order.expire_time().map(nanos_to_secs_i64);

        // Determine order_flags based on order type for later cancellation
        let order_flags = match order.order_type() {
            // Conditional orders always use ORDER_FLAG_CONDITIONAL
            OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched => types::ORDER_FLAG_CONDITIONAL,
            // Market orders are always short-term
            OrderType::Market => types::ORDER_FLAG_SHORT_TERM,
            // Limit orders depend on time_in_force and expire_time
            OrderType::Limit => {
                let lifetime = types::OrderLifetime::from_time_in_force(
                    order.time_in_force(),
                    expire_time,
                    false,
                    order_builder.max_short_term_secs(),
                );
                lifetime.order_flags()
            }
            // Default to long-term for unknown types
            _ => types::ORDER_FLAG_LONG_TERM,
        };

        // Register order context for WebSocket correlation and cancellation
        let ts_submitted = self.clock.get_time_ns();
        let trader_id = order.trader_id();
        self.register_order_context(
            client_id_u32,
            OrderContext {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                submitted_at: ts_submitted,
                order_flags,
            },
        );

        // Register dispatch identity so the WS handler emits proper order
        // events (OrderAccepted, OrderFilled, OrderCanceled) instead of reports
        self.dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id,
                order_side: order.order_side(),
                order_type: order.order_type(),
            },
        );

        self.spawn_order_task(
            "submit_order",
            strategy_id,
            instrument_id,
            client_order_id,
            async move {
                // Build the order message based on order type
                let (msg, order_type_str) = match order.order_type() {
                    OrderType::Market => {
                        let msg = order_builder.build_market_order(
                            instrument_id,
                            client_id_u32,
                            client_metadata,
                            order.order_side(),
                            order.quantity(),
                            block_height,
                        )?;
                        (msg, "market")
                    }
                    OrderType::Limit => {
                        // Use pre-computed expire_time (with default_short_term_expiry applied)
                        let msg = order_builder.build_limit_order(
                            instrument_id,
                            client_id_u32,
                            client_metadata,
                            order.order_side(),
                            order
                                .price()
                                .ok_or_else(|| anyhow::anyhow!("Limit order missing price"))?,
                            order.quantity(),
                            order.time_in_force(),
                            order.is_post_only(),
                            order.is_reduce_only(),
                            block_height,
                            expire_time, // Uses default_short_term_expiry if configured
                        )?;
                        (msg, "limit")
                    }
                    // Conditional orders use their own expiration logic (not affected by default_short_term_expiry)
                    // They are always stored on-chain with long-term semantics
                    OrderType::StopMarket => {
                        let trigger_price = order.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Stop market order missing trigger_price")
                        })?;
                        let cond_expire = order.expire_time().map(nanos_to_secs_i64);
                        let msg = order_builder.build_stop_market_order(
                            instrument_id,
                            client_id_u32,
                            client_metadata,
                            order.order_side(),
                            trigger_price,
                            order.quantity(),
                            order.is_reduce_only(),
                            cond_expire,
                        )?;
                        (msg, "stop_market")
                    }
                    OrderType::StopLimit => {
                        let trigger_price = order.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Stop limit order missing trigger_price")
                        })?;
                        let limit_price = order.price().ok_or_else(|| {
                            anyhow::anyhow!("Stop limit order missing limit price")
                        })?;
                        let cond_expire = order.expire_time().map(nanos_to_secs_i64);
                        let msg = order_builder.build_stop_limit_order(
                            instrument_id,
                            client_id_u32,
                            client_metadata,
                            order.order_side(),
                            trigger_price,
                            limit_price,
                            order.quantity(),
                            order.time_in_force(),
                            order.is_post_only(),
                            order.is_reduce_only(),
                            cond_expire,
                        )?;
                        (msg, "stop_limit")
                    }
                    // dYdX TakeProfitMarket maps to Nautilus MarketIfTouched
                    OrderType::MarketIfTouched => {
                        let trigger_price = order.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Take profit market order missing trigger_price")
                        })?;
                        let cond_expire = order.expire_time().map(nanos_to_secs_i64);
                        let msg = order_builder.build_take_profit_market_order(
                            instrument_id,
                            client_id_u32,
                            client_metadata,
                            order.order_side(),
                            trigger_price,
                            order.quantity(),
                            order.is_reduce_only(),
                            cond_expire,
                        )?;
                        (msg, "take_profit_market")
                    }
                    // dYdX TakeProfitLimit maps to Nautilus LimitIfTouched
                    OrderType::LimitIfTouched => {
                        let trigger_price = order.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Take profit limit order missing trigger_price")
                        })?;
                        let limit_price = order.price().ok_or_else(|| {
                            anyhow::anyhow!("Take profit limit order missing limit price")
                        })?;
                        let cond_expire = order.expire_time().map(nanos_to_secs_i64);
                        let msg = order_builder.build_take_profit_limit_order(
                            instrument_id,
                            client_id_u32,
                            client_metadata,
                            order.order_side(),
                            trigger_price,
                            limit_price,
                            order.quantity(),
                            order.time_in_force(),
                            order.is_post_only(),
                            order.is_reduce_only(),
                            cond_expire,
                        )?;
                        (msg, "take_profit_limit")
                    }
                    _ => unreachable!("Order type already validated"),
                };

                // Broadcast: short-term orders use cached sequence (no increment),
                // stateful orders use broadcast_with_retry (proper sequence management)
                let operation = format!("Submit {order_type_str} order {client_order_id}");

                if order_flags == types::ORDER_FLAG_SHORT_TERM {
                    broadcaster
                        .broadcast_short_term(&tx_manager, vec![msg], &operation)
                        .await?;
                } else {
                    broadcaster
                        .broadcast_with_retry(&tx_manager, vec![msg], &operation)
                        .await?;
                }
                log::debug!("Successfully submitted {order_type_str} order: {client_order_id}");

                Ok(())
            },
        );

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let orders = self.core.get_orders_for_list(&cmd.order_list)?;
        let order_count = orders.len();

        // Check connection status
        if !self.is_connected() {
            let reason = "Cannot submit order list: execution client not connected";
            log::error!("{reason}");
            anyhow::bail!(reason);
        }

        // Check block height is available
        let current_block = self.block_time_monitor.current_block_height();
        if current_block == 0 {
            let reason = "Block height not initialized";
            log::warn!("Cannot submit order list: {reason}");
            // Reject all orders in the list
            let ts_event = self.clock.get_time_ns();

            for order in &orders {
                self.emitter.emit_order_rejected_event(
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    reason,
                    ts_event,
                    false,
                );
            }
            return Ok(());
        }

        // Get execution components early so we can register order contexts
        let (tx_manager, broadcaster, order_builder) = match self.get_execution_components() {
            Ok(components) => components,
            Err(e) => {
                log::error!("Failed to get execution components for batch: {e}");
                // Reject all orders in the list
                let ts_event = self.clock.get_time_ns();

                for order in &orders {
                    self.emitter.emit_order_rejected_event(
                        order.strategy_id(),
                        order.instrument_id(),
                        order.client_order_id(),
                        &e.to_string(),
                        ts_event,
                        false,
                    );
                }
                return Ok(());
            }
        };

        // Collect limit order parameters for batch submission
        let mut order_params: Vec<LimitOrderParams> = Vec::with_capacity(order_count);
        let mut order_info: Vec<(ClientOrderId, InstrumentId, StrategyId)> =
            Vec::with_capacity(order_count);

        for order in &orders {
            // Only limit orders can be batched
            if order.order_type() != OrderType::Limit {
                log::warn!(
                    "Order {} has type {:?}, falling back to individual submission",
                    order.client_order_id(),
                    order.order_type()
                );
                // Fall back to individual submission for non-limit orders
                let submit_cmd = SubmitOrder::new(
                    cmd.trader_id,
                    cmd.client_id,
                    cmd.strategy_id,
                    order.instrument_id(),
                    order.client_order_id(),
                    order.init_event().clone(),
                    cmd.exec_algorithm_id,
                    cmd.position_id,
                    cmd.params.clone(),
                    UUID4::new(),
                    cmd.ts_init,
                );

                if let Err(e) = self.submit_order(submit_cmd) {
                    log::error!(
                        "Failed to submit order {} from order list: {e}",
                        order.client_order_id()
                    );
                }
                continue;
            }

            // Get price (required for limit orders)
            let Some(price) = order.price() else {
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_rejected_event(
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    "Limit order missing price",
                    ts_event,
                    false,
                );
                continue;
            };

            // Generate client order ID as (u32, u32) pair
            let encoded = match self.encoder.encode(order.client_order_id()) {
                Ok(enc) => enc,
                Err(e) => {
                    log::error!("Failed to generate client order ID: {e}");
                    let ts_event = self.clock.get_time_ns();
                    self.emitter.emit_order_rejected_event(
                        order.strategy_id(),
                        order.instrument_id(),
                        order.client_order_id(),
                        &e.to_string(),
                        ts_event,
                        false,
                    );
                    continue;
                }
            };
            let client_id_u32 = encoded.client_id;
            let client_metadata = encoded.client_metadata;

            // Send OrderSubmitted event
            self.emitter.emit_order_submitted(order);

            // Determine order_flags for limit orders
            let expire_time_secs = order.expire_time().map(nanos_to_secs_i64);
            let lifetime = types::OrderLifetime::from_time_in_force(
                order.time_in_force(),
                expire_time_secs,
                false,
                order_builder.max_short_term_secs(),
            );

            // Register order context for WebSocket correlation and cancellation
            let ts_submitted = self.clock.get_time_ns();
            self.register_order_context(
                client_id_u32,
                OrderContext {
                    client_order_id: order.client_order_id(),
                    trader_id: order.trader_id(),
                    strategy_id: order.strategy_id(),
                    instrument_id: order.instrument_id(),
                    submitted_at: ts_submitted,
                    order_flags: lifetime.order_flags(),
                },
            );

            // Register dispatch identity for tracked order event emission
            self.dispatch_state.order_identities.insert(
                order.client_order_id(),
                OrderIdentity {
                    instrument_id: order.instrument_id(),
                    strategy_id: order.strategy_id(),
                    order_side: order.order_side(),
                    order_type: order.order_type(),
                },
            );

            // Collect order parameters (builder will apply default_short_term_expiry if needed)
            order_params.push(LimitOrderParams {
                instrument_id: order.instrument_id(),
                client_order_id: client_id_u32,
                client_metadata,
                side: order.order_side(),
                price,
                quantity: order.quantity(),
                time_in_force: order.time_in_force(),
                post_only: order.is_post_only(),
                reduce_only: order.is_reduce_only(),
                expire_time_ns: order.expire_time(),
            });
            order_info.push((
                order.client_order_id(),
                order.instrument_id(),
                order.strategy_id(),
            ));
        }

        // If no limit orders to batch, we're done
        if order_params.is_empty() {
            return Ok(());
        }

        // Check if any orders are short-term
        // dYdX protocol restriction: short-term orders CANNOT be batched
        // Each short-term order must be in its own transaction
        let has_short_term = order_params
            .iter()
            .any(|params| order_builder.is_short_term_order(params));

        let block_height = current_block as u32;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        if has_short_term {
            // Submit each order individually (short-term orders cannot be batched).
            log::debug!(
                "Submitting {} short-term limit orders concurrently (sequence not consumed)",
                order_params.len()
            );

            let order_count = order_params.len();

            let handle = get_runtime().spawn(async move {
                // Build and broadcast all orders concurrently -- no sequence coordination needed.
                // Short-term orders use cached sequence (not incremented) via broadcast_short_term.
                let mut handles = Vec::with_capacity(order_count);

                for (params, (client_order_id, instrument_id, strategy_id)) in
                    order_params.into_iter().zip(order_info)
                {
                    let tx_manager = tx_manager.clone();
                    let broadcaster = broadcaster.clone();
                    let order_builder = order_builder.clone();
                    let emitter = emitter.clone();

                    let handle = get_runtime().spawn(async move {
                        // Build order message
                        let msg = match order_builder
                            .build_limit_order_from_params(&params, block_height)
                        {
                            Ok(m) => m,
                            Err(e) => {
                                let error_msg = format!("Failed to build order message: {e:?}");
                                log::error!("{error_msg}");
                                let ts_event = clock.get_time_ns();
                                emitter.emit_order_rejected_event(
                                    strategy_id,
                                    instrument_id,
                                    client_order_id,
                                    &error_msg,
                                    ts_event,
                                    false,
                                );
                                return;
                            }
                        };

                        // Broadcast with cached sequence (short-term orders don't consume sequences)
                        let operation = format!("Submit short-term order {client_order_id}");

                        if let Err(e) = broadcaster
                            .broadcast_short_term(&tx_manager, vec![msg], &operation)
                            .await
                        {
                            let error_msg = format!("Order submission failed: {e:?}");
                            log::error!("{error_msg}");
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_rejected_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                &error_msg,
                                ts_event,
                                false,
                            );
                        }
                    });

                    handles.push(handle);
                }

                // Wait for all orders to be submitted
                for handle in handles {
                    let _ = handle.await;
                }
            });

            // Track the task
            self.pending_tasks
                .lock()
                .expect(MUTEX_POISONED)
                .push(handle);
        } else {
            // All orders are long-term - can batch in single transaction
            log::info!(
                "Batch submitting {} long-term limit orders in single transaction",
                order_params.len()
            );

            let handle = get_runtime().spawn(async move {
                // Build all order messages
                let msgs: Result<Vec<_>, _> = order_params
                    .iter()
                    .map(|params| order_builder.build_limit_order_from_params(params, block_height))
                    .collect();

                let msgs = match msgs {
                    Ok(m) => m,
                    Err(e) => {
                        let error_msg = format!("Failed to build batch order messages: {e:?}");
                        log::error!("{error_msg}");
                        // Send OrderRejected for all orders
                        let ts_event = clock.get_time_ns();

                        for (client_order_id, instrument_id, strategy_id) in order_info {
                            emitter.emit_order_rejected_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                &error_msg,
                                ts_event,
                                false,
                            );
                        }
                        return;
                    }
                };

                // Broadcast batch with retry
                let operation = format!("Submit batch of {} limit orders", msgs.len());

                if let Err(e) = broadcaster
                    .broadcast_with_retry(&tx_manager, msgs, &operation)
                    .await
                {
                    let error_msg = format!("Batch order submission failed: {e:?}");
                    log::error!("{error_msg}");

                    // Send OrderRejected for all orders in the batch
                    let ts_event = clock.get_time_ns();

                    for (client_order_id, instrument_id, strategy_id) in order_info {
                        emitter.emit_order_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            &error_msg,
                            ts_event,
                            false,
                        );
                    }
                }
            });

            // Track the task
            self.pending_tasks
                .lock()
                .expect(MUTEX_POISONED)
                .push(handle);
        }

        Ok(())
    }

    /// dYdX does not support native order modification.
    ///
    /// Strategies should handle `OrderModifyRejected` by canceling and resubmitting.
    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let reason = "dYdX does not support order modification. Use cancel and resubmit instead.";
        log::error!("{reason}");

        self.send_modify_rejected(
            cmd.strategy_id,
            cmd.instrument_id,
            cmd.client_order_id,
            cmd.venue_order_id,
            reason,
        );
        Ok(())
    }

    /// Cancels an order on dYdX exchange.
    ///
    /// Validates the order state and retrieves instrument details before
    /// spawning an async task to cancel via gRPC.
    ///
    /// # Validation
    ///
    /// - Checks order exists in cache.
    /// - Validates order is not already closed.
    /// - Retrieves instrument from cache for order builder.
    ///
    /// The `cmd` contains client/venue order IDs. Returns `Ok(())` if cancel request is
    /// spawned successfully or validation fails gracefully. Returns `Err` if not connected.
    ///
    /// # Events
    ///
    /// - `OrderCanceled` - Generated when WebSocket confirms cancellation.
    /// - `OrderCancelRejected` - Generated if exchange rejects cancellation.
    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        if !self.is_connected() {
            anyhow::bail!("Cannot cancel order: not connected");
        }

        let client_order_id = cmd.client_order_id;
        let instrument_id = cmd.instrument_id;
        let strategy_id = cmd.strategy_id;
        let venue_order_id = cmd.venue_order_id;

        let (order_time_in_force, order_expire_time) = {
            let cache = self.core.cache();

            let order = match cache.order(&client_order_id) {
                Some(order) => order,
                None => {
                    log::error!("Cannot cancel order {client_order_id}: not found in cache");
                    return Ok(()); // Not an error - order may have been filled/canceled already
                }
            };

            // Validate order is not already closed
            if order.is_closed() {
                log::warn!(
                    "CancelOrder command for {} when order already {} (will not send to exchange)",
                    client_order_id,
                    order.status()
                );
                return Ok(());
            }

            // Verify instrument exists (no need to hold reference)
            if cache.instrument(&instrument_id).is_none() {
                log::error!(
                    "Cannot cancel order {client_order_id}: instrument {instrument_id} not found in cache"
                );
                return Ok(()); // Not an error - missing instrument is a cache issue
            }

            // Extract data needed for order_flags fallback
            (
                order.time_in_force(),
                order.expire_time().map(nanos_to_secs_i64),
            )
        }; // Cache borrow released here

        log::debug!("Cancelling order {client_order_id} for instrument {instrument_id}");

        // Get execution components (no cache borrow held)
        let (tx_manager, broadcaster, order_builder) = match self.get_execution_components() {
            Ok(components) => components,
            Err(e) => {
                log::error!("Failed to get execution components for cancel: {e}");
                return Ok(());
            }
        };

        let block_height = self.block_time_monitor.current_block_height() as u32;

        // Convert client_order_id to (u32, u32) pair before async block
        let encoded = match self.encoder.get(&client_order_id) {
            Some(enc) => enc,
            None => {
                log::error!("Client order ID {client_order_id} not found in cache");
                anyhow::bail!("Client order ID not found in cache")
            }
        };
        let client_id_u32 = encoded.client_id;

        log::info!(
            "[CANCEL_ORDER] Nautilus '{client_order_id}' -> dYdX u32={client_id_u32} | instrument={instrument_id}"
        );

        // Get stored order_flags from order context (set at submission time)
        // This ensures we use the correct flags even if the order has expired
        let order_flags = self.get_order_context(client_id_u32).map_or_else(
            || {
                // Fallback: derive from order parameters if context not found
                log::warn!(
                    "Order context not found for {client_order_id}, deriving flags from order"
                );
                types::OrderLifetime::from_time_in_force(
                    order_time_in_force, // Using extracted value
                    order_expire_time,   // Using extracted value
                    false,
                    order_builder.max_short_term_secs(),
                )
                .order_flags()
            },
            |ctx| ctx.order_flags,
        );

        let clock = self.clock;
        let emitter = self.emitter.clone();

        self.spawn_task("cancel_order", async move {
            // Build cancel message using stored order_flags
            let cancel_msg = match order_builder.build_cancel_order_with_flags(
                instrument_id,
                client_id_u32,
                order_flags,
                block_height,
            ) {
                Ok(msg) => msg,
                Err(e) => {
                    log::error!("Failed to build cancel message for {client_order_id}: {e:?}");
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &format!("Cancel build failed: {e:?}"),
                        ts_event,
                    );
                    return Ok(());
                }
            };

            // Broadcast cancel: short-term uses cached sequence, stateful uses retry
            let cancel_op = format!("Cancel order {client_order_id}");
            let result = if order_flags == types::ORDER_FLAG_SHORT_TERM {
                broadcaster
                    .broadcast_short_term(&tx_manager, vec![cancel_msg], &cancel_op)
                    .await
            } else {
                broadcaster
                    .broadcast_with_retry(&tx_manager, vec![cancel_msg], &cancel_op)
                    .await
            };

            match result {
                Ok(_) => {
                    log::debug!("Successfully cancelled order: {client_order_id}");
                }
                Err(e) => {
                    log::error!("Failed to cancel order {client_order_id}: {e:?}");

                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &format!("Cancel order failed: {e:?}"),
                        ts_event,
                    );
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        if !self.is_connected() {
            anyhow::bail!("Cannot cancel orders: not connected");
        }

        let instrument_id = cmd.instrument_id;
        let order_side_filter = cmd.order_side;

        // Extract order data from cache with short-lived borrow
        // Collect (client_order_id, time_in_force, expire_time) for each matching order
        let order_data: Vec<(ClientOrderId, TimeInForce, Option<UnixNanos>)> = {
            let cache = self.core.cache();
            cache
                .orders_open(None, None, None, None, None)
                .into_iter()
                .filter(|order| order.instrument_id() == instrument_id)
                .filter(|order| {
                    order_side_filter == OrderSide::NoOrderSide
                        || order.order_side() == order_side_filter
                })
                .map(|order| {
                    (
                        order.client_order_id(),
                        order.time_in_force(),
                        order.expire_time(),
                    )
                })
                .collect()
        }; // Cache borrow released here

        // Count short-term vs long-term for logging
        let short_term_count = order_data
            .iter()
            .filter(|(_, tif, _)| matches!(tif, TimeInForce::Ioc | TimeInForce::Fok))
            .count();
        let long_term_count = order_data.len() - short_term_count;

        log::debug!(
            "Cancel all orders: total={}, short_term={}, long_term={}, instrument_id={instrument_id}, order_side={order_side_filter:?}",
            order_data.len(),
            short_term_count,
            long_term_count
        );

        // Get execution components (no cache borrow held)
        let (tx_manager, broadcaster, order_builder) = match self.get_execution_components() {
            Ok(components) => components,
            Err(e) => {
                log::error!("Failed to get execution components for cancel_all: {e}");
                return Ok(());
            }
        };

        let block_height = self.block_time_monitor.current_block_height() as u32;

        // Collect (instrument_id, client_id, order_flags) tuples for cancel
        // Use stored order_flags from order context to ensure correct cancellation
        let mut orders_to_cancel = Vec::new();

        for (client_order_id, _time_in_force, _expire_time) in &order_data {
            let Some(encoded) = self.encoder.get(client_order_id) else {
                log::warn!("Cannot cancel order {client_order_id}: not found in encoder");
                continue;
            };
            let client_id_u32 = encoded.client_id;

            // Skip if context already cleaned up (terminal WS event received)
            let Some(ctx) = self.get_order_context(client_id_u32) else {
                log::debug!(
                    "Skipping cancel for {client_order_id}: order context already cleaned up (terminal)"
                );
                continue;
            };
            orders_to_cancel.push((instrument_id, client_id_u32, ctx.order_flags));
        }

        if orders_to_cancel.is_empty() {
            return Ok(());
        }

        log::debug!(
            "Cancel all: {} orders (short_term={}, long_term={}), instrument_id={instrument_id}, order_side={order_side_filter:?}",
            orders_to_cancel.len(),
            orders_to_cancel
                .iter()
                .filter(|(_, _, f)| *f == types::ORDER_FLAG_SHORT_TERM)
                .count(),
            orders_to_cancel
                .iter()
                .filter(|(_, _, f)| *f != types::ORDER_FLAG_SHORT_TERM)
                .count(),
        );

        self.spawn_task("cancel_all_orders", async move {
            broadcast_partitioned_cancels(
                orders_to_cancel,
                block_height,
                tx_manager,
                broadcaster,
                order_builder,
            )
            .await
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        if cmd.cancels.is_empty() {
            return Ok(());
        }

        if !self.is_connected() {
            anyhow::bail!("Cannot cancel orders: not connected");
        }

        // Get execution components for broadcasting
        let (tx_manager, broadcaster, order_builder) = match self.get_execution_components() {
            Ok(components) => components,
            Err(e) => {
                log::error!("Failed to get execution components for batch cancel: {e}");
                return Ok(());
            }
        };

        // Convert ClientOrderIds to u32 and get order_flags
        let mut orders_to_cancel = Vec::with_capacity(cmd.cancels.len());
        for cancel in &cmd.cancels {
            let client_order_id = cancel.client_order_id;
            let encoded = match self.encoder.get(&client_order_id) {
                Some(enc) => enc,
                None => {
                    log::warn!(
                        "No u32 mapping found for client_order_id={client_order_id}, skipping cancel"
                    );
                    continue;
                }
            };
            let client_id_u32 = encoded.client_id;

            // Skip if context already cleaned up (terminal WS event received)
            let Some(ctx) = self.get_order_context(client_id_u32) else {
                log::debug!(
                    "Skipping cancel for {client_order_id}: order context already cleaned up (terminal)"
                );
                continue;
            };

            orders_to_cancel.push((cancel.instrument_id, client_id_u32, ctx.order_flags));
        }

        if orders_to_cancel.is_empty() {
            log::warn!("No valid orders to cancel in batch");
            return Ok(());
        }

        let block_height = self.block_time_monitor.current_block_height() as u32;

        log::debug!(
            "Batch cancelling {} orders via partitioned strategy",
            orders_to_cancel.len(),
        );

        self.spawn_task("batch_cancel_orders", async move {
            broadcast_partitioned_cancels(
                orders_to_cancel,
                block_height,
                tx_manager,
                broadcaster,
                order_builder,
            )
            .await
        });

        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let wallet_address = self.wallet_address.clone();
        let subaccount_number = self.subaccount_number;
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();

        self.spawn_task("query_account", async move {
            let account_state = http_client
                .request_account_state(&wallet_address, subaccount_number, account_id)
                .await
                .context("failed to query account state")?;

            emitter.emit_account_state(
                account_state.balances.clone(),
                account_state.margins.clone(),
                account_state.is_reported,
                account_state.ts_event,
            );
            Ok(())
        });

        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        log::debug!("Querying order: client_order_id={}", cmd.client_order_id);

        let http_client = self.http_client.clone();
        let wallet_address = self.wallet_address.clone();
        let subaccount_number = self.subaccount_number;
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let instrument_id = cmd.instrument_id;

        self.spawn_task("query_order", async move {
            let reports = http_client
                .request_order_status_reports(
                    &wallet_address,
                    subaccount_number,
                    account_id,
                    Some(instrument_id),
                )
                .await
                .context("failed to query order status")?;

            // Find matching report by client_order_id or venue_order_id
            let report = reports.into_iter().find(|r| {
                if venue_order_id.is_some_and(|vid| r.venue_order_id == vid) {
                    return true;
                }
                r.client_order_id.is_some_and(|cid| cid == client_order_id)
            });

            if let Some(report) = report {
                emitter.send_order_status_report(report);
            } else {
                log::warn!(
                    "No order found for client_order_id={client_order_id}, venue_order_id={venue_order_id:?}"
                );
            }

            Ok(())
        });

        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            log::warn!("dYdX execution client already connected");
            return Ok(());
        }

        log::info!("Connecting to dYdX");

        log::debug!("Loading instruments from HTTP API");
        self.http_client.fetch_and_cache_instruments().await?;
        log::debug!(
            "Loaded {} instruments from HTTP into shared cache",
            self.http_client.cached_instruments_count()
        );
        self.mark_instruments_initialized();

        // Initialize gRPC client (deferred from constructor to avoid blocking)
        let grpc_urls = self.config.get_grpc_urls();
        let mut grpc_client = DydxGrpcClient::new_with_fallback(&grpc_urls)
            .await
            .context("failed to construct dYdX gRPC client")?;
        log::debug!("gRPC client initialized");

        // Fetch initial block height synchronously so orders can be submitted immediately after connect()
        let initial_height = grpc_client
            .latest_block_height()
            .await
            .context("failed to fetch initial block height")?;
        // Use current time as approximation; actual timestamps will come from WebSocket updates
        self.block_time_monitor
            .record_block(initial_height.0 as u64, chrono::Utc::now());
        log::info!("Initial block height: {}", initial_height.0);

        *self.grpc_client.write().await = Some(grpc_client.clone());

        // Resolve private key and create TransactionManager (owns wallet and sequence management)
        let private_key =
            Self::resolve_private_key(&self.config).context("failed to resolve private key")?;
        let tx_manager = Arc::new(
            TransactionManager::new(
                grpc_client.clone(),
                &private_key,
                self.wallet_address.clone(),
                self.get_chain_id(),
            )
            .context("failed to create TransactionManager")?,
        );

        tx_manager
            .resolve_authenticators()
            .await
            .context("failed to resolve authenticators")?;

        // Proactively initialize sequence from chain so orders can be submitted
        // immediately after connect() without first-transaction latency penalty.
        tx_manager
            .initialize_sequence()
            .await
            .context("failed to initialize sequence")?;

        self.tx_manager = Some(tx_manager);
        self.broadcaster = Some(Arc::new(TxBroadcaster::new(
            grpc_client,
            self.config.grpc_quota(),
        )));
        self.order_builder = Some(Arc::new(OrderMessageBuilder::new(
            self.http_client.clone(),
            self.wallet_address.clone(),
            self.subaccount_number,
            self.block_time_monitor.clone(),
        )));
        log::debug!(
            "OrderMessageBuilder initialized (block_time_monitor ready: {}, max_short_term: {:.1}s)",
            self.block_time_monitor.is_ready(),
            SHORT_TERM_ORDER_MAXIMUM_LIFETIME as f64
                * self.block_time_monitor.seconds_per_block_or_default()
        );

        // Connect WebSocket
        self.ws_client.connect().await?;
        log::debug!("WebSocket connected");

        // Subscribe to block height updates
        self.ws_client.subscribe_block_height().await?;
        log::debug!("Subscribed to block height updates");

        // Subscribe to markets for instrument data
        self.ws_client.subscribe_markets().await?;
        log::debug!("Subscribed to markets");

        // Subscribe to subaccount updates (wallet is always initialized for execution client)
        log::info!(
            "Using wallet address for queries: {} (subaccount {})",
            self.wallet_address,
            self.subaccount_number
        );
        self.ws_client
            .subscribe_subaccount(&self.wallet_address, self.subaccount_number)
            .await?;
        log::debug!(
            "Subscribed to subaccount updates: {}/{}",
            self.wallet_address,
            self.subaccount_number
        );

        let stream = self.ws_client.stream();
        self.spawn_ws_stream_handler(stream);

        // Wait for account to be registered in cache before continuing.
        // This ensures execution state reconciliation can process fills correctly
        // (fills require the account to be registered for portfolio updates).
        self.await_account_registered(30.0).await?;

        self.core.set_connected();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            log::warn!("dYdX execution client not connected");
            return Ok(());
        }

        log::info!("Disconnecting from dYdX");

        // Unsubscribe from subaccount (execution client always has credentials)
        let _ = self
            .ws_client
            .unsubscribe_subaccount(&self.wallet_address, self.subaccount_number)
            .await
            .map_err(|e| log::warn!("Failed to unsubscribe from subaccount: {e}"));

        // Unsubscribe from markets
        let _ = self
            .ws_client
            .unsubscribe_markets()
            .await
            .map_err(|e| log::warn!("Failed to unsubscribe from markets: {e}"));

        // Unsubscribe from block height
        let _ = self
            .ws_client
            .unsubscribe_block_height()
            .await
            .map_err(|e| log::warn!("Failed to unsubscribe from block height: {e}"));

        // Disconnect WebSocket
        self.ws_client.disconnect().await?;

        // Abort WebSocket message processing task
        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
            log::debug!("Aborted WebSocket message processing task");
        }

        // Abort any pending tasks
        self.abort_pending_tasks();

        self.core.set_disconnected();
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        // dYdX Indexer `/v4/orders` caps at `limit` and has no offset cursor, so we
        // request the maximum page (1000) to maximise the chance of finding a match
        // on active subaccounts. Callers looking for older orders should prefer
        // `generate_mass_status` or narrow via `instrument_id`.
        const ORDER_LOOKUP_LIMIT: u32 = 1_000;

        // Fetch orders, narrowing by market when an instrument filter is provided
        let market = cmd
            .instrument_id
            .map(|id| id.symbol.as_str().trim_end_matches("-PERP").to_string());

        let response = self
            .http_client
            .inner
            .get_orders(
                &self.wallet_address,
                self.subaccount_number,
                market.as_deref(),
                Some(ORDER_LOOKUP_LIMIT),
            )
            .await
            .context("failed to fetch order from dYdX API")?;

        if response.is_empty() {
            log::debug!(
                "No orders returned for {}/subaccount={} (market_filter={:?})",
                self.wallet_address,
                self.subaccount_number,
                market,
            );
            return Ok(None);
        }

        let ts_init = UnixNanos::default();
        let scanned_count = response.len();

        let report = find_matching_order_report(
            &response,
            cmd.instrument_id,
            cmd.client_order_id,
            cmd.venue_order_id,
            |clob_pair_id| self.get_instrument_by_clob_pair_id(clob_pair_id),
            &self.encoder,
            self.core.account_id,
            ts_init,
        )?;

        if report.is_none() {
            // The target order was not in the fetched page. Surface the scope so
            // callers can tell whether the order is older than the page or the
            // filters simply didn't match any returned order.
            let page_full = scanned_count == ORDER_LOOKUP_LIMIT as usize;
            log::debug!(
                "No order matched filters for {}/subaccount={} \
                 (client_order_id={:?}, venue_order_id={:?}, instrument_id={:?}, \
                 scanned={scanned_count}, page_full={page_full}, limit={ORDER_LOOKUP_LIMIT})",
                self.wallet_address,
                self.subaccount_number,
                cmd.client_order_id,
                cmd.venue_order_id,
                cmd.instrument_id,
            );
        }

        Ok(report)
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        // Query orders from dYdX API
        let response = self
            .http_client
            .inner
            .get_orders(
                &self.wallet_address,
                self.subaccount_number,
                None, // market filter
                None, // limit
            )
            .await
            .context("failed to fetch orders from dYdX API")?;

        let mut reports = Vec::new();
        let ts_init = UnixNanos::default();

        for order in response {
            let instrument = match self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
                Some(inst) => inst,
                None => continue,
            };

            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            match parse_order_status_report(&order, &instrument, self.core.account_id, ts_init) {
                Ok(mut r) => {
                    if !order.client_id.is_empty()
                        && let Ok(client_id_u32) = order.client_id.parse::<u32>()
                    {
                        self.encoder.register_known_client_id(client_id_u32);

                        if let Some(decoded) = self
                            .encoder
                            .decode_if_known(client_id_u32, order.client_metadata)
                        {
                            log::debug!(
                                "Decoded order: dYdX client_id={} meta={:#x} -> '{}'",
                                client_id_u32,
                                order.client_metadata,
                                decoded,
                            );
                            r.client_order_id = Some(decoded);
                        }
                    }
                    reports.push(r);
                }
                Err(e) => {
                    log::warn!("Failed to parse order status report: {e}");
                }
            }
        }

        // Filter by open_only if specified
        if cmd.open_only {
            reports.retain(|r| r.order_status.is_open());
        }

        // Filter by time range if specified
        if let Some(start) = cmd.start {
            reports.retain(|r| r.ts_last >= start);
        }

        if let Some(end) = cmd.end {
            reports.retain(|r| r.ts_last <= end);
        }

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let response = self
            .http_client
            .inner
            .get_fills(
                &self.wallet_address,
                self.subaccount_number,
                None, // market filter
                None, // limit
            )
            .await
            .context("failed to fetch fills from dYdX API")?;

        let mut reports = Vec::new();
        let ts_init = UnixNanos::default();

        for fill in response.fills {
            let instrument = match self.get_instrument_by_market(&fill.market) {
                Some(inst) => inst,
                None => {
                    log::warn!("Unknown market in fill: {}", fill.market);
                    continue;
                }
            };

            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            let report = match parse_fill_report(&fill, &instrument, self.core.account_id, ts_init)
            {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("Failed to parse fill report: {e}");
                    continue;
                }
            };

            reports.push(report);
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|r| r.venue_order_id.as_str() == venue_order_id.as_str());
        }

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        // Query subaccount positions from dYdX API
        let response = self
            .http_client
            .inner
            .get_subaccount(&self.wallet_address, self.subaccount_number)
            .await
            .context("failed to fetch subaccount from dYdX API")?;

        let mut reports = Vec::new();
        let ts_init = UnixNanos::default();

        for (market_ticker, perp_position) in &response.subaccount.open_perpetual_positions {
            let instrument = match self.get_instrument_by_market(market_ticker) {
                Some(inst) => inst,
                None => {
                    log::warn!("Unknown market in position: {market_ticker}");
                    continue;
                }
            };

            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            let report = match parse_position_status_report(
                perp_position,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("Failed to parse position status report: {e}");
                    continue;
                }
            };

            reports.push(report);
        }

        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ts_init = UnixNanos::default();

        // Query orders
        let orders_response = self
            .http_client
            .inner
            .get_orders(&self.wallet_address, self.subaccount_number, None, None)
            .await
            .context("failed to fetch orders for mass status")?;

        // Query subaccount for positions
        let subaccount_response = self
            .http_client
            .inner
            .get_subaccount(&self.wallet_address, self.subaccount_number)
            .await
            .context("failed to fetch subaccount for mass status")?;

        // Query fills
        let fills_response = self
            .http_client
            .inner
            .get_fills(&self.wallet_address, self.subaccount_number, None, None)
            .await
            .context("failed to fetch fills for mass status")?;

        // Parse order reports
        let mut order_reports = Vec::new();
        let mut orders_filtered = 0usize;

        for order in orders_response {
            let instrument = match self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
                Some(inst) => inst,
                None => {
                    orders_filtered += 1;
                    continue;
                }
            };

            match parse_order_status_report(&order, &instrument, self.core.account_id, ts_init) {
                Ok(mut r) => {
                    if !order.client_id.is_empty()
                        && let Ok(client_id_u32) = order.client_id.parse::<u32>()
                    {
                        self.encoder.register_known_client_id(client_id_u32);

                        if let Some(decoded) = self
                            .encoder
                            .decode_if_known(client_id_u32, order.client_metadata)
                        {
                            log::debug!(
                                "Decoded reconciliation order: dYdX client_id={} meta={:#x} -> '{}'",
                                client_id_u32,
                                order.client_metadata,
                                decoded,
                            );
                            r.client_order_id = Some(decoded);
                        }
                    }
                    order_reports.push(r);
                }
                Err(e) => {
                    log::warn!("Failed to parse order status report: {e}");
                    orders_filtered += 1;
                }
            }
        }

        // Parse position reports
        let mut position_reports = Vec::new();

        for (market_ticker, perp_position) in
            &subaccount_response.subaccount.open_perpetual_positions
        {
            let instrument = match self.get_instrument_by_market(market_ticker) {
                Some(inst) => inst,
                None => continue,
            };

            match parse_position_status_report(
                perp_position,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(r) => position_reports.push(r),
                Err(e) => {
                    log::warn!("Failed to parse position status report: {e}");
                }
            }
        }

        // Parse fill reports
        let mut fill_reports = Vec::new();
        let mut fills_filtered = 0usize;

        for fill in fills_response.fills {
            let instrument = match self.get_instrument_by_market(&fill.market) {
                Some(inst) => inst,
                None => {
                    fills_filtered += 1;
                    continue;
                }
            };

            match parse_fill_report(&fill, &instrument, self.core.account_id, ts_init) {
                Ok(r) => fill_reports.push(r),
                Err(e) => {
                    log::warn!("Failed to parse fill report: {e}");
                    fills_filtered += 1;
                }
            }
        }

        apply_avg_px_from_fills(&mut order_reports, &fill_reports);

        // Apply lookback filter to orders and fills (positions are always current state)
        if let Some(mins) = lookback_mins {
            let now_ns = self.clock.get_time_ns();
            let cutoff_ns = now_ns.as_u64().saturating_sub(mins * 60 * 1_000_000_000);
            let cutoff = UnixNanos::from(cutoff_ns);

            let orders_before = order_reports.len();
            order_reports.retain(|r| r.ts_last >= cutoff);
            let orders_removed = orders_before - order_reports.len();

            let fills_before = fill_reports.len();
            fill_reports.retain(|r| r.ts_event >= cutoff);
            let fills_removed = fills_before - fill_reports.len();

            log::info!(
                "Lookback filter ({}min): orders {}->{} (removed {}), fills {}->{} (removed {}), positions {} (unfiltered)",
                mins,
                orders_before,
                order_reports.len(),
                orders_removed,
                fills_before,
                fill_reports.len(),
                fills_removed,
                position_reports.len(),
            );
        } else {
            log::debug!(
                "Generated mass status: {} orders ({} filtered), {} positions, {} fills ({} filtered)",
                order_reports.len(),
                orders_filtered,
                position_reports.len(),
                fill_reports.len(),
                fills_filtered,
            );
        }

        // Create mass status and add reports
        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            self.core.venue,
            ts_init,
            None, // report_id will be auto-generated
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_position_reports(position_reports);
        mass_status.add_fill_reports(fill_reports);

        Ok(Some(mass_status))
    }
}

/// Iterates `orders` and returns the first report whose parsed fields match every active
/// filter. Extracted from `generate_order_status_report` so the matching loop can be
/// exercised in isolation.
#[allow(clippy::too_many_arguments)]
fn find_matching_order_report<F>(
    orders: &[crate::http::models::Order],
    instrument_filter: Option<InstrumentId>,
    client_order_id_filter: Option<ClientOrderId>,
    venue_order_id_filter: Option<VenueOrderId>,
    lookup_instrument: F,
    encoder: &ClientOrderIdEncoder,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<OrderStatusReport>>
where
    F: Fn(u32) -> Option<InstrumentAny>,
{
    for order in orders {
        let instrument = match lookup_instrument(order.clob_pair_id) {
            Some(inst) => inst,
            None => continue,
        };

        if let Some(filter_id) = instrument_filter
            && instrument.id() != filter_id
        {
            continue;
        }

        let mut report = parse_order_status_report(order, &instrument, account_id, ts_init)
            .context("failed to parse order status report")?;

        if !order.client_id.is_empty()
            && let Ok(client_id_u32) = order.client_id.parse::<u32>()
        {
            encoder.register_known_client_id(client_id_u32);

            if let Some(decoded) = encoder.decode_if_known(client_id_u32, order.client_metadata) {
                log::debug!(
                    "Decoded order: dYdX client_id={} meta={:#x} -> '{}'",
                    client_id_u32,
                    order.client_metadata,
                    decoded,
                );
                report.client_order_id = Some(decoded);
            }
        }

        if let Some(client_order_id) = client_order_id_filter
            && report.client_order_id != Some(client_order_id)
        {
            continue;
        }

        if let Some(venue_order_id) = venue_order_id_filter
            && report.venue_order_id.as_str() != venue_order_id.as_str()
        {
            continue;
        }

        return Ok(Some(report));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::OrderSide as NautilusOrderSide,
        identifiers::Symbol,
        instruments::{CryptoPerpetual, InstrumentAny},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        common::enums::{DydxOrderStatus, DydxOrderType, DydxTimeInForce},
        http::models::Order,
    };

    fn test_instrument(symbol: &str, venue: &str) -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new(symbol), Venue::new(venue));
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            instrument_id.symbol,
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false,
            2,
            3,
            Price::new(0.01, 2),
            Quantity::new(0.001, 3),
            None,
            None,
            None,
            None,
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

    fn test_order(id: &str, clob_pair_id: u32, client_id: &str) -> Order {
        Order {
            id: id.to_string(),
            subaccount_id: "sub-1".to_string(),
            client_id: client_id.to_string(),
            clob_pair_id,
            side: NautilusOrderSide::Buy,
            size: dec!(1.0),
            total_filled: dec!(0),
            price: dec!(50000),
            status: DydxOrderStatus::Open,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            reduce_only: false,
            post_only: false,
            order_flags: 64,
            good_til_block: None,
            good_til_block_time: None,
            created_at_height: Some(100),
            client_metadata: 4,
            trigger_price: None,
            condition_type: None,
            conditional_order_trigger_subticks: None,
            execution: None,
            updated_at: None,
            updated_at_height: None,
            ticker: None,
            subaccount_number: 0,
            order_router_address: None,
        }
    }

    #[rstest]
    fn test_find_matching_order_report_returns_later_match() {
        // Regression guard: earlier implementation fetched limit=1 and returned None
        // when the first order didn't match the filter. The fixed iteration logic must
        // scan the whole response and return the matching entry.
        let btc_inst = test_instrument("BTC-USD-PERP", "DYDX");
        let eth_inst = test_instrument("ETH-USD-PERP", "DYDX");

        // Response ordered so the non-matching order comes first.
        let orders = vec![
            test_order("order-eth", 1, "22222"),
            test_order("order-btc", 0, "11111"),
        ];

        let encoder = ClientOrderIdEncoder::new();
        let report = find_matching_order_report(
            &orders,
            Some(btc_inst.id()),
            None,
            None,
            |clob_pair_id| match clob_pair_id {
                0 => Some(btc_inst.clone()),
                1 => Some(eth_inst.clone()),
                _ => None,
            },
            &encoder,
            AccountId::new("DYDX-001"),
            UnixNanos::default(),
        )
        .expect("lookup should succeed");

        let report = report.expect("matching order should be found");
        assert_eq!(report.instrument_id, btc_inst.id());
        assert_eq!(report.venue_order_id.as_str(), "order-btc");
    }

    #[rstest]
    fn test_find_matching_order_report_returns_none_when_no_match() {
        let btc_inst = test_instrument("BTC-USD-PERP", "DYDX");
        let eth_inst = test_instrument("ETH-USD-PERP", "DYDX");

        let orders = vec![
            test_order("order-eth-1", 1, "22222"),
            test_order("order-eth-2", 1, "33333"),
        ];

        let encoder = ClientOrderIdEncoder::new();
        let report = find_matching_order_report(
            &orders,
            Some(btc_inst.id()),
            None,
            None,
            |clob_pair_id| match clob_pair_id {
                0 => Some(btc_inst.clone()),
                1 => Some(eth_inst.clone()),
                _ => None,
            },
            &encoder,
            AccountId::new("DYDX-001"),
            UnixNanos::default(),
        )
        .expect("lookup should succeed");

        assert!(report.is_none());
    }

    #[rstest]
    fn test_find_matching_order_report_filters_by_venue_order_id() {
        let btc_inst = test_instrument("BTC-USD-PERP", "DYDX");

        let orders = vec![
            test_order("order-a", 0, "11111"),
            test_order("order-b", 0, "22222"),
            test_order("order-c", 0, "33333"),
        ];

        let encoder = ClientOrderIdEncoder::new();
        let target = VenueOrderId::new("order-b");
        let report = find_matching_order_report(
            &orders,
            None,
            None,
            Some(target),
            |_| Some(btc_inst.clone()),
            &encoder,
            AccountId::new("DYDX-001"),
            UnixNanos::default(),
        )
        .expect("lookup should succeed")
        .expect("matching order should be found");

        assert_eq!(report.venue_order_id.as_str(), "order-b");
    }

    #[rstest]
    fn test_find_matching_order_report_skips_orders_without_cached_instrument() {
        let btc_inst = test_instrument("BTC-USD-PERP", "DYDX");

        let orders = vec![
            // First order's clob_pair_id does not resolve -- must be skipped.
            test_order("order-unknown", 99, "11111"),
            test_order("order-btc", 0, "22222"),
        ];

        let encoder = ClientOrderIdEncoder::new();
        let report = find_matching_order_report(
            &orders,
            Some(btc_inst.id()),
            None,
            None,
            |clob_pair_id| (clob_pair_id == 0).then(|| btc_inst.clone()),
            &encoder,
            AccountId::new("DYDX-001"),
            UnixNanos::default(),
        )
        .expect("lookup should succeed")
        .expect("matching order should be found");

        assert_eq!(report.venue_order_id.as_str(), "order-btc");
    }
}
