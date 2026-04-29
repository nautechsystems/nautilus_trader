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
    fmt,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    live::{
        get_runtime,
        runner::{get_data_event_sender, get_exec_event_sender},
    },
    messages::{
        DataEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateFillReportsBuilder, GenerateOrderStatusReports,
            GenerateOrderStatusReportsBuilder, ModifyOrder, QueryOrder, SubmitOrder,
            SubmitOrderList,
        },
    },
};
use nautilus_core::{
    MUTEX_POISONED, UnixNanos,
    datetime::NANOSECONDS_IN_SECOND,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    data::Data,
    enums::{AccountType, OmsType, OrderStatus, OrderType, TimeInForce},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, Venue, VenueOrderId},
    instruments::InstrumentAny,
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, MarginBalance},
};
use nautilus_network::socket::TcpMessageHandler;
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use crate::{
    common::{
        consts::{
            BETFAIR_VENUE, METHOD_CANCEL_ORDERS, METHOD_GET_ACCOUNT_FUNDS,
            METHOD_LIST_CURRENT_ORDERS, METHOD_PLACE_ORDERS, METHOD_REPLACE_ORDERS,
        },
        credential::BetfairCredential,
        enums::{
            BetfairOrderStatus, BetfairOrderType, BetfairSide, BetfairTimeInForce,
            ExecutionReportErrorCode, ExecutionReportStatus, InstructionReportErrorCode,
            InstructionReportStatus, OrderProjection, PersistenceType, StreamingOrderStatus,
            StreamingSide,
        },
        parse::{
            extract_market_id, extract_selection_id, make_customer_order_ref,
            make_customer_order_ref_legacy, make_instrument_id, parse_account_state,
            parse_millis_timestamp,
        },
        types::BetId,
    },
    config::BetfairExecConfig,
    data::custom_data_with_instrument,
    data_types::{BetfairOrderVoided, register_betfair_custom_data},
    http::{
        client::BetfairHttpClient,
        models::{
            AccountFundsResponse, CancelExecutionReport, CancelInstruction, CancelOrdersParams,
            CurrentOrderSummary, CurrentOrderSummaryReport, LimitOnCloseOrder, LimitOrder,
            ListCurrentOrdersParams, MarketOnCloseOrder, MarketVersion, PlaceExecutionReport,
            PlaceInstruction, PlaceInstructionReport, PlaceOrdersParams, ReplaceExecutionReport,
            ReplaceInstruction, ReplaceInstructionReport, ReplaceOrdersParams, TimeRange,
        },
        parse::{parse_current_order_fill_report, parse_current_order_report},
    },
    stream::{
        client::BetfairStreamClient,
        config::BetfairStreamConfig,
        messages::{StreamMessage, stream_decode},
        parse::{FillTracker, has_cancel_quantity, parse_order_status_report},
    },
};

/// Keep-alive interval in seconds (10 hours, matching Python default).
const KEEP_ALIVE_INTERVAL_SECS: u64 = 36_000;

/// Delay in seconds before retrying after a rate limit error.
const RATE_LIMIT_RETRY_DELAY_SECS: u64 = 5;

/// Shared mutable state for the OCM stream handler.
///
/// Accessed by both the TCP reader closure and the execution client methods
/// (submit, modify, connect/disconnect). All access goes through `Arc<Mutex<>>`.
#[derive(Debug, Default)]
pub struct OcmState {
    pub fill_tracker: FillTracker,
    /// Maps customer_order_ref (rfo) to ClientOrderId for stream resolution.
    pub customer_order_refs: AHashMap<String, ClientOrderId>,
    /// Client order IDs that already received an OCM order status update.
    pub stream_reported_client_orders: AHashSet<ClientOrderId>,
    /// Bet IDs that have received a terminal event (cancel, lapse, fill-complete).
    pub terminal_orders: AHashSet<String>,
    /// Old bet IDs from replace operations, to suppress late stream updates.
    pub replaced_venue_order_ids: AHashSet<String>,
    /// (client_order_id, old_bet_id) pairs for in-flight replace operations.
    pub pending_update_keys: AHashSet<(ClientOrderId, String)>,
}

impl OcmState {
    /// Registers a customer_order_ref mapping for a new order.
    pub fn register_customer_order_ref(&mut self, client_order_id: ClientOrderId) {
        let rfo = make_customer_order_ref(client_order_id.as_str());
        self.customer_order_refs.insert(rfo, client_order_id);
    }

    /// Registers both current and legacy customer_order_ref truncations.
    ///
    /// Used during reconnect sync for pre-existing orders that may
    /// have been placed with either truncation format.
    pub fn register_customer_order_ref_with_legacy(&mut self, client_order_id: ClientOrderId) {
        let rfo = make_customer_order_ref(client_order_id.as_str());
        let rfo_legacy = make_customer_order_ref_legacy(client_order_id.as_str());
        self.customer_order_refs.insert(rfo, client_order_id);
        if rfo_legacy != client_order_id.as_str() {
            self.customer_order_refs.insert(rfo_legacy, client_order_id);
        }
    }

    /// Removes customer_order_ref mappings for a client_order_id.
    pub fn remove_customer_order_refs(&mut self, client_order_id: &ClientOrderId) {
        let rfo = make_customer_order_ref(client_order_id.as_str());
        let rfo_legacy = make_customer_order_ref_legacy(client_order_id.as_str());
        self.customer_order_refs.remove(&rfo);
        self.customer_order_refs.remove(&rfo_legacy);
    }

    /// Resolves a client_order_id from the unmatched order's rfo field.
    pub fn resolve_client_order_id(&self, rfo: Option<&str>) -> Option<ClientOrderId> {
        rfo.and_then(|r| self.customer_order_refs.get(r).copied())
    }

    /// Returns `true` if the bet_id already has a terminal event and should be skipped.
    /// Otherwise marks it as terminal and returns `false`.
    pub fn try_mark_terminal(&mut self, bet_id: &str) -> bool {
        !self.terminal_orders.insert(bet_id.to_string())
    }

    /// Returns `true` if a cancel/lapse for this bet should be suppressed
    /// because a replace operation is pending or the bet was already replaced.
    pub fn should_suppress_cancel(&self, client_order_id: &ClientOrderId, bet_id: &str) -> bool {
        if self.replaced_venue_order_ids.contains(bet_id) {
            return true;
        }
        self.pending_update_keys
            .contains(&(*client_order_id, bet_id.to_string()))
    }

    /// Cleans up customer_order_ref mappings for a terminal order,
    /// unless a pending replace exists for this client_order_id.
    pub fn cleanup_terminal_order(&mut self, client_order_id: &ClientOrderId) {
        let has_pending = self
            .pending_update_keys
            .iter()
            .any(|(cid, _)| cid == client_order_id);

        if !has_pending {
            self.remove_customer_order_refs(client_order_id);
        }
    }

    /// Syncs fill tracker state from existing order fills.
    ///
    /// Pre-populates filled quantities and average prices so that
    /// the first stream update after reconnect computes correct
    /// incremental fills instead of treating cumulative size as new.
    pub fn sync_from_orders(&mut self, orders: &[(String, ClientOrderId, Decimal, Decimal, bool)]) {
        for (bet_id, client_order_id, filled_qty, avg_px, is_closed) in orders {
            if *is_closed {
                self.terminal_orders.insert(bet_id.clone());
            } else {
                self.register_customer_order_ref_with_legacy(*client_order_id);
            }

            if *filled_qty > Decimal::ZERO {
                self.fill_tracker.sync_order(bet_id, *filled_qty, *avg_px);
            }
        }
    }
}

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
    config: BetfairExecConfig,
    currency: Currency,
    ocm_state: Arc<Mutex<OcmState>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    keep_alive_handle: Option<JoinHandle<()>>,
    account_state_handle: Option<JoinHandle<()>>,
    reconnect_handle: Option<JoinHandle<()>>,
}

