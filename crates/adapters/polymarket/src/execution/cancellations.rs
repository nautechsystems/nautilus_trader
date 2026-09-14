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

use std::{sync::Arc, time::Duration};

use nautilus_common::messages::execution::{BatchCancelOrders, CancelAllOrders, CancelOrder};
use nautilus_core::time::AtomicTime;
use nautilus_live::{ExecutionEventEmitter, execution::failure::CommandFailure};
use nautilus_model::{
    identifiers::{ClientOrderId, InstrumentId, VenueOrderId},
    instruments::Instrument,
    orders::{Order, OrderAny},
};
use parking_lot::Mutex;

use super::{PolymarketExecutionClient, pending::PendingCancelTracker};
use crate::{
    execution::types::{CancelOutcome, classify_http_command_failure},
    http::{error::sanitize_error_text, query::CancelResponse},
    websocket::dispatch::WsDispatchState,
};

struct CancelCommandGuard {
    state: Arc<Mutex<WsDispatchState>>,
    client_order_ids: Vec<ClientOrderId>,
    market: Option<InstrumentId>,
}

impl CancelCommandGuard {
    fn orders(
        state: Arc<Mutex<WsDispatchState>>,
        orders: &[(ClientOrderId, InstrumentId)],
    ) -> Option<Self> {
        if !state.lock().begin_cancels(orders) {
            return None;
        }

        Some(Self {
            state,
            client_order_ids: orders
                .iter()
                .map(|(client_order_id, _)| *client_order_id)
                .collect(),
            market: None,
        })
    }

    fn available_orders(
        state: Arc<Mutex<WsDispatchState>>,
        orders: &[(ClientOrderId, InstrumentId)],
    ) -> Option<Self> {
        let client_order_ids = state.lock().begin_available_cancels(orders)?;
        Some(Self {
            state,
            client_order_ids,
            market: None,
        })
    }

    fn market(state: Arc<Mutex<WsDispatchState>>, instrument_id: InstrumentId) -> Option<Self> {
        if !state.lock().begin_market_cancel(instrument_id) {
            return None;
        }

        Some(Self {
            state,
            client_order_ids: Vec::new(),
            market: Some(instrument_id),
        })
    }
}

impl Drop for CancelCommandGuard {
    fn drop(&mut self) {
        let mut state = self.state.lock();
        state.finish_cancels(&self.client_order_ids);
        if let Some(instrument_id) = self.market {
            state.finish_market_cancel(instrument_id);
        }
    }
}

