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

//! Live execution client implementation for the Polymarket adapter.

pub mod order_builder;
pub(crate) mod order_fill_tracker;
pub mod parse;
pub(crate) mod reconciliation;
pub(crate) mod submitter;
pub(crate) mod types;

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ahash::AHashSet;
use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    cache::fifo::FifoCacheMap,
    clients::ExecutionClient,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    MUTEX_POISONED, UUID4, UnixNanos,
    collections::AtomicMap,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderEventAny, OrderUpdated},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use nautilus_network::retry::RetryConfig;
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use ustr::Ustr;

use self::{
    order_builder::PolymarketOrderBuilder,
    order_fill_tracker::OrderFillTrackerMap,
    parse::{
        compute_commission, instrument_fee_exponent, instrument_taker_fee, parse_balance_allowance,
        parse_order_status_report,
    },
    reconciliation::{
        FillContext, apply_fill_filters, build_fill_reports_from_trades, build_position_reports,
    },
    submitter::{MarketBuyFeeContext, OrderSubmitter},
    types::{BatchLimitOrderContext, CancelOutcome, LimitOrderSubmitRequest},
};
use crate::{
    common::{
        consts::{BATCH_ORDER_LIMIT, POLYMARKET_VENUE},
        credential::Secrets,
        enums::SignatureType,
    },
    config::PolymarketExecClientConfig,
    http::{
        clob::PolymarketClobHttpClient,
        data_api::PolymarketDataApiHttpClient,
        query::{CancelResponse, GetBalanceAllowanceParams, GetTradesParams, OrderResponse},
    },
    signing::eip712::OrderSigner,
    websocket::{
        client::PolymarketWebSocketClient,
        dispatch::{WsDispatchContext, WsDispatchState, dispatch_user_message},
        messages::PolymarketWsMessage,
    },
};

/// Live execution client for the Polymarket prediction market.
#[derive(Debug)]
pub struct PolymarketExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: PolymarketExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: PolymarketClobHttpClient,
    data_api_client: PolymarketDataApiHttpClient,
    submitter: OrderSubmitter,
    ws_client: PolymarketWebSocketClient,
    secrets: Secrets,
    pending_tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    stopping: Arc<AtomicBool>,
    ws_stream_handle: Mutex<Option<JoinHandle<()>>>,
    shared_token_instruments: Arc<AtomicMap<Ustr, InstrumentAny>>,
    neg_risk_index: Arc<AtomicMap<InstrumentId, bool>>,
    fill_tracker: Arc<OrderFillTrackerMap>,
    pending_fills: Arc<Mutex<FifoCacheMap<VenueOrderId, Vec<FillReport>, 1_000>>>,
    pending_order_reports: Arc<Mutex<FifoCacheMap<VenueOrderId, Vec<OrderStatusReport>, 1_000>>>,
    pending_cancels: Arc<Mutex<AHashSet<ClientOrderId>>>,
}

