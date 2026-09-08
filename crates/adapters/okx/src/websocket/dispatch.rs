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

//! WebSocket message dispatch for the OKX execution client.
//!
//! Routes incoming [`OKXWsMessage`] variants to the appropriate parsing and
//! event emission paths. Tracked orders (submitted through this client) produce
//! proper order events; untracked orders fall back to execution reports for
//! downstream reconciliation.

use std::{collections::VecDeque, fmt::Debug, hash::Hash, sync::Arc};

use ahash::AHashMap;
use dashmap::DashMap;
use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
use nautilus_core::{AtomicMap, UUID4, UnixNanos, time::AtomicTime};
use nautilus_live::{
    ExecutionEventEmitter,
    execution::{
        context::{OrderContext, OrderIdentity},
        failure::CommandFailure,
    },
};
use nautilus_model::{
    enums::OrderStatus,
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderFilled, OrderRejected, OrderTriggered,
        OrderUpdated,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::TRIGGERABLE_ORDER_TYPES,
    reports::FillReport,
    types::{Currency, Money, Quantity},
};
use parking_lot::Mutex;
use ustr::Ustr;

use crate::{
    common::{
        consts::{
            OKX_FIELD_CLORDID, OKX_FIELD_SCODE, OKX_FIELD_SMSG, OKX_FIELD_SUBCODE,
            OKX_POST_ONLY_CANCEL_REASON, OKX_POST_ONLY_CANCEL_SOURCE, OKX_SUCCESS_CODE,
        },
        enums::{OKXAlgoOrderStatus, OKXAlgoOrderType, OKXOrderStatus, OKXOrderType},
        failure::{classify_okx_venue_code, classify_okx_ws_failure},
        parse::{
            is_market_price, parse_client_order_id, parse_millisecond_timestamp, parse_price,
            parse_quantity,
        },
    },
    http::models::{OKXAccount, OKXCancelAlgoOrderResponse, OKXPosition, OKXSpreadOrder},
    websocket::{
        client::PendingOrderInfo,
        enums::OKXWsOperation,
        handler::{is_post_only_auto_cancel, is_unfilled_rpi_cancel},
        messages::{ExecutionReport, OKXAlgoOrderMsg, OKXOrderMsg, OKXWsMessage},
        parse::{
            OrderStateSnapshot, ParsedOrderEvent, parse_algo_order_msg,
            parse_algo_order_status_report, parse_order_event, parse_order_msg,
            parse_spread_order_event, parse_spread_order_msg, update_fee_fill_caches,
        },
    },
};

/// Maximum entries held by the dedup sets before the oldest is evicted.
const DEDUP_CAPACITY: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OrderVenueBinding {
    parent: VenueOrderId,
    child: Option<VenueOrderId>,
}

#[derive(Debug)]
struct OrderLifecycleBindings {
    client_by_parent: AHashMap<VenueOrderId, ClientOrderId>,
    venue_by_client: AHashMap<ClientOrderId, OrderVenueBinding>,
    terminal_client_by_parent: FifoCacheMap<VenueOrderId, ClientOrderId, DEDUP_CAPACITY>,
}

impl Default for OrderLifecycleBindings {
    fn default() -> Self {
        Self {
            client_by_parent: AHashMap::new(),
            venue_by_client: AHashMap::new(),
            terminal_client_by_parent: FifoCacheMap::new(),
        }
    }
}

impl OrderLifecycleBindings {
    fn client_order_id(&self, parent: &VenueOrderId) -> Option<ClientOrderId> {
        self.client_by_parent
            .get(parent)
            .or_else(|| self.terminal_client_by_parent.get(parent))
            .copied()
    }

