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
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use ahash::{AHashMap, AHashSet};
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
    providers::InstrumentProvider,
};
use nautilus_core::{
    MUTEX_POISONED, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, CurrencyType, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderEventAny, OrderUpdated},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Price, Quantity},
};
use nautilus_network::retry::RetryConfig;
use tokio::task::JoinHandle;

use self::{
    order_builder::PolymarketOrderBuilder,
    order_fill_tracker::OrderFillTrackerMap,
    parse::{parse_balance_allowance, parse_order_status_report},
    reconciliation::{FillContext, apply_fill_filters, build_fill_reports_from_trades},
    submitter::OrderSubmitter,
    types::CancelOutcome,
};
use crate::{
    common::{
        consts::{POLYMARKET_VENUE, USDC},
        credential::Secrets,
        enums::SignatureType,
    },
    config::PolymarketExecClientConfig,
    filters::InstrumentFilter,
    http::{
        clob::PolymarketClobHttpClient,
        gamma::PolymarketGammaHttpClient,
        query::{CancelResponse, GetBalanceAllowanceParams, GetTradesParams, OrderResponse},
    },
    providers::PolymarketInstrumentProvider,
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
    submitter: OrderSubmitter,
    ws_client: PolymarketWebSocketClient,
    provider: PolymarketInstrumentProvider,
    secrets: Secrets,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    ws_stream_handle: Mutex<Option<JoinHandle<()>>>,
    neg_risk_index: AHashMap<InstrumentId, bool>,
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
    #[allow(clippy::too_many_arguments)]
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
            Some(config.http_timeout_secs),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create CLOB HTTP client")?;

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
        let submitter =
            OrderSubmitter::new(http_client.clone(), order_builder, retry_config.clone());

        let ws_client = PolymarketWebSocketClient::new_user(
            config.base_url_ws.clone(),
            secrets.credential.clone(),
        );

        let gamma_http = PolymarketGammaHttpClient::new(
            config.base_url_gamma.clone(),
            Some(config.http_timeout_secs),
            retry_config,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create Gamma HTTP client")?;
        let provider = PolymarketInstrumentProvider::new(gamma_http);

        let clock = get_atomic_clock_realtime();
        let usdc = get_usdc_currency();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Cash,
            Some(usdc),
        );

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            submitter,
            ws_client,
            provider,
            secrets,
            pending_tasks: Mutex::new(Vec::new()),
            ws_stream_handle: Mutex::new(None),
            neg_risk_index: AHashMap::new(),
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

    /// Adds an instrument filter on the underlying provider.
    pub fn add_instrument_filter(&mut self, filter: Arc<dyn InstrumentFilter>) {
        self.provider.add_filter(filter);
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
        let token_instruments = self.provider.build_token_map();
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
            .get(instrument_id)
            .copied()
            .unwrap_or(false)
    }

    fn build_neg_risk_index(&mut self) {
        self.neg_risk_index.clear();

        for instrument in self.provider.store().list_all() {
            if let InstrumentAny::BinaryOption(inst) = instrument {
                let neg_risk = inst
                    .info
                    .as_ref()
                    .and_then(|info| info.get_bool("neg_risk"))
                    .unwrap_or(false);
                self.neg_risk_index.insert(inst.id, neg_risk);
            }
        }
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
                    let ts = clock.get_time_ns();
                    emitter.emit_order_rejected(&order, &format!("{e}"), ts, false);
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

        self.spawn_task("submit_market_order", async move {
            match submitter
                .submit_market_order(&token_id, side, amount, neg_risk, tick_decimals)
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
                             (expected from book walk)",
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
                    let ts = clock.get_time_ns();
                    emitter.emit_order_rejected(&order, &format!("{e}"), ts, false);
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

    /// Builds the shared fill context from client state.
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
            usdc: get_usdc_currency(),
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

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.abort_pending_tasks();

        if self.core.is_connected() {
            let runtime = get_runtime();
            runtime.block_on(async {
                if let Err(e) = self.ws_client.disconnect().await {
                    log::warn!("Error disconnecting WebSocket client: {e}");
                }
            });
        }

        self.core.set_disconnected();
        self.core.set_stopped();

        log::info!("Polymarket execution client stopped");
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
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

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        for (i, order_init) in cmd.order_inits.iter().enumerate() {
            let submit = SubmitOrder::new(
                cmd.trader_id,
                cmd.client_id,
                cmd.strategy_id,
                cmd.instrument_id,
                order_init.client_order_id,
                cmd.order_inits[i].clone(),
                cmd.exec_algorithm_id,
                cmd.position_id,
                cmd.params.clone(),
                UUID4::new(),
                self.clock.get_time_ns(),
            );

            if let Err(e) = self.submit_order(&submit) {
                log::warn!(
                    "Failed to submit order {} from list: {e}",
                    order_init.client_order_id
                );
            }
        }
        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        let order = self.core.cache().order(&cmd.client_order_id).cloned();
        if let Some(order) = order {
            let venue_order_id = order.venue_order_id();
            let ts = self.clock.get_time_ns();
            self.emitter.emit_order_modify_rejected(
                &order,
                venue_order_id,
                "Order modification not supported on Polymarket",
                ts,
            );
        }
        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
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
            let ts = self.clock.get_time_ns();
            self.emitter.emit_order_cancel_rejected(
                order_ref,
                order_ref.venue_order_id(),
                &format!("Order is not open (status: {:?})", order_ref.status()),
                ts,
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
                    let ts = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected(
                        &order_clone,
                        Some(venue_order_id),
                        &format!("HTTP request failed: {e}"),
                        ts,
                    );
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
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

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
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

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        let runtime = get_runtime();
        runtime.block_on(async {
            if let Err(e) = self.refresh_account_state().await {
                log::warn!("Failed to query account state: {e}");
            }
        });
        Ok(())
    }

    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
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

        let runtime = get_runtime();
        let http_client = &self.http_client;
        let emitter = &self.emitter;
        let clock = self.clock;

        runtime.block_on(async {
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

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        log::info!("Connecting Polymarket execution client");

        self.provider
            .load_all(None::<&HashMap<String, String>>)
            .await
            .context("failed to load instruments")?;
        self.build_neg_risk_index();
        self.core.set_instruments_initialized();

        self.start_ws_stream().await?;

        let post_ws = async {
            self.refresh_account_state().await?;
            self.await_account_registered(30.0).await?;
            Ok::<(), anyhow::Error>(())
        };

        if let Err(e) = post_ws.await {
            log::warn!("Connect failed after WS started, tearing down: {e}");
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

        self.ws_client.disconnect().await?;
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
            &self.provider,
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
            &self.provider,
            cmd.instrument_id,
            self.clock.get_time_ns(),
        );

        let reports = apply_fill_filters(reports, cmd.venue_order_id, cmd.start, cmd.end);

        log::info!("Generated {} fill reports", reports.len());
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        Ok(vec![])
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ctx = self.fill_context();
        reconciliation::generate_mass_status(
            &self.http_client,
            &self.provider,
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
                let ts = clock.get_time_ns();
                emitter.emit_order_cancel_rejected(order, Some(venue_order_id), &msg, ts);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
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
                    let ts = clock.get_time_ns();
                    emitter.emit_order_accepted(order, venue_order_id, ts);

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
                        for report in buffered {
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
                                get_usdc_currency(),
                                ts,
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
                let ts = clock.get_time_ns();
                emitter.emit_order_rejected(order, &reason, ts, false);
                pending_cancels
                    .lock()
                    .expect(MUTEX_POISONED)
                    .remove(&order.client_order_id());
            }
        }
        Err(e) => {
            let ts = clock.get_time_ns();
            emitter.emit_order_rejected(order, &format!("HTTP request failed: {e}"), ts, false);
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
            let ts = clock.get_time_ns();
            emitter.emit_order_cancel_rejected(
                order,
                Some(venue_order_id),
                &format!("Deferred cancel failed: {e}"),
                ts,
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
#[allow(clippy::too_many_arguments)]
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

    let ts = clock.get_time_ns();
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
        ts,
        ts,
        ts,
        None,
    );
    report.price = Some(price);

    log::info!("FOK order {order_id} resolved via REST as {order_status:?}");

    emitter.send_order_status_report(report);
}

pub fn get_usdc_currency() -> Currency {
    Currency::try_from_str(USDC)
        .unwrap_or_else(|| Currency::new(USDC, 6, 0, USDC, CurrencyType::Crypto))
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

    let usdc = get_usdc_currency();
    let account_balance = parse_balance_allowance(balance_allowance.balance, usdc)
        .context("failed to parse balance allowance")?;

    let ts_event = clock.get_time_ns();
    log::info!(
        "Account state updated: balance={} USDC",
        account_balance.total
    );
    emitter.emit_account_state(vec![account_balance], vec![], true, ts_event);
    Ok(())
}