impl PolymarketExecutionClient {
    pub(super) fn cancel_order_command(&self, cmd: &CancelOrder) {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .map(|o| o.clone());
        let order_ref = match &order {
            Some(o) => o,
            None => {
                log::warn!(
                    "Order not found in cache for cancel: {}",
                    cmd.client_order_id
                );
                return;
            }
        };

        if !order_ref.is_open() {
            log::warn!(
                "Cannot cancel order that is not open: {}",
                cmd.client_order_id
            );
            return;
        }

        let state = self.ws_dispatch_state.lock();

        if state.is_modifying(&cmd.client_order_id) {
            log::debug!(
                "Cancel for {} deferred until its modification finishes",
                cmd.client_order_id
            );

            let inserted = self.pending_cancels.insert(cmd.client_order_id);
            drop(state);

            if !inserted {
                return;
            }

            let clock = self.clock;
            let client_order_id = cmd.client_order_id;
            let emitter = self.emitter.clone();
            let order = order_ref.clone();
            let order_contexts = self.order_contexts.clone();
            let pending_cancels = self.pending_cancels.clone();
            let submitter = self.submitter.clone();
            let ws_dispatch_state = self.ws_dispatch_state.clone();
            let poll_interval =
                Duration::from_millis(self.config.retry_delay_initial_ms.clamp(1, 100));

            let spawned = self.spawn_task("cancel order after modify", async move {
                loop {
                    let (is_modifying, venue_order_id) = {
                        let state = ws_dispatch_state.lock();

                        if !pending_cancels.contains(&client_order_id) {
                            return Ok(());
                        }

                        let is_modifying = state.is_modifying(&client_order_id);
                        let venue_order_id = if is_modifying {
                            None
                        } else {
                            order_contexts.venue_order_id(&client_order_id)
                        };
                        (is_modifying, venue_order_id)
                    };

                    if !is_modifying {
                        let Some(venue_order_id) = venue_order_id else {
                            pending_cancels.remove(&client_order_id);
                            emitter.emit_order_cancel_rejected(
                                &order,
                                None,
                                "Modified order has no venue order ID to cancel",
                                clock.get_time_ns(),
                            );
                            return Ok(());
                        };

                        execute_deferred_cancel(
                            &submitter,
                            &order,
                            venue_order_id.as_str(),
                            venue_order_id,
                            &emitter,
                            &pending_cancels,
                            clock,
                        )
                        .await;
                        return Ok(());
                    }

                    tokio::time::sleep(poll_interval).await;
                }
            });

            if !spawned {
                self.pending_cancels.remove(&cmd.client_order_id);
                self.emitter.emit_order_cancel_rejected(
                    order_ref,
                    self.cancel_venue_order_id(order_ref),
                    "Polymarket execution client is shutting down",
                    self.clock.get_time_ns(),
                );
            }

            return;
        }

        drop(state);

        let Some(cancel_guard) = CancelCommandGuard::orders(
            self.ws_dispatch_state.clone(),
            &[(cmd.client_order_id, order_ref.instrument_id())],
        ) else {
            self.emitter.emit_order_cancel_rejected(
                order_ref,
                self.cancel_venue_order_id(order_ref),
                "Polymarket modification or cancellation is in flight",
                self.clock.get_time_ns(),
            );
            return;
        };

        let Some(venue_order_id) = self.cancel_venue_order_id(order_ref) else {
            log::debug!(
                "Cancel for {} deferred, venue_order_id not yet available",
                cmd.client_order_id
            );
            self.pending_cancels.insert(cmd.client_order_id);
            return;
        };

        let clock = self.clock;
        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let order_id_str = venue_order_id.to_string();
        let order_clone = order.unwrap();

        self.spawn_task("cancel_order", async move {
            let _cancel_guard = cancel_guard;

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
                    match classify_http_command_failure(&e) {
                        CommandFailure::VenueRejected(reason)
                        | CommandFailure::NotSent(reason) => {
                            let ts_now = clock.get_time_ns();
                            emitter.emit_order_cancel_rejected(
                                &order_clone,
                                Some(venue_order_id),
                                &reason,
                                ts_now,
                            );
                        }
                        CommandFailure::Ambiguous(reason) => {
                            log::warn!(
                                "Cancel outcome unknown for {} ({}), awaiting reconciliation: {reason}",
                                order_clone.client_order_id(),
                                venue_order_id,
                            );
                        }
                    }
                    return Err(anyhow::Error::new(e).context("cancel order failed"));
                }
            }
            Ok(())
        });
    }

    pub(super) fn cancel_all_orders_command(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        let cache = self.core.cache();
        let side = cmd.order_side;
        let asset_id = if side.is_none() {
            let instrument = cache.instrument(&cmd.instrument_id).ok_or_else(|| {
                anyhow::anyhow!(
                    "Cannot cancel all orders: instrument not found in cache for {}",
                    cmd.instrument_id
                )
            })?;
            Some(instrument.raw_symbol().to_string())
        } else {
            None
        };
        let open_orders = cache.orders_open(
            Some(&self.core.venue),
            Some(&cmd.instrument_id),
            None,
            Some(&self.core.account_id),
            side,
        );

        if side.is_some() && open_orders.is_empty() {
            log::debug!(
                "No cached {side:?} orders to cancel for instrument_id={}",
                cmd.instrument_id
            );
            return Ok(());
        }

        let cancel_guard = if side.is_none() {
            CancelCommandGuard::market(self.ws_dispatch_state.clone(), cmd.instrument_id)
        } else {
            let pending = open_orders
                .iter()
                .map(|order| (order.client_order_id(), order.instrument_id()))
                .collect::<Vec<_>>();
            CancelCommandGuard::available_orders(self.ws_dispatch_state.clone(), &pending)
        };

        let Some(cancel_guard) = cancel_guard else {
            anyhow::bail!(
                "Cannot cancel Polymarket orders while a modification or cancellation is in flight"
            );
        };

        let mut orders = Vec::new();

        for order in open_orders {
            if side.is_some()
                && !cancel_guard
                    .client_order_ids
                    .contains(&order.client_order_id())
            {
                continue;
            }

            if let Some(venue_order_id) = self.cancel_venue_order_id(&order) {
                orders.push((venue_order_id, order.clone()));
            } else {
                log::debug!(
                    "Cancel all for {} deferred, venue_order_id not yet available",
                    order.client_order_id()
                );
                self.pending_cancels.insert(order.client_order_id());
            }
        }

        if side.is_some() && orders.is_empty() {
            return Ok(());
        }

        let clock = self.clock;
        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let instrument_id = cmd.instrument_id;

        let spawned = self.spawn_task("cancel_all_orders", async move {
            let _cancel_guard = cancel_guard;
            let response = match side {
                None => {
                    let asset_id = asset_id
                        .as_deref()
                        .expect("asset_id must be resolved for unsided cancellation");
                    submitter.cancel_market_orders(asset_id).await
                }
                Some(_) => {
                    let venue_order_ids = orders
                        .iter()
                        .map(|(venue_order_id, _)| venue_order_id.to_string())
                        .collect::<Vec<_>>();

                    let order_id_refs =
                        venue_order_ids.iter().map(String::as_str).collect::<Vec<_>>();
                    submitter.cancel_orders(&order_id_refs).await
                }
            };

            match response {
                Ok(response) => {
                    for (venue_order_id, order) in &orders {
                        let venue_order_id_str = venue_order_id.to_string();
                        if side.is_some()
                            || response.not_canceled.contains_key(&venue_order_id_str)
                            || response
                                .canceled
                                .iter()
                                .any(|order_id| order_id == &venue_order_id_str)
                        {
                            process_cancel_result(
                                &response,
                                &venue_order_id_str,
                                order,
                                *venue_order_id,
                                &emitter,
                                clock,
                            );
                        } else {
                            log::debug!(
                                "Cancel-all response omitted local order {} ({})",
                                order.client_order_id(),
                                venue_order_id
                            );
                        }
                    }

                    log::debug!(
                        "Cancel-all completed for instrument_id={instrument_id}: canceled={}, not_canceled={}",
                        response.canceled.len(),
                        response.not_canceled.len()
                    );
                    Ok(())
                }
                Err(e) => {
                    apply_cancel_http_failure(&e, &orders, &emitter, clock);
                    Err(anyhow::Error::new(e).context("failed to cancel all orders"))
                }
            }
        });
        anyhow::ensure!(spawned, "Polymarket execution client is shutting down");

        Ok(())
    }

    pub(super) fn batch_cancel_orders_command(&self, cmd: &BatchCancelOrders) {
        if cmd.cancels.is_empty() {
            return;
        }

        let mut orders = Vec::new();

        for c in &cmd.cancels {
            if let Some(order) = self.core.cache().order(&c.client_order_id) {
                orders.push(order.clone());
            }
        }

        if orders.is_empty() {
            log::debug!("All batch cancels are awaiting venue order IDs");
            return;
        }

        let pending = orders
            .iter()
            .map(|order| (order.client_order_id(), order.instrument_id()))
            .collect::<Vec<_>>();
        let Some(cancel_guard) =
            CancelCommandGuard::orders(self.ws_dispatch_state.clone(), &pending)
        else {
            let ts_now = self.clock.get_time_ns();

            for order in &orders {
                self.emitter.emit_order_cancel_rejected(
                    order,
                    self.cancel_venue_order_id(order),
                    "Polymarket modification or cancellation is in flight",
                    ts_now,
                );
            }
            return;
        };

        let mut venue_to_order: Vec<(String, OrderAny)> = Vec::new();

        for order in orders {
            if let Some(venue_order_id) = self.cancel_venue_order_id(&order) {
                venue_to_order.push((venue_order_id.to_string(), order));
            } else {
                log::debug!(
                    "Batch cancel for {} deferred, venue_order_id not yet available",
                    order.client_order_id()
                );
                self.pending_cancels.insert(order.client_order_id());
            }
        }

        if venue_to_order.is_empty() {
            return;
        }

        let clock = self.clock;
        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let order_ids: Vec<String> = venue_to_order.iter().map(|(id, _)| id.clone()).collect();

        self.spawn_task("batch_cancel_orders", async move {
            let _cancel_guard = cancel_guard;
            let order_id_refs: Vec<&str> = order_ids.iter().map(String::as_str).collect();
            match submitter.cancel_orders(&order_id_refs).await {
                Ok(response) => {
                    for (venue_id_str, order) in &venue_to_order {
                        let vid = VenueOrderId::from(venue_id_str.as_str());
                        process_cancel_result(&response, venue_id_str, order, vid, &emitter, clock);
                    }

                    log::debug!("Batch canceled {} orders", response.canceled.len());
                    Ok(())
                }
                Err(e) => {
                    let orders: Vec<(VenueOrderId, OrderAny)> = venue_to_order
                        .iter()
                        .map(|(venue_id_str, order)| {
                            (VenueOrderId::from(venue_id_str.as_str()), order.clone())
                        })
                        .collect();
                    apply_cancel_http_failure(&e, &orders, &emitter, clock);
                    Err(anyhow::Error::new(e).context("failed to batch cancel orders"))
                }
            }
        });
    }

    fn cancel_venue_order_id(&self, order: &OrderAny) -> Option<VenueOrderId> {
        self.order_contexts
            .venue_order_id(&order.client_order_id())
            .or_else(|| order.venue_order_id())
            .or_else(|| {
                self.core
                    .cache()
                    .venue_order_id(&order.client_order_id())
                    .copied()
            })
    }
}

