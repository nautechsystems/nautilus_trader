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

pub mod parse;

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    cache::fifo::FifoCache,
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
    enums::{
        AccountType, CurrencyType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType,
        TimeInForce,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use ustr::Ustr;

use self::parse::{
    build_maker_fill_report, compute_commission, compute_maker_taker_amounts, determine_order_side,
    make_composite_trade_id, parse_fill_report, parse_liquidity_side, parse_order_status_report,
    parse_timestamp,
};
use crate::{
    common::{
        consts::{CANCEL_ALREADY_DONE, POLYMARKET_VENUE, USDC},
        credential::Secrets,
        enums::{
            PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOrderType,
            PolymarketTradeStatus, SignatureType,
        },
        models::PolymarketMakerOrder,
    },
    config::PolymarketExecClientConfig,
    http::{
        client::{PolymarketHttpClient, PolymarketRawHttpClient},
        models::PolymarketOrder,
        query::{GetBalanceAllowanceParams, GetOrdersParams, GetTradesParams},
    },
    providers::PolymarketInstrumentProvider,
    signing::eip712::OrderSigner,
    websocket::{
        client::PolymarketWebSocketClient,
        messages::{PolymarketUserOrder, PolymarketUserTrade, PolymarketWsMessage, UserWsMessage},
    },
};

/// Live execution client for the Polymarket prediction market.
#[derive(Debug)]
pub struct PolymarketExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: PolymarketExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: PolymarketRawHttpClient,
    order_signer: OrderSigner,
    ws_client: PolymarketWebSocketClient,
    provider: PolymarketInstrumentProvider,
    secrets: Secrets,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    ws_stream_handle: Mutex<Option<JoinHandle<()>>>,
    neg_risk_index: AHashMap<InstrumentId, bool>,
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

        let http_client = PolymarketRawHttpClient::with_credential(
            secrets.credential.clone(),
            secrets.address.clone(),
            config.base_url_http.clone(),
            None, // gamma not needed for execution HTTP client
            Some(config.http_timeout_secs),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create HTTP client")?;

        let order_signer =
            OrderSigner::new(&secrets.private_key).context("failed to create order signer")?;

        let ws_client = PolymarketWebSocketClient::new_user(
            config.base_url_ws.clone(),
            secrets.credential.clone(),
        );

        let domain_http = PolymarketHttpClient::new(
            config.base_url_http.clone(),
            config.base_url_gamma.clone(),
            Some(config.http_timeout_secs),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create domain HTTP client")?;
        let provider = PolymarketInstrumentProvider::new(domain_http);

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
            order_signer,
            ws_client,
            provider,
            secrets,
            pending_tasks: Mutex::new(Vec::new()),
            ws_stream_handle: Mutex::new(None),
            neg_risk_index: AHashMap::new(),
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

        let handle = get_runtime().spawn(async move {
            let mut processed_fills: FifoCache<String, 10_000> = FifoCache::default();

            loop {
                match rx.recv().await {
                    Some(PolymarketWsMessage::User(user_msg)) => match user_msg {
                        UserWsMessage::Order(order) => {
                            let instrument = match token_instruments.get(&order.asset_id) {
                                Some(i) => i,
                                None => {
                                    log::warn!(
                                        "Unknown asset_id in order update: {}",
                                        order.asset_id
                                    );
                                    continue;
                                }
                            };
                            let ts_event = parse_timestamp_ms_str(&order.timestamp)
                                .unwrap_or_else(|| clock.get_time_ns());
                            let report =
                                build_ws_order_report(&order, instrument, account_id, ts_event);
                            emitter.send_order_status_report(report);
                        }
                        UserWsMessage::Trade(trade) => {
                            if !trade.status.is_finalized()
                                && !matches!(trade.status, PolymarketTradeStatus::Matched)
                            {
                                log::debug!(
                                    "Skipping trade with status {:?}: {}",
                                    trade.status,
                                    trade.id
                                );
                                continue;
                            }

                            let dedup_key = format!("{}-{}", trade.id, trade.taker_order_id);
                            let is_duplicate = processed_fills.contains(&dedup_key);

                            if trade.status.is_finalized() {
                                spawn_account_refresh(
                                    http_client.clone(),
                                    emitter.clone(),
                                    clock,
                                    account_id,
                                    signature_type,
                                );
                            }

                            if is_duplicate {
                                log::debug!("Duplicate fill skipped: {dedup_key}");
                                continue;
                            }
                            processed_fills.add(dedup_key.clone());

                            let is_maker = trade.trader_side == PolymarketLiquiditySide::Maker;
                            let liquidity_side = parse_liquidity_side(trade.trader_side);
                            let ts_event = parse_timestamp_ms_str(&trade.timestamp)
                                .unwrap_or_else(|| clock.get_time_ns());

                            if is_maker {
                                let user_orders: Vec<_> = trade
                                    .maker_orders
                                    .iter()
                                    .filter(|mo| {
                                        mo.maker_address == user_address || mo.owner == user_api_key
                                    })
                                    .collect();

                                if user_orders.is_empty() {
                                    log::warn!(
                                        "No matching maker orders for user in trade: {}",
                                        trade.id
                                    );
                                    continue;
                                }

                                for mo in user_orders {
                                    let asset_id = Ustr::from(mo.asset_id.as_str());
                                    let instrument = match token_instruments.get(&asset_id) {
                                        Some(i) => i,
                                        None => {
                                            log::warn!(
                                                "Unknown asset_id in maker order: {asset_id}"
                                            );
                                            continue;
                                        }
                                    };
                                    let report = build_ws_maker_fill_report(
                                        mo,
                                        &trade,
                                        instrument,
                                        account_id,
                                        liquidity_side,
                                        ts_event,
                                    );
                                    emitter.send_fill_report(report);
                                }
                            } else {
                                let instrument = match token_instruments.get(&trade.asset_id) {
                                    Some(i) => i,
                                    None => {
                                        log::warn!("Unknown asset_id in trade: {}", trade.asset_id);
                                        continue;
                                    }
                                };
                                let report = build_ws_taker_fill_report(
                                    &trade,
                                    instrument,
                                    account_id,
                                    liquidity_side,
                                    ts_event,
                                );
                                emitter.send_fill_report(report);
                            }
                        }
                    },
                    Some(PolymarketWsMessage::Market(_)) => {
                        // Market messages are not expected on the user channel
                    }
                    Some(PolymarketWsMessage::Reconnected) => {
                        log::info!("User WebSocket reconnected");
                    }
                    None => {
                        log::warn!("User WebSocket stream ended");
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
        use nautilus_model::instruments::InstrumentAny;

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

        if order.is_reduce_only() {
            self.emitter
                .emit_order_denied(&order, "Reduce-only orders not supported on Polymarket");
            return Ok(());
        }

        if order.order_type() != OrderType::Limit {
            self.emitter.emit_order_denied(
                &order,
                &format!(
                    "Unsupported order type for Polymarket: {:?}",
                    order.order_type()
                ),
            );
            return Ok(());
        }

        if order.is_quote_quantity() {
            self.emitter
                .emit_order_denied(&order, "Quote quantity not supported for limit orders");
            return Ok(());
        }

        let price = match order.price() {
            Some(p) => p,
            None => {
                self.emitter
                    .emit_order_denied(&order, "Limit orders require a price");
                return Ok(());
            }
        };

        let poly_order_type = match PolymarketOrderType::try_from(order.time_in_force()) {
            Ok(t) => t,
            Err(e) => {
                self.emitter
                    .emit_order_denied(&order, &format!("Unsupported time in force: {e}"));
                return Ok(());
            }
        };

        if order.is_post_only()
            && !matches!(order.time_in_force(), TimeInForce::Gtc | TimeInForce::Gtd)
        {
            self.emitter
                .emit_order_denied(&order, "Post-only orders require GTC or GTD time in force");
            return Ok(());
        }

        let poly_side = match PolymarketOrderSide::try_from(order.order_side()) {
            Ok(s) => s,
            Err(e) => {
                self.emitter
                    .emit_order_denied(&order, &format!("Invalid order side: {e}"));
                return Ok(());
            }
        };

        let instrument = self
            .core
            .cache()
            .instrument(&order.instrument_id())
            .cloned();
        let instrument = match instrument {
            Some(i) => i,
            None => {
                self.emitter.emit_order_denied(
                    &order,
                    &format!("Instrument not found: {}", order.instrument_id()),
                );
                return Ok(());
            }
        };

        let neg_risk = self.get_neg_risk(&order.instrument_id());
        let token_id = instrument.raw_symbol();
        let post_only = order.is_post_only();

        let price_dec = price.as_decimal();
        let qty_dec = order.quantity().as_decimal();
        let (maker_amount, taker_amount) =
            compute_maker_taker_amounts(price_dec, qty_dec, poly_side);

        let salt: u64 = {
            let bytes = uuid::Uuid::new_v4().into_bytes();
            u64::from_le_bytes(bytes[..8].try_into().unwrap()) & ((1u64 << 53) - 1)
        };

        let signer_address = self.secrets.address.clone();
        let maker_address = self
            .secrets
            .funder
            .clone()
            .unwrap_or_else(|| signer_address.clone());
        let signature_type = self.config.signature_type;

        let expiration = match order.expire_time() {
            Some(ns) if ns.as_u64() > 0 => {
                let secs = ns.as_u64() / 1_000_000_000;
                secs.to_string()
            }
            _ => "0".to_string(),
        };

        let mut poly_order = PolymarketOrder {
            salt,
            maker: maker_address,
            signer: signer_address,
            taker: "0x0000000000000000000000000000000000000000".to_string(),
            token_id: Ustr::from(token_id.as_str()),
            maker_amount,
            taker_amount,
            expiration,
            nonce: "0".to_string(),
            fee_rate_bps: Decimal::ZERO,
            side: poly_side,
            signature_type,
            signature: String::new(),
        };

        let signature = match self.order_signer.sign_order(&poly_order, neg_risk) {
            Ok(sig) => sig,
            Err(e) => {
                self.emitter
                    .emit_order_denied(&order, &format!("EIP-712 signing failed: {e}"));
                return Ok(());
            }
        };
        poly_order.signature = signature;

        self.emitter.emit_order_submitted(&order);

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("submit_order", async move {
            match http_client
                .post_order(&poly_order, poly_order_type, post_only)
                .await
            {
                Ok(response) => {
                    if response.success {
                        if let Some(order_id) = response.order_id {
                            let venue_order_id = VenueOrderId::from(order_id.as_str());
                            let ts = clock.get_time_ns();
                            emitter.emit_order_accepted(&order, venue_order_id, ts);
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
                        emitter.emit_order_rejected(&order, &reason, ts, false);
                    }
                }
                Err(e) => {
                    let ts = clock.get_time_ns();
                    emitter.emit_order_rejected(
                        &order,
                        &format!("HTTP request failed: {e}"),
                        ts,
                        false,
                    );
                }
            }
            Ok(())
        });

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
            return Ok(());
        }

        let venue_order_id = match order_ref.venue_order_id() {
            Some(id) => id,
            None => {
                log::warn!("No venue_order_id for cancel: {}", cmd.client_order_id);
                return Ok(());
            }
        };

        let order_id_str = venue_order_id.to_string();
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let order_clone = order.unwrap();

        self.spawn_task("cancel_order", async move {
            match http_client.cancel_order(&order_id_str).await {
                Ok(response) => {
                    if let Some(reason) = response.not_canceled {
                        if reason.contains(CANCEL_ALREADY_DONE) {
                            log::info!(
                                "Cancel rejected for {}: {reason} - awaiting WS for terminal state",
                                order_clone.client_order_id()
                            );
                        } else {
                            let ts = clock.get_time_ns();
                            emitter.emit_order_cancel_rejected(
                                &order_clone,
                                Some(venue_order_id),
                                &reason,
                                ts,
                            );
                        }
                    }
                    // Success: WS will deliver the cancellation event
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

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let orders: Vec<OrderAny> = open_orders.into_iter().cloned().collect();

        self.spawn_task("cancel_all_orders", async move {
            let order_id_refs: Vec<&str> = venue_order_ids.iter().map(String::as_str).collect();
            let response = http_client
                .cancel_orders(&order_id_refs)
                .await
                .context("failed to cancel all orders")?;

            for order in &orders {
                if let Some(vid) = order.venue_order_id() {
                    let vid_str = vid.to_string();
                    if let Some(reason_opt) = response.not_canceled.get(&vid_str) {
                        let reason = reason_opt.as_deref().unwrap_or("unknown reason");

                        if reason.contains(CANCEL_ALREADY_DONE) {
                            log::info!(
                                "Cancel rejected for {}: {reason} - awaiting WS for terminal state",
                                order.client_order_id()
                            );
                        } else {
                            let ts = clock.get_time_ns();
                            emitter.emit_order_cancel_rejected(order, Some(vid), reason, ts);
                        }
                    }
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
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("batch_cancel_orders", async move {
            let order_id_refs: Vec<&str> = order_ids.iter().map(String::as_str).collect();
            let response = http_client
                .cancel_orders(&order_id_refs)
                .await
                .context("failed to batch cancel orders")?;

            for (venue_id_str, order) in &venue_to_order {
                if let Some(reason_opt) = response.not_canceled.get(venue_id_str) {
                    let reason = reason_opt.as_deref().unwrap_or("unknown reason");

                    if reason.contains(CANCEL_ALREADY_DONE) {
                        log::info!(
                            "Cancel rejected for {}: {reason} - awaiting WS for terminal state",
                            order.client_order_id()
                        );
                    } else {
                        let ts = clock.get_time_ns();
                        let vid = VenueOrderId::from(venue_id_str.as_str());
                        emitter.emit_order_cancel_rejected(order, Some(vid), reason, ts);
                    }
                }
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

    fn query_order(&self, _cmd: &QueryOrder) -> anyhow::Result<()> {
        log::warn!("query_order not yet implemented for Polymarket");
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

        let order = self
            .http_client
            .get_order(&venue_order_id)
            .await
            .context("failed to fetch order")?;

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
        let params = GetOrdersParams::default();
        let orders = self
            .http_client
            .get_orders(params)
            .await
            .context("failed to fetch orders")?;

        let mut reports = Vec::new();
        for order in &orders {
            let token_id = Ustr::from(order.asset_id.as_str());
            let instrument = self.provider.get_by_token_id(&token_id);
            let (instrument_id, price_prec, size_prec) = match instrument {
                Some(i) => (i.id(), i.price_precision(), i.size_precision()),
                None => continue,
            };

            if let Some(filter_id) = cmd.instrument_id
                && instrument_id != filter_id
            {
                continue;
            }

            let report = parse_order_status_report(
                order,
                instrument_id,
                self.core.account_id,
                None,
                price_prec,
                size_prec,
                self.clock.get_time_ns(),
            );

            if cmd.open_only && !report.order_status.is_open() {
                continue;
            }

            reports.push(report);
        }

        log::info!("Generated {} order status reports", reports.len());
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let params = GetTradesParams::default();
        let trades = self
            .http_client
            .get_trades(params)
            .await
            .context("failed to fetch trades")?;

        let usdc = get_usdc_currency();
        let user_addr = self
            .secrets
            .funder
            .clone()
            .unwrap_or_else(|| self.secrets.address.clone());
        let api_key = self.secrets.credential.api_key().to_string();
        let mut reports = Vec::new();

        for trade in &trades {
            let is_maker = trade.trader_side == PolymarketLiquiditySide::Maker;

            if is_maker {
                for mo in &trade.maker_orders {
                    if mo.maker_address != user_addr && mo.owner != api_key {
                        continue;
                    }
                    let token_id = Ustr::from(mo.asset_id.as_str());
                    let instrument = self.provider.get_by_token_id(&token_id);
                    let (instrument_id, price_prec, size_prec) = match instrument {
                        Some(i) => (i.id(), i.price_precision(), i.size_precision()),
                        None => continue,
                    };

                    if let Some(filter_id) = cmd.instrument_id
                        && instrument_id != filter_id
                    {
                        continue;
                    }

                    let ts_event =
                        parse_timestamp(&trade.match_time).unwrap_or(self.clock.get_time_ns());
                    let report = build_maker_fill_report(
                        mo,
                        &trade.id,
                        trade.trader_side,
                        trade.side,
                        trade.asset_id.as_str(),
                        self.core.account_id,
                        instrument_id,
                        price_prec,
                        size_prec,
                        usdc,
                        LiquiditySide::Maker,
                        ts_event,
                        self.clock.get_time_ns(),
                    );
                    reports.push(report);
                }
            } else {
                let token_id = Ustr::from(trade.asset_id.as_str());
                let instrument = self.provider.get_by_token_id(&token_id);
                let (instrument_id, price_prec, size_prec) = match instrument {
                    Some(i) => (i.id(), i.price_precision(), i.size_precision()),
                    None => continue,
                };

                if let Some(filter_id) = cmd.instrument_id
                    && instrument_id != filter_id
                {
                    continue;
                }

                let report = parse_fill_report(
                    trade,
                    instrument_id,
                    self.core.account_id,
                    None,
                    price_prec,
                    size_prec,
                    usdc,
                    self.clock.get_time_ns(),
                );
                reports.push(report);
            }
        }

        let reports = if let Some(vid) = cmd.venue_order_id {
            reports
                .into_iter()
                .filter(|r| r.venue_order_id == vid)
                .collect()
        } else {
            reports
        };

        let reports = if let (Some(start), Some(end)) = (cmd.start, cmd.end) {
            reports
                .into_iter()
                .filter(|r| r.ts_event >= start && r.ts_event <= end)
                .collect()
        } else if let Some(start) = cmd.start {
            reports
                .into_iter()
                .filter(|r| r.ts_event >= start)
                .collect()
        } else if let Some(end) = cmd.end {
            reports.into_iter().filter(|r| r.ts_event <= end).collect()
        } else {
            reports
        };

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
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::warn!("generate_mass_status not yet implemented for Polymarket");
        Ok(None)
    }
}

fn get_usdc_currency() -> Currency {
    Currency::try_from_str(USDC)
        .unwrap_or_else(|| Currency::new(USDC, 6, 0, USDC, CurrencyType::Crypto))
}

fn parse_timestamp_ms_str(ms_str: &str) -> Option<UnixNanos> {
    let ms: u64 = ms_str.parse().ok()?;
    Some(UnixNanos::from(ms * 1_000_000))
}

fn build_ws_order_report(
    order: &PolymarketUserOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_event: UnixNanos,
) -> OrderStatusReport {
    let venue_order_id = VenueOrderId::from(order.id.as_str());
    let order_status = OrderStatus::from(order.status);
    let order_side = OrderSide::from(order.side);
    let time_in_force = TimeInForce::from(order.order_type);
    let quantity = Quantity::new(
        order.original_size.parse::<f64>().unwrap_or(0.0),
        instrument.size_precision(),
    );
    let filled_qty = Quantity::new(
        order.size_matched.parse::<f64>().unwrap_or(0.0),
        instrument.size_precision(),
    );
    let price = Price::new(
        order.price.parse::<f64>().unwrap_or(0.0),
        instrument.price_precision(),
    );

    let mut report = OrderStatusReport::new(
        account_id,
        instrument.id(),
        None,
        venue_order_id,
        order_side,
        OrderType::Limit,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_event,
        ts_event,
        ts_event,
        None,
    );
    report.price = Some(price);
    report
}

fn build_ws_maker_fill_report(
    mo: &PolymarketMakerOrder,
    trade: &PolymarketUserTrade,
    instrument: &InstrumentAny,
    account_id: AccountId,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
) -> FillReport {
    build_maker_fill_report(
        mo,
        &trade.id,
        trade.trader_side,
        trade.side,
        trade.asset_id.as_str(),
        account_id,
        instrument.id(),
        instrument.price_precision(),
        instrument.size_precision(),
        get_usdc_currency(),
        liquidity_side,
        ts_event,
        ts_event,
    )
}

fn build_ws_taker_fill_report(
    trade: &PolymarketUserTrade,
    instrument: &InstrumentAny,
    account_id: AccountId,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
) -> FillReport {
    let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
    let trade_id = make_composite_trade_id(&trade.id, &trade.taker_order_id);
    let order_side = determine_order_side(
        trade.trader_side,
        trade.side,
        trade.asset_id.as_str(),
        trade.asset_id.as_str(),
    );
    let last_qty = Quantity::new(
        trade.size.parse::<f64>().unwrap_or(0.0),
        instrument.size_precision(),
    );
    let last_px = Price::new(
        trade.price.parse::<f64>().unwrap_or(0.0),
        instrument.price_precision(),
    );

    let fee_bps: Decimal = trade.fee_rate_bps.parse().unwrap_or_default();
    let size: Decimal = trade.size.parse().unwrap_or_default();
    let price_dec: Decimal = trade.price.parse().unwrap_or_default();
    let commission_value = compute_commission(fee_bps, size, price_dec);
    let usdc = get_usdc_currency();

    FillReport {
        account_id,
        instrument_id: instrument.id(),
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission: Money::new(commission_value, usdc),
        liquidity_side,
        report_id: UUID4::new(),
        ts_event,
        ts_init: ts_event,
        client_order_id: None,
        venue_position_id: None,
    }
}

fn spawn_account_refresh(
    http_client: PolymarketRawHttpClient,
    emitter: ExecutionEventEmitter,
    clock: &'static AtomicTime,
    account_id: AccountId,
    signature_type: SignatureType,
) {
    get_runtime().spawn(async move {
        match fetch_and_emit_account_state(&http_client, &emitter, clock, signature_type).await {
            Ok(()) => {
                log::info!("Account state refreshed after finalized trade for {account_id}");
            }
            Err(e) => {
                log::warn!("Failed to refresh account after finalized trade: {e}");
            }
        }
    });
}

async fn fetch_and_emit_account_state(
    http_client: &PolymarketRawHttpClient,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    signature_type: SignatureType,
) -> anyhow::Result<()> {
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
    let balance_value: f64 = balance_allowance.balance.to_string().parse().unwrap_or(0.0);

    let balances = vec![AccountBalance::new(
        Money::new(balance_value, usdc),
        Money::new(0.0, usdc),
        Money::new(balance_value, usdc),
    )];

    let ts_event = clock.get_time_ns();
    emitter.emit_account_state(balances, vec![], true, ts_event);

    log::info!("Account state updated: balance={balance_value} USDC");
    Ok(())
}