impl PolymarketExecutionClient {
    /// Creates a new [`PolymarketExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if credentials cannot be resolved or clients fail to construct.
    pub fn new(
        core: ExecutionClientCore,
        config: PolymarketExecClientConfig,
    ) -> anyhow::Result<Self> {
        let secrets = Secrets::resolve(
            config.private_key.as_deref(),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.passphrase.clone(),
            config.funder.clone(),
        )
        .context("failed to resolve Polymarket credentials")?;

        let http_client = PolymarketClobHttpClient::new(
            secrets.credential.clone(),
            secrets.address.clone(),
            config.base_url_http.clone(),
            config.http_timeout_secs,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create CLOB HTTP client")?;

        let data_api_client =
            PolymarketDataApiHttpClient::new(Some(config.data_api_url()), config.http_timeout_secs)
                .map_err(|e| anyhow::anyhow!("{e}"))
                .context("failed to create Data API HTTP client")?;

        let order_signer =
            OrderSigner::new(&secrets.private_key).context("failed to create order signer")?;

        let signer_address = secrets.address.clone();
        let maker_address = secrets
            .funder
            .clone()
            .unwrap_or_else(|| signer_address.clone());
        let order_builder = Arc::new(PolymarketOrderBuilder::new(
            order_signer,
            signer_address,
            maker_address,
            config.signature_type,
        ));

        let retry_config = RetryConfig {
            max_retries: config.max_retries,
            initial_delay_ms: config.retry_delay_initial_ms,
            max_delay_ms: config.retry_delay_max_ms,
            backoff_factor: 2.0,
            jitter_ms: 1_000,
            operation_timeout_ms: Some(config.http_timeout_secs * 1_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };
        let submitter = OrderSubmitter::new(http_client.clone(), order_builder, retry_config);

        let ws_client = PolymarketWebSocketClient::new_user(
            config.base_url_ws.clone(),
            secrets.credential.clone(),
            config.transport_backend,
        );

        let clock = get_atomic_clock_realtime();
        let pusd = get_pusd_currency();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Cash,
            Some(pusd),
        );

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            data_api_client,
            submitter,
            ws_client,
            secrets,
            pending_tasks: Arc::new(Mutex::new(Vec::new())),
            stopping: Arc::new(AtomicBool::new(false)),
            ws_stream_handle: Mutex::new(None),
            shared_token_instruments: Arc::new(AtomicMap::new()),
            neg_risk_index: Arc::new(AtomicMap::new()),
            fill_tracker: Arc::new(OrderFillTrackerMap::new()),
            pending_fills: Arc::new(Mutex::new(FifoCacheMap::default())),
            pending_order_reports: Arc::new(Mutex::new(FifoCacheMap::default())),
            pending_cancels: Arc::new(Mutex::new(AHashSet::new())),
        })
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
            }
        });

        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        tasks.retain(|handle| !handle.is_finished());
        tasks.push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    async fn refresh_account_state(&self) -> anyhow::Result<()> {
        fetch_and_emit_account_state(
            &self.http_client,
            &self.emitter,
            self.clock,
            self.config.signature_type,
        )
        .await
    }

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

    async fn start_ws_stream(&mut self) -> anyhow::Result<()> {
        self.ws_client
            .connect()
            .await
            .context("failed to connect user WebSocket")?;

        self.ws_client
            .subscribe_user()
            .await
            .context("failed to subscribe to user channel")?;

        let mut rx = self
            .ws_client
            .take_message_receiver()
            .ok_or_else(|| anyhow::anyhow!("WebSocket message receiver not available"))?;

        let emitter = self.emitter.clone();
        let token_instruments = self.shared_token_instruments.clone();
        let account_id = self.core.account_id;
        let http_client = self.http_client.clone();
        let clock = self.clock;
        let signature_type = self.config.signature_type;
        let user_address = self
            .secrets
            .funder
            .clone()
            .unwrap_or_else(|| self.secrets.address.clone());
        let user_api_key = self.secrets.credential.api_key().to_string();

        let fill_tracker = self.fill_tracker.clone();
        let pending_fills = self.pending_fills.clone();
        let pending_order_reports = self.pending_order_reports.clone();

        let handle = get_runtime().spawn(async move {
            let mut state = WsDispatchState::default();
            let ctx = WsDispatchContext {
                token_instruments: &token_instruments,
                fill_tracker: &fill_tracker,
                pending_fills: &pending_fills,
                pending_order_reports: &pending_order_reports,
                emitter: &emitter,
                account_id,
                clock,
                user_address: &user_address,
                user_api_key: &user_api_key,
            };

            loop {
                match rx.recv().await {
                    Some(PolymarketWsMessage::User(user_msg)) => {
                        if let Some(_refresh) =
                            dispatch_user_message(&user_msg, &ctx, &mut state)
                        {
                            let http = http_client.clone();
                            let emit = emitter.clone();

                            get_runtime().spawn(async move {
                                match fetch_and_emit_account_state(
                                    &http, &emit, clock, signature_type,
                                )
                                .await
                                {
                                    Ok(()) => log::info!(
                                        "Account state refreshed after finalized trade for {account_id}"
                                    ),
                                    Err(e) => log::warn!(
                                        "Failed to refresh account after finalized trade: {e}"
                                    ),
                                }
                            });
                        }
                    }
                    Some(PolymarketWsMessage::Market(_)) => {}
                    Some(PolymarketWsMessage::Reconnected) => {
                        log::info!("User WebSocket reconnected");
                    }
                    None => {
                        log::debug!("User WebSocket stream ended");
                        break;
                    }
                }
            }

            log::debug!("User WebSocket handler task completed");
        });

        *self.ws_stream_handle.lock().expect(MUTEX_POISONED) = Some(handle);
        Ok(())
    }

    fn get_neg_risk(&self, instrument_id: &InstrumentId) -> bool {
        self.neg_risk_index
            .get_cloned(instrument_id)
            .unwrap_or(false)
    }

    fn load_instruments_from_cache(&self) {
        let cache = self.core.cache();
        let instruments: Vec<InstrumentAny> = cache
            .instruments(&self.core.venue, None)
            .into_iter()
            .cloned()
            .collect();
        drop(cache);

        // Populate shared AtomicMap for WS handler and reconciliation
        for inst in &instruments {
            self.shared_token_instruments
                .insert(Ustr::from(inst.raw_symbol().as_str()), inst.clone());
        }

        // Build neg_risk_index
        for inst in &instruments {
            if let InstrumentAny::BinaryOption(bo) = inst {
                let neg_risk = bo
                    .info
                    .as_ref()
                    .and_then(|i| i.get_bool("neg_risk"))
                    .unwrap_or(false);
                self.neg_risk_index.insert(bo.id, neg_risk);
            }
        }

        log::info!("Loaded {} instruments from cache", instruments.len());
    }

    fn submit_limit_order(&self, order: OrderAny) {
        if let Err(reason) = PolymarketOrderBuilder::validate_limit_order(&order) {
            self.emitter.emit_order_denied(&order, &reason);
            return;
        }

        let instrument = match self.resolve_instrument(&order) {
            Some(i) => i,
            None => return,
        };

        let neg_risk = self.get_neg_risk(&order.instrument_id());
        let token_id = instrument.raw_symbol().to_string();
        let tick_decimals = instrument.price_precision() as u32;
        let price = order.price().unwrap(); // validated above
        let quantity = order.quantity();
        let tif = order.time_in_force();
        let post_only = order.is_post_only();
        let side = order.order_side();
        let expire_time = order.expire_time();

        self.emitter.emit_order_submitted(&order);

        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let fill_tracker = self.fill_tracker.clone();
        let pending_fills = self.pending_fills.clone();
        let pending_order_reports = self.pending_order_reports.clone();
        let pending_cancels = self.pending_cancels.clone();
        let account_id = self.core.account_id;
        let size_precision = instrument.size_precision();
        let price_precision = instrument.price_precision();

        self.spawn_task("submit_limit_order", async move {
            match submitter
                .submit_limit_order(
                    &token_id,
                    side,
                    price,
                    quantity,
                    tif,
                    post_only,
                    neg_risk,
                    expire_time,
                    tick_decimals,
                )
                .await
            {
                Ok(response) => {
                    if let Some((order_id_str, venue_order_id)) = handle_order_response(
                        Ok(response),
                        &order,
                        &emitter,
                        clock,
                        &fill_tracker,
                        &pending_fills,
                        &pending_order_reports,
                        &pending_cancels,
                        account_id,
                        size_precision,
                        price_precision,
                    ) {
                        execute_deferred_cancel(
                            &submitter,
                            &order,
                            &order_id_str,
                            venue_order_id,
                            &emitter,
                            clock,
                        )
                        .await;
                    }
                }
                Err(e) => {
                    let ts_now = clock.get_time_ns();
                    emitter.emit_order_rejected(&order, &format!("{e}"), ts_now, false);
                }
            }
            Ok(())
        });
    }

    fn submit_market_order(&self, order: OrderAny) {
        if let Err(reason) = PolymarketOrderBuilder::validate_market_order(&order) {
            self.emitter.emit_order_denied(&order, &reason);
            return;
        }

        let instrument = match self.resolve_instrument(&order) {
            Some(i) => i,
            None => return,
        };

        let neg_risk = self.get_neg_risk(&order.instrument_id());
        let token_id = instrument.raw_symbol().to_string();
        let tick_decimals = instrument.price_precision() as u32;
        let side = order.order_side();
        let amount = order.quantity();
        let is_quote_qty = order.is_quote_quantity();

        // Quote-quantity BUYs are sized in pUSD; the venue computes taker
        // fees against `amount + fees`, so we shrink the spend to fit the
        // user's collateral balance before signing. SELL orders are sized
        // in shares and skip this step.
        let needs_fee_adjustment = side == OrderSide::Buy && is_quote_qty;
        let fee_rate = if needs_fee_adjustment {
            instrument_taker_fee(&instrument)
        } else {
            Decimal::ZERO
        };
        let fee_exponent = if needs_fee_adjustment {
            instrument_fee_exponent(&instrument)
        } else {
            1.0
        };

        let submitter = self.submitter.clone();
        let http_client = self.http_client.clone();
        let signature_type = self.config.signature_type;
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let fill_tracker = self.fill_tracker.clone();
        let pending_fills = self.pending_fills.clone();
        let pending_order_reports = self.pending_order_reports.clone();
        let pending_cancels = self.pending_cancels.clone();
        let account_id = self.core.account_id;
        let size_precision = instrument.size_precision();
        let price_precision = instrument.price_precision();

        self.spawn_task("submit_market_order", async move {
            let fee_context = if needs_fee_adjustment {
                match fetch_collateral_balance_pusd(&http_client, signature_type).await {
                    Ok(balance) => Some(MarketBuyFeeContext {
                        user_pusd_balance: balance,
                        fee_rate,
                        fee_exponent,
                        builder_taker_fee_rate: Decimal::ZERO,
                    }),
                    Err(e) => {
                        emitter.emit_order_rejected(
                            &order,
                            &format!("Failed to fetch pUSD balance for fee adjustment: {e}"),
                            clock.get_time_ns(),
                            false,
                        );
                        return Ok(());
                    }
                }
            } else {
                None
            };

            match submitter
                .submit_market_order(
                    &token_id,
                    side,
                    amount,
                    neg_risk,
                    tick_decimals,
                    fee_context,
                )
                .await
            {
                Ok((response, expected_base_qty)) => {
                    let mut order = order;
                    emitter.emit_order_submitted(&order);

                    // Convert quote quantity to base only after successful submission
                    if response.success
                        && is_quote_qty
                        && side == OrderSide::Buy
                        && !expected_base_qty.is_zero()
                        && let Ok(base_qty) =
                            Quantity::from_decimal_dp(expected_base_qty, size_precision)
                    {
                        log::info!(
                            "Converted {} quote quantity {} to base quantity {} \
                             (from signed taker_amount)",
                            order.instrument_id(),
                            amount,
                            base_qty,
                        );

                        let ts_now = clock.get_time_ns();
                        let updated = OrderUpdated::new(
                            order.trader_id(),
                            order.strategy_id(),
                            order.instrument_id(),
                            order.client_order_id(),
                            base_qty,
                            UUID4::new(),
                            ts_now,
                            ts_now,
                            false,
                            order.venue_order_id(),
                            order.account_id(),
                            order.price(),
                            None,
                            None,
                            false, // is_quote_quantity
                        );

                        let event = OrderEventAny::Updated(updated);
                        emitter.send_order_event(event.clone());

                        if let Err(e) = order.apply(event) {
                            log::error!("Failed to apply quote-to-base OrderUpdated: {e}");
                        }
                    }

                    let fok_order_id = response
                        .order_id
                        .as_ref()
                        .filter(|_| response.success)
                        .cloned();

                    if let Some((order_id_str, venue_order_id)) = handle_order_response(
                        Ok(response),
                        &order,
                        &emitter,
                        clock,
                        &fill_tracker,
                        &pending_fills,
                        &pending_order_reports,
                        &pending_cancels,
                        account_id,
                        size_precision,
                        price_precision,
                    ) {
                        execute_deferred_cancel(
                            &submitter,
                            &order,
                            &order_id_str,
                            venue_order_id,
                            &emitter,
                            clock,
                        )
                        .await;
                    }

                    if let Some(order_id) = fok_order_id {
                        check_fok_status(
                            &submitter,
                            &order_id,
                            &fill_tracker,
                            &emitter,
                            account_id,
                            order.instrument_id(),
                            order.order_side(),
                            size_precision,
                            price_precision,
                            clock,
                        )
                        .await;
                    }
                }
                Err(e) => {
                    let ts_now = clock.get_time_ns();
                    emitter.emit_order_rejected(&order, &format!("{e}"), ts_now, false);
                }
            }
            Ok(())
        });
    }

    fn resolve_instrument(&self, order: &OrderAny) -> Option<InstrumentAny> {
        let instrument = self
            .core
            .cache()
            .instrument(&order.instrument_id())
            .cloned();

        match instrument {
            Some(i) => Some(i),
            None => {
                self.emitter.emit_order_denied(
                    order,
                    &format!("Instrument not found: {}", order.instrument_id()),
                );
                None
            }
        }
    }

    fn fill_context(&self) -> FillContext<'_> {
        let user_address = self
            .secrets
            .funder
            .as_deref()
            .unwrap_or(&self.secrets.address);
        FillContext {
            account_id: self.core.account_id,
            user_address,
            api_key: self.secrets.credential.api_key().as_str(),
            pusd: get_pusd_currency(),
            clock: self.clock,
        }
    }
}