    fn finish(&mut self, client_order_id: ClientOrderId, binding: OrderVenueBinding) {
        self.client_by_parent.remove(&binding.parent);
        self.venue_by_client.remove(&client_order_id);
        self.terminal_client_by_parent
            .insert(binding.parent, client_order_id);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExecutionUpdateRoute {
    Tracked(ClientOrderId, OrderContext),
    External,
    Suppressed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkedChildResolution {
    Bound(ClientOrderId),
    Held,
    External,
}

#[derive(Debug)]
struct PendingLinkedChild {
    parent_venue_order_id: VenueOrderId,
    candidate_client_order_ids: Vec<ClientOrderId>,
    message: OKXOrderMsg,
}

#[derive(Debug)]
struct DedupCache<K>
where
    K: Clone + Debug + Eq + Hash,
{
    inner: Mutex<FifoCache<K, DEDUP_CAPACITY>>,
}

impl<K> DedupCache<K>
where
    K: Clone + Debug + Eq + Hash,
{
    fn new() -> Self {
        Self {
            inner: Mutex::new(FifoCache::new()),
        }
    }

    fn contains(&self, key: &K) -> bool {
        self.inner.lock().contains(key)
    }

    fn insert(&self, key: K) -> bool {
        self.inner.lock().insert(key)
    }

    fn remove(&self, key: &K) {
        self.inner.lock().remove(key);
    }
}

/// Shared state for cross-stream event deduplication between the private
/// and business WebSocket dispatch loops.
#[derive(Debug)]
pub struct WsDispatchState {
    pub order_identities: DashMap<ClientOrderId, OrderIdentity>,
    order_contexts: DashMap<ClientOrderId, OrderContext>,
    pub(crate) pending_orders: Arc<DashMap<String, PendingOrderInfo>>,
    pub(crate) pending_cancels: Arc<DashMap<String, PendingOrderInfo>>,
    pub(crate) pending_amends: Arc<DashMap<String, PendingOrderInfo>>,
    accepted_venue_order_ids: Mutex<FifoCacheMap<ClientOrderId, VenueOrderId, DEDUP_CAPACITY>>,
    triggered_orders: DedupCache<ClientOrderId>,
    filled_orders: DedupCache<ClientOrderId>,
    terminal_orders: DedupCache<ClientOrderId>,
    emitted_trades: DedupCache<TradeId>,
    post_only_rejections: DedupCache<Ustr>,
    lifecycle_bindings: Mutex<OrderLifecycleBindings>,
    pending_linked_children: Mutex<VecDeque<PendingLinkedChild>>,
    linked_child_notify: tokio::sync::Notify,
}

impl Default for WsDispatchState {
    fn default() -> Self {
        Self {
            order_identities: DashMap::new(),
            order_contexts: DashMap::new(),
            pending_orders: Arc::new(DashMap::new()),
            pending_cancels: Arc::new(DashMap::new()),
            pending_amends: Arc::new(DashMap::new()),
            accepted_venue_order_ids: Mutex::new(FifoCacheMap::new()),
            triggered_orders: DedupCache::new(),
            filled_orders: DedupCache::new(),
            terminal_orders: DedupCache::new(),
            emitted_trades: DedupCache::new(),
            post_only_rejections: DedupCache::new(),
            lifecycle_bindings: Mutex::new(OrderLifecycleBindings::default()),
            pending_linked_children: Mutex::new(VecDeque::new()),
            linked_child_notify: tokio::sync::Notify::new(),
        }
    }
}

impl WsDispatchState {
    // Creates a dispatch state sharing the pending operation maps
    // with the WebSocket client that populates them
    pub(crate) fn with_pending_maps(
        pending_orders: Arc<DashMap<String, PendingOrderInfo>>,
        pending_cancels: Arc<DashMap<String, PendingOrderInfo>>,
        pending_amends: Arc<DashMap<String, PendingOrderInfo>>,
    ) -> Self {
        Self {
            pending_orders,
            pending_cancels,
            pending_amends,
            ..Default::default()
        }
    }

    pub(crate) fn track_order_context(&self, context: OrderContext) {
        self.order_contexts
            .insert(context.identity.client_order_id, context);
    }

    pub(crate) fn bind_algo_parent(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) {
        let mut bindings = self.lifecycle_bindings.lock();
        let binding = bindings.venue_by_client.get(&client_order_id).copied();
        if binding.is_some_and(|binding| binding.parent != venue_order_id) {
            log::error!(
                "Ignoring conflicting algo parent binding for {client_order_id}: expected={:?} received={venue_order_id}",
                binding.map(|binding| binding.parent),
            );
            return;
        }

        bindings
            .client_by_parent
            .insert(venue_order_id, client_order_id);
        bindings.venue_by_client.insert(
            client_order_id,
            binding.unwrap_or(OrderVenueBinding {
                parent: venue_order_id,
                child: None,
            }),
        );
        self.linked_child_notify.notify_one();
    }

    pub(crate) fn order_venue_binding(
        &self,
        client_order_id: ClientOrderId,
    ) -> Option<(VenueOrderId, bool)> {
        let bindings = self.lifecycle_bindings.lock();
        bindings
            .venue_by_client
            .get(&client_order_id)
            .map(|binding| {
                (
                    binding.child.unwrap_or(binding.parent),
                    binding.child.is_some(),
                )
            })
    }

    pub(crate) fn order_identity(&self, client_order_id: ClientOrderId) -> Option<OrderIdentity> {
        self.order_contexts
            .get(&client_order_id)
            .map(|entry| entry.identity)
            .or_else(|| {
                self.order_identities
                    .get(&client_order_id)
                    .map(|entry| *entry)
            })
    }

    pub(crate) fn remove_order_tracking(&self, client_order_id: ClientOrderId) {
        let pending = self.pending_linked_children.lock();
        let removed_context = self.order_contexts.remove(&client_order_id).is_some();
        self.order_identities.remove(&client_order_id);
        drop(pending);

        if removed_context {
            self.linked_child_notify.notify_one();
        }
    }

    pub(crate) fn resolve_algo_submit_failure(
        &self,
        client_order_id: ClientOrderId,
        failure: &CommandFailure,
    ) {
        let bindings = self.lifecycle_bindings.lock();
        if matches!(failure, CommandFailure::Ambiguous(_))
            && bindings.venue_by_client.contains_key(&client_order_id)
        {
            return;
        }

        self.remove_order_tracking(client_order_id);
    }

    pub(crate) async fn wait_for_linked_child_route(&self) {
        self.linked_child_notify.notified().await;
    }

    fn resolve_or_hold_linked_child(
        &self,
        parent_venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        message: &OKXOrderMsg,
    ) -> LinkedChildResolution {
        let candidate_client_order_ids = self
            .order_contexts
            .iter()
            .filter_map(|entry| {
                (entry.identity.instrument_id == instrument_id)
                    .then_some(entry.identity.client_order_id)
            })
            .collect::<Vec<_>>();
        let bindings = self.lifecycle_bindings.lock();
        if let Some(client_order_id) = bindings.client_order_id(&parent_venue_order_id) {
            return LinkedChildResolution::Bound(client_order_id);
        }

        let mut pending = self.pending_linked_children.lock();
        let candidate_client_order_ids = candidate_client_order_ids
            .iter()
            .filter(|client_order_id| {
                self.order_contexts.contains_key(client_order_id)
                    && !bindings.venue_by_client.contains_key(client_order_id)
            })
            .copied()
            .collect::<Vec<_>>();

        if candidate_client_order_ids.is_empty() {
            return LinkedChildResolution::External;
        }

        pending.push_back(PendingLinkedChild {
            parent_venue_order_id,
            candidate_client_order_ids,
            message: message.clone(),
        });
        LinkedChildResolution::Held
    }

    fn take_routable_linked_children(&self) -> Vec<OKXOrderMsg> {
        if self.pending_linked_children.lock().is_empty() {
            return Vec::new();
        }

        let bindings = self.lifecycle_bindings.lock();
        let mut pending = self.pending_linked_children.lock();
        let mut held = VecDeque::with_capacity(pending.len());
        let mut routable = Vec::new();

        while let Some(child) = pending.pop_front() {
            let is_bound = bindings
                .client_order_id(&child.parent_venue_order_id)
                .is_some();
            let is_pending = child
                .candidate_client_order_ids
                .iter()
                .any(|client_order_id| {
                    self.order_contexts.contains_key(client_order_id)
                        && !bindings.venue_by_client.contains_key(client_order_id)
                });

            if is_bound || !is_pending {
                routable.push(child.message);
            } else {
                held.push_back(child);
            }
        }

        *pending = held;
        routable
    }
}

impl WsDispatchState {
    /// Returns whether acceptance was already emitted for the order.
    #[must_use]
    pub fn contains_accepted(&self, cid: &ClientOrderId) -> bool {
        self.accepted_venue_order_ids.lock().contains_key(cid)
    }

    /// Records that acceptance was emitted for the order.
    pub fn insert_accepted(&self, cid: ClientOrderId, venue_order_id: VenueOrderId) {
        self.accepted_venue_order_ids
            .lock()
            .insert(cid, venue_order_id);
    }

    fn accepted_venue_order_id(&self, cid: &ClientOrderId) -> Option<VenueOrderId> {
        self.accepted_venue_order_ids.lock().get(cid).copied()
    }

    /// Returns whether the order was already triggered.
    #[must_use]
    pub fn contains_triggered(&self, cid: &ClientOrderId) -> bool {
        self.triggered_orders.contains(cid)
    }

    /// Records that the order was triggered.
    pub fn insert_triggered(&self, cid: ClientOrderId) {
        let _ = self.triggered_orders.insert(cid);
    }

    /// Returns whether the order was already filled.
    #[must_use]
    pub fn contains_filled(&self, cid: &ClientOrderId) -> bool {
        self.filled_orders.contains(cid)
    }

    /// Records that the order was filled.
    pub fn insert_filled(&self, cid: ClientOrderId) {
        let _ = self.filled_orders.insert(cid);
    }

    /// Returns whether the order already reached a terminal state.
    #[must_use]
    pub fn contains_terminal(&self, cid: &ClientOrderId) -> bool {
        self.terminal_orders.contains(cid)
    }

    /// Records that the order reached a terminal state.
    pub fn insert_terminal(&self, cid: ClientOrderId) {
        let _ = self.terminal_orders.insert(cid);
    }

    /// Returns `true` if this trade was already emitted (duplicate).
    /// Uses atomic insert to avoid TOCTOU races between concurrent streams.
    pub fn check_and_insert_trade(&self, trade_id: TradeId) -> bool {
        !self.emitted_trades.insert(trade_id)
    }

    #[must_use]
    pub fn contains_trade(&self, trade_id: &TradeId) -> bool {
        self.emitted_trades.contains(trade_id)
    }

    fn remove_accepted(&self, cid: &ClientOrderId) {
        self.accepted_venue_order_ids.lock().remove(cid);
    }

    fn remove_triggered(&self, cid: &ClientOrderId) {
        self.triggered_orders.remove(cid);
    }

    fn remove_filled(&self, cid: &ClientOrderId) {
        self.filled_orders.remove(cid);
    }

    fn insert_post_only_rejection(&self, order_id: Ustr) {
        let _ = self.post_only_rejections.insert(order_id);
    }

    fn contains_post_only_rejection(&self, order_id: &Ustr) -> bool {
        self.post_only_rejections.contains(order_id)
    }
}

/// Dispatches a WebSocket message with cross-stream deduplication.
///
/// For orders with a tracked identity (submitted through this client), produces
/// proper order events (OrderAccepted, OrderCanceled, OrderFilled, etc.).
/// For untracked orders (external or pre-existing), falls back to execution
/// reports for downstream reconciliation.
#[expect(clippy::too_many_arguments)]
pub fn dispatch_ws_message(
    message: OKXWsMessage,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    account_id: AccountId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    fee_cache: &mut AHashMap<Ustr, Money>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    order_state_cache: &mut AHashMap<ClientOrderId, OrderStateSnapshot>,
    clock: &AtomicTime,
) {
    let guard = instruments.load();
    let instruments: &AHashMap<Ustr, InstrumentAny> = &guard;

    match message {
        OKXWsMessage::Orders(order_msgs) => {
            let ts_init = clock.get_time_ns();
            let pending_order_msgs = state.take_routable_linked_children();
            if !pending_order_msgs.is_empty() {
                dispatch_order_messages(
                    &pending_order_msgs,
                    emitter,
                    state,
                    account_id,
                    instruments,
                    fee_cache,
                    filled_qty_cache,
                    order_state_cache,
                    ts_init,
                );
            }
            dispatch_order_messages(
                &order_msgs,
                emitter,
                state,
                account_id,
                instruments,
                fee_cache,
                filled_qty_cache,
                order_state_cache,
                ts_init,
            );
        }
        OKXWsMessage::SpreadOrders(order_msgs) => {
            let ts_init = clock.get_time_ns();
            dispatch_spread_order_messages(
                &order_msgs,
                emitter,
                state,
                account_id,
                instruments,
                filled_qty_cache,
                order_state_cache,
                ts_init,
            );
        }
        OKXWsMessage::AlgoOrders(algo_msgs) => {
            let ts_init = clock.get_time_ns();
            for msg in &algo_msgs {
                dispatch_algo_order_message(msg, emitter, state, account_id, instruments, ts_init);
            }
        }
        OKXWsMessage::Account(data) => {
            let ts_init = clock.get_time_ns();

            match serde_json::from_value::<Vec<OKXAccount>>(data) {
                Ok(accounts) => {
                    for account in &accounts {
                        match crate::common::parse::parse_account_state(
                            account, account_id, ts_init,
                        ) {
                            Ok(account_state) => emitter.send_account_state(account_state),
                            Err(e) => log::error!("Failed to parse account state: {e}"),
                        }
                    }
                }
                Err(e) => log::error!("Failed to deserialize account data: {e}"),
            }
        }
        OKXWsMessage::Positions(data) => {
            let ts_init = clock.get_time_ns();

            match serde_json::from_value::<Vec<OKXPosition>>(data) {
                Ok(positions) => {
                    for position in positions {
                        let Some(instrument) = instruments.get(&position.inst_id) else {
                            log::warn!("No cached instrument for position: {}", position.inst_id);
                            continue;
                        };
                        let instrument_id = instrument.id();
                        let size_precision = instrument.size_precision();

                        match crate::common::parse::parse_position_status_report(
                            &position,
                            account_id,
                            instrument_id,
                            size_precision,
                            ts_init,
                        ) {
                            Ok(report) => emitter.send_position_report(report),
                            Err(e) => log::error!("Failed to parse position report: {e}"),
                        }
                    }
                }
                Err(e) => log::error!("Failed to deserialize positions data: {e}"),
            }
        }
        OKXWsMessage::OrderResponse {
            id,
            op,
            code,
            msg,
            data,
        } => {
            let ts_init = clock.get_time_ns();

            for item in &data {
                let s_code = item
                    .get(OKX_FIELD_SCODE)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let s_msg = item
                    .get(OKX_FIELD_SMSG)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let sub_code = item
                    .get(OKX_FIELD_SUBCODE)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let reason = format_order_response_reason(s_code, s_msg, sub_code);
                let cl_ord_id = item
                    .get(OKX_FIELD_CLORDID)
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if s_code == OKX_SUCCESS_CODE {
                    log::debug!("Order response ok: op={op:?} cl_ord_id={cl_ord_id}");
                    match op {
                        OKXWsOperation::Order
                        | OKXWsOperation::BatchOrders
                        | OKXWsOperation::OrderAlgo => {
                            state.pending_orders.remove(cl_ord_id);
                        }
                        OKXWsOperation::CancelOrder
                        | OKXWsOperation::BatchCancelOrders
                        | OKXWsOperation::MassCancel
                        | OKXWsOperation::CancelAlgos => {
                            state.pending_cancels.remove(cl_ord_id);
                        }
                        OKXWsOperation::AmendOrder | OKXWsOperation::BatchAmendOrders => {
                            state.pending_amends.remove(cl_ord_id);
                        }
                        _ => {}
                    }
                    continue;
                }

                let Some(client_order_id) = parse_client_order_id(cl_ord_id) else {
                    log::warn!(
                        "Order response error without client_order_id: \
                         op={op:?} s_code={s_code} s_msg={s_msg}"
                    );
                    continue;
                };

                let Some(ident) = state.order_identity(client_order_id) else {
                    log::warn!(
                        "Order response error for untracked order: \
                         op={op:?} cl_ord_id={cl_ord_id} s_code={s_code} s_msg={s_msg}"
                    );
                    continue;
                };

                let venue_order_id = item
                    .get("ordId")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(VenueOrderId::new);

                match classify_okx_venue_code(s_code, reason.clone()) {
                    CommandFailure::Ambiguous(reason) => {
                        log::warn!(
                            "Ambiguous order response for {client_order_id}, awaiting reconciliation: \
                             op={op:?} s_code={s_code} {reason}"
                        );
                        continue;
                    }
                    CommandFailure::NotSent(_) => {
                        log::warn!(
                            "Unexpected NotSent classification for venue order response: \
                             op={op:?} cl_ord_id={cl_ord_id} s_code={s_code}"
                        );
                        continue;
                    }
                    CommandFailure::VenueRejected(_) => {}
                }

                match op {
                    OKXWsOperation::Order | OKXWsOperation::BatchOrders => {
                        state.remove_order_tracking(client_order_id);
                        state.pending_orders.remove(cl_ord_id);
                        emitter.emit_order_rejected_event(
                            ident.strategy_id,
                            ident.instrument_id,
                            client_order_id,
                            &reason,
                            ts_init,
                            false,
                        );
                    }
                    OKXWsOperation::CancelOrder
                    | OKXWsOperation::BatchCancelOrders
                    | OKXWsOperation::MassCancel => {
                        state.pending_cancels.remove(cl_ord_id);
                        emitter.emit_order_cancel_rejected_event(
                            ident.strategy_id,
                            ident.instrument_id,
                            client_order_id,
                            venue_order_id,
                            &reason,
                            ts_init,
                        );
                    }
                    OKXWsOperation::AmendOrder | OKXWsOperation::BatchAmendOrders => {
                        state.pending_amends.remove(cl_ord_id);
                        emitter.emit_order_modify_rejected_event(
                            ident.strategy_id,
                            ident.instrument_id,
                            client_order_id,
                            venue_order_id,
                            &reason,
                            ts_init,
                        );
                    }
                    _ => {
                        log::warn!(
                            "Order response error for unhandled op: \
                             op={op:?} cl_ord_id={cl_ord_id} s_code={s_code} s_msg={s_msg}"
                        );
                    }
                }
            }

            if code != "0" && data.is_empty() {
                log::warn!(
                    "Order response error (no data): id={id:?} op={op:?} code={code} msg={msg}"
                );
            }
        }
        OKXWsMessage::SendFailed {
            request_id,
            client_order_ids,
            op,
            error,
        } => {
            let failure = classify_okx_ws_failure(&error);
            let is_ambiguous = matches!(failure, CommandFailure::Ambiguous(_));
            log::warn!(
                "WebSocket send failed without structured venue response: \
                 request_id={request_id}, client_order_ids={client_order_ids:?}, \
                 op={op:?}, {failure:?}"
            );

            for client_order_id in client_order_ids {
                let key = client_order_id.as_str();

                match op {
                    Some(
                        OKXWsOperation::Order
                        | OKXWsOperation::BatchOrders
                        | OKXWsOperation::OrderAlgo,
                    ) => {
                        if !is_ambiguous {
                            state.pending_orders.remove(key);
                        }
                        emit_send_failed_submit(&failure, state, emitter, clock, client_order_id);
                    }
                    Some(
                        OKXWsOperation::CancelOrder
                        | OKXWsOperation::BatchCancelOrders
                        | OKXWsOperation::MassCancel
                        | OKXWsOperation::CancelAlgos,
                    ) => {
                        if !is_ambiguous {
                            state.pending_cancels.remove(key);
                        }
                    }
                    Some(OKXWsOperation::AmendOrder | OKXWsOperation::BatchAmendOrders) => {
                        if !is_ambiguous {
                            state.pending_amends.remove(key);
                        }
                        emit_send_failed_modify(&failure, state, emitter, clock, client_order_id);
                    }
                    _ => {}
                }
            }
        }
        OKXWsMessage::ChannelData { channel, .. } => {
            log::debug!("Ignoring data channel message on execution client: {channel:?}");
        }
        OKXWsMessage::SubscriptionFailed {
            channel,
            inst_id,
            code,
            msg,
        } => {
            log::error!(
                "OKX rejected {channel:?} subscription for {inst_id:?} \
                 (code={code}, msg={msg}); execution updates for it will not flow"
            );
        }
        OKXWsMessage::LiquidationWarnings(warnings) => {
            for warning in warnings {
                log::warn!(
                    "Liquidation warning: inst_id={}, pos_side={:?}, pos={}, mgn_ratio={}, mark_px={}, mgn_mode={:?}",
                    warning.inst_id,
                    warning.pos_side,
                    warning.pos,
                    warning.mgn_ratio,
                    warning.mark_px,
                    warning.mgn_mode,
                );
            }
        }
        OKXWsMessage::BookData { .. }
        | OKXWsMessage::RpiBookData { .. }
        | OKXWsMessage::Instruments(_) => {
            log::debug!("Ignoring data message on execution client");
        }
        OKXWsMessage::Error(e) => {
            log::warn!(
                "Websocket error: code={} message={} conn_id={:?}",
                e.code,
                e.message,
                e.conn_id
            );
        }
        OKXWsMessage::Reconnected => {
            log::info!("Websocket reconnected");
        }
        OKXWsMessage::Authenticated => {
            log::debug!("Websocket authenticated");
        }
    }
}

fn route_algo_order_message(
    msg: &OKXAlgoOrderMsg,
    state: &WsDispatchState,
) -> ExecutionUpdateRoute {
    if matches!(
        msg.ord_type,
        OKXAlgoOrderType::Iceberg
            | OKXAlgoOrderType::Twap
            | OKXAlgoOrderType::Chase
            | OKXAlgoOrderType::Other
    ) || msg.state == OKXAlgoOrderStatus::Unknown
    {
        return ExecutionUpdateRoute::Suppressed;
    }

    let direct_client_order_id = parse_client_order_id(&msg.algo_cl_ord_id)
        .or_else(|| parse_client_order_id(&msg.cl_ord_id));

    if let Some(client_order_id) = direct_client_order_id {
        if state.contains_terminal(&client_order_id) {
            return ExecutionUpdateRoute::Suppressed;
        }

        if let Some(context) = state
            .order_contexts
            .get(&client_order_id)
            .map(|entry| *entry)
        {
            return ExecutionUpdateRoute::Tracked(client_order_id, context);
        }

        if state.order_identities.contains_key(&client_order_id) {
            return ExecutionUpdateRoute::Suppressed;
        }
    }

    let parent_venue_order_id = VenueOrderId::new(msg.algo_id.as_str());
    let client_order_id = {
        let bindings = state.lifecycle_bindings.lock();
        bindings.client_order_id(&parent_venue_order_id)
    };

    let Some(client_order_id) = client_order_id else {
        return ExecutionUpdateRoute::External;
    };

    if state.contains_terminal(&client_order_id) {
        return ExecutionUpdateRoute::Suppressed;
    }

    state
        .order_contexts
        .get(&client_order_id)
        .map_or(ExecutionUpdateRoute::Suppressed, |entry| {
            ExecutionUpdateRoute::Tracked(client_order_id, *entry)
        })
}

fn dispatch_algo_order_message(
    msg: &OKXAlgoOrderMsg,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    ts_init: UnixNanos,
) {
    let route = route_algo_order_message(msg, state);

    match route {
        ExecutionUpdateRoute::External => {
            match parse_algo_order_msg(msg, account_id, instruments, ts_init) {
                Ok(Some(report)) => dispatch_execution_reports(vec![report], emitter, state),
                Ok(None) => {}
                Err(e) => log::error!("Failed to parse external algo order message: {e}"),
            }
        }
        ExecutionUpdateRoute::Suppressed => {
            log::debug!(
                "Suppressing algo order update: algo_id={} state={:?}",
                msg.algo_id,
                msg.state,
            );
        }
        ExecutionUpdateRoute::Tracked(client_order_id, context) => {
            let Some(instrument) = instruments.get(&msg.inst_id) else {
                log::warn!(
                    "No instrument for {}, skipping algo order message",
                    msg.inst_id
                );
                return;
            };
            dispatch_tracked_algo_order_message(
                msg,
                client_order_id,
                context,
                instrument,
                emitter,
                state,
                account_id,
                ts_init,
            );
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "tracked routing requires the resolved context, venue state, and event timestamps"
)]
fn dispatch_tracked_algo_order_message(
    msg: &OKXAlgoOrderMsg,
    client_order_id: ClientOrderId,
    mut context: OrderContext,
    instrument: &InstrumentAny,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    let parent_venue_order_id = VenueOrderId::new(msg.algo_id.as_str());
    let ts_event = parse_millisecond_timestamp(msg.u_time);
    let mut bindings = state.lifecycle_bindings.lock();
    let mut is_terminal = false;
    let mut binding = bindings
        .venue_by_client
        .get(&client_order_id)
        .copied()
        .unwrap_or(OrderVenueBinding {
            parent: parent_venue_order_id,
            child: None,
        });

    if binding.parent != parent_venue_order_id {
        log::error!(
            "Suppressing conflicting algo parent binding for {client_order_id}: expected={} received={parent_venue_order_id}",
            binding.parent,
        );
        return;
    }

    if binding.child.is_none() {
        context = refresh_algo_order_context(msg, context, instrument, account_id, ts_init);
        state.track_order_context(context);
    }

    bindings
        .client_by_parent
        .insert(parent_venue_order_id, client_order_id);

    match msg.state {
        OKXAlgoOrderStatus::Live | OKXAlgoOrderStatus::Pause => {
            if binding.child.is_some() {
                log::debug!(
                    "Suppressing stale algo parent acceptance for {client_order_id}: algo_id={}",
                    msg.algo_id,
                );
            } else {
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    parent_venue_order_id,
                    &context.identity,
                    emitter,
                    state,
                    ts_event,
                    ts_init,
                );
            }
        }
        OKXAlgoOrderStatus::Effective
        | OKXAlgoOrderStatus::OrderPlaced
        | OKXAlgoOrderStatus::PartiallyEffective
        | OKXAlgoOrderStatus::Filled
        | OKXAlgoOrderStatus::PartiallyFailed => {
            let child_venue_order_id = algo_child_venue_order_id(msg);
            if let Some(child_venue_order_id) = child_venue_order_id {
                bind_algo_child_and_emit_transition(
                    client_order_id,
                    parent_venue_order_id,
                    child_venue_order_id,
                    &mut binding,
                    context,
                    account_id,
                    ts_event,
                    ts_init,
                    emitter,
                    state,
                );
            } else {
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    parent_venue_order_id,
                    &context.identity,
                    emitter,
                    state,
                    ts_event,
                    ts_init,
                );
            }

            if matches!(
                msg.state,
                OKXAlgoOrderStatus::Filled | OKXAlgoOrderStatus::PartiallyFailed
            ) {
                log::debug!(
                    "Deferring tracked algo {:?} update for {client_order_id} to regular child execution updates",
                    msg.state,
                );
            }
        }
        OKXAlgoOrderStatus::Canceled => {
            if binding.child.is_some() {
                log::debug!(
                    "Suppressing stale canceled algo parent for triggered order {client_order_id}"
                );
            } else {
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    parent_venue_order_id,
                    &context.identity,
                    emitter,
                    state,
                    ts_event,
                    ts_init,
                );
                let canceled = OrderCanceled::new(
                    emitter.trader_id(),
                    context.identity.strategy_id,
                    context.identity.instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(parent_venue_order_id),
                    Some(account_id),
                    None,
                );
                state.insert_terminal(client_order_id);
                state.remove_accepted(&client_order_id);
                state.remove_order_tracking(client_order_id);
                is_terminal = true;
                emitter.send_order_event(OrderEventAny::Canceled(canceled));
            }
        }
        OKXAlgoOrderStatus::OrderFailed => {
            if binding.child.is_some() {
                log::debug!(
                    "Suppressing stale failed algo parent for triggered order {client_order_id}"
                );
            } else {
                let reason = if msg.fail_code.is_empty() {
                    "OKX algo order failed"
                } else {
                    msg.fail_code.as_str()
                };
                let rejected = OrderRejected::new(
                    emitter.trader_id(),
                    context.identity.strategy_id,
                    context.identity.instrument_id,
                    client_order_id,
                    account_id,
                    Ustr::from(reason),
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    false,
                );
                state.insert_terminal(client_order_id);
                state.remove_accepted(&client_order_id);
                state.remove_order_tracking(client_order_id);
                is_terminal = true;
                emitter.send_order_event(OrderEventAny::Rejected(rejected));
            }
        }
        OKXAlgoOrderStatus::Unknown => {}
    }

    if is_terminal {
        bindings.finish(client_order_id, binding);
    } else {
        bindings.venue_by_client.insert(client_order_id, binding);
    }

    drop(bindings);
    state.linked_child_notify.notify_one();
}

fn algo_child_venue_order_id(msg: &OKXAlgoOrderMsg) -> Option<VenueOrderId> {
    if !msg.ord_id.is_empty() {
        return Some(VenueOrderId::new(msg.ord_id.as_str()));
    }

    match msg.ord_id_list.as_slice() {
        [order_id] if !order_id.is_empty() => Some(VenueOrderId::new(order_id.as_str())),
        [] => None,
        order_ids => {
            log::warn!(
                "Cannot bind algo order {} to {} triggered child IDs",
                msg.algo_id,
                order_ids.len(),
            );
            None
        }
    }
}

fn refresh_algo_order_context(
    msg: &OKXAlgoOrderMsg,
    mut context: OrderContext,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> OrderContext {
    match parse_algo_order_status_report(msg, instrument, account_id, ts_init) {
        Ok(report) => {
            if !msg.sz.is_empty() {
                context.quantity = report.quantity;
            }

            context.price = report.price;
            context.trigger_price = report.trigger_price;
            context.trigger_type = report.trigger_type;
        }
        Err(e) => {
            log::error!(
                "Failed to refresh tracked algo order context for {}: {e}",
                context.identity.client_order_id,
            );
            return context;
        }
    }

    if !msg.actual_sz.is_empty() && msg.actual_sz != "0" {
        match parse_quantity(msg.actual_sz.as_str(), instrument.size_precision()) {
            Ok(quantity) => context.quantity = quantity,
            Err(e) => log::error!(
                "Failed to refresh tracked algo actual quantity for {}: {e}",
                context.identity.client_order_id,
            ),
        }
    }

    context
}

fn refresh_regular_child_context(
    msg: &OKXOrderMsg,
    mut context: OrderContext,
    instrument: &InstrumentAny,
) -> OrderContext {
    match parse_quantity(&msg.sz, instrument.size_precision()) {
        Ok(quantity) => context.quantity = quantity,
        Err(e) => log::error!(
            "Failed to refresh tracked child quantity for {}: {e}",
            context.identity.client_order_id,
        ),
    }

    context.price = if is_market_price(&msg.px) {
        None
    } else {
        match parse_price(&msg.px, instrument.price_precision()) {
            Ok(price) => Some(price),
            Err(e) => {
                log::error!(
                    "Failed to refresh tracked child price for {}: {e}",
                    context.identity.client_order_id,
                );
                context.price
            }
        }
    };
    context
}

#[expect(clippy::too_many_arguments)]
fn bind_algo_child_and_emit_transition(
    client_order_id: ClientOrderId,
    parent_venue_order_id: VenueOrderId,
    child_venue_order_id: VenueOrderId,
    binding: &mut OrderVenueBinding,
    context: OrderContext,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
) {
    if let Some(bound_child) = binding.child {
        if bound_child != child_venue_order_id {
            log::error!(
                "Suppressing conflicting algo child binding for {client_order_id}: expected={bound_child} received={child_venue_order_id}"
            );
        }
        return;
    }

    binding.child = Some(child_venue_order_id);
    state.track_order_context(context);
    ensure_accepted_emitted(
        client_order_id,
        account_id,
        parent_venue_order_id,
        &context.identity,
        emitter,
        state,
        ts_event,
        ts_init,
    );

    if state.accepted_venue_order_id(&client_order_id) != Some(child_venue_order_id) {
        state.insert_accepted(client_order_id, child_venue_order_id);
        emit_child_update(
            client_order_id,
            child_venue_order_id,
            context,
            account_id,
            ts_event,
            ts_init,
            emitter,
        );
    }

    if !state.contains_triggered(&client_order_id) {
        state.insert_triggered(client_order_id);

        if TRIGGERABLE_ORDER_TYPES.contains(&context.identity.order_type) {
            let triggered = OrderTriggered::new(
                emitter.trader_id(),
                context.identity.strategy_id,
                context.identity.instrument_id,
                client_order_id,
                UUID4::new(),
                ts_event,
                ts_init,
                false,
                Some(child_venue_order_id),
                Some(account_id),
            );
            emitter.send_order_event(OrderEventAny::Triggered(triggered));
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn refresh_bound_child_and_emit_update(
    client_order_id: ClientOrderId,
    child_venue_order_id: VenueOrderId,
    context: OrderContext,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    emit_update: bool,
) {
    let terms_changed = state
        .order_contexts
        .get(&client_order_id)
        .is_some_and(|previous| {
            previous.quantity != context.quantity
                || previous.price != context.price
                || previous.trigger_price != context.trigger_price
        });
    state.track_order_context(context);

    if !terms_changed || !emit_update {
        return;
    }

    state.insert_accepted(client_order_id, child_venue_order_id);
    emit_child_update(
        client_order_id,
        child_venue_order_id,
        context,
        account_id,
        ts_event,
        ts_init,
        emitter,
    );
}

fn emit_child_update(
    client_order_id: ClientOrderId,
    child_venue_order_id: VenueOrderId,
    context: OrderContext,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
    emitter: &ExecutionEventEmitter,
) {
    let updated = OrderUpdated::new(
        emitter.trader_id(),
        context.identity.strategy_id,
        context.identity.instrument_id,
        client_order_id,
        context.quantity,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
        Some(child_venue_order_id),
        Some(account_id),
        context.price,
        context.trigger_price,
        None,
        false,
    );
    emitter.send_order_event(OrderEventAny::Updated(updated));
}

/// Dispatches order messages, producing proper order events for tracked orders
/// and falling back to execution reports for untracked/external orders.
#[expect(clippy::too_many_arguments)]
fn dispatch_order_messages(
    order_msgs: &[OKXOrderMsg],
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    fee_cache: &mut AHashMap<Ustr, Money>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    order_state_cache: &mut AHashMap<ClientOrderId, OrderStateSnapshot>,
    ts_init: UnixNanos,
) {
    for msg in order_msgs {
        let Some(instrument) = instruments.get(&msg.inst_id) else {
            log::warn!("No instrument for {}, skipping order message", msg.inst_id);
            continue;
        };

        let direct_client_order_id = parse_client_order_id(&msg.cl_ord_id);
        let parent_client_order_id = msg
            .algo_cl_ord_id
            .as_deref()
            .and_then(parse_client_order_id);
        let linked_parent_venue_order_id = msg
            .algo_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                msg.linked_algo_ord
                    .as_ref()
                    .map(|linked| linked.algo_id.as_str())
                    .filter(|value| !value.is_empty())
            })
            .map(VenueOrderId::new);

        // Triggered child orders may have a generated or empty cl_ord_id.
        // Resolve the tracked parent before falling back to a report.
        let direct_resolution = [direct_client_order_id, parent_client_order_id]
            .into_iter()
            .flatten()
            .find_map(|client_order_id| {
                state
                    .order_identity(client_order_id)
                    .map(|identity| (client_order_id, Some(identity)))
            });

        let bound_client_order_id = if direct_resolution.is_none()
            && let Some(parent_venue_order_id) = linked_parent_venue_order_id
        {
            match state.resolve_or_hold_linked_child(parent_venue_order_id, instrument.id(), msg) {
                LinkedChildResolution::Bound(client_order_id) => Some(client_order_id),
                LinkedChildResolution::Held => {
                    log::debug!(
                        "Holding linked child update during algo parent binding: ord_id={} parent_id={parent_venue_order_id}",
                        msg.ord_id,
                    );
                    continue;
                }
                LinkedChildResolution::External => None,
            }
        } else {
            None
        };

        let resolved = direct_resolution
            .or_else(|| {
                bound_client_order_id.and_then(|client_order_id| {
                    state
                        .order_identity(client_order_id)
                        .map(|identity| (client_order_id, Some(identity)))
                })
            })
            .or_else(|| {
                bound_client_order_id
                    .or(parent_client_order_id)
                    .or(direct_client_order_id)
                    .map(|client_order_id| (client_order_id, None))
            });

        let Some((client_order_id, identity)) = resolved else {
            log::debug!(
                "Order without client or algo client order ID (ord_id={}), sending as report",
                msg.ord_id
            );
            dispatch_order_msg_as_report(
                msg,
                account_id,
                instruments,
                fee_cache,
                filled_qty_cache,
                emitter,
                state,
                ts_init,
            );
            continue;
        };

        if let Some(ident) = identity {
            let context = state
                .order_contexts
                .get(&client_order_id)
                .map(|entry| refresh_regular_child_context(msg, *entry, instrument));
            let mut lifecycle_bindings = context.map(|_| state.lifecycle_bindings.lock());

            if let (Some(context), Some(bindings)) = (context, lifecycle_bindings.as_mut()) {
                let parent_venue_order_id = linked_parent_venue_order_id.or_else(|| {
                    bindings
                        .venue_by_client
                        .get(&client_order_id)
                        .map(|binding| binding.parent)
                });

                let child_venue_order_id = VenueOrderId::new(msg.ord_id);
                let parent_venue_order_id = parent_venue_order_id.unwrap_or(child_venue_order_id);
                let mut binding = bindings
                    .venue_by_client
                    .get(&client_order_id)
                    .copied()
                    .unwrap_or(OrderVenueBinding {
                        parent: parent_venue_order_id,
                        child: None,
                    });

                if binding.parent != parent_venue_order_id {
                    log::error!(
                        "Suppressing conflicting child parent binding for {client_order_id}: expected={} received={parent_venue_order_id}",
                        binding.parent,
                    );
                    continue;
                }

                bindings
                    .client_by_parent
                    .insert(parent_venue_order_id, client_order_id);
                let ts_event = parse_millisecond_timestamp(msg.u_time);

                if binding.child == Some(child_venue_order_id) {
                    refresh_bound_child_and_emit_update(
                        client_order_id,
                        child_venue_order_id,
                        context,
                        account_id,
                        ts_event,
                        ts_init,
                        emitter,
                        state,
                        !order_state_cache.contains_key(&client_order_id),
                    );
                } else {
                    bind_algo_child_and_emit_transition(
                        client_order_id,
                        parent_venue_order_id,
                        child_venue_order_id,
                        &mut binding,
                        context,
                        account_id,
                        ts_event,
                        ts_init,
                        emitter,
                        state,
                    );
                }

                bindings.venue_by_client.insert(client_order_id, binding);
            }

            let is_post_only_cancel = is_post_only_auto_cancel(msg);

            if is_post_only_cancel
                || (!state.contains_accepted(&client_order_id) && is_unfilled_rpi_cancel(msg))
            {
                if is_post_only_cancel {
                    state.insert_post_only_rejection(msg.ord_id);
                }

                let ts_event = parse_millisecond_timestamp(msg.u_time);
                let reason = if msg.ord_type == OKXOrderType::Rpi {
                    msg.cancel_source_reason
                        .as_deref()
                        .filter(|reason| !reason.is_empty())
                        .unwrap_or("RPI order canceled before acceptance")
                } else {
                    "Post-only order would have taken liquidity"
                };
                let rejected = OrderRejected::new(
                    emitter.trader_id(),
                    ident.strategy_id,
                    instrument.id(),
                    client_order_id,
                    account_id,
                    Ustr::from(reason),
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    true, // due_post_only
                );
                state.remove_order_tracking(client_order_id);
                if let Some(bindings) = lifecycle_bindings.as_mut() {
                    state.insert_terminal(client_order_id);
                    if let Some(binding) = bindings.venue_by_client.get(&client_order_id).copied() {
                        bindings.finish(client_order_id, binding);
                    }
                }

                order_state_cache.remove(&client_order_id);
                fee_cache.remove(&msg.ord_id);
                filled_qty_cache.remove(&msg.ord_id);
                emitter.send_order_event(OrderEventAny::Rejected(rejected));
                continue;
            }

            let previous_fee = fee_cache.get(&msg.ord_id).copied();
            let previous_filled_qty = filled_qty_cache.get(&msg.ord_id).copied();
            let previous_state = order_state_cache.get(&client_order_id);

            match parse_order_event(
                msg,
                client_order_id,
                account_id,
                emitter.trader_id(),
                ident.strategy_id,
                instrument,
                previous_fee,
                previous_filled_qty,
                previous_state,
                ts_init,
            ) {
                Ok(event) => {
                    update_order_state_cache(msg, instrument, client_order_id, order_state_cache);
                    dispatch_parsed_order_event(
                        event,
                        client_order_id,
                        account_id,
                        VenueOrderId::new(msg.ord_id),
                        &ident,
                        instrument,
                        msg.state,
                        emitter,
                        state,
                        order_state_cache,
                        ts_init,
                    );

                    if state.contains_terminal(&client_order_id)
                        && let Some(bindings) = lifecycle_bindings.as_mut()
                        && let Some(binding) =
                            bindings.venue_by_client.get(&client_order_id).copied()
                    {
                        bindings.finish(client_order_id, binding);
                    }

                    update_fee_fill_caches(msg, instrument, fee_cache, filled_qty_cache);
                }
                Err(e) => log::error!("Failed to parse order event for {client_order_id}: {e}"),
            }
        } else if state.contains_terminal(&client_order_id) {
            dispatch_terminal_order_fill_as_report(
                msg,
                client_order_id,
                account_id,
                instruments,
                fee_cache,
                filled_qty_cache,
                emitter,
                state,
                ts_init,
            );
        } else if is_post_only_auto_cancel(msg) && state.contains_post_only_rejection(&msg.ord_id) {
            log::debug!(
                "Skipping replayed post-only rejection for {client_order_id}: ord_id={}",
                msg.ord_id
            );
        } else {
            log::debug!(
                "Untracked order {client_order_id} (ord_id={}), sending as report for reconciliation",
                msg.ord_id
            );
            dispatch_order_msg_as_report(
                msg,
                account_id,
                instruments,
                fee_cache,
                filled_qty_cache,
                emitter,
                state,
                ts_init,
            );
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn dispatch_spread_order_messages(
    order_msgs: &[OKXSpreadOrder],
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    order_state_cache: &mut AHashMap<ClientOrderId, OrderStateSnapshot>,
    ts_init: UnixNanos,
) {
    for msg in order_msgs {
        let Some(instrument) = instruments.get(&msg.sprd_id) else {
            log::warn!(
                "No instrument for {}, skipping spread order message",
                msg.sprd_id
            );
            continue;
        };

        let Some(client_order_id) = parse_client_order_id(msg.cl_ord_id.as_str()) else {
            log::debug!(
                "Spread order without client_order_id (ord_id={}), sending as report",
                msg.ord_id
            );
            dispatch_spread_order_msg_as_report(
                msg,
                account_id,
                instruments,
                filled_qty_cache,
                emitter,
                state,
                ts_init,
            );
            continue;
        };

        let identity = state.order_identity(client_order_id);

        if let Some(ident) = identity {
            if is_spread_post_only_auto_cancel(msg) {
                let ts_event = msg
                    .u_time
                    .or(msg.c_time)
                    .map_or(ts_init, parse_millisecond_timestamp);
                let rejected = OrderRejected::new(
                    emitter.trader_id(),
                    ident.strategy_id,
                    instrument.id(),
                    client_order_id,
                    account_id,
                    Ustr::from(OKX_POST_ONLY_CANCEL_REASON),
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    true,
                );
                state.remove_order_tracking(client_order_id);
                order_state_cache.remove(&client_order_id);
                filled_qty_cache.remove(&msg.ord_id);
                emitter.send_order_event(OrderEventAny::Rejected(rejected));
                continue;
            }

            let previous_filled_qty = filled_qty_cache.get(&msg.ord_id).copied();
            let previous_state = order_state_cache.get(&client_order_id);

            match parse_spread_order_event(
                msg,
                client_order_id,
                account_id,
                emitter.trader_id(),
                ident.strategy_id,
                instrument,
                previous_filled_qty,
                previous_state,
                ts_init,
            ) {
                Ok(event) => {
                    update_spread_order_state_cache(
                        msg,
                        instrument,
                        client_order_id,
                        order_state_cache,
                    );
                    dispatch_parsed_order_event(
                        event,
                        client_order_id,
                        account_id,
                        VenueOrderId::new(msg.ord_id.as_str()),
                        &ident,
                        instrument,
                        msg.state,
                        emitter,
                        state,
                        order_state_cache,
                        ts_init,
                    );
                    update_spread_fill_cache(msg, instrument, filled_qty_cache);
                }
                Err(e) => {
                    log::error!("Failed to parse spread order event for {client_order_id}: {e}");
                }
            }
        } else {
            log::debug!(
                "Untracked spread order {client_order_id} (ord_id={}), sending as report for reconciliation",
                msg.ord_id
            );
            dispatch_spread_order_msg_as_report(
                msg,
                account_id,
                instruments,
                filled_qty_cache,
                emitter,
                state,
                ts_init,
            );
        }
    }
}

/// Dispatches a parsed order event as a proper `OrderEventAny`.
///
/// Guarantees the `Submitted -> Accepted -> ...` lifecycle by synthesizing
/// `OrderAccepted` before any other event when one has not yet been emitted.
/// Duplicate `Accepted` events (e.g. from reconnect replays) are suppressed.
#[expect(clippy::too_many_arguments)]
fn dispatch_parsed_order_event(
    event: ParsedOrderEvent,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    instrument: &InstrumentAny,
    venue_status: OKXOrderStatus,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    order_state_cache: &mut AHashMap<ClientOrderId, OrderStateSnapshot>,
    ts_init: UnixNanos,
) {
    let is_terminal;

    match event {
        ParsedOrderEvent::Accepted(e) => {
            if state.contains_filled(&client_order_id) || state.contains_terminal(&client_order_id)
            {
                log::debug!("Skipping duplicate Accepted for {client_order_id}");
                return;
            }

            if state.contains_accepted(&client_order_id) {
                emit_venue_order_id_update_if_changed(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    identity,
                    e.ts_event,
                    emitter,
                    state,
                    order_state_cache,
                    ts_init,
                );
                return;
            }

            if state.contains_triggered(&client_order_id) {
                log::debug!("Skipping duplicate Accepted for {client_order_id}");
                return;
            }

            state.insert_accepted(client_order_id, venue_order_id);
            is_terminal = false;
            emitter.send_order_event(OrderEventAny::Accepted(e));
        }
        ParsedOrderEvent::Triggered(e) => {
            if state.contains_filled(&client_order_id) {
                log::debug!("Skipping stale Triggered for {client_order_id} (already filled)");
                return;
            }

            if !TRIGGERABLE_ORDER_TYPES.contains(&identity.order_type) {
                log::debug!(
                    "Skipping OrderTriggered for {} order {client_order_id}: market-style stops have no TRIGGERED state",
                    identity.order_type,
                );
                state.insert_triggered(client_order_id);
                return;
            }

            ensure_accepted_emitted(
                client_order_id,
                account_id,
                venue_order_id,
                identity,
                emitter,
                state,
                ts_init,
                ts_init,
            );
            state.insert_triggered(client_order_id);
            is_terminal = false;
            emitter.send_order_event(OrderEventAny::Triggered(e));
        }
        ParsedOrderEvent::Canceled(e) => {
            ensure_accepted_emitted(
                client_order_id,
                account_id,
                venue_order_id,
                identity,
                emitter,
                state,
                ts_init,
                ts_init,
            );
            state.remove_triggered(&client_order_id);
            state.remove_filled(&client_order_id);
            is_terminal = true;
            emitter.send_order_event(OrderEventAny::Canceled(e));
        }
        ParsedOrderEvent::Expired(e) => {
            ensure_accepted_emitted(
                client_order_id,
                account_id,
                venue_order_id,
                identity,
                emitter,
                state,
                ts_init,
                ts_init,
            );
            state.remove_triggered(&client_order_id);
            state.remove_filled(&client_order_id);
            is_terminal = true;
            emitter.send_order_event(OrderEventAny::Expired(e));
        }
        ParsedOrderEvent::Updated(e) => {
            ensure_accepted_emitted(
                client_order_id,
                account_id,
                venue_order_id,
                identity,
                emitter,
                state,
                ts_init,
                ts_init,
            );
            is_terminal = false;
            emitter.send_order_event(OrderEventAny::Updated(e));
        }
        ParsedOrderEvent::Fill(fill_report) => {
            is_terminal = venue_status == OKXOrderStatus::Filled;

            if state.check_and_insert_trade(fill_report.trade_id) {
                log::debug!(
                    "Skipping duplicate fill for {client_order_id}: trade_id={}",
                    fill_report.trade_id
                );
            } else {
                emit_venue_order_id_update_if_changed(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    identity,
                    fill_report.ts_event,
                    emitter,
                    state,
                    order_state_cache,
                    ts_init,
                );
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    identity,
                    emitter,
                    state,
                    ts_init,
                    ts_init,
                );
                state.insert_filled(client_order_id);
                state.remove_triggered(&client_order_id);
                let filled = fill_report_to_order_filled(
                    &fill_report,
                    emitter.trader_id(),
                    identity,
                    instrument.quote_currency(),
                );
                emitter.send_order_event(OrderEventAny::Filled(filled));
            }
        }
        ParsedOrderEvent::StatusOnly(report) => {
            is_terminal = matches!(
                report.order_status,
                OrderStatus::Filled | OrderStatus::Canceled | OrderStatus::Expired
            );
            emitter.send_order_status_report(*report);
        }
        ParsedOrderEvent::Skipped => return,
    }

    if is_terminal {
        state.insert_terminal(client_order_id);
        state.remove_order_tracking(client_order_id);
        state.remove_accepted(&client_order_id);
        order_state_cache.remove(&client_order_id);
        // Keep fee_cache and filled_qty_cache entries: replayed terminal
        // messages go through the untracked report path and need prior
        // cumulative state to avoid re-emitting the full fill quantity
    }
}

/// Synthesizes and emits `OrderAccepted` if one has not yet been emitted for
/// this order. Handles fast-filling orders that skip the `Live` state on OKX.
#[expect(
    clippy::too_many_arguments,
    reason = "acceptance reconstruction requires identity, venue state, and event timestamps"
)]
fn ensure_accepted_emitted(
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    if state.contains_accepted(&client_order_id)
        || state.contains_terminal(&client_order_id)
        || state.contains_filled(&client_order_id)
    {
        return;
    }

    state.insert_accepted(client_order_id, venue_order_id);
    let accepted = OrderAccepted::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
    );
    emitter.send_order_event(OrderEventAny::Accepted(accepted));
}

#[expect(clippy::too_many_arguments)]
fn emit_venue_order_id_update_if_changed(
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    ts_event: UnixNanos,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    order_state_cache: &AHashMap<ClientOrderId, OrderStateSnapshot>,
    ts_init: UnixNanos,
) {
    let Some(accepted_venue_order_id) = state.accepted_venue_order_id(&client_order_id) else {
        return;
    };

    if accepted_venue_order_id == venue_order_id {
        return;
    }

    let Some(snapshot) = order_state_cache.get(&client_order_id) else {
        return;
    };

    state.insert_accepted(client_order_id, venue_order_id);
    let updated = OrderUpdated::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        snapshot.quantity,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
        Some(venue_order_id),
        Some(account_id),
        snapshot.price,
        None,
        None,
        false,
    );
    emitter.send_order_event(OrderEventAny::Updated(updated));
}

/// Converts a [`FillReport`] into an [`OrderFilled`] event using tracked identity.
fn fill_report_to_order_filled(
    report: &FillReport,
    trader_id: TraderId,
    identity: &OrderIdentity,
    quote_currency: Currency,
) -> OrderFilled {
    OrderFilled::new(
        trader_id,
        identity.strategy_id,
        report.instrument_id,
        report
            .client_order_id
            .expect("tracked order has client_order_id"),
        report.venue_order_id,
        report.account_id,
        report.trade_id,
        identity.order_side,
        identity.order_type,
        report.last_qty,
        report.last_px,
        quote_currency,
        report.liquidity_side,
        UUID4::new(),
        report.ts_event,
        report.ts_init,
        false,
        report.venue_position_id,
        Some(report.commission),
        None,
    )
}

/// Falls back to the report path for a single order message.
#[expect(clippy::too_many_arguments)]
fn dispatch_order_msg_as_report(
    msg: &OKXOrderMsg,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    fee_cache: &mut AHashMap<Ustr, Money>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    match parse_order_msg(
        msg,
        account_id,
        instruments,
        fee_cache,
        filled_qty_cache,
        ts_init,
    ) {
        Ok(report) => {
            dispatch_execution_reports(vec![report], emitter, state);

            if let Some(instrument) = instruments.get(&msg.inst_id) {
                update_fee_fill_caches(msg, instrument, fee_cache, filled_qty_cache);
            }
        }
        Err(e) => log::error!("Failed to parse order message as report: {e}"),
    }
}

#[expect(clippy::too_many_arguments)]
fn dispatch_terminal_order_fill_as_report(
    msg: &OKXOrderMsg,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    fee_cache: &mut AHashMap<Ustr, Money>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    match parse_order_msg(
        msg,
        account_id,
        instruments,
        fee_cache,
        filled_qty_cache,
        ts_init,
    ) {
        Ok(ExecutionReport::Fill(mut report)) => {
            report.client_order_id = Some(client_order_id);
            dispatch_execution_reports(vec![ExecutionReport::Fill(report)], emitter, state);

            if let Some(instrument) = instruments.get(&msg.inst_id) {
                update_fee_fill_caches(msg, instrument, fee_cache, filled_qty_cache);
            }
        }
        Ok(ExecutionReport::Order(_)) => {
            log::debug!(
                "Suppressing stale regular order status for terminal tracked order {client_order_id}: ord_id={}",
                msg.ord_id,
            );
        }
        Err(e) => log::error!("Failed to parse terminal order update: {e}"),
    }
}

fn dispatch_spread_order_msg_as_report(
    msg: &OKXSpreadOrder,
    account_id: AccountId,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    match parse_spread_order_msg(msg, account_id, instruments, filled_qty_cache, ts_init) {
        Ok(report) => {
            dispatch_execution_reports(vec![report], emitter, state);

            if let Some(instrument) = instruments.get(&msg.sprd_id) {
                update_spread_fill_cache(msg, instrument, filled_qty_cache);
            }
        }
        Err(e) => log::error!("Failed to parse spread order message as report: {e}"),
    }
}

/// Updates fee, fill, and order state caches from a raw OKX order message.
fn update_order_state_cache(
    msg: &OKXOrderMsg,
    instrument: &InstrumentAny,
    client_order_id: ClientOrderId,
    order_state_cache: &mut AHashMap<ClientOrderId, OrderStateSnapshot>,
) {
    let venue_order_id = VenueOrderId::new(msg.ord_id);
    let quantity = parse_quantity(&msg.sz, instrument.size_precision()).unwrap_or_default();
    let price = if is_market_price(&msg.px) {
        None
    } else {
        parse_price(&msg.px, instrument.price_precision()).ok()
    };

    order_state_cache.insert(
        client_order_id,
        OrderStateSnapshot {
            venue_order_id,
            quantity,
            price,
        },
    );
}

fn update_spread_order_state_cache(
    msg: &OKXSpreadOrder,
    instrument: &InstrumentAny,
    client_order_id: ClientOrderId,
    order_state_cache: &mut AHashMap<ClientOrderId, OrderStateSnapshot>,
) {
    let venue_order_id = VenueOrderId::new(msg.ord_id.as_str());
    let quantity = parse_quantity(&msg.sz, instrument.size_precision()).unwrap_or_default();
    let price = if is_market_price(&msg.px) {
        None
    } else {
        parse_price(&msg.px, instrument.price_precision()).ok()
    };

    order_state_cache.insert(
        client_order_id,
        OrderStateSnapshot {
            venue_order_id,
            quantity,
            price,
        },
    );
}

fn update_spread_fill_cache(
    msg: &OKXSpreadOrder,
    instrument: &InstrumentAny,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
) {
    if !msg.acc_fill_sz.is_empty()
        && msg.acc_fill_sz != "0"
        && let Ok(qty) = parse_quantity(&msg.acc_fill_sz, instrument.size_precision())
    {
        filled_qty_cache.insert(msg.ord_id, qty);
    }
}

fn is_spread_post_only_auto_cancel(msg: &OKXSpreadOrder) -> bool {
    msg.state == OKXOrderStatus::Canceled && msg.cancel_source == OKX_POST_ONLY_CANCEL_SOURCE
}

/// Dispatches execution reports with cross-stream deduplication.
pub fn dispatch_execution_reports(
    reports: Vec<ExecutionReport>,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
) {
    log::debug!("Processing {} execution report(s)", reports.len());

    for report in reports {
        match report {
            ExecutionReport::Order(order_report) => {
                if let Some(cid) = order_report.client_order_id {
                    match order_report.order_status {
                        // Guard form reformats awkwardly across multiple lines
                        #[allow(clippy::collapsible_match)]
                        OrderStatus::Accepted => {
                            if state.contains_terminal(&cid)
                                || state.contains_filled(&cid)
                                || state.contains_triggered(&cid)
                            {
                                log::debug!(
                                    "Skipping stale OrderStatusReport(Accepted) \
                                     for {cid} (order already terminal)"
                                );
                                continue;
                            }

                            if !state.contains_accepted(&cid) {
                                state.insert_accepted(cid, order_report.venue_order_id);
                            }
                        }
                        OrderStatus::Triggered => {
                            if state.contains_filled(&cid) {
                                log::debug!(
                                    "Skipping stale OrderStatusReport(Triggered) \
                                     for {cid} (already filled)"
                                );
                                continue;
                            }
                            state.insert_triggered(cid);
                        }
                        OrderStatus::Filled => {
                            state.insert_filled(cid);
                            state.insert_terminal(cid);
                            state.remove_triggered(&cid);
                        }
                        OrderStatus::Canceled | OrderStatus::Expired | OrderStatus::Rejected => {
                            state.insert_terminal(cid);
                            state.remove_triggered(&cid);
                            state.remove_filled(&cid);
                        }
                        _ => {}
                    }
                }
                emitter.send_order_status_report(order_report);
            }
            ExecutionReport::Fill(fill_report) => {
                if state.check_and_insert_trade(fill_report.trade_id) {
                    log::debug!(
                        "Skipping duplicate fill report: trade_id={}",
                        fill_report.trade_id
                    );
                    continue;
                }

                if let Some(cid) = fill_report.client_order_id {
                    state.insert_filled(cid);
                    state.remove_triggered(&cid);
                }
                emitter.send_fill_report(fill_report);
            }
        }
    }
}

fn emit_send_failed_submit(
    failure: &CommandFailure,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    clock: &AtomicTime,
    client_order_id: ClientOrderId,
) {
    let (CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason)) = failure else {
        return;
    };
    let Some(ident) = state.order_identity(client_order_id) else {
        return;
    };