fn apply_cancel_http_failure(
    error: &crate::http::error::Error,
    orders: &[(VenueOrderId, OrderAny)],
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    match classify_http_command_failure(error) {
        CommandFailure::VenueRejected(reason) | CommandFailure::NotSent(reason) => {
            let ts_now = clock.get_time_ns();
            for (venue_order_id, order) in orders {
                emitter.emit_order_cancel_rejected(order, Some(*venue_order_id), &reason, ts_now);
            }
        }
        CommandFailure::Ambiguous(reason) => {
            for (venue_order_id, order) in orders {
                log::warn!(
                    "Cancel outcome unknown for {} ({}), awaiting reconciliation: {reason}",
                    order.client_order_id(),
                    venue_order_id,
                );
            }
        }
    }
}

pub(super) fn process_cancel_result(
    response: &CancelResponse,
    venue_order_id_str: &str,
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) -> CancelResponseStatus {
    if let Some(reason_opt) = response.not_canceled.get(venue_order_id_str) {
        let reason = sanitize_error_text(reason_opt.as_deref().unwrap_or("unknown reason"));

        match CancelOutcome::classify(&reason) {
            CancelOutcome::AlreadyDone => {
                log::debug!(
                    "Cancel rejected for {}: {reason} - awaiting WS for terminal state",
                    order.client_order_id()
                );
            }
            CancelOutcome::Rejected(msg) => {
                let ts_now = clock.get_time_ns();
                emitter.emit_order_cancel_rejected(order, Some(venue_order_id), &msg, ts_now);
            }
        }

        return CancelResponseStatus::PerOrderResult;
    }

    if response
        .canceled
        .iter()
        .any(|order_id| order_id == venue_order_id_str)
    {
        return CancelResponseStatus::PerOrderResult;
    }

    log::warn!(
        "Cancel response for {} did not include per-order result for {}",
        order.client_order_id(),
        venue_order_id
    );
    CancelResponseStatus::MissingPerOrderResult
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CancelResponseStatus {
    PerOrderResult,
    MissingPerOrderResult,
}

pub(super) async fn execute_deferred_cancel(
    submitter: &super::submitter::OrderSubmitter,
    order: &OrderAny,
    order_id_str: &str,
    venue_order_id: VenueOrderId,
    emitter: &ExecutionEventEmitter,
    pending_cancels: &PendingCancelTracker,
    clock: &'static AtomicTime,
) {
    match submitter.cancel_order(order_id_str).await {
        Ok(response) => {
            let status = process_cancel_result(
                &response,
                order_id_str,
                order,
                venue_order_id,
                emitter,
                clock,
            );

            if status == CancelResponseStatus::PerOrderResult {
                pending_cancels.remove(&order.client_order_id());
            }
        }
        Err(e) => match classify_http_command_failure(&e) {
            CommandFailure::VenueRejected(reason) | CommandFailure::NotSent(reason) => {
                let ts_now = clock.get_time_ns();
                emitter.emit_order_cancel_rejected(order, Some(venue_order_id), &reason, ts_now);
                pending_cancels.remove(&order.client_order_id());
            }
            CommandFailure::Ambiguous(reason) => {
                log::warn!(
                    "Deferred cancel outcome unknown for {} ({}), awaiting reconciliation: {reason}",
                    order.client_order_id(),
                    venue_order_id,
                );
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[tokio::test]
    async fn test_cancel_command_guard_releases_reservation_after_task_completion() {
        let state = Arc::new(Mutex::new(WsDispatchState::default()));
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let client_order_id = ClientOrderId::from("O-CANCEL-GUARD");
        let venue_order_id = VenueOrderId::from("0xcancel-guard");
        let guard = CancelCommandGuard::orders(state.clone(), &[(client_order_id, instrument_id)])
            .expect("cancel reservation must be acquired");

        assert!(
            !state
                .lock()
                .begin_modify(client_order_id, venue_order_id, instrument_id,)
        );

        tokio::spawn(async move {
            let _cancel_guard = guard;
        })
        .await
        .unwrap();

        assert!(
            state
                .lock()
                .begin_modify(client_order_id, venue_order_id, instrument_id,)
        );
    }
}
