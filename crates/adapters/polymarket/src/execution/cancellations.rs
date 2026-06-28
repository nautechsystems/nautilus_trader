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

use anyhow::Context;
use nautilus_common::messages::execution::{BatchCancelOrders, CancelAllOrders, CancelOrder};
use nautilus_core::time::AtomicTime;
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    identifiers::VenueOrderId,
    orders::{Order, OrderAny},
};

use super::{PolymarketExecutionClient, pending::PendingCancelTracker};
use crate::{execution::types::CancelOutcome, http::query::CancelResponse};

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

        let venue_order_id = match order_ref.venue_order_id() {
            Some(id) => id,
            None => match self
                .core
                .cache()
                .venue_order_id(&cmd.client_order_id)
                .copied()
            {
                Some(id) => id,
                None => {
                    log::debug!(
                        "Cancel for {} deferred, venue_order_id not yet available",
                        cmd.client_order_id
                    );
                    self.pending_cancels.insert(cmd.client_order_id);
                    return;
                }
            },
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
                    log::warn!(
                        "Cancel outcome unknown for {} ({}), awaiting reconciliation: {e}",
                        order_clone.client_order_id(),
                        venue_order_id,
                    );
                    return Err(anyhow::Error::new(e).context("cancel order failed"));
                }
            }
            Ok(())
        });
    }

    pub(super) fn cancel_all_orders_command(&self, cmd: &CancelAllOrders) {
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
            return;
        }

        let venue_order_ids: Vec<String> = open_orders
            .iter()
            .filter_map(|o| o.venue_order_id().map(|v| v.to_string()))
            .collect();

        if venue_order_ids.is_empty() {
            log::warn!("No venue order IDs found for cancel all");
            return;
        }

        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let orders: Vec<OrderAny> = open_orders.into_iter().map(|o| o.clone()).collect();

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

            log::debug!("Canceled {} orders", response.canceled.len());
            Ok(())
        });
    }

    pub(super) fn batch_cancel_orders_command(&self, cmd: &BatchCancelOrders) {
        if cmd.cancels.is_empty() {
            return;
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
            return;
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

            log::debug!("Batch canceled {} orders", response.canceled.len());
            Ok(())
        });
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
        let reason = reason_opt.as_deref().unwrap_or("unknown reason");
        match CancelOutcome::classify(reason) {
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
        Err(e) => {
            log::warn!(
                "Deferred cancel outcome unknown for {} ({}), awaiting reconciliation: {e}",
                order.client_order_id(),
                venue_order_id,
            );
        }
    }
}