    state.remove_order_tracking(client_order_id);
    emitter.emit_order_rejected_event(
        ident.strategy_id,
        ident.instrument_id,
        client_order_id,
        reason,
        clock.get_time_ns(),
        false,
    );
}

fn emit_send_failed_modify(
    failure: &CommandFailure,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    clock: &AtomicTime,
    client_order_id: ClientOrderId,
) {
    let (CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason)) = failure else {
        return;
    };
    let Some(ident) = state.order_identity(client_order_id) else {
        return;
    };

    emitter.emit_order_modify_rejected_event(
        ident.strategy_id,
        ident.instrument_id,
        client_order_id,
        None,
        reason,
        clock.get_time_ns(),
    );
}

fn format_order_response_reason(s_code: &str, s_msg: &str, sub_code: &str) -> String {
    match (s_msg.is_empty(), sub_code.is_empty(), s_code.is_empty()) {
        (false, true, _) => s_msg.to_string(),
        (false, false, _) => format!("{s_msg} (subCode={sub_code})"),
        (true, false, false) => format!("sCode={s_code} subCode={sub_code}"),
        (true, false, true) => format!("subCode={sub_code}"),
        (true, true, false) => format!("sCode={s_code}"),
        (true, true, true) => String::new(),
    }
}

#[derive(Debug, Clone)]
pub struct AlgoCancelContext {
    pub client_order_id: ClientOrderId,
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub venue_order_id: Option<VenueOrderId>,
}