impl BetfairExecutionClient {
    /// Creates a new [`BetfairExecutionClient`] instance.
    #[must_use]
    pub fn new(
        core: ExecutionClientCore,
        http_client: BetfairHttpClient,
        credential: BetfairCredential,
        stream_config: BetfairStreamConfig,
        config: BetfairExecConfig,
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
            config,
            currency,
            ocm_state: Arc::new(Mutex::new(OcmState::default())),
            pending_tasks: Mutex::new(Vec::new()),
            keep_alive_handle: None,
            account_state_handle: None,
            reconnect_handle: None,
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

    fn reconcile_market_ids(&self) -> Option<Vec<String>> {
        if self.config.reconcile_market_ids_only
            && let Some(ids) = &self.config.reconcile_market_ids
        {
            return Some(ids.clone());
        }
        self.config.stream_market_ids_filter.clone()
    }

    /// Returns the market version for price protection on order placement.
    ///
    /// When `use_market_version` is enabled, reads the `version` field from
    /// the instrument's `info` metadata. Betfair lapses orders submitted with
    /// a stale version rather than matching against a moved book.
    fn get_market_version(&self, instrument_id: &InstrumentId) -> Option<MarketVersion> {
        if !self.config.use_market_version {
            return None;
        }

        let cache = self.core.cache();
        let instrument = cache.instrument(instrument_id)?;

        if let InstrumentAny::Betting(betting) = instrument {
            let version = betting.info.as_ref()?.get_i64("version")?;
            return Some(MarketVersion {
                version: Some(version),
            });
        }

        None
    }

    /// Pre-populates OCM state from cached orders to prevent duplicate fills
    /// and terminal events after reconnect.
    fn sync_ocm_state_from_cache(&self) {
        let cache = self.core.cache();
        let venue = *BETFAIR_VENUE;
        let orders = cache.orders(Some(&venue), None, None, None, None);

        let order_data: Vec<_> = orders
            .iter()
            .filter_map(|order| {
                let venue_order_id = order.venue_order_id()?;
                let bet_id = venue_order_id.to_string();
                let filled_qty = order.filled_qty().as_decimal();
                let avg_px = order.avg_px().map_or(Decimal::ZERO, |px| {
                    Decimal::try_from(px).unwrap_or(Decimal::ZERO)
                });
                Some((
                    bet_id,
                    order.client_order_id(),
                    filled_qty,
                    avg_px,
                    order.is_closed(),
                ))
            })
            .collect();

        let mut state = self.ocm_state.lock().expect(MUTEX_POISONED);
        state.sync_from_orders(&order_data);

        log::info!("Synced OCM state from {} cached orders", order_data.len());
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

        if let Some(handle) = self.reconnect_handle.take() {
            handle.abort();
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn create_ocm_handler(
        emitter: ExecutionEventEmitter,
        account_id: AccountId,
        currency: Currency,
        ocm_state: Arc<Mutex<OcmState>>,
        data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        market_ids_filter: Option<ahash::AHashSet<String>>,
        ignore_external_orders: bool,
        reconnect_tx: tokio::sync::mpsc::UnboundedSender<()>,
    ) -> TcpMessageHandler {
        let has_initial_connection = Arc::new(AtomicBool::new(false));

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
                        if let Some(ref filter) = market_ids_filter
                            && !filter.contains(&omc.id)
                        {
                            continue;
                        }
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
                                if ignore_external_orders && uo.rfo.is_none() {
                                    continue;
                                }

                                Self::process_unmatched_order(
                                    uo,
                                    instrument_id,
                                    account_id,
                                    currency,
                                    &emitter,
                                    &ocm_state,
                                    ts_event,
                                    ts_init,
                                );

                                if uo.status == StreamingOrderStatus::ExecutionComplete
                                    && uo.sv.is_some_and(|sv| sv > Decimal::ZERO)
                                {
                                    let sv = uo.sv.unwrap();
                                    let side_str = match uo.side {
                                        StreamingSide::Back => "BACK",
                                        StreamingSide::Lay => "LAY",
                                    };
                                    let dec_to_f64 = |d: Decimal| -> f64 {
                                        d.to_string().parse::<f64>().unwrap_or(0.0)
                                    };
                                    let voided = BetfairOrderVoided::new(
                                        instrument_id,
                                        uo.rfo.as_deref().unwrap_or("").to_string(),
                                        uo.id.clone(),
                                        dec_to_f64(sv),
                                        dec_to_f64(uo.p),
                                        dec_to_f64(uo.s),
                                        side_str.to_string(),
                                        uo.avp.map_or(f64::NAN, dec_to_f64),
                                        uo.sm.map_or(f64::NAN, dec_to_f64),
                                        String::new(),
                                        ts_event,
                                        ts_init,
                                    );
                                    log::info!("Order voided: bet_id={}, size_voided={sv}", uo.id,);
                                    let custom = custom_data_with_instrument(
                                        Arc::new(voided),
                                        instrument_id,
                                    );

                                    if let Err(e) =
                                        data_sender.send(DataEvent::Data(Data::Custom(custom)))
                                    {
                                        log::warn!("Failed to send voided event: {e}");
                                    }
                                }
                            }
                        }
                    }
                }
                StreamMessage::Connection(_) => {
                    if has_initial_connection.swap(true, Ordering::SeqCst) {
                        log::info!("Betfair execution stream reconnected");
                        let _ = reconnect_tx.send(());
                    } else {
                        log::info!("Betfair execution stream connected");
                    }
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
                StreamMessage::MarketChange(_) | StreamMessage::RaceChange(_) => {}
            }
        })
    }

    #[expect(clippy::too_many_arguments)]
    fn process_unmatched_order(
        uo: &crate::stream::messages::UnmatchedOrder,
        instrument_id: InstrumentId,
        account_id: AccountId,
        currency: Currency,
        emitter: &ExecutionEventEmitter,
        ocm_state: &Arc<Mutex<OcmState>>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> bool {
        let mut report =
            match parse_order_status_report(uo, instrument_id, account_id, ts_event, ts_init) {
                Ok(report) => report,
                Err(e) => {
                    log::warn!("Failed to parse order status report for {instrument_id}: {e}");
                    return false;
                }
            };

        let Ok(mut state) = ocm_state.lock() else {
            log::error!("OcmState mutex poisoned");
            return false;
        };

        if state.terminal_orders.contains(&uo.id) {
            return false;
        }

        let resolved_client_order_id = state.resolve_client_order_id(uo.rfo.as_deref());

        // Patch the truncated rfo-derived client_order_id with the full
        // resolved value so downstream reconciliation matches correctly.
        if resolved_client_order_id.is_some() {
            report.client_order_id = resolved_client_order_id;
        }

        if uo.status == StreamingOrderStatus::ExecutionComplete
            && has_cancel_quantity(uo)
            && let Some(ref client_oid) = resolved_client_order_id
        {
            if state.should_suppress_cancel(client_oid, &uo.id) {
                log::debug!(
                    "Suppressing cancel for bet_id={} (pending replace or already replaced)",
                    uo.id,
                );
                return false;
            }

            if state.try_mark_terminal(&uo.id) {
                log::debug!("Duplicate terminal event for bet_id={}, skipping", uo.id);
                return false;
            }
        }

        if let Some(client_oid) = resolved_client_order_id {
            state.stream_reported_client_orders.insert(client_oid);
        }

        // Emit fill reports before order status reports so reconciliation does
        // not infer a duplicate fill from the cumulative filled_qty on the
        // status report.
        if let Some(mut fill_report) = state.fill_tracker.maybe_fill_report(
            uo,
            uo.s,
            instrument_id,
            account_id,
            currency,
            ts_event,
            ts_init,
        ) {
            if resolved_client_order_id.is_some() {
                fill_report.client_order_id = resolved_client_order_id;
            }
            log::debug!(
                "Fill: bet_id={}, last_qty={}, last_px={}",
                uo.id,
                fill_report.last_qty,
                fill_report.last_px,
            );
            emitter.send_fill_report(fill_report);
        }

        if report.order_status == OrderStatus::Canceled
            && let Some(reason) = report.cancel_reason.as_deref()
        {
            log::info!(
                "Betfair order {} ({}) canceled: reason={}, matched={}, canceled={}, lapsed={}, voided={}",
                report
                    .client_order_id
                    .unwrap_or_else(|| ClientOrderId::from(uo.id.as_str())),
                uo.id,
                reason,
                uo.sm.unwrap_or(Decimal::ZERO),
                uo.sc.unwrap_or(Decimal::ZERO),
                uo.sl.unwrap_or(Decimal::ZERO),
                uo.sv.unwrap_or(Decimal::ZERO),
            );
        }

        emitter.send_order_status_report(report);

        if uo.status == StreamingOrderStatus::ExecutionComplete {
            state.terminal_orders.insert(uo.id.clone());
            state.fill_tracker.prune(&uo.id);

            if let Some(ref client_oid) = resolved_client_order_id {
                state.cleanup_terminal_order(client_oid);
            }
        }

        true
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

        register_betfair_custom_data();

        self.http_client
            .connect()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let funds: AccountFundsResponse = self
            .http_client
            .send_accounts(METHOD_GET_ACCOUNT_FUNDS, serde_json::json!({}))
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

        // Sync OCM state from cached orders before stream connects
        self.sync_ocm_state_from_cache();

        let market_ids_filter = self
            .config
            .stream_market_ids_filter
            .as_ref()
            .map(|ids| ids.iter().cloned().collect::<ahash::AHashSet<String>>());

        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::unbounded_channel();

        let handler = Self::create_ocm_handler(
            self.emitter.clone(),
            self.core.account_id,
            self.currency,
            Arc::clone(&self.ocm_state),
            get_data_event_sender(),
            market_ids_filter,
            self.config.ignore_external_orders,
            reconnect_tx,
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
        let keep_alive_stream = Arc::clone(self.stream_client.as_ref().unwrap());
        let keep_alive_app_key = self.credential.app_key().to_string();

        self.keep_alive_handle = Some(get_runtime().spawn(async move {
            let interval = tokio::time::Duration::from_secs(KEEP_ALIVE_INTERVAL_SECS);
            loop {
                tokio::time::sleep(interval).await;

                match keep_alive_client.keep_alive().await {
                    Ok(()) => {}
                    Err(ref e) if e.is_login_failed() => {
                        log::warn!("Betfair execution session expired, attempting re-login: {e}");
                        if let Err(e) = keep_alive_client.reconnect().await {
                            log::error!("Betfair execution re-login failed: {e}");
                            continue;
                        }
                    }
                    Err(e) => {
                        log::warn!("Betfair execution keep-alive failed (transient): {e}");
                        continue;
                    }
                }

                if let Some(token) = keep_alive_client.session_token().await {
                    keep_alive_stream.update_auth(&keep_alive_app_key, token);
                }
                log::debug!("Betfair execution session keep-alive sent");
            }
        }));

        if self.config.calculate_account_state && self.config.request_account_state_secs > 0 {
            let acct_client = Arc::clone(&self.http_client);
            let acct_emitter = self.emitter.clone();
            let acct_id = self.core.account_id;
            let acct_currency = self.currency;
            let acct_clock = self.clock;
            let interval_secs = self.config.request_account_state_secs;
            self.account_state_handle = Some(get_runtime().spawn(async move {
                let interval = tokio::time::Duration::from_secs(interval_secs);
                loop {
                    tokio::time::sleep(interval).await;

                    match acct_client
                        .send_accounts::<AccountFundsResponse, _>(
                            METHOD_GET_ACCOUNT_FUNDS,
                            serde_json::json!({}),
                        )
                        .await
                    {
                        Ok(funds) => {
                            let ts_init = acct_clock.get_time_ns();

                            match parse_account_state(
                                &funds,
                                acct_id,
                                acct_currency,
                                ts_init,
                                ts_init,
                            ) {
                                Ok(state) => acct_emitter.send_account_state(state),
                                Err(e) => log::warn!("Failed to parse account state: {e}"),
                            }
                        }
                        Err(e) => log::warn!("Failed to fetch account state: {e}"),
                    }
                }
            }));
        }

        let reconnect_http = Arc::clone(&self.http_client);
        let reconnect_stream = Arc::clone(self.stream_client.as_ref().unwrap());
        let reconnect_app_key = self.credential.app_key().to_string();
        let reconnect_emitter = self.emitter.clone();
        let reconnect_clock = self.clock;
        let reconnect_acct_id = self.core.account_id;
        let reconnect_currency = self.currency;

        self.reconnect_handle = Some(get_runtime().spawn(async move {
            while reconnect_rx.recv().await.is_some() {
                log::info!("Handling execution stream reconnection");

                match reconnect_http.keep_alive().await {
                    Ok(()) => {}
                    Err(ref e) if e.is_login_failed() => {
                        log::warn!("Session expired on reconnect, attempting re-login: {e}");
                        if let Err(e) = reconnect_http.reconnect().await {
                            log::error!("Re-login failed on reconnect: {e}");
                            continue;
                        }
                    }
                    Err(e) => {
                        log::warn!("Keep-alive failed on reconnect (transient): {e}");
                        continue;
                    }
                }

                if let Some(token) = reconnect_http.session_token().await {
                    reconnect_stream.update_auth(&reconnect_app_key, token);
                }

                match reconnect_http
                    .send_accounts::<AccountFundsResponse, _>(
                        METHOD_GET_ACCOUNT_FUNDS,
                        serde_json::json!({}),
                    )
                    .await
                {
                    Ok(funds) => {
                        let ts_init = reconnect_clock.get_time_ns();

                        match parse_account_state(
                            &funds,
                            reconnect_acct_id,
                            reconnect_currency,
                            ts_init,
                            ts_init,
                        ) {
                            Ok(state) => reconnect_emitter.send_account_state(state),
                            Err(e) => {
                                log::warn!("Failed to parse account state on reconnect: {e}");
                            }
                        }
                    }
                    Err(e) => log::warn!("Failed to fetch account state on reconnect: {e}"),
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

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let account_id = self.core.account_id;
        let ocm_state = Arc::clone(&self.ocm_state);
        let clock = self.clock;
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let instrument_id = cmd.instrument_id;

        self.spawn_task("query_order", async move {
            let mut candidates: Vec<CurrentOrderSummary> = Vec::new();
            let mut seen_bet_ids: AHashSet<String> = AHashSet::new();

            // Customer_order_ref lookup: Betfair reuses the ref across a
            // replace (old bet cancelled + new bet live), so this returns the
            // live replacement even when the cached bet_id is stale.
            let rfo = make_customer_order_ref(client_order_id.as_str());
            let rfo_params = list_current_orders_filter_ref(rfo.clone());
            match list_current_orders_with_retry(&http_client, &rfo_params).await {
                Ok(r) => extend_unique(&mut candidates, &mut seen_bet_ids, r.current_orders),
                Err(e) => log::warn!("Betfair query_order ref lookup failed: {e}"),
            }

            if candidates.is_empty() {
                let rfo_legacy = make_customer_order_ref_legacy(client_order_id.as_str());
                if rfo_legacy != rfo {
                    let legacy_params = list_current_orders_filter_ref(rfo_legacy);
                    match list_current_orders_with_retry(&http_client, &legacy_params).await {
                        Ok(r) => {
                            extend_unique(&mut candidates, &mut seen_bet_ids, r.current_orders);
                        }
                        Err(e) => log::warn!("Betfair query_order legacy lookup failed: {e}"),
                    }
                }
            }

            // Always also query by bet_id when known. This rescues
            // pre-existing orders without a recognizable ref and orders whose
            // ref-based results came back as foreign-market collisions only.
            if let Some(ref bet_id) = venue_order_id {
                let params = list_current_orders_filter_bet_id(bet_id.to_string());
                match list_current_orders_with_retry(&http_client, &params).await {
                    Ok(r) => extend_unique(&mut candidates, &mut seen_bet_ids, r.current_orders),
                    Err(e) => log::warn!("Betfair query_order bet_id lookup failed: {e}"),
                }
            }

            if candidates.is_empty() {
                log::warn!(
                    "Betfair query_order found no order for client_order_id={client_order_id}, venue_order_id={venue_order_id:?}",
                );
                return Ok(());
            }

            let Some(order) = select_order_for_query(
                &candidates,
                instrument_id,
                client_order_id,
                venue_order_id,
            ) else {
                return Ok(());
            };

            let ts_init = clock.get_time_ns();
            let mut report = match parse_current_order_report(order, account_id, ts_init) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("Failed to parse order report for {}: {e}", order.bet_id);
                    return Ok(());
                }
            };

            if report.client_order_id.is_none()
                && let Some(rfo) = order.customer_order_ref.as_deref()
                && let Ok(state) = ocm_state.lock()
                && let Some(full_id) = state.resolve_client_order_id(Some(rfo))
            {
                report.client_order_id = Some(full_id);
            }

            if report.client_order_id.is_none() {
                report.client_order_id = Some(client_order_id);
            }

            emitter.send_order_status_report(report);
            Ok(())
        });

        Ok(())
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = self.clock.get_time_ns();
        let start = lookback_mins.map(|mins| {
            let lookback_ns = mins
                .saturating_mul(60)
                .saturating_mul(NANOSECONDS_IN_SECOND);
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

        let (order_reports, fill_reports) = tokio::try_join!(
            self.generate_order_status_reports(&order_cmd),
            self.generate_fill_reports(fill_cmd),
        )?;

        log::info!("Received {} OrderStatusReports", order_reports.len());
        log::info!("Received {} FillReports", fill_reports.len());

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *BETFAIR_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);

        Ok(Some(mass_status))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let order_projection = if cmd.open_only {
            Some(OrderProjection::Executable)
        } else {
            Some(OrderProjection::All)
        };

        let ts_init = self.clock.get_time_ns();
        let mut reports = Vec::new();
        let mut from_record: u32 = 0;

        loop {
            let params = ListCurrentOrdersParams {
                bet_ids: None,
                market_ids: self.reconcile_market_ids(),
                order_projection,
                customer_order_refs: None,
                customer_strategy_refs: None,
                date_range: None,
                order_by: None,
                sort_dir: None,
                from_record: if from_record > 0 {
                    Some(from_record)
                } else {
                    None
                },
                record_count: None,
            };

            let response: CurrentOrderSummaryReport = match self
                .http_client
                .send_betting(METHOD_LIST_CURRENT_ORDERS, &params)
                .await
            {
                Ok(r) => r,
                Err(e) if e.is_session_error() || e.is_rate_limit_error() => {
                    if e.is_rate_limit_error() {
                        log::warn!("Rate limited, retrying in {RATE_LIMIT_RETRY_DELAY_SECS}s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(
                            RATE_LIMIT_RETRY_DELAY_SECS,
                        ))
                        .await;
                    } else {
                        log::warn!("Session error, refreshing session");

                        if self.http_client.keep_alive().await.is_err() {
                            let _ = self.http_client.reconnect().await;
                        }
                    }
                    self.http_client
                        .send_betting(METHOD_LIST_CURRENT_ORDERS, &params)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                }
                Err(e) => anyhow::bail!("{e}"),
            };

            let page_size = response.current_orders.len() as u32;

            for order in &response.current_orders {
                match parse_current_order_report(order, self.core.account_id, ts_init) {
                    Ok(mut r) => {
                        if let Some(ref rfo) = order.customer_order_ref
                            && let Ok(state) = self.ocm_state.lock()
                            && let Some(full_id) = state.resolve_client_order_id(Some(rfo.as_str()))
                        {
                            r.client_order_id = Some(full_id);
                        }
                        reports.push(r);
                    }
                    Err(e) => {
                        log::warn!("Failed to parse order report for {}: {e}", order.bet_id);
                    }
                }
            }

            if !response.more_available {
                break;
            }

            from_record += page_size;
        }

        log::info!("Generated {} order status reports", reports.len());
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let date_range = match (cmd.start, cmd.end) {
            (Some(start), Some(end)) => Some(TimeRange {
                from: Some(start.to_rfc3339()),
                to: Some(end.to_rfc3339()),
            }),
            (Some(start), None) => Some(TimeRange {
                from: Some(start.to_rfc3339()),
                to: None,
            }),
            (None, Some(end)) => Some(TimeRange {
                from: None,
                to: Some(end.to_rfc3339()),
            }),
            (None, None) => None,
        };

        let ts_init = self.clock.get_time_ns();
        let mut reports = Vec::new();
        let mut from_record: u32 = 0;

        loop {
            let params = ListCurrentOrdersParams {
                bet_ids: None,
                market_ids: self.reconcile_market_ids(),
                order_projection: Some(OrderProjection::All),
                customer_order_refs: None,
                customer_strategy_refs: None,
                date_range: date_range.clone(),
                order_by: None,
                sort_dir: None,
                from_record: if from_record > 0 {
                    Some(from_record)
                } else {
                    None
                },
                record_count: None,
            };

            let response: CurrentOrderSummaryReport = match self
                .http_client
                .send_betting(METHOD_LIST_CURRENT_ORDERS, &params)
                .await
            {
                Ok(r) => r,
                Err(e) if e.is_session_error() || e.is_rate_limit_error() => {
                    if e.is_rate_limit_error() {
                        log::warn!("Rate limited, retrying in {RATE_LIMIT_RETRY_DELAY_SECS}s");
                        tokio::time::sleep(tokio::time::Duration::from_secs(
                            RATE_LIMIT_RETRY_DELAY_SECS,
                        ))
                        .await;
                    } else {
                        log::warn!("Session error, refreshing session");

                        if self.http_client.keep_alive().await.is_err() {
                            let _ = self.http_client.reconnect().await;
                        }
                    }
                    self.http_client
                        .send_betting(METHOD_LIST_CURRENT_ORDERS, &params)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?
                }
                Err(e) => anyhow::bail!("{e}"),
            };

            let page_size = response.current_orders.len() as u32;

            for order in &response.current_orders {
                let size_matched = order.size_matched.unwrap_or(Decimal::ZERO);
                if size_matched == Decimal::ZERO {
                    continue;
                }

                match parse_current_order_fill_report(
                    order,
                    self.core.account_id,
                    self.currency,
                    ts_init,
                ) {
                    Ok(mut r) => {
                        if let Some(ref rfo) = order.customer_order_ref
                            && let Ok(state) = self.ocm_state.lock()
                            && let Some(full_id) = state.resolve_client_order_id(Some(rfo.as_str()))
                        {
                            r.client_order_id = Some(full_id);
                        }
                        reports.push(r);
                    }
                    Err(e) => {
                        log::warn!("Failed to parse fill report for {}: {e}", order.bet_id);
                    }
                }
            }

            if !response.more_available {
                break;
            }

            from_record += page_size;
        }

        log::info!("Generated {} fill reports", reports.len());
        Ok(reports)
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;

        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        if let Ok(mut state) = self.ocm_state.lock() {
            state.register_customer_order_ref(order.client_order_id());
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

                // BSP LimitOnClose: participates in starting price calculation
                // with a price limit, using liability instead of size
                if matches!(
                    order.time_in_force(),
                    TimeInForce::AtTheClose | TimeInForce::AtTheOpen
                ) {
                    PlaceInstruction {
                        order_type: BetfairOrderType::LimitOnClose,
                        selection_id,
                        handicap: handicap_opt,
                        side,
                        limit_order: None,
                        limit_on_close_order: Some(LimitOnCloseOrder {
                            liability: size,
                            price,
                        }),
                        market_on_close_order: None,
                        customer_order_ref,
                    }
                } else {
                    let (persistence_type, time_in_force, min_fill_size) =
                        match order.time_in_force() {
                            TimeInForce::Ioc => (
                                None,
                                Some(BetfairTimeInForce::FillOrKill),
                                Some(Decimal::ZERO),
                            ),
                            TimeInForce::Fok => (None, Some(BetfairTimeInForce::FillOrKill), None),
                            TimeInForce::Gtc => (Some(PersistenceType::Persist), None, None),
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

        let market_version = self.get_market_version(&instrument_id);

        let params = PlaceOrdersParams {
            market_id,
            instructions: vec![instruction],
            customer_ref: None,
            market_version,
            customer_strategy_ref: None,
        };

        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();

        log::debug!("OrderSubmitted client_order_id={client_order_id}");
        self.emitter.emit_order_submitted(&order);

        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let ocm_state = Arc::clone(&self.ocm_state);

        self.spawn_task("submit-order", async move {
            let report: PlaceExecutionReport = match http_client
                .send_betting_order(METHOD_PLACE_ORDERS, &params)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    // Transport errors (502, timeout, network reset) may mean the
                    // order was placed but the response was lost. Do not reject
                    // because the OCM stream will reconcile via customerOrderRef.
                    if e.is_order_placement_ambiguous() {
                        log::warn!(
                            "Ambiguous submit response for {client_order_id}: {e}. \
                             Order may be live, awaiting OCM reconciliation",
                        );
                        return Ok(());
                    }

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

            if report.status == ExecutionReportStatus::Timeout {
                log::warn!(
                    "Betfair Timeout for {client_order_id}. \
                     Order may be live, awaiting OCM reconciliation",
                );
                return Ok(());
            }

            if let Some(instruction_reports) = &report.instruction_reports {
                if let Some(ir) = instruction_reports.first() {
                    if ir.status == InstructionReportStatus::Failure {
                        let reason = format_place_instruction_reason(ir, &report);
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            &reason,
                            ts_event,
                            false,
                        );
                        return Ok(());
                    }

                    if let Some(bet_id) = &ir.bet_id {
                        let venue_order_id = VenueOrderId::from(bet_id.as_str());
                        let ts_event = clock.get_time_ns();

                        if should_emit_http_accept(&ocm_state, &client_order_id) {
                            emitter.emit_order_accepted(&order, venue_order_id, ts_event);
                        }
                    }
                } else if report.status == ExecutionReportStatus::Failure
                    || report.status == ExecutionReportStatus::ProcessedWithErrors
                {
                    let reason = format_betfair_reason(
                        report.error_message.as_deref(),
                        report.error_code,
                        None,
                        "unknown error",
                    );
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
            } else if report.status == ExecutionReportStatus::Failure
                || report.status == ExecutionReportStatus::ProcessedWithErrors
            {
                let reason = format_betfair_reason(
                    report.error_message.as_deref(),
                    report.error_code,
                    None,
                    "unknown error",
                );
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

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
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
                .send_betting_order(METHOD_CANCEL_ORDERS, &params)
                .await;

            let report = match result {
                Ok(r) => r,
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
                    return Ok(());
                }
            };

            if report.status == ExecutionReportStatus::Timeout {
                log::warn!(
                    "Betfair Timeout for cancel {client_order_id}. \
                     Cancel may be delayed (in-play), awaiting OCM reconciliation",
                );
                return Ok(());
            }

            if let Some(instruction_reports) = &report.instruction_reports
                && !instruction_reports.is_empty()
            {
                for ir in instruction_reports {
                    match ir.status {
                        InstructionReportStatus::Success => {}
                        InstructionReportStatus::Timeout => {
                            log::warn!(
                                "Cancel instruction timeout for {client_order_id}",
                            );
                        }
                        InstructionReportStatus::Failure => {
                            if ir.error_code
                                == Some(InstructionReportErrorCode::BetTakenOrLapsed)
                            {
                                log::debug!(
                                    "Cancel {client_order_id}: BetTakenOrLapsed, treating as success",
                                );
                                continue;
                            }

                            let reason = format_cancel_instruction_reason(
                                ir.error_message.as_deref(),
                                ir.error_code,
                                report.error_message.as_deref(),
                                report.error_code,
                            );
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_cancel_rejected_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                Some(venue_order_id),
                                &reason,
                                ts_event,
                            );
                            return Ok(());
                        }
                    }
                }
            } else if report.status != ExecutionReportStatus::Success {
                let reason = format_betfair_reason(
                    report.error_message.as_deref(),
                    report.error_code,
                    None,
                    "unknown error",
                );
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

            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
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
            let old_bet_id = bet_id.clone();

            // Track pending replace so the OCM handler suppresses the
            // cancel event for the old bet that Betfair emits as part
            // of the replace operation.
            if let Ok(mut state) = self.ocm_state.lock() {
                state
                    .pending_update_keys
                    .insert((client_order_id, old_bet_id.clone()));
            }

            let market_version = self.get_market_version(&instrument_id);

            let params = ReplaceOrdersParams {
                market_id,
                instructions: vec![ReplaceInstruction { bet_id, new_price }],
                customer_ref: None,
                market_version,
            };

            let ocm_state = Arc::clone(&self.ocm_state);

            self.spawn_task("modify-order-price", async move {
                let result: Result<ReplaceExecutionReport, _> = http_client
                    .send_betting_order(METHOD_REPLACE_ORDERS, &params)
                    .await;

                match result {
                    Ok(report) if report.status == ExecutionReportStatus::Success => {
                        if let Ok(mut state) = ocm_state.lock() {
                            state
                                .pending_update_keys
                                .remove(&(client_order_id, old_bet_id.clone()));
                            state.replaced_venue_order_ids.insert(old_bet_id);
                        }
                    }
                    Ok(report) if report.status == ExecutionReportStatus::Timeout => {
                        log::warn!(
                            "Betfair Timeout for modify {client_order_id}. \
                             Replace may be pending, awaiting OCM reconciliation",
                        );
                    }
                    Ok(report) => {
                        if let Ok(mut state) = ocm_state.lock() {
                            state
                                .pending_update_keys
                                .remove(&(client_order_id, old_bet_id));
                        }

                        if let Some(instruction_reports) = &report.instruction_reports
                            && !instruction_reports.is_empty()
                        {
                            for ir in instruction_reports {
                                match ir.status {
                                    InstructionReportStatus::Success => {}
                                    InstructionReportStatus::Timeout => {
                                        log::warn!(
                                            "Replace instruction timeout for {client_order_id}",
                                        );
                                    }
                                    InstructionReportStatus::Failure => {
                                        let reason = format_replace_instruction_reason(ir, &report);
                                        let ts_event = clock.get_time_ns();
                                        emitter.emit_order_modify_rejected_event(
                                            strategy_id,
                                            instrument_id,
                                            client_order_id,
                                            Some(venue_order_id),
                                            &reason,
                                            ts_event,
                                        );
                                        return Ok(());
                                    }
                                }
                            }
                        }

                        let reason = format_betfair_reason(
                            report.error_message.as_deref(),
                            report.error_code,
                            None,
                            "unknown error",
                        );
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
                        if let Ok(mut state) = ocm_state.lock() {
                            state
                                .pending_update_keys
                                .remove(&(client_order_id, old_bet_id));
                        }
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
                    .send_betting_order(METHOD_CANCEL_ORDERS, &params)
                    .await;

                match result {
                    Ok(report) if report.status != ExecutionReportStatus::Success => {
                        let reason = format_betfair_reason(
                            report.error_message.as_deref(),
                            report.error_code,
                            None,
                            "unknown error",
                        );
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

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
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
                .send_betting_order::<serde_json::Value, _>(METHOD_CANCEL_ORDERS, &params)
                .await;

            if let Err(e) = result {
                log::warn!("Failed to cancel all orders: {e}");
            }

            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let market_id = extract_market_id(&instrument_id)?;

        let mut instructions = Vec::new();
        let mut valid_cancels = Vec::new();

        for cancel in &cmd.cancels {
            match cancel.venue_order_id {
                Some(venue_order_id) => {
                    let bet_id: BetId = venue_order_id.to_string();
                    instructions.push(CancelInstruction {
                        bet_id,
                        size_reduction: None,
                    });
                    valid_cancels.push(cancel);
                }
                None => {
                    let ts_event = self.clock.get_time_ns();
                    self.emitter.emit_order_cancel_rejected_event(
                        cancel.strategy_id,
                        cancel.instrument_id,
                        cancel.client_order_id,
                        None,
                        "no venue_order_id",
                        ts_event,
                    );
                }
            }
        }

        if valid_cancels.is_empty() {
            return Ok(());
        }

        let params = CancelOrdersParams {
            market_id: Some(market_id),
            instructions: Some(instructions),
            customer_ref: None,
        };

        let cancel_data: Vec<_> = valid_cancels
            .iter()
            .map(|c| {
                (
                    c.strategy_id,
                    c.instrument_id,
                    c.client_order_id,
                    c.venue_order_id,
                )
            })
            .collect();

        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("batch-cancel-orders", async move {
            let report: CancelExecutionReport = match http_client
                .send_betting_order(METHOD_CANCEL_ORDERS, &params)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let ts_event = clock.get_time_ns();

                    for (strategy_id, instr_id, client_oid, venue_oid) in &cancel_data {
                        emitter.emit_order_cancel_rejected_event(
                            *strategy_id,
                            *instr_id,
                            *client_oid,
                            *venue_oid,
                            &format!("batch-cancel error: {e}"),
                            ts_event,
                        );
                    }
                    return Ok(());
                }
            };

            if report.status == ExecutionReportStatus::Failure {
                let reason = format_betfair_reason(
                    report.error_message.as_deref(),
                    report.error_code,
                    None,
                    "unknown error",
                );

                if report.instruction_reports.is_none() {
                    let ts_event = clock.get_time_ns();

                    for (strategy_id, instr_id, client_oid, venue_oid) in &cancel_data {
                        emitter.emit_order_cancel_rejected_event(
                            *strategy_id,
                            *instr_id,
                            *client_oid,
                            *venue_oid,
                            &reason,
                            ts_event,
                        );
                    }
                    return Ok(());
                }
            }

            if let Some(instruction_reports) = &report.instruction_reports {
                for (ir, (strategy_id, instr_id, client_oid, venue_oid)) in
                    instruction_reports.iter().zip(cancel_data.iter())
                {
                    match ir.status {
                        InstructionReportStatus::Success => {}
                        InstructionReportStatus::Timeout => {
                            log::warn!(
                                "Cancel timeout for {client_oid}: leaving order state unchanged",
                            );
                        }
                        InstructionReportStatus::Failure => {
                            // BetTakenOrLapsed means the bet already completed, treat as success
                            if ir.error_code == Some(InstructionReportErrorCode::BetTakenOrLapsed) {
                                continue;
                            }

                            let reason = format_cancel_instruction_reason(
                                ir.error_message.as_deref(),
                                ir.error_code,
                                report.error_message.as_deref(),
                                report.error_code,
                            );
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_cancel_rejected_event(
                                *strategy_id,
                                *instr_id,
                                *client_oid,
                                *venue_oid,
                                &reason,
                                ts_event,
                            );
                        }
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let market_id = extract_market_id(&instrument_id)?;
        let (selection_id, handicap) = extract_selection_id(&instrument_id)?;

        let handicap_opt = if handicap == Decimal::ZERO {
            None
        } else {
            Some(handicap)
        };

        let mut instructions = Vec::new();
        let mut order_snapshots = Vec::new();

        for client_order_id in &cmd.order_list.client_order_ids {
            let order = self.core.get_order(client_order_id)?;

            if order.is_closed() {
                log::warn!("Skipping closed order {client_order_id}");
                continue;
            }

            if let Ok(mut state) = self.ocm_state.lock() {
                state.register_customer_order_ref(order.client_order_id());
            }

            let side = BetfairSide::from(order.order_side());
            let size = order.quantity().as_decimal();
            let customer_order_ref =
                Some(make_customer_order_ref(order.client_order_id().as_str()));

            let instruction = match order.order_type() {
                OrderType::Limit => {
                    let price = order
                        .price()
                        .ok_or_else(|| anyhow::anyhow!("Limit order missing price"))?
                        .as_decimal();

                    if matches!(
                        order.time_in_force(),
                        TimeInForce::AtTheClose | TimeInForce::AtTheOpen
                    ) {
                        PlaceInstruction {
                            order_type: BetfairOrderType::LimitOnClose,
                            selection_id,
                            handicap: handicap_opt,
                            side,
                            limit_order: None,
                            limit_on_close_order: Some(LimitOnCloseOrder {
                                liability: size,
                                price,
                            }),
                            market_on_close_order: None,
                            customer_order_ref,
                        }
                    } else {
                        let (persistence_type, time_in_force, min_fill_size) = match order
                            .time_in_force()
                        {
                            TimeInForce::Ioc => (
                                None,
                                Some(BetfairTimeInForce::FillOrKill),
                                Some(Decimal::ZERO),
                            ),
                            TimeInForce::Fok => (None, Some(BetfairTimeInForce::FillOrKill), None),
                            TimeInForce::Gtc => (Some(PersistenceType::Persist), None, None),
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

            instructions.push(instruction);
            order_snapshots.push((order.client_order_id(), order.strategy_id(), order.clone()));
        }

        if instructions.is_empty() {
            return Ok(());
        }

        let market_version = self.get_market_version(&instrument_id);

        let params = PlaceOrdersParams {
            market_id,
            instructions,
            customer_ref: None,
            market_version,
            customer_strategy_ref: None,
        };

        for (_, _, order) in &order_snapshots {
            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            self.emitter.emit_order_submitted(order);
        }

        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let ocm_state = Arc::clone(&self.ocm_state);

        self.spawn_task("submit-order-list", async move {
            let report: PlaceExecutionReport = match http_client
                .send_betting_order(METHOD_PLACE_ORDERS, &params)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    if e.is_order_placement_ambiguous() {
                        log::warn!(
                            "Ambiguous submit response for order list: {e}. \
                             Orders may be live, awaiting OCM reconciliation",
                        );
                        return Ok(());
                    }

                    let ts_event = clock.get_time_ns();

                    for (client_oid, strategy_id, _) in &order_snapshots {
                        emitter.emit_order_rejected_event(
                            *strategy_id,
                            instrument_id,
                            *client_oid,
                            &format!("submit-order-list error: {e}"),
                            ts_event,
                            false,
                        );
                    }
                    return Ok(());
                }
            };

            if report.status == ExecutionReportStatus::Failure {
                let reason = format_betfair_reason(
                    report.error_message.as_deref(),
                    report.error_code,
                    None,
                    "unknown error",
                );

                if report.instruction_reports.is_none() {
                    let ts_event = clock.get_time_ns();

                    for (client_oid, strategy_id, _) in &order_snapshots {
                        emitter.emit_order_rejected_event(
                            *strategy_id,
                            instrument_id,
                            *client_oid,
                            &reason,
                            ts_event,
                            false,
                        );
                    }
                    return Ok(());
                }
            }

            if report.status == ExecutionReportStatus::Timeout {
                log::warn!(
                    "Betfair Timeout for order list. \
                     Orders may be live, awaiting OCM reconciliation",
                );
                return Ok(());
            }

            if let Some(instruction_reports) = &report.instruction_reports {
                for (ir, (client_oid, strategy_id, order)) in
                    instruction_reports.iter().zip(order_snapshots.iter())
                {
                    match ir.status {
                        InstructionReportStatus::Success => {
                            if let Some(bet_id) = &ir.bet_id {
                                let venue_order_id = VenueOrderId::from(bet_id.as_str());
                                let ts_event = clock.get_time_ns();

                                if should_emit_http_accept(&ocm_state, client_oid) {
                                    emitter.emit_order_accepted(order, venue_order_id, ts_event);
                                }
                            }
                        }
                        InstructionReportStatus::Timeout => {
                            log::warn!(
                                "Submit timeout for {client_oid}: \
                                 leaving SUBMITTED for reconciliation",
                            );
                        }
                        InstructionReportStatus::Failure => {
                            let reason = format_place_instruction_reason(ir, &report);
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_rejected_event(
                                *strategy_id,
                                instrument_id,
                                *client_oid,
                                &reason,
                                ts_event,
                                false,
                            );
                        }
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }
}

fn list_current_orders_filter_bet_id(bet_id: String) -> ListCurrentOrdersParams {
    ListCurrentOrdersParams {
        bet_ids: Some(vec![bet_id]),
        market_ids: None,
        order_projection: None,
        customer_order_refs: None,
        customer_strategy_refs: None,
        date_range: None,
        order_by: None,
        sort_dir: None,
        from_record: None,
        record_count: None,
    }
}

fn list_current_orders_filter_ref(customer_order_ref: String) -> ListCurrentOrdersParams {
    ListCurrentOrdersParams {
        bet_ids: None,
        market_ids: None,
        order_projection: None,
        customer_order_refs: Some(vec![customer_order_ref]),
        customer_strategy_refs: None,
        date_range: None,
        order_by: None,
        sort_dir: None,
        from_record: None,
        record_count: None,
    }
}

fn extend_unique(
    candidates: &mut Vec<CurrentOrderSummary>,
    seen: &mut AHashSet<String>,
    orders: Vec<CurrentOrderSummary>,
) {
    for order in orders {
        if seen.insert(order.bet_id.clone()) {
            candidates.push(order);
        }
    }
}

fn select_order_for_query(
    orders: &[CurrentOrderSummary],
    expected_instrument_id: InstrumentId,
    expected_client_order_id: ClientOrderId,
    expected_venue_order_id: Option<VenueOrderId>,
) -> Option<&CurrentOrderSummary> {
    let matching: Vec<&CurrentOrderSummary> = orders
        .iter()
        .filter(|o| {
            make_instrument_id(&o.market_id, o.selection_id, o.handicap) == expected_instrument_id
        })
        .collect();

    let candidates: Vec<&CurrentOrderSummary> = if matching.is_empty() {
        // No instrument match: accept only an exact venue_order_id hit
        // (pre-existing orders without a recognizable customer_order_ref).
        // A lone foreign-instrument candidate is not enough, since a 32-char
        // customer_order_ref collision can surface a single unrelated bet.
        if let Some(vid) = expected_venue_order_id
            && let Some(order) = orders.iter().find(|o| o.bet_id == vid.as_str())
        {
            return Some(order);
        }
        log::warn!(
            "Betfair query_order returned {} orders for client_order_id={expected_client_order_id}, none matching instrument {expected_instrument_id}; skipping to avoid cross-instrument reconciliation",
            orders.len(),
        );
        return None;
    } else {
        matching
    };

    // Prefer EXECUTABLE so a live replacement wins over a cancelled
    // predecessor sharing the same customer_order_ref.
    let executable: Vec<&CurrentOrderSummary> = candidates
        .iter()
        .copied()
        .filter(|o| o.status == BetfairOrderStatus::Executable)
        .collect();

    let pool = if executable.is_empty() {
        candidates
    } else {
        executable
    };

    // Tiebreaker: most recently placed bet. Picks the replacement over the
    // predecessor even when both are already terminal by poll time.
    pool.into_iter()
        .max_by(|a, b| a.placed_date.cmp(&b.placed_date))
}

async fn list_current_orders_with_retry(
    http_client: &Arc<BetfairHttpClient>,
    params: &ListCurrentOrdersParams,
) -> anyhow::Result<CurrentOrderSummaryReport> {
    match http_client
        .send_betting(METHOD_LIST_CURRENT_ORDERS, params)
        .await
    {
        Ok(r) => Ok(r),
        Err(e) if e.is_session_error() || e.is_rate_limit_error() => {
            if e.is_rate_limit_error() {
                log::warn!("Rate limited, retrying in {RATE_LIMIT_RETRY_DELAY_SECS}s");
                tokio::time::sleep(tokio::time::Duration::from_secs(
                    RATE_LIMIT_RETRY_DELAY_SECS,
                ))
                .await;
            } else {
                log::warn!("Session error, refreshing session");

                if http_client.keep_alive().await.is_err() {
                    let _ = http_client.reconnect().await;
                }
            }
            http_client
                .send_betting(METHOD_LIST_CURRENT_ORDERS, params)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
        Err(e) => Err(anyhow::anyhow!("{e}")),
    }
}

fn should_emit_http_accept(
    ocm_state: &Arc<Mutex<OcmState>>,
    client_order_id: &ClientOrderId,
) -> bool {
    let Ok(state) = ocm_state.lock() else {
        log::error!("OcmState mutex poisoned");
        return true;
    };

    if state
        .stream_reported_client_orders
        .contains(client_order_id)
    {
        log::info!(
            "Suppressing late HTTP acceptance for {client_order_id}: OCM already reported order state"
        );
        return false;
    }

    true
}

fn format_betfair_reason(
    error_message: Option<&str>,
    error_code: Option<impl fmt::Debug>,
    fallback: Option<String>,
    unknown: &str,
) -> String {
    if let Some(message) = error_message
        .map(str::trim)
        .filter(|message| !message.is_empty())
    {
        return match error_code {
            Some(code) => format!("{message} ({code:?})"),
            None => message.to_string(),
        };
    }

    error_code
        .map(|code| format!("{code:?}"))
        .or(fallback.filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| unknown.to_string())
}

fn format_place_instruction_reason(
    instruction_report: &PlaceInstructionReport,
    report: &PlaceExecutionReport,
) -> String {
    format_betfair_reason(
        instruction_report.error_message.as_deref(),
        instruction_report.error_code,
        report_fallback(report.error_message.as_deref(), report.error_code),
        "unknown error",
    )
}

fn format_cancel_instruction_reason(
    error_message: Option<&str>,
    error_code: Option<InstructionReportErrorCode>,
    report_error_message: Option<&str>,
    report_error_code: Option<ExecutionReportErrorCode>,
) -> String {
    format_betfair_reason(
        error_message,
        error_code,
        report_fallback(report_error_message, report_error_code),
        "unknown instruction error",
    )
}

fn format_replace_instruction_reason(
    instruction_report: &ReplaceInstructionReport,
    report: &ReplaceExecutionReport,
) -> String {
    let nested_reason = instruction_report
        .place_instruction_report
        .as_ref()
        .and_then(|ir| instruction_fallback(ir.error_message.as_deref(), ir.error_code))
        .or_else(|| {
            instruction_report
                .cancel_instruction_report
                .as_ref()
                .and_then(|ir| instruction_fallback(ir.error_message.as_deref(), ir.error_code))
        });

    format_betfair_reason(
        instruction_report.error_message.as_deref(),
        instruction_report.error_code,
        nested_reason
            .or_else(|| report_fallback(report.error_message.as_deref(), report.error_code)),
        "unknown instruction error",
    )
}

fn report_fallback(
    error_message: Option<&str>,
    error_code: Option<ExecutionReportErrorCode>,
) -> Option<String> {
    error_message
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| error_code.map(|code| format!("{code:?}")))
}

fn instruction_fallback(
    error_message: Option<&str>,
    error_code: Option<InstructionReportErrorCode>,
) -> Option<String> {
    error_message
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| error_code.map(|code| format!("{code:?}")))
}

#[cfg(test)]
mod tests {
    use nautilus_model::types::Quantity;
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;

    #[rstest]
    #[case(
        Some("Price out of range"),
        Some(InstructionReportErrorCode::InvalidOdds),
        None,
        "unknown",
        "Price out of range (InvalidOdds)"
    )]
    #[case(
        Some("Price out of range"),
        None,
        None,
        "unknown",
        "Price out of range"
    )]
    #[case(
        None,
        Some(InstructionReportErrorCode::ErrorInOrder),
        None,
        "unknown",
        "ErrorInOrder"
    )]
    #[case(None, None, Some("report-level msg".to_string()), "unknown", "report-level msg")]
    #[case(None, None, None, "unknown error", "unknown error")]
    #[case(
        Some("  "),
        Some(InstructionReportErrorCode::ErrorInOrder),
        None,
        "unknown",
        "ErrorInOrder"
    )]
    #[case(Some(""), None, Some(String::new()), "fallback", "fallback")]
    #[case(Some("  \n "), None, Some("  ".to_string()), "unknown", "unknown")]
    fn test_format_betfair_reason(
        #[case] error_message: Option<&str>,
        #[case] error_code: Option<InstructionReportErrorCode>,
        #[case] fallback: Option<String>,
        #[case] unknown: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(
            format_betfair_reason(error_message, error_code, fallback, unknown),
            expected,
        );
    }

    #[rstest]
    fn test_ocm_state_register_and_resolve() {
        let mut state = OcmState::default();
        let client_oid = ClientOrderId::from("O-20240101-001");

        state.register_customer_order_ref(client_oid);

        let rfo = make_customer_order_ref(client_oid.as_str());
        let resolved = state.resolve_client_order_id(Some(&rfo));
        assert_eq!(resolved, Some(client_oid));
    }

    #[rstest]
    fn test_ocm_state_resolve_none_for_unknown_rfo() {
        let state = OcmState::default();
        assert!(state.resolve_client_order_id(Some("unknown")).is_none());
        assert!(state.resolve_client_order_id(None).is_none());
    }

    #[rstest]
    fn test_ocm_state_register_with_legacy() {
        let mut state = OcmState::default();
        let id = "O-20240101-550e8400-e29b-41d4-a716-446655440000";
        let client_oid = ClientOrderId::from(id);

        state.register_customer_order_ref_with_legacy(client_oid);

        let rfo_current = make_customer_order_ref(id);
        let rfo_legacy = make_customer_order_ref_legacy(id);
        assert_ne!(rfo_current, rfo_legacy);

        assert_eq!(
            state.resolve_client_order_id(Some(&rfo_current)),
            Some(client_oid)
        );
        assert_eq!(
            state.resolve_client_order_id(Some(&rfo_legacy)),
            Some(client_oid)
        );
    }

    #[rstest]
    fn test_ocm_state_remove_customer_order_refs() {
        let mut state = OcmState::default();
        let id = "O-20240101-550e8400-e29b-41d4-a716-446655440000";
        let client_oid = ClientOrderId::from(id);

        state.register_customer_order_ref_with_legacy(client_oid);
        state.remove_customer_order_refs(&client_oid);

        let rfo_current = make_customer_order_ref(id);
        let rfo_legacy = make_customer_order_ref_legacy(id);
        assert!(state.resolve_client_order_id(Some(&rfo_current)).is_none());
        assert!(state.resolve_client_order_id(Some(&rfo_legacy)).is_none());
    }

    #[rstest]
    fn test_should_emit_http_accept_without_stream_report() {
        let state = Arc::new(Mutex::new(OcmState::default()));
        let client_oid = ClientOrderId::from("O-001");

        assert!(should_emit_http_accept(&state, &client_oid));
    }

    #[rstest]
    fn test_should_not_emit_http_accept_after_stream_report() {
        let client_oid = ClientOrderId::from("O-001");
        let mut inner = OcmState::default();
        inner.stream_reported_client_orders.insert(client_oid);
        let state = Arc::new(Mutex::new(inner));

        assert!(!should_emit_http_accept(&state, &client_oid));
    }

    #[rstest]
    fn test_ocm_state_terminal_deduplication() {
        let mut state = OcmState::default();

        // First call marks as terminal, returns false (not duplicate)
        assert!(!state.try_mark_terminal("bet123"));

        // Second call returns true (already terminal)
        assert!(state.try_mark_terminal("bet123"));
    }

    #[rstest]
    fn test_ocm_state_suppress_cancel_for_replaced() {
        let mut state = OcmState::default();
        let client_oid = ClientOrderId::from("O-001");

        state.replaced_venue_order_ids.insert("old_bet".to_string());
        assert!(state.should_suppress_cancel(&client_oid, "old_bet"));
        assert!(!state.should_suppress_cancel(&client_oid, "new_bet"));
    }

    #[rstest]
    fn test_ocm_state_suppress_cancel_for_pending_replace() {
        let mut state = OcmState::default();
        let client_oid = ClientOrderId::from("O-001");

        state
            .pending_update_keys
            .insert((client_oid, "old_bet".to_string()));

        assert!(state.should_suppress_cancel(&client_oid, "old_bet"));
        assert!(!state.should_suppress_cancel(&client_oid, "other_bet"));
    }

    #[rstest]
    fn test_ocm_state_cleanup_terminal_with_pending_replace() {
        let mut state = OcmState::default();
        let client_oid = ClientOrderId::from("O-001");

        state.register_customer_order_ref(client_oid);
        state
            .pending_update_keys
            .insert((client_oid, "old_bet".to_string()));

        // Should NOT remove refs because replace is pending
        state.cleanup_terminal_order(&client_oid);
        let rfo = make_customer_order_ref(client_oid.as_str());
        assert!(state.resolve_client_order_id(Some(&rfo)).is_some());
    }

    #[rstest]
    fn test_ocm_state_cleanup_terminal_without_pending() {
        let mut state = OcmState::default();
        let client_oid = ClientOrderId::from("O-001");

        state.register_customer_order_ref(client_oid);

        // Should remove refs because no pending replace
        state.cleanup_terminal_order(&client_oid);
        let rfo = make_customer_order_ref(client_oid.as_str());
        assert!(state.resolve_client_order_id(Some(&rfo)).is_none());
    }

    #[rstest]
    fn test_ocm_state_sync_from_orders() {
        let mut state = OcmState::default();

        let orders = vec![
            (
                "bet1".to_string(),
                ClientOrderId::from("O-001"),
                Decimal::new(10, 0),
                Decimal::new(25, 1),
                false,
            ),
            (
                "bet2".to_string(),
                ClientOrderId::from("O-002"),
                Decimal::new(5, 0),
                Decimal::new(30, 1),
                true,
            ),
        ];

        state.sync_from_orders(&orders);

        // Open order: should have customer_order_ref registered
        let rfo1 = make_customer_order_ref("O-001");
        assert!(state.resolve_client_order_id(Some(&rfo1)).is_some());

        // Closed order: should be in terminal_orders, no customer_order_ref
        assert!(state.terminal_orders.contains("bet2"));
        let rfo2 = make_customer_order_ref("O-002");
        assert!(state.resolve_client_order_id(Some(&rfo2)).is_none());
    }

    #[rstest]
    fn test_reconnect_signal_not_sent_on_initial_connection() {
        let has_initial_connection = Arc::new(AtomicBool::new(false));
        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let has_initial = Arc::clone(&has_initial_connection);
        let handler = move |_data: &[u8]| {
            if has_initial.swap(true, Ordering::SeqCst) {
                let _ = reconnect_tx.send(());
            }
        };

        // First connection message: no signal
        handler(br#"{"op":"connection","connectionId":"abc"}"#);
        assert!(reconnect_rx.try_recv().is_err());
        assert!(has_initial_connection.load(Ordering::SeqCst));
    }

    #[rstest]
    fn test_reconnect_signal_sent_on_subsequent_connection() {
        let has_initial_connection = Arc::new(AtomicBool::new(false));
        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let has_initial = Arc::clone(&has_initial_connection);
        let tx = reconnect_tx;
        let handler = move |_data: &[u8]| {
            if has_initial.swap(true, Ordering::SeqCst) {
                let _ = tx.send(());
            }
        };

        // First connection: no signal
        handler(br#"{"op":"connection","connectionId":"abc"}"#);
        assert!(reconnect_rx.try_recv().is_err());

        // Second connection: signal sent
        handler(br#"{"op":"connection","connectionId":"def"}"#);
        assert!(reconnect_rx.try_recv().is_ok());

        // Third connection: signal sent again
        handler(br#"{"op":"connection","connectionId":"ghi"}"#);
        assert!(reconnect_rx.try_recv().is_ok());
    }

    #[rstest]
    fn test_ocm_state_persists_across_reconnections() {
        let ocm_state = Arc::new(Mutex::new(OcmState::default()));

        // Populate state before "reconnect"
        {
            let mut state = ocm_state.lock().unwrap();
            let orders = vec![
                (
                    "bet1".to_string(),
                    ClientOrderId::from("O-001"),
                    Decimal::new(10, 0),
                    Decimal::new(25, 1),
                    false,
                ),
                (
                    "bet2".to_string(),
                    ClientOrderId::from("O-002"),
                    Decimal::ZERO,
                    Decimal::ZERO,
                    true,
                ),
            ];
            state.sync_from_orders(&orders);
        }

        // Verify state survives (simulates reconnection where Arc<Mutex<OcmState>> persists)
        let state = ocm_state.lock().unwrap();
        let rfo = make_customer_order_ref("O-001");
        assert_eq!(
            state.resolve_client_order_id(Some(&rfo)),
            Some(ClientOrderId::from("O-001")),
        );
        assert!(state.terminal_orders.contains("bet2"));
        assert!(!state.terminal_orders.contains("bet1"));
    }

    #[rstest]
    fn test_ocm_state_sync_from_orders_populates_fill_tracker() {
        let mut state = OcmState::default();

        let orders = vec![(
            "bet_fill".to_string(),
            ClientOrderId::from("O-FILL-001"),
            Decimal::new(15, 0),
            Decimal::new(30, 1),
            false,
        )];

        state.sync_from_orders(&orders);

        // Fill tracker should be pre-populated so that a stream update with
        // sm=15 does NOT produce a duplicate fill
        let uo = crate::stream::messages::UnmatchedOrder {
            id: "bet_fill".to_string(),
            p: Decimal::new(30, 1),
            s: Decimal::new(20, 0),
            side: crate::common::enums::StreamingSide::Back,
            status: crate::common::enums::StreamingOrderStatus::Executable,
            pt: Some(crate::common::enums::StreamingPersistenceType::Lapse),
            ot: crate::common::enums::StreamingOrderType::Limit,
            pd: 1617863365000,
            bsp: None,
            rfo: Some("O-FILL-001".to_string()),
            rfs: None,
            rc: None,
            rac: None,
            md: None,
            cd: None,
            ld: None,
            avp: Some(Decimal::new(30, 1)),
            sm: Some(Decimal::new(15, 0)),
            sr: None,
            sl: None,
            sc: None,
            sv: None,
            lsrc: None,
        };

        let instrument_id = InstrumentId::from("1.234567-12345-0.0.BETFAIR");
        let result = state.fill_tracker.maybe_fill_report(
            &uo,
            uo.s,
            instrument_id,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(
            result.is_none(),
            "synced fill should prevent duplicate fill report"
        );
    }

    #[rstest]
    fn test_ocm_state_sync_from_orders_incremental_fill_after_sync() {
        let mut state = OcmState::default();

        let orders = vec![(
            "bet_inc".to_string(),
            ClientOrderId::from("O-INC-001"),
            Decimal::new(10, 0),
            Decimal::new(25, 1),
            false,
        )];

        state.sync_from_orders(&orders);

        // Stream update with sm=18 (8 more than synced 10)
        let uo = crate::stream::messages::UnmatchedOrder {
            id: "bet_inc".to_string(),
            p: Decimal::new(25, 1),
            s: Decimal::new(20, 0),
            side: crate::common::enums::StreamingSide::Lay,
            status: crate::common::enums::StreamingOrderStatus::Executable,
            pt: Some(crate::common::enums::StreamingPersistenceType::Persist),
            ot: crate::common::enums::StreamingOrderType::Limit,
            pd: 1617863365000,
            bsp: None,
            rfo: Some("O-INC-001".to_string()),
            rfs: None,
            rc: None,
            rac: None,
            md: None,
            cd: None,
            ld: None,
            avp: Some(Decimal::new(26, 1)),
            sm: Some(Decimal::new(18, 0)),
            sr: None,
            sl: None,
            sc: None,
            sv: None,
            lsrc: None,
        };

        let instrument_id = InstrumentId::from("1.234567-12345-0.0.BETFAIR");
        let result = state.fill_tracker.maybe_fill_report(
            &uo,
            uo.s,
            instrument_id,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let fill = result.expect("should produce incremental fill of 8");
        assert_eq!(fill.last_qty, Quantity::from("8.00"));
    }

    #[rstest]
    fn test_ocm_state_sync_from_orders_zero_filled_not_synced() {
        let mut state = OcmState::default();

        let orders = vec![(
            "bet_zero".to_string(),
            ClientOrderId::from("O-ZERO-001"),
            Decimal::ZERO,
            Decimal::ZERO,
            false,
        )];

        state.sync_from_orders(&orders);

        // RFO should still be registered even if no fills
        let rfo = make_customer_order_ref("O-ZERO-001");
        assert!(state.resolve_client_order_id(Some(&rfo)).is_some());

        // A stream update with sm=5 should produce a fill (not blocked by sync)
        let uo = crate::stream::messages::UnmatchedOrder {
            id: "bet_zero".to_string(),
            p: Decimal::new(30, 1),
            s: Decimal::new(10, 0),
            side: crate::common::enums::StreamingSide::Back,
            status: crate::common::enums::StreamingOrderStatus::Executable,
            pt: Some(crate::common::enums::StreamingPersistenceType::Lapse),
            ot: crate::common::enums::StreamingOrderType::Limit,
            pd: 1617863365000,
            bsp: None,
            rfo: None,
            rfs: None,
            rc: None,
            rac: None,
            md: None,
            cd: None,
            ld: None,
            avp: Some(Decimal::new(30, 1)),
            sm: Some(Decimal::new(5, 0)),
            sr: None,
            sl: None,
            sc: None,
            sv: None,
            lsrc: None,
        };
        let instrument_id = InstrumentId::from("1.234567-12345-0.0.BETFAIR");
        let result = state.fill_tracker.maybe_fill_report(
            &uo,
            uo.s,
            instrument_id,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        assert!(
            result.is_some(),
            "zero-filled order should not block new fills"
        );
    }

    #[rstest]
    fn test_ocm_state_sync_multiple_open_and_closed() {
        let mut state = OcmState::default();

        let orders = vec![
            (
                "bet_a".to_string(),
                ClientOrderId::from("O-A"),
                Decimal::new(5, 0),
                Decimal::new(20, 1),
                false,
            ),
            (
                "bet_b".to_string(),
                ClientOrderId::from("O-B"),
                Decimal::ZERO,
                Decimal::ZERO,
                true,
            ),
            (
                "bet_c".to_string(),
                ClientOrderId::from("O-C"),
                Decimal::new(100, 0),
                Decimal::new(15, 1),
                true,
            ),
            (
                "bet_d".to_string(),
                ClientOrderId::from("O-D"),
                Decimal::ZERO,
                Decimal::ZERO,
                false,
            ),
        ];

        state.sync_from_orders(&orders);

        // Open orders have RFO registered
        assert!(
            state
                .resolve_client_order_id(Some(&make_customer_order_ref("O-A")))
                .is_some()
        );
        assert!(
            state
                .resolve_client_order_id(Some(&make_customer_order_ref("O-D")))
                .is_some()
        );

        // Closed orders are terminal
        assert!(state.terminal_orders.contains("bet_b"));
        assert!(state.terminal_orders.contains("bet_c"));
        assert!(!state.terminal_orders.contains("bet_a"));
        assert!(!state.terminal_orders.contains("bet_d"));

        // Closed orders do NOT get RFO registered
        assert!(
            state
                .resolve_client_order_id(Some(&make_customer_order_ref("O-B")))
                .is_none()
        );
    }

    fn make_summary(
        bet_id: &str,
        market_id: &str,
        selection_id: u64,
        handicap: Decimal,
        status: BetfairOrderStatus,
        placed_date: &str,
    ) -> CurrentOrderSummary {
        CurrentOrderSummary {
            bet_id: bet_id.to_string(),
            market_id: market_id.to_string(),
            selection_id,
            handicap,
            price_size: crate::http::models::PriceSize {
                price: Decimal::new(20, 1),
                size: Decimal::new(10, 0),
            },
            bsp_liability: Decimal::ZERO,
            side: BetfairSide::Back,
            status,
            persistence_type: PersistenceType::Lapse,
            order_type: BetfairOrderType::Limit,
            placed_date: placed_date.to_string(),
            matched_date: None,
            average_price_matched: None,
            size_matched: None,
            size_remaining: Some(Decimal::new(10, 0)),
            size_lapsed: None,
            size_cancelled: None,
            size_voided: None,
            regulator_auth_code: None,
            regulator_code: None,
            customer_order_ref: None,
            customer_strategy_ref: None,
        }
    }

    #[rstest]
    fn test_select_order_for_query_single_executable() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![make_summary(
            "bet_1",
            "1.100",
            12345,
            Decimal::ZERO,
            BetfairOrderStatus::Executable,
            "2026-04-18T10:00:00Z",
        )];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_1"));
    }

    #[rstest]
    fn test_select_order_for_query_single_terminal() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![make_summary(
            "bet_1",
            "1.100",
            12345,
            Decimal::ZERO,
            BetfairOrderStatus::ExecutionComplete,
            "2026-04-18T10:00:00Z",
        )];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_1"));
    }

    #[rstest]
    fn test_select_order_for_query_replace_prefers_executable() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![
            make_summary(
                "bet_old",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::ExecutionComplete,
                "2026-04-18T10:00:00Z",
            ),
            make_summary(
                "bet_new",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:05:00Z",
            ),
        ];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_new"));
    }

    #[rstest]
    fn test_select_order_for_query_multiple_executable_prefers_most_recent() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![
            make_summary(
                "bet_old",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:00:00Z",
            ),
            make_summary(
                "bet_new",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:05:00Z",
            ),
        ];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_new"));
    }

    #[rstest]
    fn test_select_order_for_query_multiple_terminal_prefers_most_recent() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![
            make_summary(
                "bet_old",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::ExecutionComplete,
                "2026-04-18T10:00:00Z",
            ),
            make_summary(
                "bet_new",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::ExecutionComplete,
                "2026-04-18T10:05:00Z",
            ),
        ];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_new"));
    }

    #[rstest]
    fn test_select_order_for_query_foreign_only_without_vid_returns_none() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![make_summary(
            "bet_foreign",
            "1.999",
            99999,
            Decimal::ZERO,
            BetfairOrderStatus::Executable,
            "2026-04-18T10:00:00Z",
        )];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert!(selected.is_none());
    }

    #[rstest]
    fn test_select_order_for_query_foreign_only_with_vid_match_returns_match() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![make_summary(
            "bet_foreign",
            "1.999",
            99999,
            Decimal::ZERO,
            BetfairOrderStatus::Executable,
            "2026-04-18T10:00:00Z",
        )];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);
        let vid = VenueOrderId::from("bet_foreign");

        let selected = select_order_for_query(&orders, expected, cid, Some(vid));
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_foreign"));
    }

    #[rstest]
    fn test_select_order_for_query_foreign_only_vid_mismatch_returns_none() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![
            make_summary(
                "bet_foreign_1",
                "1.999",
                99999,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:00:00Z",
            ),
            make_summary(
                "bet_foreign_2",
                "1.888",
                88888,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:05:00Z",
            ),
        ];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);
        let vid = VenueOrderId::from("bet_unknown");

        let selected = select_order_for_query(&orders, expected, cid, Some(vid));
        assert!(selected.is_none());
    }

    #[rstest]
    fn test_select_order_for_query_mixed_returns_matching_instrument() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![
            make_summary(
                "bet_foreign",
                "1.999",
                99999,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:05:00Z",
            ),
            make_summary(
                "bet_match",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::ExecutionComplete,
                "2026-04-18T10:00:00Z",
            ),
        ];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_match"));
    }

    #[rstest]
    fn test_extend_unique_filters_duplicates() {
        let mut candidates: Vec<CurrentOrderSummary> = Vec::new();
        let mut seen: AHashSet<String> = AHashSet::new();

        let orders = vec![
            make_summary(
                "bet_1",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:00:00Z",
            ),
            make_summary(
                "bet_1",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:01:00Z",
            ),
            make_summary(
                "bet_2",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:02:00Z",
            ),
        ];

        extend_unique(&mut candidates, &mut seen, orders);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].bet_id, "bet_1");
        assert_eq!(candidates[0].placed_date, "2026-04-18T10:00:00Z");
        assert_eq!(candidates[1].bet_id, "bet_2");
        assert!(seen.contains("bet_1"));
        assert!(seen.contains("bet_2"));
    }

    #[rstest]
    fn test_extend_unique_skips_already_seen() {
        let mut candidates: Vec<CurrentOrderSummary> = vec![make_summary(
            "bet_1",
            "1.100",
            12345,
            Decimal::ZERO,
            BetfairOrderStatus::Executable,
            "2026-04-18T10:00:00Z",
        )];
        let mut seen: AHashSet<String> = AHashSet::new();
        seen.insert("bet_1".to_string());

        let orders = vec![make_summary(
            "bet_1",
            "1.100",
            12345,
            Decimal::ZERO,
            BetfairOrderStatus::Executable,
            "2026-04-18T10:05:00Z",
        )];

        extend_unique(&mut candidates, &mut seen, orders);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].placed_date, "2026-04-18T10:00:00Z");
    }

    #[rstest]
    fn test_list_current_orders_filter_bet_id_sets_only_bet_ids() {
        let params = list_current_orders_filter_bet_id("bet_abc".to_string());

        assert_eq!(
            params.bet_ids.as_deref(),
            Some(&["bet_abc".to_string()][..])
        );
        assert!(params.customer_order_refs.is_none());
        assert!(params.market_ids.is_none());
        assert!(params.order_projection.is_none());
        assert!(params.customer_strategy_refs.is_none());
        assert!(params.date_range.is_none());
        assert!(params.order_by.is_none());
        assert!(params.sort_dir.is_none());
        assert!(params.from_record.is_none());
        assert!(params.record_count.is_none());
    }

    #[rstest]
    fn test_list_current_orders_filter_ref_sets_only_customer_order_refs() {
        let params = list_current_orders_filter_ref("rfo_abc".to_string());

        assert_eq!(
            params.customer_order_refs.as_deref(),
            Some(&["rfo_abc".to_string()][..])
        );
        assert!(params.bet_ids.is_none());
        assert!(params.market_ids.is_none());
        assert!(params.order_projection.is_none());
        assert!(params.customer_strategy_refs.is_none());
        assert!(params.date_range.is_none());
        assert!(params.order_by.is_none());
        assert!(params.sort_dir.is_none());
        assert!(params.from_record.is_none());
        assert!(params.record_count.is_none());
    }
}
