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

//! Live execution client for the Betfair adapter.

use std::{
    future::Future,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    live::{get_runtime, runner::get_exec_event_sender},
    messages::execution::{
        CancelAllOrders, CancelOrder, GenerateOrderStatusReports, ModifyOrder, SubmitOrder,
    },
};
use nautilus_core::{
    MUTEX_POISONED, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderType, TimeInForce},
    identifiers::{AccountId, ClientId, InstrumentId, Venue, VenueOrderId},
    orders::Order,
    reports::OrderStatusReport,
    types::{AccountBalance, Currency, MarginBalance},
};
use nautilus_network::socket::TcpMessageHandler;
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use crate::{
    common::{
        consts::BETFAIR_VENUE,
        credential::BetfairCredential,
        enums::{
            BetfairOrderType, BetfairSide, BetfairTimeInForce, ExecutionReportStatus,
            PersistenceType, StreamingOrderStatus,
        },
        parse::{
            extract_market_id, extract_selection_id, make_customer_order_ref, make_instrument_id,
            parse_account_state, parse_millis_timestamp,
        },
        types::BetId,
    },
    http::{
        client::BetfairHttpClient,
        models::{
            AccountFundsResponse, CancelExecutionReport, CancelInstruction, CancelOrdersParams,
            LimitOrder, MarketOnCloseOrder, PlaceExecutionReport, PlaceInstruction,
            PlaceOrdersParams, ReplaceExecutionReport, ReplaceInstruction, ReplaceOrdersParams,
        },
    },
    stream::{
        client::BetfairStreamClient,
        config::BetfairStreamConfig,
        messages::{StreamMessage, stream_decode},
        parse::{FillTracker, parse_order_status_report},
    },
};

/// Keep-alive interval in seconds (10 hours, matching Python default).
const KEEP_ALIVE_INTERVAL_SECS: u64 = 36_000;

/// Account state polling interval in seconds (5 minutes, matching Python default).
const ACCOUNT_STATE_INTERVAL_SECS: u64 = 300;

/// Betfair live execution client.
#[derive(Debug)]
pub struct BetfairExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    emitter: ExecutionEventEmitter,
    http_client: Arc<BetfairHttpClient>,
    stream_client: Option<Arc<BetfairStreamClient>>,
    credential: BetfairCredential,
    stream_config: BetfairStreamConfig,
    currency: Currency,
    fill_tracker: Arc<Mutex<FillTracker>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    keep_alive_handle: Option<JoinHandle<()>>,
    account_state_handle: Option<JoinHandle<()>>,
}

impl BetfairExecutionClient {
    /// Creates a new [`BetfairExecutionClient`] instance.
    #[must_use]
    pub fn new(
        core: ExecutionClientCore,
        http_client: BetfairHttpClient,
        credential: BetfairCredential,
        stream_config: BetfairStreamConfig,
        currency: Currency,
    ) -> Self {
        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Betting,
            None,
        );

        Self {
            core,
            clock,
            emitter,
            http_client: Arc::new(http_client),
            stream_client: None,
            credential,
            stream_config,
            currency,
            fill_tracker: Arc::new(Mutex::new(FillTracker::new())),
            pending_tasks: Mutex::new(Vec::new()),
            keep_alive_handle: None,
            account_state_handle: None,
        }
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
        tasks.retain(|handle| !handle.is_finished());
        tasks.push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    fn abort_background_tasks(&mut self) {
        if let Some(handle) = self.keep_alive_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.account_state_handle.take() {
            handle.abort();
        }
    }