// Contexts must correspond 1:1 with the requests that produced
// the responses (OKX preserves request order in batch responses).
pub fn emit_algo_cancel_rejections(
    responses: &[OKXCancelAlgoOrderResponse],
    contexts: &[AlgoCancelContext],
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    for (i, item) in responses.iter().enumerate() {
        let code = item.s_code.as_deref().unwrap_or(OKX_SUCCESS_CODE);
        if code == OKX_SUCCESS_CODE {
            continue;
        }

        let msg = item.s_msg.as_deref().unwrap_or("");

        if matches!(
            classify_okx_venue_code(code, msg),
            CommandFailure::Ambiguous(_) | CommandFailure::NotSent(_)
        ) {
            if let Some(ctx) = contexts.get(i) {
                log::warn!(
                    "Ambiguous algo cancel response for {}, awaiting reconciliation: \
                     algo_id={} sCode={code} sMsg={msg}",
                    ctx.client_order_id,
                    item.algo_id
                );
            } else {
                log::warn!(
                    "Ambiguous algo cancel response without context at index {i}: \
                     algo_id={} sCode={code} sMsg={msg}",
                    item.algo_id
                );
            }
            continue;
        }

        if let Some(ctx) = contexts.get(i) {
            let ts = clock.get_time_ns();
            emitter.emit_order_cancel_rejected_event(
                ctx.strategy_id,
                ctx.instrument_id,
                ctx.client_order_id,
                ctx.venue_order_id,
                msg,
                ts,
            );
        } else {
            log::warn!(
                "Algo cancel rejected but no context at index {i}: \
                 algo_id={} sCode={code} sMsg={msg}",
                item.algo_id
            );
        }
    }
}