#[async_trait(?Send)]
impl ExecutionClient for PolymarketExecutionClient {
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
        *POLYMARKET_VENUE
    }

    fn oms_type(&self) -> OmsType {
        OmsType::Netting
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
            return Ok(());
        }

        self.stopping.store(false, Ordering::Release);
        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();

        log::info!(
            "Started: client_id={}, account_id={}",
            self.core.client_id,
            self.core.account_id,
        );

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        log::info!("Stopping Polymarket execution client");

        // Block new background work from being spawned before we drain.
        self.stopping.store(true, Ordering::Release);

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.abort_pending_tasks();
        self.ws_client.abort();

        self.core.set_disconnected();
        self.core.set_stopped();

        log::info!("Polymarket execution client stopped");
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;

        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        match order.order_type() {
            OrderType::Limit => self.submit_limit_order(order),
            OrderType::Market => self.submit_market_order(order),
            _ => {
                self.emitter.emit_order_denied(
                    &order,
                    &format!(
                        "Unsupported order type for Polymarket: {:?}",
                        order.order_type()
                    ),
                );
            }
        }
        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let mut batch_orders = Vec::with_capacity(cmd.order_inits.len());

        for order_init in &cmd.order_inits {
            let Some(order) = self
                .core
                .cache()
                .order(&order_init.client_order_id)
                .cloned()
            else {
                log::warn!(
                    "Order not found in cache for {}",
                    order_init.client_order_id
                );
                continue;
            };

            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                continue;
            }

            // Market orders cannot go through the /orders batch endpoint; route them
            // through the single-order path which synthesizes a crossing limit order.
            match order.order_type() {
                OrderType::Limit => {}
                OrderType::Market => {
                    self.submit_market_order(order);
                    continue;
                }
                other => {
                    self.emitter.emit_order_denied(
                        &order,
                        &format!("Unsupported order type for Polymarket: {other:?}"),
                    );
                    continue;
                }
            }

            if let Err(reason) = PolymarketOrderBuilder::validate_limit_order(&order) {
                self.emitter.emit_order_denied(&order, &reason);
                continue;
            }

            let instrument = match self.resolve_instrument(&order) {
                Some(i) => i,
                None => continue,
            };

            let price = order
                .price()
                .expect("validated limit order must have a price");
            batch_orders.push(BatchLimitOrderContext {
                request: LimitOrderSubmitRequest {
                    token_id: instrument.raw_symbol().to_string(),
                    side: order.order_side(),
                    price,
                    quantity: order.quantity(),
                    time_in_force: order.time_in_force(),
                    post_only: order.is_post_only(),
                    neg_risk: self.get_neg_risk(&order.instrument_id()),
                    expire_time: order.expire_time(),
                    tick_decimals: instrument.price_precision() as u32,
                },
                size_precision: instrument.size_precision(),
                price_precision: instrument.price_precision(),
                order,
            });
        }

        if batch_orders.is_empty() {
            return Ok(());
        }

        if batch_orders.len() == 1 {
            // Route through the single-order path to preserve retry semantics;
            // the batch endpoint deliberately disables retry due to missing idempotency keys.
            let batch_order = batch_orders.pop().expect("len checked");
            self.submit_limit_order(batch_order.order);
            return Ok(());
        }

        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let fill_tracker = self.fill_tracker.clone();
        let pending_fills = self.pending_fills.clone();
        let pending_order_reports = self.pending_order_reports.clone();
        let pending_cancels = self.pending_cancels.clone();
        let pending_tasks = self.pending_tasks.clone();
        let stopping = self.stopping.clone();
        let account_id = self.core.account_id;

        self.spawn_task("submit_order_list", async move {
            for batch_order in &batch_orders {
                emitter.emit_order_submitted(&batch_order.order);
            }

            let requests: Vec<LimitOrderSubmitRequest> =
                batch_orders.iter().map(|bo| bo.request.clone()).collect();
            let prepare_results = submitter.prepare_limit_order_submissions(&requests).await;

            let mut prepared_orders = Vec::with_capacity(batch_orders.len());
            let mut submissions = Vec::with_capacity(batch_orders.len());

            for (batch_order, result) in batch_orders.into_iter().zip(prepare_results) {
                match result {
                    Ok(submission) => {
                        prepared_orders.push(batch_order);
                        submissions.push(submission);
                    }
                    Err(e) => {
                        reject_submit_order(
                            &batch_order.order,
                            &format!("{e}"),
                            &emitter,
                            clock,
                            &pending_cancels,
                        );
                    }
                }
            }

            if submissions.is_empty() {
                return Ok(());
            }

            // Chunk into venue-sized batches; POST /orders caps at BATCH_ORDER_LIMIT orders.
            // A remainder chunk of size 1 goes through the single-order path so it keeps
            // the same retry semantics as a list of length 1.
            let total = submissions.len();
            let mut offset = 0;
            while offset < total {
                let end = (offset + BATCH_ORDER_LIMIT).min(total);
                let mut submissions_chunk = submissions[offset..end].to_vec();
                let mut orders_chunk = prepared_orders[offset..end].to_vec();

                if submissions_chunk.len() == 1 {
                    let submission = submissions_chunk.pop().expect("len 1");
                    let batch_order = orders_chunk.pop().expect("len 1");
                    handle_single_order_response(
                        submitter.post_limit_order_submission(submission).await,
                        batch_order,
                        &submitter,
                        &emitter,
                        clock,
                        &fill_tracker,
                        &pending_fills,
                        &pending_order_reports,
                        &pending_cancels,
                        account_id,
                    )
                    .await;
                } else {
                    match submitter
                        .post_limit_order_submissions(submissions_chunk)
                        .await
                    {
                        Ok(responses) => {
                            handle_batch_order_responses(
                                responses,
                                orders_chunk,
                                &submitter,
                                &emitter,
                                clock,
                                &fill_tracker,
                                &pending_fills,
                                &pending_order_reports,
                                &pending_cancels,
                                &pending_tasks,
                                &stopping,
                                account_id,
                            )
                            .await;
                        }
                        Err(e) => {
                            for batch_order in orders_chunk {
                                reject_submit_order(
                                    &batch_order.order,
                                    &format!("{e}"),
                                    &emitter,
                                    clock,
                                    &pending_cancels,
                                );
                            }
                        }
                    }
                }

                offset = end;
            }

            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let order = self.core.cache().order(&cmd.client_order_id).cloned();
        if let Some(order) = order {
            let venue_order_id = order.venue_order_id();
            let ts_now = self.clock.get_time_ns();
            self.emitter.emit_order_modify_rejected(
                &order,
                venue_order_id,
                "Order modification not supported on Polymarket",
                ts_now,
            );
        }
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let order = self.core.cache().order(&cmd.client_order_id).cloned();
        let order_ref = match &order {
            Some(o) => o,
            None => {
                log::warn!(
                    "Order not found in cache for cancel: {}",
                    cmd.client_order_id
                );
                return Ok(());
            }
        };

        if !order_ref.is_open() {
            log::warn!(
                "Cannot cancel order that is not open: {}",
                cmd.client_order_id
            );
            let ts_now = self.clock.get_time_ns();
            self.emitter.emit_order_cancel_rejected(
                order_ref,
                order_ref.venue_order_id(),
                &format!("Order is not open (status: {:?})", order_ref.status()),
                ts_now,
            );
            return Ok(());
        }

        let venue_order_id = match order_ref.venue_order_id() {
            Some(id) => id,
            None => {
                // Check cache index: submit may have cached it before OrderAccepted was applied
                match self
                    .core
                    .cache()
                    .venue_order_id(&cmd.client_order_id)
                    .copied()
                {
                    Some(id) => id,
                    None => {
                        log::info!(
                            "Cancel for {} deferred, venue_order_id not yet available",
                            cmd.client_order_id
                        );
                        self.pending_cancels
                            .lock()
                            .expect(MUTEX_POISONED)
                            .insert(cmd.client_order_id);
                        return Ok(());
                    }
                }
            }
        };

        let order_id_str = venue_order_id.to_string();
        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let order_clone = order.unwrap();

        self.spawn_task("cancel_order", async move {
            match submitter.cancel_order(&order_id_str).await {
                Ok(response) => {
                    process_cancel_result(
                        &response,
                        &order_id_str,
                        &order_clone,
                        venue_order_id,
                        &emitter,
                        clock,
                    );
                }
                Err(e) => {
                    let ts_now = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected(
                        &order_clone,
                        Some(venue_order_id),
                        &format!("HTTP request failed: {e}"),
                        ts_now,
                    );
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let cache = self.core.cache();
        let open_orders = cache.orders_open(
            Some(&self.core.venue),
            Some(&cmd.instrument_id),
            Some(&cmd.strategy_id),
            None,
            Some(cmd.order_side),
        );

        if open_orders.is_empty() {
            log::debug!("No open orders to cancel for {}", cmd.instrument_id);
            return Ok(());
        }

        let venue_order_ids: Vec<String> = open_orders
            .iter()
            .filter_map(|o| o.venue_order_id().map(|v| v.to_string()))
            .collect();

        if venue_order_ids.is_empty() {
            log::warn!("No venue order IDs found for cancel all");
            return Ok(());
        }

        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let orders: Vec<OrderAny> = open_orders.into_iter().cloned().collect();

        self.spawn_task("cancel_all_orders", async move {
            let order_id_refs: Vec<&str> = venue_order_ids.iter().map(String::as_str).collect();
            let response = submitter
                .cancel_orders(&order_id_refs)
                .await
                .context("failed to cancel all orders")?;

            for order in &orders {
                if let Some(vid) = order.venue_order_id() {
                    let vid_str = vid.to_string();
                    process_cancel_result(&response, &vid_str, order, vid, &emitter, clock);
                }
            }

            log::info!("Canceled {} orders", response.canceled.len());
            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        if cmd.cancels.is_empty() {
            return Ok(());
        }

        let mut venue_to_order: Vec<(String, OrderAny)> = Vec::new();

        for c in &cmd.cancels {
            if let Some(order) = self.core.cache().order(&c.client_order_id)
                && let Some(vid) = order.venue_order_id()
            {
                venue_to_order.push((vid.to_string(), order.clone()));
            }
        }

        if venue_to_order.is_empty() {
            log::warn!("No venue order IDs found for batch cancel");
            return Ok(());
        }

        let order_ids: Vec<String> = venue_to_order.iter().map(|(id, _)| id.clone()).collect();
        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("batch_cancel_orders", async move {
            let order_id_refs: Vec<&str> = order_ids.iter().map(String::as_str).collect();
            let response = submitter
                .cancel_orders(&order_id_refs)
                .await
                .context("failed to batch cancel orders")?;

            for (venue_id_str, order) in &venue_to_order {
                let vid = VenueOrderId::from(venue_id_str.as_str());
                process_cancel_result(&response, venue_id_str, order, vid, &emitter, clock);
            }

            log::info!("Batch canceled {} orders", response.canceled.len());
            Ok(())
        });

        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let signature_type = self.config.signature_type;

        self.spawn_task("query_account", async move {
            fetch_and_emit_account_state(&http_client, &emitter, clock, signature_type).await
        });
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        log::debug!("Querying order: client_order_id={}", cmd.client_order_id);

        let venue_order_id = match &cmd.venue_order_id {
            Some(id) => id.to_string(),
            None => {
                log::warn!("query_order requires venue_order_id for Polymarket");
                return Ok(());
            }
        };

        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let account_id = self.core.account_id;
        let cache = self.core.cache();

        let (price_prec, size_prec) = match cache.instrument(&instrument_id) {
            Some(i) => (i.price_precision(), i.size_precision()),
            None => (4, 6),
        };

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("query_order", async move {
            match http_client.get_order_optional(&venue_order_id).await {
                Ok(Some(order)) => {
                    let report = parse_order_status_report(
                        &order,
                        instrument_id,
                        account_id,
                        Some(client_order_id),
                        price_prec,
                        size_prec,
                        clock.get_time_ns(),
                    );
                    emitter.send_order_status_report(report);
                }
                Ok(None) => {
                    log::warn!("Order {venue_order_id} not found (empty response)");
                }
                Err(e) => {
                    log::warn!("Failed to query order {venue_order_id}: {e}");
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn register_external_order(
        &self,
        _client_order_id: ClientOrderId,
        _venue_order_id: VenueOrderId,
        _instrument_id: InstrumentId,
        _strategy_id: StrategyId,
        _ts_init: UnixNanos,
    ) {
    }

    fn on_instrument(&mut self, instrument: InstrumentAny) {
        let token_id = Ustr::from(instrument.raw_symbol().as_str());
        if let InstrumentAny::BinaryOption(bo) = &instrument {
            let neg_risk = bo
                .info
                .as_ref()
                .and_then(|i| i.get_bool("neg_risk"))
                .unwrap_or(false);
            self.neg_risk_index.insert(bo.id, neg_risk);
        }
        self.shared_token_instruments.insert(token_id, instrument);
    }

    fn calculate_commission(
        &self,
        instrument: &InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
    ) -> Option<Money> {
        let fee_rate = instrument_taker_fee(instrument);
        let commission = compute_commission(
            fee_rate,
            last_qty.as_decimal(),
            last_px.as_decimal(),
            liquidity_side,
        );

        Some(Money::new(commission, instrument.quote_currency()))
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        log::info!("Connecting Polymarket execution client");

        self.stopping.store(false, Ordering::Release);

        // Read instruments from global cache (populated by data client)
        self.load_instruments_from_cache();
        self.core.set_instruments_initialized();

        self.start_ws_stream().await?;

        let post_ws = async {
            self.refresh_account_state().await?;
            self.await_account_registered(30.0).await?;
            Ok::<(), anyhow::Error>(())
        };

        if let Err(e) = post_ws.await {
            log::warn!("Connect failed after WS started, tearing down: {e}");
            self.stopping.store(true, Ordering::Release);
            let _ = self.ws_client.disconnect().await;
            self.abort_pending_tasks();
            return Err(e);
        }

        self.core.set_connected();

        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        log::info!("Disconnecting Polymarket execution client");

        // Block new background work from being spawned before we drain.
        self.stopping.store(true, Ordering::Release);

        self.ws_client.disconnect().await?;

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.abort_pending_tasks();
        self.core.set_disconnected();

        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let venue_order_id = match &cmd.venue_order_id {
            Some(id) => id.to_string(),
            None => {
                log::warn!("generate_order_status_report requires venue_order_id");
                return Ok(None);
            }
        };

        let instrument_id = match cmd.instrument_id {
            Some(id) => id,
            None => {
                log::warn!("generate_order_status_report requires instrument_id");
                return Ok(None);
            }
        };

        let order = match self
            .http_client
            .get_order_optional(&venue_order_id)
            .await
            .context("failed to fetch order")?
        {
            Some(o) => o,
            None => {
                log::info!("Order {venue_order_id} not found (empty response)");
                return Ok(None);
            }
        };

        let instrument = self.core.cache().instrument(&instrument_id).cloned();
        let (price_prec, size_prec) = match &instrument {
            Some(i) => (i.price_precision(), i.size_precision()),
            None => (4, 6),
        };

        let report = parse_order_status_report(
            &order,
            instrument_id,
            self.core.account_id,
            cmd.client_order_id,
            price_prec,
            size_prec,
            self.clock.get_time_ns(),
        );

        Ok(Some(report))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let params = crate::http::query::GetOrdersParams::default();
        let orders = self
            .http_client
            .get_orders(params)
            .await
            .context("failed to fetch orders")?;

        let (reports, _) = reconciliation::build_order_reports_from_orders(
            &orders,
            &self.shared_token_instruments,
            self.core.account_id,
            cmd.instrument_id,
            self.clock.get_time_ns(),
        );

        let reports = if cmd.open_only {
            reports
                .into_iter()
                .filter(|r| r.order_status.is_open())
                .collect()
        } else {
            reports
        };

        log::info!("Generated {} order status reports", reports.len());
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let trades = self
            .http_client
            .get_trades(GetTradesParams::default())
            .await
            .context("failed to fetch trades")?;

        let ctx = self.fill_context();
        let (reports, _) = build_fill_reports_from_trades(
            &trades,
            &ctx,
            &self.shared_token_instruments,
            cmd.instrument_id,
            self.clock.get_time_ns(),
        );

        let reports = apply_fill_filters(reports, cmd.venue_order_id, cmd.start, cmd.end);

        log::info!("Generated {} fill reports", reports.len());
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let ctx = self.fill_context();
        let positions = self
            .data_api_client
            .get_positions(ctx.user_address)
            .await
            .context("failed to fetch positions from Data API")?;

        let ts_now = self.clock.get_time_ns();
        let mut reports = build_position_reports(&positions, self.core.account_id, ts_now);

        if let Some(ref filter_id) = cmd.instrument_id {
            reports.retain(|r| &r.instrument_id == filter_id);
        }

        log::info!("Generated {} position status reports", reports.len());
        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ctx = self.fill_context();
        reconciliation::generate_mass_status(
            &self.http_client,
            &self.data_api_client,
            &self.shared_token_instruments,
            &ctx,
            self.core.client_id,
            self.core.venue,
            lookback_mins,
        )
        .await
    }
}

fn process_cancel_result(
    response: &CancelResponse,
    venue_order_id_str: &str,
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    if let Some(reason_opt) = response.not_canceled.get(venue_order_id_str) {
        let reason = reason_opt.as_deref().unwrap_or("unknown reason");
        match CancelOutcome::classify(reason) {
            CancelOutcome::AlreadyDone => {
                log::info!(
                    "Cancel rejected for {}: {reason} - awaiting WS for terminal state",
                    order.client_order_id()
                );
            }
            CancelOutcome::Rejected(msg) => {
                let ts_now = clock.get_time_ns();
                emitter.emit_order_cancel_rejected(order, Some(venue_order_id), &msg, ts_now);
            }
        }
    }
}

#[expect(clippy::too_many_arguments)]
async fn handle_batch_order_responses(
    responses: Vec<OrderResponse>,
    batch_orders: Vec<BatchLimitOrderContext>,
    submitter: &OrderSubmitter,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    pending_fills: &Arc<Mutex<FifoCacheMap<VenueOrderId, Vec<FillReport>, 1_000>>>,
    pending_order_reports: &Arc<Mutex<FifoCacheMap<VenueOrderId, Vec<OrderStatusReport>, 1_000>>>,
    pending_cancels: &Arc<Mutex<AHashSet<ClientOrderId>>>,
    pending_tasks: &Arc<Mutex<Vec<JoinHandle<()>>>>,
    stopping: &Arc<AtomicBool>,
    account_id: AccountId,
) {
    let response_len = responses.len();
    let order_len = batch_orders.len();

    if response_len != order_len {
        log::warn!(
            "Batch submit response length ({response_len}) does not match order count ({order_len})"
        );
    }

    // Polymarket batch responses do not include a client-side correlation key.
    // We map entries by submission order and rely on the API preserving array order.
    // Reference: https://docs.polymarket.com/#create-and-place-multiple-orders
    let mut deferred = Vec::new();

    for (batch_order, response) in batch_orders.iter().zip(responses) {
        if let Some((order_id_str, venue_order_id)) = handle_order_response(
            Ok(response),
            &batch_order.order,
            emitter,
            clock,
            fill_tracker,
            pending_fills,
            pending_order_reports,
            pending_cancels,
            account_id,
            batch_order.size_precision,
            batch_order.price_precision,
        ) {
            deferred.push((batch_order.order.clone(), order_id_str, venue_order_id));
        }
    }

    if order_len > response_len {
        for batch_order in batch_orders.iter().skip(response_len) {
            reject_submit_order(
                &batch_order.order,
                "Order not included in API response",
                emitter,
                clock,
                pending_cancels,
            );
        }
    }

    // Spawn deferred cancels as independent tasks so retrying cancels cannot stall
    // terminal-event emission or delay posting subsequent chunks. Handles are tracked
    // in pending_tasks so client shutdown aborts them like any other background work.
    // Holding the pending_tasks lock across the spawn loop (and the stopping check)
    // closes the race with stop(): abort_pending_tasks() blocks on the same lock,
    // so either all new handles are enqueued before the drain runs, or stopping has
    // already been observed and no new handles are spawned.
    if !deferred.is_empty() {
        let mut tasks = pending_tasks.lock().expect(MUTEX_POISONED);

        if stopping.load(Ordering::Acquire) {
            return;
        }
        tasks.retain(|handle| !handle.is_finished());

        for (order, order_id_str, venue_order_id) in deferred {
            let submitter = submitter.clone();
            let emitter = emitter.clone();

            let handle = get_runtime().spawn(async move {
                execute_deferred_cancel(
                    &submitter,
                    &order,
                    &order_id_str,
                    venue_order_id,
                    &emitter,
                    clock,
                )
                .await;
            });
            tasks.push(handle);
        }
    }
}

fn reject_submit_order(
    order: &OrderAny,
    reason: &str,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    pending_cancels: &Arc<Mutex<AHashSet<ClientOrderId>>>,
) {
    let ts_now = clock.get_time_ns();
    emitter.emit_order_rejected(order, reason, ts_now, false);
    pending_cancels
        .lock()
        .expect(MUTEX_POISONED)
        .remove(&order.client_order_id());
}

#[expect(clippy::too_many_arguments)]
async fn handle_single_order_response(
    result: anyhow::Result<OrderResponse>,
    batch_order: BatchLimitOrderContext,
    submitter: &OrderSubmitter,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    pending_fills: &Arc<Mutex<FifoCacheMap<VenueOrderId, Vec<FillReport>, 1_000>>>,
    pending_order_reports: &Arc<Mutex<FifoCacheMap<VenueOrderId, Vec<OrderStatusReport>, 1_000>>>,
    pending_cancels: &Arc<Mutex<AHashSet<ClientOrderId>>>,
    account_id: AccountId,
) {
    match result {
        Ok(response) => {
            if let Some((order_id_str, venue_order_id)) = handle_order_response(
                Ok(response),
                &batch_order.order,
                emitter,
                clock,
                fill_tracker,
                pending_fills,
                pending_order_reports,
                pending_cancels,
                account_id,
                batch_order.size_precision,
                batch_order.price_precision,
            ) {
                execute_deferred_cancel(
                    submitter,
                    &batch_order.order,
                    &order_id_str,
                    venue_order_id,
                    emitter,
                    clock,
                )
                .await;
            }
        }
        Err(e) => {
            reject_submit_order(
                &batch_order.order,
                &format!("{e}"),
                emitter,
                clock,
                pending_cancels,
            );
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_order_response(
    result: crate::http::error::Result<OrderResponse>,
    order: &OrderAny,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    pending_fills: &Arc<Mutex<FifoCacheMap<VenueOrderId, Vec<FillReport>, 1_000>>>,
    pending_order_reports: &Arc<Mutex<FifoCacheMap<VenueOrderId, Vec<OrderStatusReport>, 1_000>>>,
    pending_cancels: &Arc<Mutex<AHashSet<ClientOrderId>>>,
    account_id: AccountId,
    size_precision: u8,
    price_precision: u8,
) -> Option<(String, VenueOrderId)> {
    match result {
        Ok(response) => {
            if response.success {
                if let Some(order_id) = response.order_id {
                    let venue_order_id = VenueOrderId::from(order_id.as_str());
                    let ts_now = clock.get_time_ns();
                    emitter.emit_order_accepted(order, venue_order_id, ts_now);

                    // Register order in fill tracker for dust detection
                    fill_tracker.register(
                        venue_order_id,
                        order.quantity(),
                        order.order_side(),
                        order.instrument_id(),
                        size_precision,
                        price_precision,
                    );

                    // Drain any fills buffered during the HTTP round-trip,
                    // snapping dust fills and recording in tracker
                    if let Some(buffered) = pending_fills
                        .lock()
                        .expect(MUTEX_POISONED)
                        .remove(&venue_order_id)
                    {
                        for mut fill in buffered {
                            fill.last_qty =
                                fill_tracker.snap_fill_qty(&venue_order_id, fill.last_qty);
                            fill_tracker.record_fill(
                                &venue_order_id,
                                fill.last_qty.as_f64(),
                                fill.last_px.as_f64(),
                                fill.ts_event,
                            );
                            emitter.send_fill_report(fill);
                        }
                    }

                    // Drain any order reports buffered during the HTTP round-trip
                    if let Some(buffered) = pending_order_reports
                        .lock()
                        .expect(MUTEX_POISONED)
                        .remove(&venue_order_id)
                    {
                        let mut has_filled = false;

                        for report in &buffered {
                            if report.order_status == OrderStatus::Filled {
                                has_filled = true;
                            }
                        }

                        // Cap filled_qty to tracked fills to prevent
                        // duplicate inferred fills from the race with trades
                        let tracked_filled = fill_tracker
                            .get_cumulative_filled(&venue_order_id)
                            .unwrap_or(0.0);
                        let tracked_qty = Quantity::new(tracked_filled, size_precision);

                        for mut report in buffered {
                            if report.filled_qty > tracked_qty {
                                log::debug!(
                                    "Capping buffered filled_qty for {venue_order_id} \
                                     from {} to {} (awaiting trade messages)",
                                    report.filled_qty,
                                    tracked_qty,
                                );
                                report.filled_qty = tracked_qty;
                            }
                            emitter.send_order_status_report(report);
                        }

                        // If a MATCHED (Filled) status was buffered, check for dust residual
                        if has_filled {
                            let fallback_px = order.price().map_or(0.0, |p| p.as_f64());
                            if let Some(dust_fill) = fill_tracker.check_dust_and_build_fill(
                                &venue_order_id,
                                account_id,
                                &order_id,
                                fallback_px,
                                get_pusd_currency(),
                                ts_now,
                                ts_now,
                            ) {
                                emitter.send_fill_report(dust_fill);
                            }
                        }
                    }

                    // Check if cancel was requested during the HTTP round-trip
                    if pending_cancels
                        .lock()
                        .expect(MUTEX_POISONED)
                        .remove(&order.client_order_id())
                    {
                        log::info!(
                            "Order {} has pending cancel, issuing deferred cancel for {}",
                            order.client_order_id(),
                            venue_order_id
                        );
                        return Some((order_id, venue_order_id));
                    }
                } else {
                    log::warn!(
                        "Order accepted but no order_id returned for {}",
                        order.client_order_id()
                    );
                }
            } else {
                let reason = response
                    .error_msg
                    .unwrap_or_else(|| "unknown error".to_string());
                let ts_now = clock.get_time_ns();
                emitter.emit_order_rejected(order, &reason, ts_now, false);
                pending_cancels
                    .lock()
                    .expect(MUTEX_POISONED)
                    .remove(&order.client_order_id());
            }
        }
        Err(e) => {
            let ts_now = clock.get_time_ns();
            emitter.emit_order_rejected(order, &format!("HTTP request failed: {e}"), ts_now, false);
            pending_cancels
                .lock()
                .expect(MUTEX_POISONED)
                .remove(&order.client_order_id());
        }
    }
    None
}

async fn execute_deferred_cancel(
    submitter: &OrderSubmitter,
    order: &OrderAny,
    order_id_str: &str,
    venue_order_id: VenueOrderId,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    match submitter.cancel_order(order_id_str).await {
        Ok(response) => {
            process_cancel_result(
                &response,
                order_id_str,
                order,
                venue_order_id,
                emitter,
                clock,
            );
        }
        Err(e) => {
            let ts_now = clock.get_time_ns();
            emitter.emit_order_cancel_rejected(
                order,
                Some(venue_order_id),
                &format!("Deferred cancel failed: {e}"),
                ts_now,
            );
        }
    }
}

/// Deferred FOK status check.
///
/// Waits 5 seconds then queries the CLOB REST API for the order status.
/// If the order has reached a terminal state that the WS stream missed
/// (e.g. UNMATCHED for an unfilled FOK), emits an order status report
/// so the engine can reconcile it.
#[expect(clippy::too_many_arguments)]
async fn check_fok_status(
    submitter: &OrderSubmitter,
    order_id: &str,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    instrument_id: InstrumentId,
    order_side: OrderSide,
    size_precision: u8,
    price_precision: u8,
    clock: &'static AtomicTime,
) {
    const FOK_CHECK_DELAY: Duration = Duration::from_secs(5);

    tokio::time::sleep(FOK_CHECK_DELAY).await;

    let venue_order_id = VenueOrderId::from(order_id);
    if fill_tracker.has_fills_or_settled(&venue_order_id) {
        return;
    }

    log::info!("FOK order {order_id} unresolved after 5s, checking REST status");

    let venue_order = match submitter.get_order(order_id).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            log::info!("FOK order {order_id} not found (empty response), WS will reconcile");
            return;
        }
        Err(e) => {
            log::warn!("FOK status check failed for {order_id}: {e}");
            return;
        }
    };

    let order_status = OrderStatus::from(venue_order.status);

    if !matches!(
        order_status,
        OrderStatus::Rejected | OrderStatus::Canceled | OrderStatus::Expired | OrderStatus::Filled
    ) {
        return;
    }

    let quantity = Quantity::new(
        venue_order
            .original_size
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.0),
        size_precision,
    );
    let filled_qty = Quantity::new(
        venue_order
            .size_matched
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.0),
        size_precision,
    );
    let price = Price::new(
        venue_order.price.to_string().parse::<f64>().unwrap_or(0.0),
        price_precision,
    );

    let ts_now = clock.get_time_ns();
    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None,
        venue_order_id,
        order_side,
        OrderType::Limit,
        TimeInForce::Ioc,
        order_status,
        quantity,
        filled_qty,
        ts_now,
        ts_now,
        ts_now,
        None,
    );
    report.price = Some(price);

    log::info!("FOK order {order_id} resolved via REST as {order_status:?}");

    emitter.send_order_status_report(report);
}

pub fn get_pusd_currency() -> Currency {
    Currency::pUSD()
}

async fn fetch_and_emit_account_state(
    http_client: &PolymarketClobHttpClient,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    signature_type: SignatureType,
) -> anyhow::Result<()> {
    use anyhow::Context;

    let params = GetBalanceAllowanceParams {
        asset_type: Some(crate::http::query::AssetType::Collateral),
        signature_type: Some(signature_type),
        ..Default::default()
    };

    let balance_allowance = http_client
        .get_balance_allowance(params)
        .await
        .context("failed to fetch balance allowance")?;

    let pusd = get_pusd_currency();
    let account_balance = parse_balance_allowance(balance_allowance.balance, pusd)
        .context("failed to parse balance allowance")?;

    let ts_event = clock.get_time_ns();
    log::info!(
        "Account state updated: balance={} pUSD",
        account_balance.total
    );
    emitter.emit_account_state(vec![account_balance], vec![], true, ts_event);
    Ok(())
}

/// Fetches the user's pUSD collateral balance as a `Decimal`. Mirrors
/// [`fetch_and_emit_account_state`] but returns the value directly so the
/// market-BUY fee-adjustment path can size against a fresh balance.
async fn fetch_collateral_balance_pusd(
    http_client: &PolymarketClobHttpClient,
    signature_type: SignatureType,
) -> anyhow::Result<Decimal> {
    use anyhow::Context;

    let params = GetBalanceAllowanceParams {
        asset_type: Some(crate::http::query::AssetType::Collateral),
        signature_type: Some(signature_type),
        ..Default::default()
    };

    let balance_allowance = http_client
        .get_balance_allowance(params)
        .await
        .context("failed to fetch balance allowance")?;

    // The API returns balances as integer micro-pUSD (e.g. `20000000` = 20 pUSD).
    let usdc_scale = Decimal::from(1_000_000u32);
    Ok(balance_allowance.balance / usdc_scale)
}