    fn create_ocm_handler(
        emitter: ExecutionEventEmitter,
        account_id: AccountId,
        currency: Currency,
        fill_tracker: Arc<Mutex<FillTracker>>,
    ) -> TcpMessageHandler {
        Arc::new(move |data: &[u8]| {
            let msg = match stream_decode(data) {
                Ok(msg) => msg,
                Err(e) => {
                    log::warn!("Failed to decode stream message: {e}");
                    return;
                }
            };

            match msg {
                StreamMessage::OrderChange(ocm) => {
                    if ocm.is_heartbeat() {
                        return;
                    }

                    let Some(order_changes) = &ocm.oc else {
                        return;
                    };

                    let ts_event = parse_millis_timestamp(ocm.pt);
                    let ts_init = ts_event;

                    for omc in order_changes {
                        let Some(orc_list) = &omc.orc else {
                            continue;
                        };

                        for orc in orc_list {
                            let handicap = orc.hc.unwrap_or(Decimal::ZERO);
                            let instrument_id = make_instrument_id(&omc.id, orc.id, handicap);

                            let Some(unmatched_orders) = &orc.uo else {
                                continue;
                            };

                            for uo in unmatched_orders {
                                Self::process_unmatched_order(
                                    uo,
                                    instrument_id,
                                    account_id,
                                    currency,
                                    &emitter,
                                    &fill_tracker,
                                    ts_event,
                                    ts_init,
                                );
                            }
                        }
                    }
                }
                StreamMessage::Connection(_) => {
                    log::info!("Betfair execution stream connected");
                }
                StreamMessage::Status(status) => {
                    if status.connection_closed {
                        log::error!(
                            "Betfair execution stream closed: {:?} - {:?}",
                            status.error_code,
                            status.error_message,
                        );
                    }
                }
                StreamMessage::MarketChange(_) => {}
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn process_unmatched_order(
        uo: &crate::stream::messages::UnmatchedOrder,
        instrument_id: InstrumentId,
        account_id: AccountId,
        currency: Currency,
        emitter: &ExecutionEventEmitter,
        fill_tracker: &Arc<Mutex<FillTracker>>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) {
        let report =
            match parse_order_status_report(uo, instrument_id, account_id, ts_event, ts_init) {
                Ok(report) => report,
                Err(e) => {
                    log::warn!("Failed to parse order status report for {instrument_id}: {e}");
                    return;
                }
            };

        // Emit fill reports before order status reports so reconciliation does
        // not infer a duplicate fill from the cumulative filled_qty on the
        // status report.
        if let Ok(mut tracker) = fill_tracker.lock()
            && let Some(fill_report) = tracker.maybe_fill_report(
                uo,
                uo.s,
                instrument_id,
                account_id,
                currency,
                ts_event,
                ts_init,
            )
        {
            log::debug!(
                "Fill: bet_id={}, last_qty={}, last_px={}",
                uo.id,
                fill_report.last_qty,
                fill_report.last_px,
            );
            emitter.send_fill_report(fill_report);
        }

        emitter.send_order_status_report(report);

        // Prune fill tracker state for terminal orders
        if uo.status == StreamingOrderStatus::ExecutionComplete
            && let Ok(mut tracker) = fill_tracker.lock()
        {
            tracker.prune(&uo.id);
        }
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BetfairExecutionClient {
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
        *BETFAIR_VENUE
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

        self.core.set_stopped();
        self.core.set_disconnected();
        self.abort_background_tasks();
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        self.http_client
            .connect()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let funds: AccountFundsResponse = self
            .http_client
            .send_accounts("AccountAPING/v1.0/getAccountFunds", serde_json::json!({}))
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let ts_init = self.clock.get_time_ns();
        let account_state = parse_account_state(
            &funds,
            self.core.account_id,
            self.currency,
            ts_init,
            ts_init,
        )?;
        self.emitter.send_account_state(account_state);

        let session_token = self
            .http_client
            .session_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No session token after login"))?;

        let handler = Self::create_ocm_handler(
            self.emitter.clone(),
            self.core.account_id,
            self.currency,
            Arc::clone(&self.fill_tracker),
        );

        let stream_client = BetfairStreamClient::connect(
            &self.credential,
            session_token,
            handler,
            self.stream_config.clone(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        let stream_client = Arc::new(stream_client);

        stream_client
            .subscribe_orders(None, None)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        self.stream_client = Some(stream_client);

        // Spawn periodic keep-alive to prevent session expiry
        let keep_alive_client = Arc::clone(&self.http_client);
        self.keep_alive_handle = Some(get_runtime().spawn(async move {
            let interval = tokio::time::Duration::from_secs(KEEP_ALIVE_INTERVAL_SECS);
            loop {
                tokio::time::sleep(interval).await;

                if let Err(e) = keep_alive_client.keep_alive().await {
                    log::warn!("Betfair execution keep-alive failed: {e}");
                } else {
                    log::debug!("Betfair execution session keep-alive sent");
                }
            }
        }));

        // Spawn periodic account state polling
        let acct_client = Arc::clone(&self.http_client);
        let acct_emitter = self.emitter.clone();
        let acct_id = self.core.account_id;
        let acct_currency = self.currency;
        let acct_clock = self.clock;
        self.account_state_handle = Some(get_runtime().spawn(async move {
            let interval = tokio::time::Duration::from_secs(ACCOUNT_STATE_INTERVAL_SECS);
            loop {
                tokio::time::sleep(interval).await;
                match acct_client
                    .send_accounts::<AccountFundsResponse, _>(
                        "AccountAPING/v1.0/getAccountFunds",
                        serde_json::json!({}),
                    )
                    .await
                {
                    Ok(funds) => {
                        let ts_init = acct_clock.get_time_ns();
                        match parse_account_state(&funds, acct_id, acct_currency, ts_init, ts_init)
                        {
                            Ok(state) => acct_emitter.send_account_state(state),
                            Err(e) => log::warn!("Failed to parse account state: {e}"),
                        }
                    }
                    Err(e) => log::warn!("Failed to fetch account state: {e}"),
                }
            }
        }));

        self.core.set_connected();

        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        self.abort_background_tasks();
        self.abort_pending_tasks();

        if let Some(client) = &self.stream_client {
            client.close().await;
        }

        self.http_client.disconnect().await;
        self.core.set_disconnected();

        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;

        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        let instrument_id = order.instrument_id();
        let market_id = extract_market_id(&instrument_id)?;
        let (selection_id, handicap) = extract_selection_id(&instrument_id)?;

        let side = BetfairSide::from(order.order_side());
        let size = order.quantity().as_decimal();
        let handicap_opt = if handicap == Decimal::ZERO {
            None
        } else {
            Some(handicap)
        };
        let customer_order_ref = Some(make_customer_order_ref(order.client_order_id().as_str()));

        let instruction = match order.order_type() {
            OrderType::Limit => {
                let price = order
                    .price()
                    .ok_or_else(|| anyhow::anyhow!("Limit order missing price"))?
                    .as_decimal();

                let (persistence_type, time_in_force, min_fill_size) = match order.time_in_force() {
                    TimeInForce::Ioc => (
                        None,
                        Some(BetfairTimeInForce::FillOrKill),
                        Some(Decimal::ZERO),
                    ),
                    TimeInForce::Fok => (None, Some(BetfairTimeInForce::FillOrKill), None),
                    TimeInForce::Gtc => (Some(PersistenceType::Persist), None, None),
                    TimeInForce::AtTheClose => (Some(PersistenceType::MarketOnClose), None, None),
                    _ => (Some(PersistenceType::Lapse), None, None),
                };

                PlaceInstruction {
                    order_type: BetfairOrderType::Limit,
                    selection_id,
                    handicap: handicap_opt,
                    side,
                    limit_order: Some(LimitOrder {
                        size,
                        price,
                        persistence_type,
                        time_in_force,
                        min_fill_size,
                        bet_target_type: None,
                        bet_target_size: None,
                    }),
                    limit_on_close_order: None,
                    market_on_close_order: None,
                    customer_order_ref,
                }
            }
            OrderType::Market => {
                if order.time_in_force() != TimeInForce::AtTheClose {
                    anyhow::bail!(
                        "Market orders on Betfair are only supported with AtTheClose \
                         time in force (BSP MarketOnClose)"
                    );
                }
                PlaceInstruction {
                    order_type: BetfairOrderType::MarketOnClose,
                    selection_id,
                    handicap: handicap_opt,
                    side,
                    limit_order: None,
                    limit_on_close_order: None,
                    market_on_close_order: Some(MarketOnCloseOrder { liability: size }),
                    customer_order_ref,
                }
            }
            other => {
                anyhow::bail!("Unsupported order type for Betfair: {other:?}");
            }
        };

        let params = PlaceOrdersParams {
            market_id,
            instructions: vec![instruction],
            customer_ref: None,
            market_version: None,
            customer_strategy_ref: None,
        };

        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();

        log::debug!("OrderSubmitted client_order_id={client_order_id}");
        self.emitter.emit_order_submitted(&order);

        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("submit-order", async move {
            let report: PlaceExecutionReport = match http_client
                .send_betting_order("SportsAPING/v1.0/placeOrders", &params)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        &format!("submit-order error: {e}"),
                        ts_event,
                        false,
                    );
                    return Ok(());
                }
            };

            match report.status {
                ExecutionReportStatus::Success => {
                    if let Some(reports) = &report.instruction_reports
                        && let Some(ir) = reports.first()
                        && let Some(bet_id) = &ir.bet_id
                    {
                        let venue_order_id = VenueOrderId::from(bet_id.as_str());
                        let ts_event = clock.get_time_ns();
                        // Order IDs are immutable so the captured snapshot is
                        // safe here; OCM provides the authoritative state.
                        emitter.emit_order_accepted(&order, venue_order_id, ts_event);
                    }
                }
                ExecutionReportStatus::Failure
                | ExecutionReportStatus::ProcessedWithErrors
                | ExecutionReportStatus::Timeout => {
                    let reason = report
                        .error_code
                        .map_or_else(|| "unknown error".to_string(), |c| format!("{c:?}"));
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        &reason,
                        ts_event,
                        false,
                    );
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let market_id = extract_market_id(&instrument_id)?;

        let venue_order_id = cmd
            .venue_order_id
            .ok_or_else(|| anyhow::anyhow!("Cannot cancel order without venue_order_id"))?;
        let bet_id: BetId = venue_order_id.to_string();

        let params = CancelOrdersParams {
            market_id: Some(market_id),
            instructions: Some(vec![CancelInstruction {
                bet_id,
                size_reduction: None,
            }]),
            customer_ref: None,
        };

        let client_order_id = cmd.client_order_id;
        let strategy_id = cmd.strategy_id;
        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("cancel-order", async move {
            let result: Result<CancelExecutionReport, _> = http_client
                .send_betting_order("SportsAPING/v1.0/cancelOrders", &params)
                .await;

            match result {
                Ok(report) if report.status != ExecutionReportStatus::Success => {
                    let reason = report
                        .error_code
                        .map_or_else(|| "unknown error".to_string(), |c| format!("{c:?}"));
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Some(venue_order_id),
                        &reason,
                        ts_event,
                    );
                }
                Err(e) => {
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Some(venue_order_id),
                        &format!("cancel-order error: {e}"),
                        ts_event,
                    );
                }
                Ok(_) => {}
            }

            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let market_id = extract_market_id(&instrument_id)?;

        let venue_order_id = cmd
            .venue_order_id
            .ok_or_else(|| anyhow::anyhow!("Cannot modify order without venue_order_id"))?;
        let bet_id: BetId = venue_order_id.to_string();

        // Compare against existing order to determine actual changes
        let existing_order = self.core.get_order(&cmd.client_order_id);
        let has_price_change = match (&cmd.price, &existing_order) {
            (Some(new_price), Ok(order)) => order.price() != Some(*new_price),
            (Some(_), Err(_)) => true,
            (None, _) => false,
        };
        let has_quantity_change = match (&cmd.quantity, &existing_order) {
            (Some(new_qty), Ok(order)) => order.quantity() != *new_qty,
            (Some(_), Err(_)) => true,
            (None, _) => false,
        };

        // Betfair does not support atomic price+quantity modification
        if has_price_change && has_quantity_change {
            let ts_event = self.clock.get_time_ns();
            self.emitter.emit_order_modify_rejected_event(
                cmd.strategy_id,
                instrument_id,
                cmd.client_order_id,
                Some(venue_order_id),
                "cannot modify price and quantity simultaneously on Betfair",
                ts_event,
            );
            return Ok(());
        }

        let client_order_id = cmd.client_order_id;
        let strategy_id = cmd.strategy_id;
        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let clock = self.clock;

        if has_price_change {
            let new_price = cmd.price.unwrap().as_decimal();

            let params = ReplaceOrdersParams {
                market_id,
                instructions: vec![ReplaceInstruction { bet_id, new_price }],
                customer_ref: None,
                market_version: None,
            };

            self.spawn_task("modify-order-price", async move {
                let result: Result<ReplaceExecutionReport, _> = http_client
                    .send_betting_order("SportsAPING/v1.0/replaceOrders", &params)
                    .await;

                match result {
                    Ok(report) if report.status == ExecutionReportStatus::Failure => {
                        let reason = report
                            .error_code
                            .map_or_else(|| "unknown error".to_string(), |c| format!("{c:?}"));
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_modify_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Some(venue_order_id),
                            &reason,
                            ts_event,
                        );
                    }
                    Err(e) => {
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_modify_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Some(venue_order_id),
                            &format!("modify-order error: {e}"),
                            ts_event,
                        );
                    }
                    Ok(_) => {}
                }

                Ok(())
            });
        } else if has_quantity_change {
            // Quantity reduction via partial cancel
            let order = self.core.get_order(&client_order_id)?;
            let existing_qty = order.quantity().as_decimal();
            let new_qty = cmd.quantity.unwrap().as_decimal();

            if new_qty >= existing_qty {
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_modify_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    Some(venue_order_id),
                    "can only reduce quantity on Betfair",
                    ts_event,
                );
                return Ok(());
            }

            let size_reduction = existing_qty - new_qty;
            let params = CancelOrdersParams {
                market_id: Some(market_id),
                instructions: Some(vec![CancelInstruction {
                    bet_id,
                    size_reduction: Some(size_reduction),
                }]),
                customer_ref: None,
            };

            self.spawn_task("modify-order-quantity", async move {
                let result: Result<CancelExecutionReport, _> = http_client
                    .send_betting_order("SportsAPING/v1.0/cancelOrders", &params)
                    .await;

                match result {
                    Ok(report) if report.status != ExecutionReportStatus::Success => {
                        let reason = report
                            .error_code
                            .map_or_else(|| "unknown error".to_string(), |c| format!("{c:?}"));
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_modify_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Some(venue_order_id),
                            &reason,
                            ts_event,
                        );
                    }
                    Err(e) => {
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_modify_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Some(venue_order_id),
                            &format!("modify-order error: {e}"),
                            ts_event,
                        );
                    }
                    Ok(_) => {}
                }

                Ok(())
            });
        } else {
            let ts_event = self.clock.get_time_ns();
            self.emitter.emit_order_modify_rejected_event(
                strategy_id,
                instrument_id,
                client_order_id,
                Some(venue_order_id),
                "no effective change in price or quantity",
                ts_event,
            );
        }

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let market_id = extract_market_id(&instrument_id)?;

        let params = CancelOrdersParams {
            market_id: Some(market_id),
            instructions: None,
            customer_ref: None,
        };

        let http_client = Arc::clone(&self.http_client);

        self.spawn_task("cancel-all-orders", async move {
            let result = http_client
                .send_betting_order::<serde_json::Value, _>(
                    "SportsAPING/v1.0/cancelOrders",
                    &params,
                )
                .await;

            if let Err(e) = result {
                log::warn!("Failed to cancel all orders: {e}");
            }

            Ok(())
        });

        Ok(())
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        log::info!("Generating order status reports not yet supported for Betfair");
        Ok(Vec::new())
    }
}