pub fn emit_batch_cancel_failure(
    contexts: &[AlgoCancelContext],
    error: &str,
    _emitter: &ExecutionEventEmitter,
    _clock: &'static AtomicTime,
) {
    for ctx in contexts {
        log::warn!(
            "Ambiguous algo batch cancel failure for {}, awaiting reconciliation: {error}",
            ctx.client_order_id
        );
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nautilus_common::messages::{ExecutionEvent, ExecutionReport as CommonExecutionReport};
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_model::{
        enums::{AccountType, OrderSide, OrderType, TimeInForce, TriggerType},
        identifiers::Symbol,
        instruments::CryptoPerpetual,
        types::Price,
    };
    use rstest::rstest;

    use super::*;
    use crate::websocket::{error::OKXWsError, messages::OKXWsFrame};

    fn load_algo_order_messages(fixture: &str) -> Vec<OKXAlgoOrderMsg> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(fixture);
        let content = std::fs::read_to_string(path).unwrap();
        let frame: OKXWsFrame = serde_json::from_str(&content).unwrap();
        let OKXWsFrame::Data { data, .. } = frame else {
            panic!("Expected algo order data frame");
        };
        serde_json::from_value(data).unwrap()
    }

    fn load_regular_order_messages(fixture: &str) -> Vec<OKXOrderMsg> {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(fixture);
        let content = std::fs::read_to_string(path).unwrap();
        let frame: OKXWsFrame = serde_json::from_str(&content).unwrap();
        let OKXWsFrame::Data { data, .. } = frame else {
            panic!("Expected regular order data frame");
        };
        serde_json::from_value(data).unwrap()
    }

    fn test_algo_context(client_order_id: ClientOrderId) -> OrderContext {
        OrderContext {
            identity: OrderIdentity {
                client_order_id,
                strategy_id: StrategyId::from("STRATEGY-001"),
                instrument_id: InstrumentId::from("BTC-USDT-SWAP.OKX"),
                order_side: OrderSide::Sell,
                order_type: OrderType::StopLimit,
            },
            quantity: Quantity::from("0.01"),
            price: Some(Price::from("102900")),
            trigger_price: Some(Price::from("95000")),
            trigger_type: Some(TriggerType::LastPrice),
            time_in_force: TimeInForce::Gtc,
            is_post_only: false,
            is_reduce_only: true,
            is_quote_quantity: false,
        }
    }

    fn test_algo_instruments() -> AtomicMap<Ustr, InstrumentAny> {
        let instrument = CryptoPerpetual::builder()
            .instrument_id(InstrumentId::from("BTC-USDT-SWAP.OKX"))
            .raw_symbol(Symbol::from("BTC-USDT-SWAP"))
            .base_currency(Currency::BTC())
            .quote_currency(Currency::USDT())
            .settlement_currency(Currency::USDT())
            .is_inverse(false)
            .price_precision(2)
            .size_precision(8)
            .price_increment(Price::from("0.01"))
            .size_increment(Quantity::from("0.00000001"))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap();
        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from("BTC-USDT-SWAP"),
            InstrumentAny::CryptoPerpetual(instrument),
        );
        instruments
    }

    fn test_execution_emitter() -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let clock = get_atomic_clock_realtime();
        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            AccountId::from("OKX-001"),
            AccountType::Margin,
            None,
        );
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        (emitter, receiver)
    }

    fn dispatch_test_message(
        message: OKXWsMessage,
        emitter: &ExecutionEventEmitter,
        state: &WsDispatchState,
        instruments: &AtomicMap<Ustr, InstrumentAny>,
    ) {
        dispatch_ws_message(
            message,
            emitter,
            state,
            AccountId::from("OKX-001"),
            instruments,
            &mut AHashMap::new(),
            &mut AHashMap::new(),
            &mut AHashMap::new(),
            get_atomic_clock_realtime(),
        );
    }

    fn drain_execution_events(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> Vec<ExecutionEvent> {
        let mut events = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }
        events
    }

    #[rstest]
    fn tracked_algo_live_and_pause_emit_one_parent_acceptance() {
        let mut messages = load_algo_order_messages("ws_orders_algo.json");
        let live = messages.remove(0);
        let mut pause = live.clone();
        pause.state = OKXAlgoOrderStatus::Pause;
        pause.u_time += 1;
        let client_order_id = ClientOrderId::new(live.algo_cl_ord_id.as_str());
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![live, pause]),
            &emitter,
            &state,
            &instruments,
        );

        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), 1);
        match &events[0] {
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) => {
                assert_eq!(accepted.client_order_id, client_order_id);
                assert_eq!(
                    accepted.venue_order_id,
                    VenueOrderId::new("706620792746729472")
                );
            }
            other => panic!("Expected tracked algo acceptance, was {other:?}"),
        }
    }

    #[rstest]
    fn algo_update_routes_are_explicit_for_external_tracked_and_suppressed() {
        let mut message = load_algo_order_messages("ws_orders_algo.json").remove(0);
        let client_order_id = ClientOrderId::new(message.algo_cl_ord_id.as_str());
        let state = WsDispatchState::default();

        assert_eq!(
            route_algo_order_message(&message, &state),
            ExecutionUpdateRoute::External
        );

        let context = test_algo_context(client_order_id);
        state.track_order_context(context);
        assert_eq!(
            route_algo_order_message(&message, &state),
            ExecutionUpdateRoute::Tracked(client_order_id, context)
        );

        message.state = OKXAlgoOrderStatus::Unknown;
        assert_eq!(
            route_algo_order_message(&message, &state),
            ExecutionUpdateRoute::Suppressed
        );
    }

    #[rstest]
    fn rest_parent_binding_routes_linked_child_before_parent_stream_update() {
        let mut child = load_regular_order_messages("ws_orders_trigger.json").remove(0);
        let client_order_id = ClientOrderId::new("STOP003BTCUSDT20250120");
        let parent_venue_order_id = VenueOrderId::new("706620792746729474");
        let child_venue_order_id = VenueOrderId::new(child.ord_id);
        child.algo_cl_ord_id = None;
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        state.bind_algo_parent(client_order_id, parent_venue_order_id);
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::Orders(vec![child.clone()]),
            &emitter,
            &state,
            &instruments,
        );

        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), 4);
        assert!(matches!(
            &events[0],
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted))
                if accepted.client_order_id == client_order_id
                    && accepted.venue_order_id == parent_venue_order_id
        ));
        assert!(matches!(
            &events[1],
            ExecutionEvent::Order(OrderEventAny::Updated(updated))
                if updated.client_order_id == client_order_id
                    && updated.venue_order_id == Some(child_venue_order_id)
        ));
        assert!(matches!(
            &events[2],
            ExecutionEvent::Order(OrderEventAny::Triggered(triggered))
                if triggered.client_order_id == client_order_id
                    && triggered.venue_order_id == Some(child_venue_order_id)
        ));
        assert!(matches!(
            &events[3],
            ExecutionEvent::Order(OrderEventAny::Filled(filled))
                if filled.client_order_id == client_order_id
                    && filled.venue_order_id == child_venue_order_id
        ));

        dispatch_test_message(
            OKXWsMessage::Orders(vec![child]),
            &emitter,
            &state,
            &instruments,
        );
        assert!(drain_execution_events(&mut receiver).is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn pre_binding_child_is_held_until_parent_binding() {
        let child = load_regular_order_messages("ws_orders_trigger.json").remove(0);
        let parent_venue_order_id = VenueOrderId::new(
            child
                .linked_algo_ord
                .as_ref()
                .expect("Expected linked algo order")
                .algo_id
                .as_str(),
        );
        let client_order_id = ClientOrderId::new("STOP003BTCUSDT20250120");
        let child_venue_order_id = VenueOrderId::new(child.ord_id);
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::Orders(vec![child.clone()]),
            &emitter,
            &state,
            &instruments,
        );
        assert!(drain_execution_events(&mut receiver).is_empty());

        state.bind_algo_parent(client_order_id, parent_venue_order_id);
        tokio::time::timeout(
            Duration::from_millis(100),
            state.wait_for_linked_child_route(),
        )
        .await
        .expect("Expected parent binding to wake the private dispatch loop");
        dispatch_test_message(
            OKXWsMessage::Orders(Vec::new()),
            &emitter,
            &state,
            &instruments,
        );

        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), 4);
        assert!(matches!(
            &events[0],
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted))
                if accepted.client_order_id == client_order_id
                    && accepted.venue_order_id == parent_venue_order_id
        ));
        assert!(matches!(
            &events[1],
            ExecutionEvent::Order(OrderEventAny::Updated(updated))
                if updated.client_order_id == client_order_id
                    && updated.venue_order_id == Some(child_venue_order_id)
        ));
        assert!(matches!(
            &events[2],
            ExecutionEvent::Order(OrderEventAny::Triggered(triggered))
                if triggered.client_order_id == client_order_id
                    && triggered.venue_order_id == Some(child_venue_order_id)
        ));
        assert!(matches!(
            &events[3],
            ExecutionEvent::Order(OrderEventAny::Filled(filled))
                if filled.client_order_id == client_order_id
                    && filled.venue_order_id == child_venue_order_id
        ));

        dispatch_test_message(OKXWsMessage::Reconnected, &emitter, &state, &instruments);
        dispatch_test_message(
            OKXWsMessage::Orders(vec![child]),
            &emitter,
            &state,
            &instruments,
        );
        assert!(drain_execution_events(&mut receiver).is_empty());
    }

    #[rstest]
    fn held_child_becomes_external_when_candidate_binds_another_parent() {
        let child = load_regular_order_messages("ws_orders_trigger.json").remove(0);
        let client_order_id = ClientOrderId::new("STOP003BTCUSDT20250120");
        let child_venue_order_id = VenueOrderId::new(child.ord_id);
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::Orders(vec![child]),
            &emitter,
            &state,
            &instruments,
        );
        assert!(drain_execution_events(&mut receiver).is_empty());

        let authoritative_parent = VenueOrderId::new("706620792746729475");
        state.bind_algo_parent(client_order_id, authoritative_parent);
        dispatch_test_message(
            OKXWsMessage::Orders(Vec::new()),
            &emitter,
            &state,
            &instruments,
        );

        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ExecutionEvent::Report(CommonExecutionReport::Fill(report))
                if report.client_order_id
                    == Some(ClientOrderId::new("706620792746729474_0"))
                    && report.venue_order_id == child_venue_order_id
        ));
        assert_eq!(
            state.order_venue_binding(client_order_id),
            Some((authoritative_parent, false))
        );
    }

    #[rstest]
    fn active_algo_bindings_are_not_evicted_with_replay_state() {
        let state = WsDispatchState::default();
        let first_client_order_id = ClientOrderId::new("STOP-ACTIVE-00000");
        let first_venue_order_id = VenueOrderId::new("700000000000000000");

        for index in 0..=DEDUP_CAPACITY {
            let client_order_id = ClientOrderId::new(format!("STOP-ACTIVE-{index:05}").as_str());
            let venue_order_id = VenueOrderId::new(
                format!("{}", 700_000_000_000_000_000_u64 + index as u64).as_str(),
            );
            state.bind_algo_parent(client_order_id, venue_order_id);
        }

        assert_eq!(
            state.order_venue_binding(first_client_order_id),
            Some((first_venue_order_id, false))
        );
    }

    #[rstest]
    fn ambiguous_submit_failure_preserves_only_bound_context() {
        let client_order_id = ClientOrderId::new("STOP-AMBIGUOUS-001");
        let failure = CommandFailure::Ambiguous("request timed out".to_string());
        let unbound_state = WsDispatchState::default();
        unbound_state.track_order_context(test_algo_context(client_order_id));

        unbound_state.resolve_algo_submit_failure(client_order_id, &failure);
        assert_eq!(unbound_state.order_identity(client_order_id), None);

        let bound_state = WsDispatchState::default();
        let context = test_algo_context(client_order_id);
        let parent_venue_order_id = VenueOrderId::new("706620792746729476");
        bound_state.track_order_context(context);
        bound_state.bind_algo_parent(client_order_id, parent_venue_order_id);

        bound_state.resolve_algo_submit_failure(client_order_id, &failure);
        assert_eq!(
            bound_state.order_identity(client_order_id),
            Some(context.identity)
        );
        assert_eq!(
            bound_state.order_venue_binding(client_order_id),
            Some((parent_venue_order_id, false))
        );
    }

    #[rstest]
    #[case::effective(0, false)]
    #[case::effective_order_id_list(0, true)]
    #[case::partially_effective(1, false)]
    #[case::order_placed(2, false)]
    fn tracked_algo_trigger_states_bind_child_before_trigger(
        #[case] index: usize,
        #[case] use_order_id_list: bool,
    ) {
        let mut messages = if index == 2 {
            load_algo_order_messages("ws_orders_algo.json")
        } else {
            load_algo_order_messages("ws_orders_algo_states.json")
        };
        let mut message = if index == 2 {
            messages.remove(2)
        } else {
            messages.remove(index)
        };
        let client_order_id = ClientOrderId::new(message.algo_cl_ord_id.as_str());
        let child_venue_order_id = VenueOrderId::new(message.ord_id.as_str());
        if use_order_id_list {
            message.ord_id_list = vec![message.ord_id.clone()];
            message.ord_id.clear();
        }

        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![message]),
            &emitter,
            &state,
            &instruments,
        );

        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), 3);
        assert!(matches!(
            &events[0],
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));
        assert!(matches!(
            &events[1],
            ExecutionEvent::Order(OrderEventAny::Updated(updated))
                if updated.venue_order_id == Some(child_venue_order_id)
        ));
        assert!(matches!(
            &events[2],
            ExecutionEvent::Order(OrderEventAny::Triggered(triggered))
                if triggered.venue_order_id == Some(child_venue_order_id)
        ));
        assert_eq!(
            state.order_venue_binding(client_order_id),
            Some((child_venue_order_id, true))
        );
    }

    #[rstest]
    fn tracked_algo_transition_uses_venue_actual_quantity() {
        let mut message = load_algo_order_messages("ws_orders_algo_states.json").remove(0);
        message.sz.clear();
        message.close_fraction = "1".to_string();
        message.actual_sz = "0.025".to_string();
        let client_order_id = ClientOrderId::new(message.algo_cl_ord_id.as_str());
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![message]),
            &emitter,
            &state,
            &instruments,
        );

        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), 3);
        assert!(matches!(
            &events[1],
            ExecutionEvent::Order(OrderEventAny::Updated(updated))
                if updated.quantity == Quantity::from("0.025")
                    && updated.price.is_none()
        ));
        assert_eq!(
            state
                .order_contexts
                .get(&client_order_id)
                .map(|context| context.quantity),
            Some(Quantity::from("0.025"))
        );
    }

    #[rstest]
    fn regular_child_refreshes_parent_transition_terms() {
        let mut parent = load_algo_order_messages("ws_orders_algo_states.json").remove(0);
        parent.ord_px = "94950".to_string();
        let parent_replay = parent.clone();
        let client_order_id = ClientOrderId::new(parent.algo_cl_ord_id.as_str());
        let parent_venue_order_id = parent.algo_id.clone();
        let child_venue_order_id = parent.ord_id.clone();
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![parent]),
            &emitter,
            &state,
            &instruments,
        );
        assert_eq!(drain_execution_events(&mut receiver).len(), 3);

        let mut child = load_regular_order_messages("ws_orders_trigger.json").remove(0);
        child.algo_id = Some(parent_venue_order_id.clone());
        child.algo_cl_ord_id = None;
        child.linked_algo_ord = Some(crate::websocket::messages::OKXLinkedAlgoOrd {
            algo_id: parent_venue_order_id,
        });
        child.ord_id = Ustr::from(child_venue_order_id.as_str());
        child.ord_type = OKXOrderType::Limit;
        child.state = OKXOrderStatus::Live;
        child.sz = "0.025".to_string();
        child.px = "94950".to_string();
        child.acc_fill_sz = Some("0".to_string());
        child.fill_sz.clear();
        child.fill_px.clear();
        child.trade_id.clear();

        dispatch_test_message(
            OKXWsMessage::Orders(vec![child]),
            &emitter,
            &state,
            &instruments,
        );

        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ExecutionEvent::Order(OrderEventAny::Updated(updated))
                if updated.quantity == Quantity::from("0.025")
                    && updated.price == Some(Price::from("94950"))
        ));
        assert_eq!(
            state
                .order_contexts
                .get(&client_order_id)
                .map(|context| (context.quantity, context.price)),
            Some((Quantity::from("0.025"), Some(Price::from("94950"))))
        );

        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![parent_replay]),
            &emitter,
            &state,
            &instruments,
        );
        assert!(drain_execution_events(&mut receiver).is_empty());
        assert_eq!(
            state
                .order_contexts
                .get(&client_order_id)
                .map(|context| context.quantity),
            Some(Quantity::from("0.025"))
        );
    }

    #[rstest]
    fn tracked_algo_holds_trigger_until_linked_child_arrives() {
        let mut parent = load_algo_order_messages("ws_orders_algo_states.json").remove(0);
        parent.algo_id = "706620792746729474".to_string();
        parent.algo_cl_ord_id = "STOP003BTCUSDT20250120".to_string();
        parent.ord_id.clear();
        parent.ord_id_list.clear();
        let client_order_id = ClientOrderId::new(parent.algo_cl_ord_id.as_str());
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![parent]),
            &emitter,
            &state,
            &instruments,
        );

        let parent_events = drain_execution_events(&mut receiver);
        assert_eq!(parent_events.len(), 1);
        assert!(matches!(
            &parent_events[0],
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));
        assert!(!state.contains_triggered(&client_order_id));

        let child = load_regular_order_messages("ws_orders_trigger.json").remove(0);
        assert!(child.algo_cl_ord_id.is_none());
        assert_eq!(child.cl_ord_id, "706620792746729474_0");
        assert_eq!(
            child
                .linked_algo_ord
                .as_ref()
                .map(|linked| linked.algo_id.as_str()),
            Some("706620792746729474")
        );
        dispatch_test_message(
            OKXWsMessage::Orders(vec![child]),
            &emitter,
            &state,
            &instruments,
        );

        let child_events = drain_execution_events(&mut receiver);
        assert_eq!(child_events.len(), 3);
        assert!(matches!(
            &child_events[0],
            ExecutionEvent::Order(OrderEventAny::Updated(_))
        ));
        assert!(matches!(
            &child_events[1],
            ExecutionEvent::Order(OrderEventAny::Triggered(_))
        ));

        match &child_events[2] {
            ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
                assert_eq!(filled.client_order_id, client_order_id);
                assert_eq!(
                    filled.venue_order_id,
                    VenueOrderId::new("706620792746729999")
                );
                assert_eq!(filled.trade_id, TradeId::new("1518905530"));
            }
            other => panic!("Expected tracked child fill, was {other:?}"),
        }

        assert!(state.contains_terminal(&client_order_id));
    }

    #[rstest]
    fn tracked_algo_child_cancellation_routes_late_fill_report() {
        let parent = load_algo_order_messages("ws_orders_algo_states.json").remove(0);
        let client_order_id = ClientOrderId::new(parent.algo_cl_ord_id.as_str());
        let parent_venue_order_id = parent.algo_id.clone();
        let child_venue_order_id = parent.ord_id.clone();
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![parent]),
            &emitter,
            &state,
            &instruments,
        );
        assert_eq!(drain_execution_events(&mut receiver).len(), 3);

        let mut child = load_regular_order_messages("ws_orders_trigger.json").remove(0);
        child.algo_id = Some(parent_venue_order_id.clone());
        child.linked_algo_ord = Some(crate::websocket::messages::OKXLinkedAlgoOrd {
            algo_id: parent_venue_order_id,
        });
        child.ord_id = Ustr::from(child_venue_order_id.as_str());
        let mut canceled = child.clone();
        canceled.state = OKXOrderStatus::Canceled;
        canceled.acc_fill_sz = Some("0".to_string());
        canceled.fill_sz.clear();
        canceled.fill_px.clear();
        canceled.trade_id.clear();
        dispatch_test_message(
            OKXWsMessage::Orders(vec![canceled]),
            &emitter,
            &state,
            &instruments,
        );

        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ExecutionEvent::Order(OrderEventAny::Canceled(canceled))
                if canceled.client_order_id == client_order_id
                    && canceled.venue_order_id
                        == Some(VenueOrderId::new(child_venue_order_id.as_str()))
        ));
        assert!(state.contains_terminal(&client_order_id));

        dispatch_test_message(
            OKXWsMessage::Orders(vec![child.clone()]),
            &emitter,
            &state,
            &instruments,
        );
        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ExecutionEvent::Report(CommonExecutionReport::Fill(report))
                if report.client_order_id == Some(client_order_id)
                    && report.venue_order_id
                        == VenueOrderId::new(child_venue_order_id.as_str())
                    && report.trade_id == TradeId::new("1518905530")
        ));

        dispatch_test_message(
            OKXWsMessage::Orders(vec![child]),
            &emitter,
            &state,
            &instruments,
        );
        assert!(drain_execution_events(&mut receiver).is_empty());
    }

    #[rstest]
    fn reconnect_replay_and_stale_parent_acceptance_are_suppressed() {
        let message = load_algo_order_messages("ws_orders_algo_states.json").remove(0);
        let client_order_id = ClientOrderId::new(message.algo_cl_ord_id.as_str());
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![message.clone()]),
            &emitter,
            &state,
            &instruments,
        );
        assert_eq!(drain_execution_events(&mut receiver).len(), 3);

        let mut stale_live = message.clone();
        stale_live.state = OKXAlgoOrderStatus::Live;
        stale_live.ord_id.clear();
        dispatch_test_message(OKXWsMessage::Reconnected, &emitter, &state, &instruments);
        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![message, stale_live]),
            &emitter,
            &state,
            &instruments,
        );

        assert!(drain_execution_events(&mut receiver).is_empty());
        assert_eq!(
            state
                .order_venue_binding(client_order_id)
                .map(|(venue_order_id, _)| venue_order_id),
            Some(VenueOrderId::new("706620792746730010"))
        );
    }

    #[rstest]
    #[case::filled("ws_orders_algo.json", 4, 3)]
    #[case::partially_failed("ws_orders_algo_states.json", 4, 1)]
    fn tracked_algo_aggregate_state_never_enters_reconciliation(
        #[case] fixture: &str,
        #[case] index: usize,
        #[case] expected_order_events: usize,
    ) {
        let message = load_algo_order_messages(fixture).remove(index);
        let client_order_id = ClientOrderId::new(message.algo_cl_ord_id.as_str());
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![message]),
            &emitter,
            &state,
            &instruments,
        );

        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), expected_order_events);
        assert!(
            events
                .iter()
                .all(|event| matches!(event, ExecutionEvent::Order(_)))
        );
        assert!(state.order_contexts.contains_key(&client_order_id));
        assert!(!state.contains_terminal(&client_order_id));
    }

    #[rstest]
    fn tracked_algo_cancel_is_terminal_and_stale_live_is_suppressed() {
        let mut message = load_algo_order_messages("ws_orders_algo.json").remove(3);
        let client_order_id = ClientOrderId::new(message.algo_cl_ord_id.as_str());
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![message.clone()]),
            &emitter,
            &state,
            &instruments,
        );
        message.state = OKXAlgoOrderStatus::Live;
        message.algo_cl_ord_id.clear();
        message.cl_ord_id.clear();
        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![message]),
            &emitter,
            &state,
            &instruments,
        );

        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));
        assert!(matches!(
            &events[1],
            ExecutionEvent::Order(OrderEventAny::Canceled(canceled))
                if canceled.client_order_id == client_order_id
        ));
        assert!(state.contains_terminal(&client_order_id));
        assert!(!state.order_contexts.contains_key(&client_order_id));
        assert_eq!(state.order_venue_binding(client_order_id), None);
    }

    #[rstest]
    fn tracked_algo_failure_uses_typed_rejection() {
        let mut message = load_algo_order_messages("ws_orders_algo_states.json").remove(3);
        message.fail_code = "51008".to_string();
        let client_order_id = ClientOrderId::new(message.algo_cl_ord_id.as_str());
        let state = WsDispatchState::default();
        state.track_order_context(test_algo_context(client_order_id));
        let instruments = test_algo_instruments();
        let (emitter, mut receiver) = test_execution_emitter();

        dispatch_test_message(
            OKXWsMessage::AlgoOrders(vec![message]),
            &emitter,
            &state,
            &instruments,
        );

        let events = drain_execution_events(&mut receiver);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ExecutionEvent::Order(OrderEventAny::Rejected(rejected))
                if rejected.client_order_id == client_order_id
                    && rejected.reason == Ustr::from("51008")
        ));
        assert!(state.contains_terminal(&client_order_id));
    }

    #[rstest]
    #[case("51000", "Rejected", "", "Rejected")]
    #[case("51000", "Rejected", "51004", "Rejected (subCode=51004)")]
    #[case("51000", "", "51004", "sCode=51000 subCode=51004")]
    #[case("51000", "", "", "sCode=51000")]
    #[case("", "", "51004", "subCode=51004")]
    #[case("", "", "", "")]
    fn test_format_order_response_reason(
        #[case] s_code: &str,
        #[case] s_msg: &str,
        #[case] sub_code: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(
            format_order_response_reason(s_code, s_msg, sub_code),
            expected
        );
    }

    #[rstest]
    #[case::ambiguous(OKXWsError::SendFailed("connection reset".to_string()), true)]
    #[case::not_sent(OKXWsError::NoActiveClient, false)]
    fn send_failure_preserves_only_ambiguous_pending_orders(
        #[case] error: OKXWsError,
        #[case] expected_pending: bool,
    ) {
        let client_order_ids = [
            ClientOrderId::from("O-batch-pending-1"),
            ClientOrderId::from("O-batch-pending-2"),
        ];
        let state = WsDispatchState::default();
        let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
        let strategy_id = StrategyId::from("STRATEGY-001");

        for client_order_id in client_order_ids {
            state.pending_orders.insert(
                client_order_id.to_string(),
                PendingOrderInfo {
                    trader_id: TraderId::from("TRADER-001"),
                    strategy_id,
                    instrument_id,
                },
            );
            state.order_identities.insert(
                client_order_id,
                OrderIdentity {
                    client_order_id,
                    instrument_id,
                    strategy_id,
                    order_side: OrderSide::Buy,
                    order_type: OrderType::Limit,
                },
            );
        }

        let clock = get_atomic_clock_realtime();
        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            AccountId::from("OKX-001"),
            AccountType::Margin,
            None,
        );
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let instruments = AtomicMap::new();
        let mut fee_cache = AHashMap::new();
        let mut filled_qty_cache = AHashMap::new();
        let mut order_state_cache = AHashMap::new();

        dispatch_ws_message(
            OKXWsMessage::SendFailed {
                request_id: "req-batch-send-failure".to_string(),
                client_order_ids: client_order_ids.to_vec(),
                op: Some(OKXWsOperation::BatchOrders),
                error,
            },
            &emitter,
            &state,
            AccountId::from("OKX-001"),
            &instruments,
            &mut fee_cache,
            &mut filled_qty_cache,
            &mut order_state_cache,
            clock,
        );

        for client_order_id in client_order_ids {
            assert_eq!(
                state.pending_orders.contains_key(client_order_id.as_str()),
                expected_pending,
                "pending state mismatch for {client_order_id}"
            );
        }
    }
}
