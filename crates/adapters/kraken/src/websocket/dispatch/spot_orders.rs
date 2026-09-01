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

//! WebSocket order-request dispatch state for Kraken Spot v2.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use dashmap::DashMap;
use nautilus_core::{UUID4, time::AtomicTime};
use nautilus_live::task::TaskSpawner;
use nautilus_model::{
    events::{
        OrderAccepted, OrderCancelRejected, OrderEventAny, OrderModifyRejected, OrderRejected,
        OrderUpdated,
    },
    identifiers::{AccountId, ClientOrderId, TraderId, VenueOrderId},
    types::{Price, Quantity},
};
use parking_lot::RwLock;
use ustr::Ustr;

use super::WsDispatchState;
use crate::{
    common::parse::truncate_cl_ord_id,
    websocket::spot_v2::{
        enums::KrakenWsMethod,
        handler::SpotHandlerCommand,
        messages::{
            KrakenWsAddOrderParams, KrakenWsAmendOrderParams, KrakenWsBatchAddParams,
            KrakenWsCancelOrderParams, KrakenWsOrderResponse, KrakenWsOrderResult, KrakenWsParams,
            KrakenWsRequest,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingOperation {
    /// Pending add_order request.
    Submit,
    /// Pending amend_order request.
    Amend,
    /// Pending cancel_order request.
    Cancel,
    /// Pending batch_add request.
    BatchAdd,
}

#[derive(Debug, Clone)]
pub struct PendingRequest {
    /// The pending operation type.
    pub operation: PendingOperation,
    /// Client order IDs associated with this request.
    pub client_order_ids: Vec<ClientOrderId>,
    /// Venue order IDs associated with this request, if known.
    pub venue_order_ids: Vec<Option<VenueOrderId>>,
    /// UNIX nanosecond timestamp when the request was sent.
    pub ts_sent_ns: u64,
    /// New quantity carried for amend operations so the success-path
    /// `OrderUpdated` event reflects the requested change rather than the
    /// originally registered quantity. `None` for non-amend operations or
    /// amends that do not modify quantity.
    pub new_quantity: Option<Quantity>,
    /// New limit price carried for amend operations so the success-path
    /// `OrderUpdated` event surfaces the amended price to strategies.
    /// `None` for non-amend operations or amends that do not modify price.
    pub new_price: Option<Price>,
    /// New trigger price carried for amend operations on conditional orders.
    /// `None` for non-amend operations or amends that do not modify the
    /// trigger price.
    pub new_trigger_price: Option<Price>,
}

#[derive(Debug)]
pub struct OrderRequestState {
    req_id_counter: Arc<AtomicU64>,
    pending: DashMap<u64, PendingRequest>,
    timeout: Duration,
    /// Shared handle to the WS handler cmd_tx. Read at send time, not at
    /// construction: `connect()` swaps the inner sender, and the
    /// post-`new()` placeholder's receiver is already dropped.
    cmd_tx_handle: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand>>>,
    event_tx: tokio::sync::mpsc::UnboundedSender<OrderEventAny>,
    dispatch_state: Arc<WsDispatchState>,
    trader_id: TraderId,
    account_id: AccountId,
    /// WS auth token shared with the client; used to build compensating
    /// cancels when a submit request times out.
    auth_token: Arc<tokio::sync::RwLock<Option<String>>>,
    task_spawner: RwLock<TaskSpawner>,
    /// Clock used to stamp local timeout diagnostics.
    clock: &'static AtomicTime,
}

impl OrderRequestState {
    /// Creates a new [`OrderRequestState`].
    #[expect(clippy::too_many_arguments, reason = "all fields are independent")]
    pub fn new(
        cmd_tx_handle: Arc<
            tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand>>,
        >,
        event_tx: tokio::sync::mpsc::UnboundedSender<OrderEventAny>,
        dispatch_state: Arc<WsDispatchState>,
        req_id_counter: Arc<AtomicU64>,
        timeout: Duration,
        trader_id: TraderId,
        account_id: AccountId,
        auth_token: Arc<tokio::sync::RwLock<Option<String>>>,
        task_spawner: TaskSpawner,
        clock: &'static AtomicTime,
    ) -> Self {
        Self {
            req_id_counter,
            pending: DashMap::new(),
            timeout,
            cmd_tx_handle,
            event_tx,
            dispatch_state,
            trader_id,
            account_id,
            auth_token,
            task_spawner: RwLock::new(task_spawner),
            clock,
        }
    }

    /// Reads a clone of the current cmd_tx. Returns `None` only when the
    /// handle's `RwLock` is being written to (connect/disconnect window).
    fn cmd_tx(&self) -> Option<tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand>> {
        self.cmd_tx_handle.try_read().ok().map(|g| g.clone())
    }

    /// Returns the next request ID and advances the counter.
    pub fn next_req_id(&self) -> u64 {
        self.req_id_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Sends an add_order request over the WebSocket transport.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON envelope fails to serialise or if the
    /// handler command channel is closed.
    pub fn submit(
        self: &Arc<Self>,
        params: KrakenWsAddOrderParams,
        identity: PendingRequest,
        ts_now_ns: u64,
    ) -> anyhow::Result<u64> {
        self.send(
            KrakenWsRequest {
                method: KrakenWsMethod::AddOrder,
                params: Some(KrakenWsParams::AddOrder(params)),
                req_id: None,
            },
            identity,
            ts_now_ns,
        )
    }

    /// Sends an amend_order request over the WebSocket transport.
    ///
    /// # Errors
    ///
    /// Returns an error if serialisation fails or the handler command channel is closed.
    pub fn amend(
        self: &Arc<Self>,
        params: KrakenWsAmendOrderParams,
        identity: PendingRequest,
        ts_now_ns: u64,
    ) -> anyhow::Result<u64> {
        self.send(
            KrakenWsRequest {
                method: KrakenWsMethod::AmendOrder,
                params: Some(KrakenWsParams::AmendOrder(params)),
                req_id: None,
            },
            identity,
            ts_now_ns,
        )
    }

    /// Sends a cancel_order request over the WebSocket transport.
    ///
    /// # Errors
    ///
    /// Returns an error if serialisation fails or the handler command channel is closed.
    pub fn cancel(
        self: &Arc<Self>,
        params: KrakenWsCancelOrderParams,
        identity: PendingRequest,
        ts_now_ns: u64,
    ) -> anyhow::Result<u64> {
        self.send(
            KrakenWsRequest {
                method: KrakenWsMethod::CancelOrder,
                params: Some(KrakenWsParams::CancelOrder(params)),
                req_id: None,
            },
            identity,
            ts_now_ns,
        )
    }

    /// Sends a batch_add request over the WebSocket transport.
    ///
    /// # Errors
    ///
    /// Returns an error if serialisation fails or the handler command channel is closed.
    pub fn batch_add(
        self: &Arc<Self>,
        params: KrakenWsBatchAddParams,
        identity: PendingRequest,
        ts_now_ns: u64,
    ) -> anyhow::Result<u64> {
        self.send(
            KrakenWsRequest {
                method: KrakenWsMethod::BatchAdd,
                params: Some(KrakenWsParams::BatchAdd(params)),
                req_id: None,
            },
            identity,
            ts_now_ns,
        )
    }

    fn send(
        self: &Arc<Self>,
        mut envelope: KrakenWsRequest,
        mut identity: PendingRequest,
        ts_now_ns: u64,
    ) -> anyhow::Result<u64> {
        let req_id = self.next_req_id();
        envelope.req_id = Some(req_id);
        identity.ts_sent_ns = ts_now_ns;

        let payload = serde_json::to_string(&envelope)
            .map_err(|e| anyhow::anyhow!("serialize WS order request: {e}"))?;

        let cmd_tx = self
            .cmd_tx()
            .ok_or_else(|| anyhow::anyhow!("WS handler command sender unavailable"))?;

        self.pending.insert(req_id, identity);

        if let Err(e) = cmd_tx.send(SpotHandlerCommand::SendOrderRequest { req_id, payload }) {
            self.pending.remove(&req_id);
            anyhow::bail!("handler command channel closed: {e}");
        }

        let state_for_timeout = Arc::downgrade(self);
        let task_spawner = self.task_spawner.read().clone();
        let cancel = task_spawner.cancellation_token();
        let timeout = self.timeout;

        if let Err(e) = task_spawner.spawn(async move {
            tokio::select! {
                biased;
                () = cancel.cancelled() => {
                    if let Some(state) = state_for_timeout.upgrade() {
                        state.pending.remove(&req_id);
                    }
                }
                () = tokio::time::sleep(timeout) => {
                    let Some(state) = state_for_timeout.upgrade() else {
                        return;
                    };

                    if let Some(pending) = state.pending.get(&req_id) {
                        if cancel.is_cancelled() {
                            drop(pending);
                            state.pending.remove(&req_id);
                            return;
                        }

                        let ts_timeout_ns = state.clock.get_time_ns().as_u64();
                        log::warn!(
                            "Kraken WS response timeout req_id={req_id} op={:?} cl_ord_ids={:?} \
                             ts_timeout_ns={ts_timeout_ns}; awaiting definitive venue evidence",
                            pending.operation,
                            pending.client_order_ids,
                        );
                        state.handle_timeout(&pending);
                    }
                }
            }
        }) {
            log::warn!(
                "Kraken order request {req_id} was sent without a local timeout task: {e}; \
                 awaiting definitive venue evidence"
            );
        }

        Ok(req_id)
    }

    /// Routes an order-method WebSocket response to the correct event emitter.
    ///
    /// `ts_event_ns` is the local receipt time. Kraken's `time_in`/`time_out`
    /// are ignored to keep event ordering monotonic against the local clock.
    /// Timed-out requests remain correlated until a definitive response arrives.
    pub fn handle_response(&self, response: &KrakenWsOrderResponse, ts_event_ns: u64) {
        let req_id = match response.req_id {
            Some(id) => id,
            None => {
                log::warn!(
                    "Kraken WS order response without req_id method={:?} success={}",
                    response.method,
                    response.success,
                );
                return;
            }
        };

        let Some(pending) = self.pending.get(&req_id) else {
            log::debug!("Kraken WS response without pending request req_id={req_id}");
            return;
        };

        let expected_method = pending_op_to_method(pending.operation);
        if response.method != expected_method {
            log::error!(
                "Kraken WS response method {:?} mismatched pending op {:?} req_id={req_id}",
                response.method,
                pending.operation,
            );
            return;
        }
        drop(pending);

        let Some((_, pending)) = self.pending.remove(&req_id) else {
            log::debug!("Kraken WS duplicate response req_id={req_id}");
            return;
        };

        match (pending.operation, response.success) {
            (PendingOperation::Submit, true) => {
                self.emit_order_accepted(&pending, response, ts_event_ns);
            }
            (PendingOperation::Submit, false) => {
                self.emit_order_rejected(&pending, response, ts_event_ns);
            }
            (PendingOperation::Amend, true) => {
                self.emit_order_updated(&pending, response, ts_event_ns);
            }
            (PendingOperation::Amend, false) => {
                self.emit_order_modify_rejected(&pending, response, ts_event_ns);
            }
            (PendingOperation::Cancel, true) => {
                log::debug!(
                    "Kraken WS cancel ack req_id={req_id} cl_ord_ids={:?}",
                    pending.client_order_ids,
                );
            }
            (PendingOperation::Cancel, false) => {
                self.emit_order_cancel_rejected(&pending, response, ts_event_ns);
            }
            (PendingOperation::BatchAdd, _) => {
                self.handle_batch_add_response(&pending, response, ts_event_ns);
            }
        }
    }

    fn handle_timeout(&self, pending: &PendingRequest) {
        match pending.operation {
            PendingOperation::Submit => {
                self.send_compensating_cancel(&pending.client_order_ids);
            }
            PendingOperation::Amend | PendingOperation::Cancel => {}
            PendingOperation::BatchAdd => {
                self.send_compensating_cancel(&pending.client_order_ids);
            }
        }
    }

    pub(crate) fn clear(&self) {
        self.pending.clear();
    }

    pub(crate) fn reset_task_spawner(&self, task_spawner: TaskSpawner) {
        *self.task_spawner.write() = task_spawner;
    }

    /// Sends a best-effort `cancel_order` over the WebSocket after a Submit or
    /// BatchAdd request has timed out, defending against the case where the
    /// venue accepted the order but the response was delayed past the
    /// configured timeout window.
    ///
    /// Fire-and-forget: the response is silently dropped because no `pending`
    /// entry is registered for the cancel `req_id`. If the auth token is not
    /// available or the command channel is closed the cancel is skipped and
    /// the original request remains resolvable by a late response or
    /// reconciliation.
    fn send_compensating_cancel(&self, cl_ord_ids: &[ClientOrderId]) {
        let Some(token) = self.auth_token.try_read().ok().and_then(|g| g.clone()) else {
            log::error!(
                "Submit timeout: no auth token for compensating cancel cl_ord_ids={cl_ord_ids:?}; \
                 relying on reconciliation to recover any orphan order",
            );
            return;
        };

        let req_id = self.next_req_id();
        let params = KrakenWsCancelOrderParams {
            token,
            order_id: None,
            cl_ord_id: Some(cl_ord_ids.iter().map(truncate_cl_ord_id).collect()),
        };
        let envelope = KrakenWsRequest {
            method: KrakenWsMethod::CancelOrder,
            params: Some(KrakenWsParams::CancelOrder(params)),
            req_id: Some(req_id),
        };

        let payload = match serde_json::to_string(&envelope) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Submit timeout: compensating cancel serialise failed: {e}");
                return;
            }
        };

        let Some(cmd_tx) = self.cmd_tx() else {
            log::error!(
                "Submit timeout: compensating cancel sender unavailable cl_ord_ids={cl_ord_ids:?}; \
                 relying on reconciliation to recover any orphan order",
            );
            return;
        };

        if let Err(e) = cmd_tx.send(SpotHandlerCommand::SendOrderRequest { req_id, payload }) {
            log::error!(
                "Submit timeout: compensating cancel channel closed: {e}; \
                 relying on reconciliation to recover any orphan order",
            );
        } else {
            log::debug!(
                "Submit timeout: compensating cancel sent req_id={req_id} cl_ord_ids={cl_ord_ids:?}",
            );
        }
    }

    fn emit_order_accepted(
        &self,
        pending: &PendingRequest,
        response: &KrakenWsOrderResponse,
        ts_event_ns: u64,
    ) {
        let Some(client_order_id) = pending.client_order_ids.first().copied() else {
            log::error!("Kraken WS add_order response without client_order_id");
            return;
        };
        let Some(identity) = self.dispatch_state.lookup_identity(&client_order_id) else {
            log::warn!(
                "Kraken WS add_order response for untracked order client_order_id={client_order_id}",
            );
            return;
        };
        let venue_order_id = response
            .result
            .as_ref()
            .and_then(|r| r.order_id.as_deref())
            .map(VenueOrderId::new);
        let Some(venue_order_id) = venue_order_id else {
            log::error!(
                "Kraken WS add_order success without order_id client_order_id={client_order_id}",
            );
            return;
        };

        if !self.dispatch_state.insert_accepted(client_order_id) {
            return;
        }

        let event = OrderAccepted::new(
            self.trader_id,
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            venue_order_id,
            self.account_id,
            UUID4::new(),
            ts_event_ns.into(),
            ts_event_ns.into(),
            false,
        );
        self.send_event(OrderEventAny::Accepted(event));
    }

    fn emit_order_rejected(
        &self,
        pending: &PendingRequest,
        response: &KrakenWsOrderResponse,
        ts_event_ns: u64,
    ) {
        let Some(client_order_id) = pending.client_order_ids.first().copied() else {
            log::error!("Kraken WS add_order rejection without client_order_id");
            return;
        };
        let Some(identity) = self.dispatch_state.lookup_identity(&client_order_id) else {
            log::warn!(
                "Kraken WS add_order rejection for untracked order client_order_id={client_order_id}",
            );
            return;
        };
        let reason = response
            .error
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("UNKNOWN");

        let event = OrderRejected::new(
            self.trader_id,
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            self.account_id,
            Ustr::from(reason),
            UUID4::new(),
            ts_event_ns.into(),
            ts_event_ns.into(),
            false,
            false,
        );
        self.send_event(OrderEventAny::Rejected(event));
    }

    fn emit_order_updated(
        &self,
        pending: &PendingRequest,
        response: &KrakenWsOrderResponse,
        ts_event_ns: u64,
    ) {
        let Some(client_order_id) = pending.client_order_ids.first().copied() else {
            log::error!("Kraken WS amend_order response without client_order_id");
            return;
        };
        let Some(identity) = self.dispatch_state.lookup_identity(&client_order_id) else {
            log::warn!(
                "Kraken WS amend_order response for untracked order client_order_id={client_order_id}",
            );
            return;
        };
        let venue_order_id = response
            .result
            .as_ref()
            .and_then(|r| r.order_id.as_deref())
            .map(VenueOrderId::new)
            .or_else(|| pending.venue_order_ids.first().copied().flatten());

        let quantity = pending.new_quantity.unwrap_or(identity.quantity);
        if pending.new_quantity.is_some() {
            self.dispatch_state
                .update_identity_quantity(&client_order_id, quantity);
        }

        let event = OrderUpdated::new(
            self.trader_id,
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            quantity,
            UUID4::new(),
            ts_event_ns.into(),
            ts_event_ns.into(),
            false,
            venue_order_id,
            Some(self.account_id),
            pending.new_price,
            pending.new_trigger_price,
            None,
            false,
        );
        self.send_event(OrderEventAny::Updated(event));
    }

    fn emit_order_modify_rejected(
        &self,
        pending: &PendingRequest,
        response: &KrakenWsOrderResponse,
        ts_event_ns: u64,
    ) {
        let Some(client_order_id) = pending.client_order_ids.first().copied() else {
            log::error!("Kraken WS amend_order rejection without client_order_id");
            return;
        };
        let Some(identity) = self.dispatch_state.lookup_identity(&client_order_id) else {
            log::warn!(
                "Kraken WS amend_order rejection for untracked order client_order_id={client_order_id}",
            );
            return;
        };
        let venue_order_id = pending.venue_order_ids.first().copied().flatten();
        let reason = response
            .error
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("UNKNOWN");

        let event = OrderModifyRejected::new(
            self.trader_id,
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            Ustr::from(reason),
            UUID4::new(),
            ts_event_ns.into(),
            ts_event_ns.into(),
            false,
            venue_order_id,
            Some(self.account_id),
        );
        self.send_event(OrderEventAny::ModifyRejected(event));
    }

    fn emit_order_cancel_rejected(
        &self,
        pending: &PendingRequest,
        response: &KrakenWsOrderResponse,
        ts_event_ns: u64,
    ) {
        let Some(client_order_id) = pending.client_order_ids.first().copied() else {
            log::error!("Kraken WS cancel_order rejection without client_order_id");
            return;
        };
        let Some(identity) = self.dispatch_state.lookup_identity(&client_order_id) else {
            log::warn!(
                "Kraken WS cancel_order rejection for untracked order client_order_id={client_order_id}",
            );
            return;
        };
        let venue_order_id = pending.venue_order_ids.first().copied().flatten();
        let reason = response
            .error
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("UNKNOWN");

        let event = OrderCancelRejected::new(
            self.trader_id,
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            Ustr::from(reason),
            UUID4::new(),
            ts_event_ns.into(),
            ts_event_ns.into(),
            false,
            venue_order_id,
            Some(self.account_id),
        );
        self.send_event(OrderEventAny::CancelRejected(event));
    }

    fn handle_batch_add_response(
        &self,
        pending: &PendingRequest,
        response: &KrakenWsOrderResponse,
        ts_event_ns: u64,
    ) {
        let per_order = response
            .result
            .as_ref()
            .and_then(|r| r.orders.as_ref())
            .map_or(&[][..], Vec::as_slice);

        // Match per-leg results by the cl_ord_id Kraken echoes back rather than
        // relying purely on positional alignment with the legs we sent. Echoed
        // matches are robust against the venue reordering or omitting entries
        // in the response array; positional fallback covers the (currently
        // dominant) case where the echoed cl_ord_id is missing.
        let echo_index: AHashMap<&str, usize> = per_order
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.cl_ord_id.as_deref().map(|cid| (cid, i)))
            .collect();

        for (idx, client_order_id) in pending.client_order_ids.iter().copied().enumerate() {
            let leg_venue = pending.venue_order_ids.get(idx).copied().flatten();
            let truncated = truncate_cl_ord_id(&client_order_id);
            let leg_result = echo_index
                .get(truncated.as_str())
                .and_then(|&i| per_order.get(i))
                .or_else(|| per_order.get(idx));

            if leg_result.is_none() {
                log::error!(
                    "Kraken WS batch_add response missing per-leg result for client_order_id={client_order_id} idx={idx} \
                     legs_sent={legs_sent} legs_received={legs_received}; treating as rejection",
                    legs_sent = pending.client_order_ids.len(),
                    legs_received = per_order.len(),
                );
            }

            let leg_success = leg_result.is_some_and(|r| r.success);
            let leg_venue_order_id = leg_result
                .and_then(|r| r.order_id.as_deref())
                .map(VenueOrderId::new)
                .or(leg_venue);
            let leg_error = leg_result.and_then(|r| r.error.clone()).or_else(|| {
                if leg_result.is_none() {
                    Some(format!(
                        "batch_add response missing per-leg result (legs_sent={}, legs_received={})",
                        pending.client_order_ids.len(),
                        per_order.len(),
                    ))
                } else {
                    response.error.clone()
                }
            });

            let leg_response = KrakenWsOrderResponse {
                method: KrakenWsMethod::AddOrder,
                req_id: response.req_id,
                success: leg_success,
                time_in: response.time_in.clone(),
                time_out: response.time_out.clone(),
                error: leg_error,
                result: leg_venue_order_id.map(|v| KrakenWsOrderResult {
                    order_id: Some(v.to_string()),
                    cl_ord_id: leg_result.and_then(|r| r.cl_ord_id.clone()),
                    order_userref: None,
                    warning: None,
                    orders: None,
                }),
            };
            let leg_pending = PendingRequest {
                operation: PendingOperation::Submit,
                client_order_ids: vec![client_order_id],
                venue_order_ids: vec![leg_venue_order_id],
                ts_sent_ns: pending.ts_sent_ns,
                new_quantity: None,
                new_price: None,
                new_trigger_price: None,
            };

            if leg_success {
                self.emit_order_accepted(&leg_pending, &leg_response, ts_event_ns);
            } else {
                self.emit_order_rejected(&leg_pending, &leg_response, ts_event_ns);
            }
        }
    }

    /// Forwards an order event to the execution-engine channel.
    ///
    /// A send failure means the receiver has been dropped, which only happens
    /// during shutdown after the forwarder task has exited. Events lost in
    /// that window are recovered on the next start by the reconciliation
    /// engine (`open_check_interval_secs`), which queries the venue for any
    /// orders or fills the local cache is missing. The error log preserves
    /// operator visibility for the (rare) shutdown-race case.
    fn send_event(&self, event: OrderEventAny) {
        if let Err(e) = self.event_tx.send(event) {
            log::error!("Kraken WS order-event channel send failed: {e}");
        }
    }
}

fn pending_op_to_method(op: PendingOperation) -> KrakenWsMethod {
    match op {
        PendingOperation::Submit => KrakenWsMethod::AddOrder,
        PendingOperation::Amend => KrakenWsMethod::AmendOrder,
        PendingOperation::Cancel => KrakenWsMethod::CancelOrder,
        PendingOperation::BatchAdd => KrakenWsMethod::BatchAdd,
    }
}

#[cfg(test)]
impl OrderRequestState {
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;

    use nautilus_live::task::TaskGroup;
    use nautilus_model::{
        enums::{OrderSide, OrderType},
        identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        common::enums::{KrakenOrderSide, KrakenOrderType},
        websocket::{
            dispatch::OrderIdentity,
            spot_v2::messages::{
                KrakenWsAddOrderParams, KrakenWsAmendOrderParams, KrakenWsBatchAddParams,
                KrakenWsBatchOrderResult, KrakenWsCancelOrderParams, KrakenWsOrderResponse,
                KrakenWsOrderResult,
            },
        },
    };

    const CLIENT_ORDER_ID: &str = "O-1";
    const VENUE_ORDER_ID: &str = "O-VENUE";
    const INSTRUMENT_ID: &str = "BTCUSD.KRAKEN";

    pub(super) struct Harness {
        pub(super) state: Arc<OrderRequestState>,
        pub(super) cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
        pub(super) event_rx: tokio::sync::mpsc::UnboundedReceiver<OrderEventAny>,
        pub(super) dispatch_state: Arc<WsDispatchState>,
        pub(super) auth_token: Arc<tokio::sync::RwLock<Option<String>>>,
        pub(super) pending_tasks: TaskGroup,
        pub(super) cmd_tx_handle:
            Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand>>>,
    }

    pub(super) fn make_harness(timeout_ms: u64) -> Harness {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let counter = Arc::new(AtomicU64::new(0));
        let dispatch_state = Arc::new(WsDispatchState::new());
        let auth_token = Arc::new(tokio::sync::RwLock::new(None));
        let pending_tasks = TaskGroup::new();
        let pending_spawner = pending_tasks.spawner().expect("pending task spawner");
        let cmd_tx_handle = Arc::new(tokio::sync::RwLock::new(cmd_tx));
        let state = Arc::new(OrderRequestState::new(
            Arc::clone(&cmd_tx_handle),
            event_tx,
            Arc::clone(&dispatch_state),
            counter,
            Duration::from_millis(timeout_ms),
            TraderId::new("TESTER-001"),
            AccountId::new("KRAKEN-001"),
            Arc::clone(&auth_token),
            pending_spawner,
            nautilus_core::time::get_atomic_clock_realtime(),
        ));
        Harness {
            state,
            cmd_rx,
            event_rx,
            dispatch_state,
            auth_token,
            pending_tasks,
            cmd_tx_handle,
        }
    }

    fn register_default_identity(dispatch_state: &WsDispatchState, cl_ord_id: ClientOrderId) {
        dispatch_state.register_identity(
            cl_ord_id,
            OrderIdentity {
                strategy_id: StrategyId::new("S-1"),
                instrument_id: InstrumentId::from(INSTRUMENT_ID),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
                quantity: Quantity::from("0.001"),
            },
        );
    }

    fn make_state(
        timeout_ms: u64,
    ) -> (
        Arc<OrderRequestState>,
        tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
    ) {
        let harness = make_harness(timeout_ms);
        (harness.state, harness.cmd_rx)
    }

    fn make_identity(op: PendingOperation) -> PendingRequest {
        PendingRequest {
            operation: op,
            client_order_ids: vec![ClientOrderId::from(CLIENT_ORDER_ID)],
            venue_order_ids: vec![None],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        }
    }

    fn make_add_order_params(token: &str) -> KrakenWsAddOrderParams {
        KrakenWsAddOrderParams {
            order_type: KrakenOrderType::Limit,
            side: KrakenOrderSide::Buy,
            order_qty: dec!(0.001),
            symbol: "BTC/USD".to_string(),
            token: token.to_string(),
            limit_price: Some(dec!(50000)),
            time_in_force: None,
            expire_time: None,
            cl_ord_id: Some(CLIENT_ORDER_ID.to_string()),
            post_only: None,
            reduce_only: None,
            leverage: None,
            trigger: None,
            conditional: None,
        }
    }

    #[rstest]
    fn test_next_req_id_is_monotonic() {
        let (state, _rx) = make_state(1_000);
        let a = state.next_req_id();
        let b = state.next_req_id();
        let c = state.next_req_id();
        assert!(b > a && c > b);
    }

    #[rstest]
    fn test_submit_registers_pending_and_sends_command() {
        let (state, mut rx) = make_state(60_000);
        let identity = make_identity(PendingOperation::Submit);

        let params = KrakenWsAddOrderParams {
            order_type: KrakenOrderType::Limit,
            side: KrakenOrderSide::Buy,
            order_qty: dec!(0.001),
            symbol: "BTC/USD".to_string(),
            token: "test-token".to_string(),
            limit_price: Some(dec!(50000)),
            time_in_force: None,
            expire_time: None,
            cl_ord_id: Some("O-1".to_string()),
            post_only: None,
            reduce_only: None,
            leverage: None,
            trigger: None,
            conditional: None,
        };

        let req_id = state.submit(params, identity, 1).expect("submit ok");
        assert_eq!(state.pending_len(), 1);

        let cmd = rx.try_recv().expect("cmd queued");
        match cmd {
            SpotHandlerCommand::SendOrderRequest {
                req_id: rid,
                payload,
            } => {
                assert_eq!(rid, req_id);
                assert!(payload.contains("\"add_order\""));
                assert!(payload.contains(&format!("\"req_id\":{req_id}")));
            }
            _ => panic!("wrong cmd variant"),
        }
    }

    #[tokio::test]
    async fn test_submit_uses_swapped_command_sender() {
        // Regression: dispatcher built before connect() must observe the
        // live cmd_tx after the swap, not the dropped placeholder.
        let harness = make_harness(60_000);

        drop(harness.cmd_rx);
        let (new_cmd_tx, mut new_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        *harness.cmd_tx_handle.write().await = new_cmd_tx;

        let params = KrakenWsAddOrderParams {
            order_type: KrakenOrderType::Limit,
            side: KrakenOrderSide::Buy,
            order_qty: dec!(0.001),
            symbol: "BTC/USD".to_string(),
            token: "TKN".to_string(),
            limit_price: Some(dec!(50000)),
            time_in_force: None,
            expire_time: None,
            cl_ord_id: Some(CLIENT_ORDER_ID.to_string()),
            post_only: None,
            reduce_only: None,
            leverage: None,
            trigger: None,
            conditional: None,
        };
        let identity = make_identity(PendingOperation::Submit);

        harness
            .state
            .submit(params, identity, 1)
            .expect("submit must succeed via the swapped sender");

        let cmd = new_cmd_rx
            .try_recv()
            .expect("swapped receiver must observe the order");

        match cmd {
            SpotHandlerCommand::SendOrderRequest { payload, .. } => {
                assert!(payload.contains("\"add_order\""));
            }
            other => panic!("expected SendOrderRequest, was {other:?}"),
        }
    }

    #[rstest]
    fn test_amend_sends_amend_order_envelope() {
        let (state, mut rx) = make_state(60_000);
        let params = KrakenWsAmendOrderParams {
            order_id: Some("O-VENUE".to_string()),
            cl_ord_id: None,
            order_qty: Some(dec!(0.005)),
            limit_price: None,
            trigger_price: None,
            token: "TKN".to_string(),
        };
        let identity = PendingRequest {
            operation: PendingOperation::Amend,
            client_order_ids: vec![ClientOrderId::from("O-1")],
            venue_order_ids: vec![Some(VenueOrderId::from("O-VENUE"))],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };
        let _ = state.amend(params, identity, 1).expect("amend ok");
        let cmd = rx.try_recv().unwrap();
        if let SpotHandlerCommand::SendOrderRequest { payload, .. } = cmd {
            assert!(payload.contains("\"amend_order\""));
        } else {
            panic!("wrong variant");
        }
    }

    #[rstest]
    fn test_cancel_sends_cancel_order_envelope() {
        let (state, mut rx) = make_state(60_000);
        let params = KrakenWsCancelOrderParams {
            order_id: Some(vec!["O-VENUE".to_string()]),
            cl_ord_id: None,
            token: "TKN".to_string(),
        };
        let identity = PendingRequest {
            operation: PendingOperation::Cancel,
            client_order_ids: vec![ClientOrderId::from("O-1")],
            venue_order_ids: vec![Some(VenueOrderId::from("O-VENUE"))],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };
        let _ = state.cancel(params, identity, 1).expect("cancel ok");
        let cmd = rx.try_recv().unwrap();
        if let SpotHandlerCommand::SendOrderRequest { payload, .. } = cmd {
            assert!(payload.contains("\"cancel_order\""));
        } else {
            panic!("wrong variant");
        }
    }

    #[rstest]
    fn test_batch_add_sends_batch_add_envelope() {
        let (state, mut rx) = make_state(60_000);
        let params = KrakenWsBatchAddParams {
            symbol: "BTC/USD".to_string(),
            orders: vec![],
            token: "TKN".to_string(),
        };
        let identity = PendingRequest {
            operation: PendingOperation::BatchAdd,
            client_order_ids: vec![ClientOrderId::from("O-A"), ClientOrderId::from("O-B")],
            venue_order_ids: vec![None, None],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };
        let _ = state.batch_add(params, identity, 1).expect("batch ok");
        let cmd = rx.try_recv().unwrap();
        if let SpotHandlerCommand::SendOrderRequest { payload, .. } = cmd {
            assert!(payload.contains("\"batch_add\""));
        } else {
            panic!("wrong variant");
        }
    }

    fn make_response(
        method: KrakenWsMethod,
        success: bool,
        req_id: u64,
        order_id: Option<&str>,
        error: Option<&str>,
    ) -> KrakenWsOrderResponse {
        KrakenWsOrderResponse {
            method,
            req_id: Some(req_id),
            success,
            time_in: None,
            time_out: None,
            error: error.map(str::to_string),
            result: order_id.map(|id| KrakenWsOrderResult {
                order_id: Some(id.to_string()),
                cl_ord_id: None,
                order_userref: None,
                warning: None,
                orders: None,
            }),
        }
    }

    #[rstest]
    fn test_handle_response_submit_success_emits_order_accepted() {
        let mut harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);

        let req_id = 42;
        harness
            .state
            .pending
            .insert(req_id, make_identity(PendingOperation::Submit));

        let response = make_response(
            KrakenWsMethod::AddOrder,
            true,
            req_id,
            Some(VENUE_ORDER_ID),
            None,
        );
        harness.state.handle_response(&response, 1_000);

        assert_eq!(harness.state.pending_len(), 0);
        let event = harness.event_rx.try_recv().expect("event emitted");
        match event {
            OrderEventAny::Accepted(e) => {
                assert_eq!(e.client_order_id, cl_ord_id);
                assert_eq!(e.venue_order_id.as_str(), VENUE_ORDER_ID);
                assert_eq!(e.account_id.as_str(), "KRAKEN-001");
            }
            other => panic!("expected Accepted, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_response_submit_failure_emits_order_rejected() {
        let mut harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);

        let req_id = 7;
        harness
            .state
            .pending
            .insert(req_id, make_identity(PendingOperation::Submit));

        let response = make_response(
            KrakenWsMethod::AddOrder,
            false,
            req_id,
            None,
            Some("Insufficient funds"),
        );
        harness.state.handle_response(&response, 2_000);

        assert_eq!(harness.state.pending_len(), 0);
        let event = harness.event_rx.try_recv().expect("event emitted");
        match event {
            OrderEventAny::Rejected(e) => {
                assert_eq!(e.client_order_id, cl_ord_id);
                assert_eq!(e.reason.as_str(), "Insufficient funds");
            }
            other => panic!("expected Rejected, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_response_amend_success_emits_order_updated() {
        let mut harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);

        let req_id = 11;
        let pending = PendingRequest {
            operation: PendingOperation::Amend,
            client_order_ids: vec![cl_ord_id],
            venue_order_ids: vec![Some(VenueOrderId::from(VENUE_ORDER_ID))],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };
        harness.state.pending.insert(req_id, pending);

        let response = make_response(
            KrakenWsMethod::AmendOrder,
            true,
            req_id,
            Some(VENUE_ORDER_ID),
            None,
        );
        harness.state.handle_response(&response, 3_000);

        let event = harness.event_rx.try_recv().expect("event emitted");
        match event {
            OrderEventAny::Updated(e) => {
                assert_eq!(e.client_order_id, cl_ord_id);
                assert_eq!(
                    e.venue_order_id.expect("venue id present").as_str(),
                    VENUE_ORDER_ID
                );
                assert_eq!(e.quantity, Quantity::from("0.001"));
            }
            other => panic!("expected Updated, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_response_amend_with_new_quantity_emits_new_quantity() {
        let mut harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);

        let req_id = 11;
        let new_qty = Quantity::from("0.005");
        let pending = PendingRequest {
            operation: PendingOperation::Amend,
            client_order_ids: vec![cl_ord_id],
            venue_order_ids: vec![Some(VenueOrderId::from(VENUE_ORDER_ID))],
            ts_sent_ns: 0,
            new_quantity: Some(new_qty),
            new_price: None,
            new_trigger_price: None,
        };
        harness.state.pending.insert(req_id, pending);

        let response = make_response(
            KrakenWsMethod::AmendOrder,
            true,
            req_id,
            Some(VENUE_ORDER_ID),
            None,
        );
        harness.state.handle_response(&response, 3_500);

        let event = harness.event_rx.try_recv().expect("event emitted");
        match event {
            OrderEventAny::Updated(e) => {
                assert_eq!(e.quantity, new_qty, "OrderUpdated must carry new quantity");
            }
            other => panic!("expected Updated, was {other:?}"),
        }

        let identity = harness
            .dispatch_state
            .lookup_identity(&cl_ord_id)
            .expect("identity present");
        assert_eq!(
            identity.quantity, new_qty,
            "dispatch identity must be updated for follow-up ops",
        );
    }

    #[rstest]
    fn test_handle_response_amend_carries_new_price_and_trigger() {
        let mut harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);

        let req_id = 13;
        let new_price = Price::from("31000.00");
        let new_trigger = Price::from("30500.00");
        let pending = PendingRequest {
            operation: PendingOperation::Amend,
            client_order_ids: vec![cl_ord_id],
            venue_order_ids: vec![Some(VenueOrderId::from(VENUE_ORDER_ID))],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: Some(new_price),
            new_trigger_price: Some(new_trigger),
        };
        harness.state.pending.insert(req_id, pending);

        let response = make_response(
            KrakenWsMethod::AmendOrder,
            true,
            req_id,
            Some(VENUE_ORDER_ID),
            None,
        );
        harness.state.handle_response(&response, 4_500);

        let event = harness.event_rx.try_recv().expect("event emitted");
        match event {
            OrderEventAny::Updated(e) => {
                assert_eq!(
                    e.price,
                    Some(new_price),
                    "OrderUpdated must carry amended price",
                );
                assert_eq!(
                    e.trigger_price,
                    Some(new_trigger),
                    "OrderUpdated must carry amended trigger price",
                );
            }
            other => panic!("expected Updated, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_response_amend_failure_emits_order_modify_rejected() {
        let mut harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);

        let req_id = 12;
        let pending = PendingRequest {
            operation: PendingOperation::Amend,
            client_order_ids: vec![cl_ord_id],
            venue_order_ids: vec![Some(VenueOrderId::from(VENUE_ORDER_ID))],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };
        harness.state.pending.insert(req_id, pending);

        let response = make_response(
            KrakenWsMethod::AmendOrder,
            false,
            req_id,
            None,
            Some("Order not found"),
        );
        harness.state.handle_response(&response, 4_000);

        let event = harness.event_rx.try_recv().expect("event emitted");
        match event {
            OrderEventAny::ModifyRejected(e) => {
                assert_eq!(e.client_order_id, cl_ord_id);
                assert_eq!(e.reason.as_str(), "Order not found");
            }
            other => panic!("expected ModifyRejected, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_response_cancel_failure_emits_order_cancel_rejected() {
        let mut harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);

        let req_id = 21;
        let pending = PendingRequest {
            operation: PendingOperation::Cancel,
            client_order_ids: vec![cl_ord_id],
            venue_order_ids: vec![Some(VenueOrderId::from(VENUE_ORDER_ID))],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };
        harness.state.pending.insert(req_id, pending);

        let response = make_response(
            KrakenWsMethod::CancelOrder,
            false,
            req_id,
            None,
            Some("Unknown order"),
        );
        harness.state.handle_response(&response, 5_000);

        let event = harness.event_rx.try_recv().expect("event emitted");
        match event {
            OrderEventAny::CancelRejected(e) => {
                assert_eq!(e.client_order_id, cl_ord_id);
                assert_eq!(e.reason.as_str(), "Unknown order");
            }
            other => panic!("expected CancelRejected, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_response_cancel_success_is_silent() {
        let mut harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);

        let req_id = 22;
        let pending = PendingRequest {
            operation: PendingOperation::Cancel,
            client_order_ids: vec![cl_ord_id],
            venue_order_ids: vec![Some(VenueOrderId::from(VENUE_ORDER_ID))],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };
        harness.state.pending.insert(req_id, pending);

        let response = make_response(KrakenWsMethod::CancelOrder, true, req_id, None, None);
        harness.state.handle_response(&response, 6_000);

        assert_eq!(harness.state.pending_len(), 0);
        assert!(harness.event_rx.try_recv().is_err());
    }

    #[rstest]
    #[case(PendingOperation::Submit)]
    #[case(PendingOperation::Amend)]
    #[case(PendingOperation::Cancel)]
    #[case(PendingOperation::BatchAdd)]
    fn test_timeout_emits_no_order_event(#[case] operation: PendingOperation) {
        let mut harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);

        let pending = PendingRequest {
            operation,
            client_order_ids: vec![cl_ord_id],
            venue_order_ids: vec![Some(VenueOrderId::from(VENUE_ORDER_ID))],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };

        harness.state.handle_timeout(&pending);

        assert!(harness.event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_late_submit_response_resolves_timed_out_request() {
        let mut harness = make_harness(50);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);
        *harness.auth_token.write().await = Some("TEST-TOKEN".to_string());

        let params = make_add_order_params("TEST-TOKEN");
        let req_id = harness
            .state
            .submit(params, make_identity(PendingOperation::Submit), 1)
            .expect("submit ok");

        let payloads = recv_send_payloads_until_cancel(&mut harness.cmd_rx).await;
        assert!(payloads.iter().any(|p| p.contains("\"cancel_order\"")));
        assert_eq!(harness.state.pending_len(), 1);
        assert!(harness.event_rx.try_recv().is_err());

        let response = make_response(
            KrakenWsMethod::AddOrder,
            true,
            req_id,
            Some(VENUE_ORDER_ID),
            None,
        );
        harness.state.handle_response(&response, 7_000);

        let event = harness.event_rx.try_recv().expect("late response event");
        match event {
            OrderEventAny::Accepted(e) => {
                assert_eq!(e.client_order_id, cl_ord_id);
                assert_eq!(e.venue_order_id.as_str(), VENUE_ORDER_ID);
            }
            other => panic!("expected Accepted, was {other:?}"),
        }
        assert_eq!(harness.state.pending_len(), 0);
    }

    #[tokio::test]
    async fn test_late_submit_rejection_resolves_timed_out_request() {
        let mut harness = make_harness(50);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);
        *harness.auth_token.write().await = Some("TEST-TOKEN".to_string());

        let params = make_add_order_params("TEST-TOKEN");
        let req_id = harness
            .state
            .submit(params, make_identity(PendingOperation::Submit), 1)
            .expect("submit ok");

        recv_send_payloads_until_cancel(&mut harness.cmd_rx).await;
        assert_eq!(harness.state.pending_len(), 1);
        assert!(harness.event_rx.try_recv().is_err());

        let response = make_response(
            KrakenWsMethod::AddOrder,
            false,
            req_id,
            None,
            Some("Insufficient funds"),
        );
        harness.state.handle_response(&response, 7_000);

        let event = harness.event_rx.try_recv().expect("late response event");
        match event {
            OrderEventAny::Rejected(e) => {
                assert_eq!(e.client_order_id, cl_ord_id);
                assert_eq!(e.reason.as_str(), "Insufficient funds");
            }
            other => panic!("expected Rejected, was {other:?}"),
        }
        assert_eq!(harness.state.pending_len(), 0);
    }

    #[rstest]
    fn test_handle_response_method_op_mismatch_retains_pending() {
        let mut harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);

        let req_id = 33;
        harness
            .state
            .pending
            .insert(req_id, make_identity(PendingOperation::Submit));

        let response = make_response(KrakenWsMethod::CancelOrder, true, req_id, None, None);
        harness.state.handle_response(&response, 8_000);

        assert_eq!(harness.state.pending_len(), 1);
        assert!(harness.event_rx.try_recv().is_err());

        let response = make_response(
            KrakenWsMethod::AddOrder,
            true,
            req_id,
            Some(VENUE_ORDER_ID),
            None,
        );
        harness.state.handle_response(&response, 8_001);

        let event = harness
            .event_rx
            .try_recv()
            .expect("matching response event");
        match event {
            OrderEventAny::Accepted(e) => {
                assert_eq!(e.client_order_id, cl_ord_id);
                assert_eq!(e.venue_order_id.as_str(), VENUE_ORDER_ID);
            }
            other => panic!("expected Accepted, was {other:?}"),
        }
        assert_eq!(harness.state.pending_len(), 0);
    }

    #[rstest]
    fn test_handle_response_batch_add_emits_per_leg_events() {
        let mut harness = make_harness(60_000);
        let cl_a = ClientOrderId::from("O-A");
        let cl_b = ClientOrderId::from("O-B");
        register_default_identity(&harness.dispatch_state, cl_a);
        register_default_identity(&harness.dispatch_state, cl_b);

        let req_id = 50;
        let pending = PendingRequest {
            operation: PendingOperation::BatchAdd,
            client_order_ids: vec![cl_a, cl_b],
            venue_order_ids: vec![None, None],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };
        harness.state.pending.insert(req_id, pending);

        let response = KrakenWsOrderResponse {
            method: KrakenWsMethod::BatchAdd,
            req_id: Some(req_id),
            success: true,
            time_in: None,
            time_out: None,
            error: None,
            result: Some(KrakenWsOrderResult {
                order_id: None,
                cl_ord_id: None,
                order_userref: None,
                warning: None,
                orders: Some(vec![
                    KrakenWsBatchOrderResult {
                        success: true,
                        order_id: Some("V-A".to_string()),
                        cl_ord_id: Some("O-A".to_string()),
                        error: None,
                    },
                    KrakenWsBatchOrderResult {
                        success: false,
                        order_id: None,
                        cl_ord_id: Some("O-B".to_string()),
                        error: Some("Bad price".to_string()),
                    },
                ]),
            }),
        };
        harness.state.handle_response(&response, 9_000);

        let first = harness.event_rx.try_recv().expect("first event");
        let second = harness.event_rx.try_recv().expect("second event");

        match first {
            OrderEventAny::Accepted(e) => {
                assert_eq!(e.client_order_id, cl_a);
                assert_eq!(e.venue_order_id.as_str(), "V-A");
            }
            other => panic!("expected Accepted, was {other:?}"),
        }

        match second {
            OrderEventAny::Rejected(e) => {
                assert_eq!(e.client_order_id, cl_b);
                assert_eq!(e.reason.as_str(), "Bad price");
            }
            other => panic!("expected Rejected, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_response_batch_add_matches_legs_by_echoed_cl_ord_id() {
        let mut harness = make_harness(60_000);
        let cl_a = ClientOrderId::from("O-A");
        let cl_b = ClientOrderId::from("O-B");
        register_default_identity(&harness.dispatch_state, cl_a);
        register_default_identity(&harness.dispatch_state, cl_b);

        let req_id = 60;
        let pending = PendingRequest {
            operation: PendingOperation::BatchAdd,
            client_order_ids: vec![cl_a, cl_b],
            venue_order_ids: vec![None, None],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };
        harness.state.pending.insert(req_id, pending);

        // Vendor returns per-leg results in REVERSE order vs sent. Echo-based
        // matching must attribute V-B to cl_b and V-A to cl_a regardless of
        // index alignment.
        let response = KrakenWsOrderResponse {
            method: KrakenWsMethod::BatchAdd,
            req_id: Some(req_id),
            success: true,
            time_in: None,
            time_out: None,
            error: None,
            result: Some(KrakenWsOrderResult {
                order_id: None,
                cl_ord_id: None,
                order_userref: None,
                warning: None,
                orders: Some(vec![
                    KrakenWsBatchOrderResult {
                        success: true,
                        order_id: Some("V-B".to_string()),
                        cl_ord_id: Some("O-B".to_string()),
                        error: None,
                    },
                    KrakenWsBatchOrderResult {
                        success: true,
                        order_id: Some("V-A".to_string()),
                        cl_ord_id: Some("O-A".to_string()),
                        error: None,
                    },
                ]),
            }),
        };
        harness.state.handle_response(&response, 14_000);

        let first = harness.event_rx.try_recv().expect("first event");
        let second = harness.event_rx.try_recv().expect("second event");
        let mut by_cl_ord = std::collections::HashMap::new();

        for event in [first, second] {
            match event {
                OrderEventAny::Accepted(e) => {
                    by_cl_ord.insert(e.client_order_id, e.venue_order_id);
                }
                other => panic!("expected Accepted, was {other:?}"),
            }
        }
        assert_eq!(
            by_cl_ord.get(&cl_a).map(|v| v.as_str()),
            Some("V-A"),
            "cl_a must be paired with V-A despite reversed response order",
        );
        assert_eq!(
            by_cl_ord.get(&cl_b).map(|v| v.as_str()),
            Some("V-B"),
            "cl_b must be paired with V-B despite reversed response order",
        );
    }

    #[rstest]
    fn test_handle_response_batch_add_truncated_per_leg_results_rejects_trailing_legs() {
        let mut harness = make_harness(60_000);
        let cl_a = ClientOrderId::from("O-A");
        let cl_b = ClientOrderId::from("O-B");
        let cl_c = ClientOrderId::from("O-C");
        register_default_identity(&harness.dispatch_state, cl_a);
        register_default_identity(&harness.dispatch_state, cl_b);
        register_default_identity(&harness.dispatch_state, cl_c);

        let req_id = 51;
        let pending = PendingRequest {
            operation: PendingOperation::BatchAdd,
            client_order_ids: vec![cl_a, cl_b, cl_c],
            venue_order_ids: vec![None, None, None],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };
        harness.state.pending.insert(req_id, pending);

        // Vendor returns envelope.success=true but only one per-leg entry for three sent.
        let response = KrakenWsOrderResponse {
            method: KrakenWsMethod::BatchAdd,
            req_id: Some(req_id),
            success: true,
            time_in: None,
            time_out: None,
            error: None,
            result: Some(KrakenWsOrderResult {
                order_id: None,
                cl_ord_id: None,
                order_userref: None,
                warning: None,
                orders: Some(vec![KrakenWsBatchOrderResult {
                    success: true,
                    order_id: Some("V-A".to_string()),
                    cl_ord_id: Some("O-A".to_string()),
                    error: None,
                }]),
            }),
        };
        harness.state.handle_response(&response, 12_000);

        let first = harness.event_rx.try_recv().expect("first event");
        let second = harness.event_rx.try_recv().expect("second event");
        let third = harness.event_rx.try_recv().expect("third event");

        match first {
            OrderEventAny::Accepted(e) => assert_eq!(e.client_order_id, cl_a),
            other => panic!("expected Accepted for present leg, was {other:?}"),
        }

        for (event, cl_id) in [(second, cl_b), (third, cl_c)] {
            match event {
                OrderEventAny::Rejected(e) => {
                    assert_eq!(
                        e.client_order_id, cl_id,
                        "missing-leg rejection cl_ord_id mismatch",
                    );
                    assert!(
                        e.reason.as_str().contains("missing per-leg result"),
                        "expected truncation reason, was {}",
                        e.reason,
                    );
                }
                other => panic!(
                    "missing per-leg result must reject (not inherit envelope.success), was {other:?}",
                ),
            }
        }
    }

    fn drain_send_payloads(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
    ) -> Vec<String> {
        let mut out = Vec::new();

        while let Ok(cmd) = rx.try_recv() {
            if let SpotHandlerCommand::SendOrderRequest { payload, .. } = cmd {
                out.push(payload);
            }
        }
        out
    }

    // The compensating cancel is the last command the timeout task emits, so
    // awaiting it means timeout handling is complete without racing a fixed
    // sleep against the global runtime.
    async fn recv_send_payloads_until_cancel(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
    ) -> Vec<String> {
        let mut out = Vec::new();

        loop {
            let cmd = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .expect("timed out awaiting compensating cancel")
                .expect("command channel closed");

            let SpotHandlerCommand::SendOrderRequest { payload, .. } = cmd else {
                continue;
            };
            let is_cancel = payload.contains("\"cancel_order\"");
            out.push(payload);

            if is_cancel {
                return out;
            }
        }
    }

    #[tokio::test]
    async fn test_submit_timeout_sends_compensating_cancel() {
        let mut harness = make_harness(50);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);
        *harness.auth_token.write().await = Some("TEST-TOKEN".to_string());

        let params = make_add_order_params("TEST-TOKEN");
        let identity = make_identity(PendingOperation::Submit);
        harness
            .state
            .submit(params, identity, 1)
            .expect("submit ok");

        let payloads = recv_send_payloads_until_cancel(&mut harness.cmd_rx).await;
        assert!(
            payloads.iter().any(|p| p.contains("\"add_order\"")),
            "original add_order missing: {payloads:?}",
        );
        let cancel = payloads
            .iter()
            .find(|p| p.contains("\"cancel_order\""))
            .expect("compensating cancel_order missing");
        assert!(
            cancel.contains(CLIENT_ORDER_ID),
            "compensating cancel must reference cl_ord_id, was {cancel}",
        );

        assert_eq!(harness.state.pending_len(), 1);
        assert!(harness.event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_compensating_cancel_response_is_silently_dropped() {
        // The compensating cancel after a submit timeout is fire-and-forget;
        // its response must not surface an event or resolve the original
        // request correlation.
        let mut harness = make_harness(50);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);
        *harness.auth_token.write().await = Some("TEST-TOKEN".to_string());

        let params = make_add_order_params("TEST-TOKEN");
        let identity = make_identity(PendingOperation::Submit);
        harness
            .state
            .submit(params, identity, 1)
            .expect("submit ok");

        let payloads = recv_send_payloads_until_cancel(&mut harness.cmd_rx).await;

        let cancel = payloads
            .iter()
            .find(|p| p.contains("\"cancel_order\""))
            .expect("compensating cancel missing");
        let cancel_value: serde_json::Value = serde_json::from_str(cancel).expect("valid json");
        let cancel_req_id = cancel_value["req_id"].as_u64().expect("req_id present");

        for success in [true, false] {
            let response = KrakenWsOrderResponse {
                method: KrakenWsMethod::CancelOrder,
                req_id: Some(cancel_req_id),
                success,
                time_in: None,
                time_out: None,
                error: (!success).then(|| "Unknown order".to_string()),
                result: None,
            };
            harness.state.handle_response(&response, 9_000);
        }

        assert!(
            harness.event_rx.try_recv().is_err(),
            "compensating-cancel responses must not surface events to strategies",
        );
        assert_eq!(harness.state.pending_len(), 1);
    }

    #[rstest]
    fn test_submit_timeout_without_token_skips_compensating_cancel() {
        let mut harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);
        let identity = make_identity(PendingOperation::Submit);

        harness.state.handle_timeout(&identity);

        let payloads = drain_send_payloads(&mut harness.cmd_rx);
        assert!(
            payloads.iter().all(|p| !p.contains("\"cancel_order\"")),
            "no compensating cancel expected without token, was {payloads:?}",
        );
    }

    #[tokio::test]
    async fn test_batch_add_timeout_sends_compensating_cancel_for_all_legs() {
        let mut harness = make_harness(50);
        let cl_a = ClientOrderId::from("O-A");
        let cl_b = ClientOrderId::from("O-B");
        register_default_identity(&harness.dispatch_state, cl_a);
        register_default_identity(&harness.dispatch_state, cl_b);
        *harness.auth_token.write().await = Some("TEST-TOKEN".to_string());

        let params = KrakenWsBatchAddParams {
            symbol: "BTC/USD".to_string(),
            orders: vec![],
            token: "TEST-TOKEN".to_string(),
        };
        let identity = PendingRequest {
            operation: PendingOperation::BatchAdd,
            client_order_ids: vec![cl_a, cl_b],
            venue_order_ids: vec![None, None],
            ts_sent_ns: 0,
            new_quantity: None,
            new_price: None,
            new_trigger_price: None,
        };
        harness
            .state
            .batch_add(params, identity, 1)
            .expect("batch ok");

        let payloads = recv_send_payloads_until_cancel(&mut harness.cmd_rx).await;
        let cancel = payloads
            .iter()
            .find(|p| p.contains("\"cancel_order\""))
            .expect("compensating cancel missing");
        assert!(cancel.contains("O-A") && cancel.contains("O-B"));
        assert_eq!(harness.state.pending_len(), 1);
        assert!(harness.event_rx.try_recv().is_err());

        let batch_req_id = payloads
            .iter()
            .find(|p| p.contains("\"batch_add\""))
            .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
            .and_then(|v| v["req_id"].as_u64())
            .expect("batch req_id missing");
        let response = KrakenWsOrderResponse {
            method: KrakenWsMethod::BatchAdd,
            req_id: Some(batch_req_id),
            success: true,
            time_in: None,
            time_out: None,
            error: None,
            result: Some(KrakenWsOrderResult {
                order_id: None,
                cl_ord_id: None,
                order_userref: None,
                warning: None,
                orders: Some(vec![
                    KrakenWsBatchOrderResult {
                        success: false,
                        order_id: None,
                        cl_ord_id: Some("O-B".to_string()),
                        error: Some("Bad price".to_string()),
                    },
                    KrakenWsBatchOrderResult {
                        success: true,
                        order_id: Some("V-A".to_string()),
                        cl_ord_id: Some("O-A".to_string()),
                        error: None,
                    },
                ]),
            }),
        };
        harness.state.handle_response(&response, 10_000);

        let first = harness.event_rx.try_recv().expect("first late event");
        let second = harness.event_rx.try_recv().expect("second late event");
        let mut accepted = None;
        let mut rejected = None;

        for event in [first, second] {
            match event {
                OrderEventAny::Accepted(e) => {
                    accepted = Some((e.client_order_id, e.venue_order_id));
                }
                OrderEventAny::Rejected(e) => {
                    rejected = Some((e.client_order_id, e.reason));
                }
                other => panic!("expected Accepted or Rejected, was {other:?}"),
            }
        }
        assert_eq!(
            accepted.map(|(cl, venue)| (cl, venue.to_string())),
            Some((cl_a, "V-A".to_string()))
        );
        assert_eq!(
            rejected.map(|(cl, reason)| (cl, reason.to_string())),
            Some((cl_b, "Bad price".to_string()))
        );
        assert_eq!(harness.state.pending_len(), 0);
    }

    #[tokio::test]
    async fn test_clear_removes_timed_out_request() {
        let mut harness = make_harness(50);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);
        *harness.auth_token.write().await = Some("TEST-TOKEN".to_string());

        let params = make_add_order_params("TEST-TOKEN");
        harness
            .state
            .submit(params, make_identity(PendingOperation::Submit), 1)
            .expect("submit ok");

        recv_send_payloads_until_cancel(&mut harness.cmd_rx).await;
        assert_eq!(harness.state.pending_len(), 1);

        harness.state.clear();

        assert_eq!(harness.state.pending_len(), 0);
    }

    #[tokio::test]
    async fn test_reset_cancellation_token_keeps_new_timeout_pending() {
        let mut harness = make_harness(50);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);
        *harness.auth_token.write().await = Some("TEST-TOKEN".to_string());
        harness.pending_tasks.begin_shutdown();
        harness
            .pending_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(1))
            .await
            .expect("finish old timeout generation");
        harness
            .pending_tasks
            .start_generation()
            .expect("start timeout generation");
        let pending_spawner = harness
            .pending_tasks
            .spawner()
            .expect("pending task spawner");
        harness.state.reset_task_spawner(pending_spawner);

        harness
            .state
            .submit(
                make_add_order_params("TEST-TOKEN"),
                make_identity(PendingOperation::Submit),
                1,
            )
            .expect("submit ok");

        recv_send_payloads_until_cancel(&mut harness.cmd_rx).await;

        assert_eq!(harness.state.pending_len(), 1);
        assert!(harness.event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_task_group_shutdown_aborts_pending_timeout() {
        let harness = make_harness(60_000);
        let cl_ord_id = ClientOrderId::from(CLIENT_ORDER_ID);
        register_default_identity(&harness.dispatch_state, cl_ord_id);

        let params = KrakenWsAddOrderParams {
            order_type: KrakenOrderType::Limit,
            side: KrakenOrderSide::Buy,
            order_qty: dec!(0.001),
            symbol: "BTC/USD".to_string(),
            token: String::new(),
            limit_price: Some(dec!(50000)),
            time_in_force: None,
            expire_time: None,
            cl_ord_id: Some(CLIENT_ORDER_ID.to_string()),
            post_only: None,
            reduce_only: None,
            leverage: None,
            trigger: None,
            conditional: None,
        };
        let identity = make_identity(PendingOperation::Submit);
        harness
            .state
            .submit(params, identity, 1)
            .expect("submit ok");
        assert_eq!(harness.state.pending_len(), 1);

        harness.pending_tasks.begin_shutdown();

        // Cancellation clears the pending entry from a task on the global runtime,
        // so poll the condition rather than racing a fixed sleep.
        nautilus_common::testing::wait_until_async(
            || {
                let state = Arc::clone(&harness.state);
                async move { state.pending_len() == 0 }
            },
            Duration::from_secs(5),
        )
        .await;

        assert_eq!(
            harness.state.pending_len(),
            0,
            "pending entry must be cleared on cancellation",
        );
    }
}

#[cfg(test)]
mod property_tests {
    use nautilus_model::identifiers::ClientOrderId;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::{tests::make_harness, *};
    use crate::websocket::spot_v2::messages::{KrakenWsBatchOrderResult, KrakenWsOrderResult};

    proptest! {
        #[rstest]
        fn no_pending_leak_on_random_response_interleaving(
            ops in proptest::collection::vec(
                prop_oneof![
                    Just((PendingOperation::Submit, true, 1usize)),
                    Just((PendingOperation::Submit, false, 1usize)),
                    Just((PendingOperation::Amend, true, 1usize)),
                    Just((PendingOperation::Amend, false, 1usize)),
                    Just((PendingOperation::Cancel, true, 1usize)),
                    Just((PendingOperation::Cancel, false, 1usize)),
                    (2usize..=4usize).prop_map(|n| (PendingOperation::BatchAdd, true, n)),
                    (2usize..=4usize).prop_map(|n| (PendingOperation::BatchAdd, false, n)),
                ],
                1..50usize,
            )
        ) {
            let h = make_harness(60_000);
            let state = &h.state;
            let mut req_ids = Vec::new();

            for (op, success, leg_count) in &ops {
                let req_id = state.next_req_id();
                let client_order_ids: Vec<ClientOrderId> = (0..*leg_count)
                    .map(|i| ClientOrderId::from(format!("O-{req_id}-{i}").as_str()))
                    .collect();
                let venue_order_ids = vec![None; *leg_count];
                state.pending.insert(req_id, PendingRequest {
                    operation: *op,
                    client_order_ids: client_order_ids.clone(),
                    venue_order_ids,
                    ts_sent_ns: 0,
                    new_quantity: None,
                    new_price: None,
                    new_trigger_price: None,
                });
                req_ids.push((req_id, *op, *success, client_order_ids));
            }
            let mut shuffled = req_ids.clone();
            shuffled.reverse();
            for (req_id, op, success, client_order_ids) in shuffled {
                let method = match op {
                    PendingOperation::Submit => KrakenWsMethod::AddOrder,
                    PendingOperation::Amend => KrakenWsMethod::AmendOrder,
                    PendingOperation::Cancel => KrakenWsMethod::CancelOrder,
                    PendingOperation::BatchAdd => KrakenWsMethod::BatchAdd,
                };
                let result = if op == PendingOperation::BatchAdd {
                    Some(KrakenWsOrderResult {
                        order_id: None,
                        cl_ord_id: None,
                        order_userref: None,
                        warning: None,
                        orders: Some(client_order_ids
                            .iter()
                            .enumerate()
                            .map(|(i, cid)| KrakenWsBatchOrderResult {
                                success,
                                order_id: success.then(|| format!("V-{req_id}-{i}")),
                                cl_ord_id: Some(cid.as_str().to_string()),
                                error: (!success).then(|| "test-error".to_string()),
                            })
                            .collect()),
                    })
                } else {
                    None
                };
                let response = KrakenWsOrderResponse {
                    method,
                    req_id: Some(req_id),
                    success,
                    time_in: None,
                    time_out: None,
                    error: if success {
                        None
                    } else {
                        Some("test-error".to_string())
                    },
                    result,
                };
                state.handle_response(&response, 1);
            }
            prop_assert_eq!(state.pending_len(), 0);
        }
    }
}
