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

use std::time::Duration;

use anyhow::Context;
use nautilus_common::messages::execution::{ModifyOrder, SubmitOrder, SubmitOrderList};
use nautilus_core::{string::secret::SecretString, time::AtomicTime};
use nautilus_live::execution::{context::OrderContext, failure::CommandFailure};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType},
    events::OrderDeniedReason,
    identifiers::VenueOrderId,
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;
#[cfg(test)]
use rust_decimal_macros::dec;

use super::{
    PolymarketExecutionClient,
    cancellations::execute_deferred_cancel,
    order_builder::PolymarketOrderBuilder,
    parse::{
        InvalidMarketPriceError, compute_commission, instrument_fee_exponent, instrument_taker_fee,
    },
    reconciliation::{
        FillContext, FillReportScope, TargetOrderReportScope, build_fill_reports_from_trades,
        build_target_order_report, confirmed_filled_quantities,
        venue_leg_filled_before_and_quantity,
    },
    reports::{fetch_collateral_balance_pusd, get_pusd_currency},
    responses::{
        check_fok_status, confirm_modify_replacement, emit_market_order_submitted,
        emit_signed_base_quantity_update, fok_check_order_id, handle_batch_order_responses,
        handle_order_response, handle_single_order_response, handle_unknown_submit_result,
        reject_submit_order,
    },
    submitter::{
        MarketBuyFeeContext, MarketOrderSubmitRequest, UnknownSubmitError,
        immediate_rejection_reason, submit_response_venue_order_id,
    },
    types::{BatchLimitOrderContext, LimitOrderSubmitRequest, classify_http_command_failure},
};
use crate::{
    common::consts::BATCH_ORDER_LIMIT,
    http::{
        error::{Error as HttpError, sanitize_error_text},
        query::GetTradesParams,
    },
};

impl PolymarketExecutionClient {
    pub(super) fn submit_limit_order(&self, order: OrderAny) {
        if let Err(reason) = PolymarketOrderBuilder::validate_limit_order(&order) {
            self.emitter.emit_order_denied(&order, &reason.to_string());
            return;
        }

        if let Err(reason) =
            PolymarketOrderBuilder::validate_limit_expiration(&order, self.clock.get_time_ns())
        {
            self.emitter.emit_order_denied(&order, &reason.to_string());
            return;
        }

        let instrument = match self.resolve_instrument(&order) {
            Some(i) => i,
            None => return,
        };

        if let Err(reason) =
            PolymarketOrderBuilder::validate_limit_price(&order, instrument.price_increment())
        {
            self.emitter.emit_order_denied(&order, &reason.to_string());
            return;
        }

        let neg_risk = self.get_neg_risk(&order.instrument_id());
        let token_id = instrument.raw_symbol().to_string();
        let tick_decimals = u32::from(instrument.min_price_increment_precision());
        let price = order.price().unwrap();
        let quantity = order.quantity();
        let quote_quantity = order.is_quote_quantity();
        let tif = order.time_in_force();
        let post_only = order.is_post_only();
        let side = order.order_side();
        let expire_time = order.expire_time();
        let request = LimitOrderSubmitRequest {
            token_id,
            side,
            price,
            quantity,
            quote_quantity,
            time_in_force: tif,
            post_only,
            neg_risk,
            expire_time,
            tick_decimals,
            size_precision: instrument.size_precision(),
        };

        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let fill_tracker = self.fill_tracker.clone();
        let order_contexts = self.order_contexts.clone();
        let ws_dispatch_state = self.ws_dispatch_state.clone();
        let pending_submits = self.pending_submits.clone();
        let pending_cancels = self.pending_cancels.clone();
        let account_id = self.core.account_id;
        let size_precision = instrument.size_precision();
        let price_precision = instrument.price_precision();

        self.spawn_task("submit_limit_order", async move {
            let mut order = order;
            if let Err(reason) =
                PolymarketOrderBuilder::validate_limit_expiration(&order, clock.get_time_ns())
            {
                emitter.emit_order_denied(&order, &reason.to_string());
                return Ok(());
            }

            emitter.emit_order_submitted(&order);

            let submission = match submitter.prepare_limit_order_submission(&request).await {
                Ok(submission) => submission,
                Err(e) => {
                    reject_submit_order(&order, &format!("{e}"), &emitter, clock, &pending_cancels);
                    return Ok(());
                }
            };

            let expected_venue_order_id = submission.expected_venue_order_id;
            emit_signed_base_quantity_update(
                &mut order,
                quote_quantity,
                side,
                quantity,
                submission.expected_base_qty,
                &emitter,
                clock,
            );

            match submitter.post_limit_order_submission(submission).await {
                Ok(response) => {
                    let fok_order_id = fok_check_order_id(&response, tif);
                    if let Some((order_id_str, venue_order_id)) = handle_order_response(
                        Ok(response),
                        &order,
                        &emitter,
                        clock,
                        &fill_tracker,
                        &order_contexts,
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
                            &pending_cancels,
                            clock,
                        )
                        .await;
                    }

                    if let Some(order_id) = fok_order_id {
                        check_fok_status(
                            &submitter,
                            &order_id,
                            &order,
                            &fill_tracker,
                            &order_contexts,
                            &ws_dispatch_state,
                            &emitter,
                            account_id,
                            size_precision,
                            price_precision,
                            clock,
                        )
                        .await;
                    }
                }
                Err(e) => match classify_http_command_failure(&e) {
                    CommandFailure::Ambiguous(reason) => {
                        if let Some((order_id_str, venue_order_id)) = handle_unknown_submit_result(
                            &order,
                            expected_venue_order_id,
                            &reason,
                            None,
                            &emitter,
                            clock,
                            &fill_tracker,
                            &order_contexts,
                            &pending_submits,
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
                                &pending_cancels,
                                clock,
                            )
                            .await;
                        }
                    }
                    CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                        reject_submit_order(&order, &reason, &emitter, clock, &pending_cancels);
                    }
                },
            }
            Ok(())
        });
    }

    pub(super) fn submit_market_order(&self, order: OrderAny) {
        if let Err(reason) = PolymarketOrderBuilder::validate_market_order(&order) {
            self.emitter.emit_order_denied(&order, &reason.to_string());
            return;
        }

        let instrument = match self.resolve_instrument(&order) {
            Some(i) => i,
            None => return,
        };

        let neg_risk = self.get_neg_risk(&order.instrument_id());
        let token_id = instrument.raw_symbol().to_string();
        let tick_size = instrument.price_increment();
        let tick_decimals = u32::from(instrument.min_price_increment_precision());
        let side = order.order_side();
        let amount = order.quantity();
        let time_in_force = order.time_in_force();
        let is_quote_qty = order.is_quote_quantity();

        let needs_fee_adjustment = side == OrderSide::Buy && is_quote_qty;
        let fee_rate = if needs_fee_adjustment {
            instrument_taker_fee(&instrument)
        } else {
            Decimal::ZERO
        };
        let fee_exponent = if needs_fee_adjustment {
            match instrument_fee_exponent(&instrument) {
                Ok(exponent) => exponent,
                Err(e) => {
                    self.emitter.emit_order_denied(&order, &e.to_string());
                    return;
                }
            }
        } else {
            Decimal::ONE
        };

        let submitter = self.submitter.clone();
        let http_client = self.http_client.clone();
        let signature_type = self.config.signature_type;
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let fill_tracker = self.fill_tracker.clone();
        let order_contexts = self.order_contexts.clone();
        let ws_dispatch_state = self.ws_dispatch_state.clone();
        let pending_submits = self.pending_submits.clone();
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
                        emitter.emit_order_denied(
                            &order,
                            &OrderDeniedReason::ValidationFailed {
                                detail: format!(
                                    "Failed to fetch pUSD balance for fee adjustment: {e}"
                                ),
                            }
                            .to_string(),
                        );
                        return Ok(());
                    }
                }
            } else {
                None
            };

            match submitter
                .submit_market_order(MarketOrderSubmitRequest {
                    token_id,
                    side,
                    amount,
                    time_in_force,
                    neg_risk,
                    tick_size,
                    tick_decimals,
                    fee_context,
                })
                .await
            {
                Ok(result) => {
                    let mut order = order;
                    emit_market_order_submitted(
                        &mut order,
                        is_quote_qty,
                        side,
                        amount,
                        result.expected_base_qty,
                        result.response.success,
                        size_precision,
                        &emitter,
                        clock,
                    );

                    if result.response.success
                        && let Some(order_id) = result.response.order_id.as_ref()
                        && !order_id.is_empty()
                    {
                        let venue_order_id = VenueOrderId::from(order_id.as_str());
                        if venue_order_id != result.expected_venue_order_id {
                            log::warn!(
                                "Market submit returned order ID {venue_order_id}, expected {}",
                                result.expected_venue_order_id
                            );
                        }
                    }

                    let fok_order_id = fok_check_order_id(&result.response, time_in_force);

                    if let Some((order_id_str, venue_order_id)) = handle_order_response(
                        Ok(result.response),
                        &order,
                        &emitter,
                        clock,
                        &fill_tracker,
                        &order_contexts,
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
                            &pending_cancels,
                            clock,
                        )
                        .await;
                    }

                    if let Some(order_id) = fok_order_id {
                        check_fok_status(
                            &submitter,
                            &order_id,
                            &order,
                            &fill_tracker,
                            &order_contexts,
                            &ws_dispatch_state,
                            &emitter,
                            account_id,
                            size_precision,
                            price_precision,
                            clock,
                        )
                        .await;
                    }
                }
                Err(e) => {
                    if let Some(unknown) = e.downcast_ref::<UnknownSubmitError>() {
                        let mut order = order;
                        emit_market_order_submitted(
                            &mut order,
                            is_quote_qty,
                            side,
                            amount,
                            unknown.expected_base_qty.unwrap_or_default(),
                            true,
                            size_precision,
                            &emitter,
                            clock,
                        );

                        let fill_tracker_quantity = if is_quote_qty && side == OrderSide::Buy {
                            unknown
                                .expected_base_qty
                                .and_then(|qty| Quantity::from_decimal_dp(qty, size_precision).ok())
                        } else {
                            None
                        };

                        if let Some((order_id_str, venue_order_id)) = handle_unknown_submit_result(
                            &order,
                            unknown.expected_venue_order_id,
                            &unknown.reason,
                            fill_tracker_quantity,
                            &emitter,
                            clock,
                            &fill_tracker,
                            &order_contexts,
                            &pending_submits,
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
                                &pending_cancels,
                                clock,
                            )
                            .await;
                        }
                    } else if let Some(invalid_price) = e.downcast_ref::<InvalidMarketPriceError>()
                    {
                        emitter.emit_order_denied(
                            &order,
                            &OrderDeniedReason::ValidationFailed {
                                detail: invalid_price.to_string(),
                            }
                            .to_string(),
                        );
                    } else {
                        let failure = e.downcast_ref::<HttpError>().map_or_else(
                            || CommandFailure::not_sent(e.to_string()),
                            classify_http_command_failure,
                        );

                        match failure {
                            CommandFailure::Ambiguous(reason) => {
                                log::warn!(
                                    "Market submit outcome unknown for {}: {reason}",
                                    order.client_order_id()
                                );
                            }
                            CommandFailure::NotSent(reason)
                            | CommandFailure::VenueRejected(reason) => {
                                reject_submit_order(
                                    &order,
                                    &reason,
                                    &emitter,
                                    clock,
                                    &pending_cancels,
                                );
                            }
                        }
                    }
                }
            }
            Ok(())
        });
    }

    pub(super) fn resolve_instrument(&self, order: &OrderAny) -> Option<InstrumentAny> {
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
                    &OrderDeniedReason::InstrumentNotFound {
                        instrument_id: order.instrument_id(),
                    }
                    .to_string(),
                );
                None
            }
        }
    }

    pub(super) fn submit_order_command(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;

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
                    &OrderDeniedReason::UnsupportedOrderType {
                        order_type: order.order_type(),
                    }
                    .to_string(),
                );
            }
        }
        Ok(())
    }

    pub(super) fn submit_order_list_command(&self, cmd: &SubmitOrderList) {
        let mut batch_orders = Vec::with_capacity(cmd.order_inits.len());
        let neg_risk_index = self.neg_risk_index.load();
        let ts_now = self.clock.get_time_ns();

        for order_init in &cmd.order_inits {
            let Some(order) = self
                .core
                .cache()
                .order(&order_init.client_order_id)
                .map(|o| o.clone())
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

            match order.order_type() {
                OrderType::Limit => {}
                OrderType::Market => {
                    self.submit_market_order(order);
                    continue;
                }
                other => {
                    self.emitter.emit_order_denied(
                        &order,
                        &OrderDeniedReason::UnsupportedOrderType { order_type: other }.to_string(),
                    );
                    continue;
                }
            }

            if let Err(reason) = PolymarketOrderBuilder::validate_limit_order(&order) {
                self.emitter.emit_order_denied(&order, &reason.to_string());
                continue;
            }

            if let Err(reason) = PolymarketOrderBuilder::validate_limit_expiration(&order, ts_now) {
                self.emitter.emit_order_denied(&order, &reason.to_string());
                continue;
            }

            let instrument = match self.resolve_instrument(&order) {
                Some(i) => i,
                None => continue,
            };

            if let Err(reason) =
                PolymarketOrderBuilder::validate_limit_price(&order, instrument.price_increment())
            {
                self.emitter.emit_order_denied(&order, &reason.to_string());
                continue;
            }

            let price = order
                .price()
                .expect("validated limit order must have a price");
            batch_orders.push(BatchLimitOrderContext {
                request: LimitOrderSubmitRequest {
                    token_id: instrument.raw_symbol().to_string(),
                    side: order.order_side(),
                    price,
                    quantity: order.quantity(),
                    quote_quantity: order.is_quote_quantity(),
                    time_in_force: order.time_in_force(),
                    post_only: order.is_post_only(),
                    neg_risk: Self::get_neg_risk_from_snapshot(
                        &neg_risk_index,
                        &order.instrument_id(),
                    ),
                    expire_time: order.expire_time(),
                    tick_decimals: u32::from(instrument.min_price_increment_precision()),
                    size_precision: instrument.size_precision(),
                },
                price_precision: instrument.price_precision(),
                order,
            });
        }

        if batch_orders.is_empty() {
            return;
        }

        if batch_orders.len() == 1 {
            let batch_order = batch_orders.pop().expect("len checked");
            self.submit_limit_order(batch_order.order);
            return;
        }

        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let fill_tracker = self.fill_tracker.clone();
        let order_contexts = self.order_contexts.clone();
        let ws_dispatch_state = self.ws_dispatch_state.clone();
        let pending_submits = self.pending_submits.clone();
        let pending_cancels = self.pending_cancels.clone();
        let account_id = self.core.account_id;

        self.spawn_task("submit_order_list", async move {
            let ts_now = clock.get_time_ns();
            batch_orders.retain(|batch_order| {
                match PolymarketOrderBuilder::validate_limit_expiration(&batch_order.order, ts_now)
                {
                    Ok(()) => true,
                    Err(reason) => {
                        emitter.emit_order_denied(&batch_order.order, &reason.to_string());
                        false
                    }
                }
            });

            if batch_orders.is_empty() {
                return Ok(());
            }

            for batch_order in &batch_orders {
                emitter.emit_order_submitted(&batch_order.order);
            }

            let requests: Vec<LimitOrderSubmitRequest> =
                batch_orders.iter().map(|bo| bo.request.clone()).collect();
            let prepare_results = submitter.prepare_limit_order_submissions(&requests).await;

            let mut prepared_orders = Vec::with_capacity(batch_orders.len());
            let mut submissions = Vec::with_capacity(batch_orders.len());

            for (mut batch_order, result) in batch_orders.into_iter().zip(prepare_results) {
                match result {
                    Ok(submission) => {
                        emit_signed_base_quantity_update(
                            &mut batch_order.order,
                            batch_order.request.quote_quantity,
                            batch_order.request.side,
                            batch_order.request.quantity,
                            submission.expected_base_qty,
                            &emitter,
                            clock,
                        );
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

            let total = submissions.len();
            let mut offset = 0;
            while offset < total {
                let end = (offset + BATCH_ORDER_LIMIT).min(total);
                let mut submissions_chunk = submissions[offset..end].to_vec();
                let mut orders_chunk = prepared_orders[offset..end].to_vec();

                if submissions_chunk.len() == 1 {
                    let submission = submissions_chunk.pop().expect("len 1");
                    let expected_venue_order_id = submission.expected_venue_order_id;
                    let batch_order = orders_chunk.pop().expect("len 1");
                    handle_single_order_response(
                        submitter.post_limit_order_submission(submission).await,
                        batch_order,
                        expected_venue_order_id,
                        &submitter,
                        &emitter,
                        clock,
                        &fill_tracker,
                        &order_contexts,
                        &ws_dispatch_state,
                        &pending_submits,
                        &pending_cancels,
                        account_id,
                    )
                    .await;
                } else {
                    let expected_venue_order_ids: Vec<VenueOrderId> = submissions_chunk
                        .iter()
                        .map(|submission| submission.expected_venue_order_id)
                        .collect();

                    match submitter
                        .post_limit_order_submissions(submissions_chunk)
                        .await
                    {
                        Ok(responses) => {
                            handle_batch_order_responses(
                                responses,
                                orders_chunk,
                                expected_venue_order_ids,
                                &submitter,
                                &emitter,
                                clock,
                                &fill_tracker,
                                &order_contexts,
                                &ws_dispatch_state,
                                &pending_submits,
                                &pending_cancels,
                                account_id,
                            )
                            .await;
                        }
                        Err(e) => match classify_http_command_failure(&e) {
                            CommandFailure::Ambiguous(reason) => {
                                for (batch_order, expected_venue_order_id) in
                                    orders_chunk.into_iter().zip(expected_venue_order_ids)
                                {
                                    if let Some((order_id_str, venue_order_id)) =
                                        handle_unknown_submit_result(
                                            &batch_order.order,
                                            expected_venue_order_id,
                                            &reason,
                                            None,
                                            &emitter,
                                            clock,
                                            &fill_tracker,
                                            &order_contexts,
                                            &pending_submits,
                                            &pending_cancels,
                                            account_id,
                                            batch_order.request.size_precision,
                                            batch_order.price_precision,
                                        )
                                    {
                                        execute_deferred_cancel(
                                            &submitter,
                                            &batch_order.order,
                                            &order_id_str,
                                            venue_order_id,
                                            &emitter,
                                            &pending_cancels,
                                            clock,
                                        )
                                        .await;
                                    }
                                }
                            }
                            CommandFailure::NotSent(reason)
                            | CommandFailure::VenueRejected(reason) => {
                                for batch_order in orders_chunk {
                                    reject_submit_order(
                                        &batch_order.order,
                                        &reason,
                                        &emitter,
                                        clock,
                                        &pending_cancels,
                                    );
                                }
                            }
                        },
                    }
                }

                offset = end;
            }

            Ok(())
        });
    }

    pub(super) fn modify_order_command(&self, cmd: &ModifyOrder) {
        let Some(order) = self.core.cache().order_owned(&cmd.client_order_id) else {
            self.emitter.emit_order_modify_rejected_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                cmd.venue_order_id,
                "Order not found in cache",
                self.clock.get_time_ns(),
            );
            return;
        };

        let venue_order_id = self
            .order_contexts
            .venue_order_id(&cmd.client_order_id)
            .or_else(|| order.venue_order_id())
            .or_else(|| {
                self.core
                    .cache()
                    .venue_order_id(&cmd.client_order_id)
                    .copied()
            });

        let reject = |reason: &str| {
            self.emitter.emit_order_modify_rejected(
                &order,
                venue_order_id,
                reason,
                self.clock.get_time_ns(),
            );
        };

        if !order.is_open() {
            reject("Cannot modify an order that is not open");
            return;
        }

        if order.order_type() != OrderType::Limit {
            reject("Polymarket modification requires a Limit order");
            return;
        }

        if order.is_quote_quantity() {
            reject("Polymarket modification requires a base-denominated order");
            return;
        }

        if cmd.trigger_price.is_some() {
            reject("Polymarket Limit orders do not support trigger price modification");
            return;
        }

        let Some(venue_order_id) = venue_order_id else {
            reject("Polymarket modification requires a venue order ID");
            return;
        };

        if cmd
            .venue_order_id
            .is_some_and(|requested| requested != venue_order_id)
        {
            reject("Modify command venue order ID does not match the current order leg");
            return;
        }

        let Some(instrument) = self
            .core
            .cache()
            .instrument(&order.instrument_id())
            .cloned()
        else {
            reject("Instrument not found in cache");
            return;
        };

        let price = cmd.price.or_else(|| order.price()).unwrap();
        if let Err(reason) =
            PolymarketOrderBuilder::validate_limit_price_value(price, instrument.price_increment())
        {
            reject(&reason.to_string());
            return;
        }

        if let Err(reason) =
            PolymarketOrderBuilder::validate_limit_expiration(&order, self.clock.get_time_ns())
        {
            reject(&reason.to_string());
            return;
        }

        let target_total_qty = cmd.quantity.unwrap_or_else(|| order.quantity());
        if target_total_qty <= order.filled_qty() {
            reject(&format!(
                "Modify quantity {target_total_qty} must be greater than filled quantity {}",
                order.filled_qty()
            ));
            return;
        }

        if target_total_qty == order.quantity() && Some(price) == order.price() {
            reject("Modify command does not change price or quantity");
            return;
        }

        let (prior_filled_qty, venue_leg_qty) = match venue_leg_filled_before_and_quantity(
            &order,
            venue_order_id,
            instrument.size_precision(),
        ) {
            Ok(quantities) => quantities,
            Err(e) => {
                reject(&format!(
                    "Cannot determine current venue-leg quantities: {e}"
                ));
                return;
            }
        };

        let cached_venue_leg_filled = order
            .filled_qty()
            .checked_sub(prior_filled_qty)
            .expect("venue-leg quantity calculation validated cumulative fills");

        if self.pending_cancels.contains(&cmd.client_order_id) {
            reject("Polymarket cancellation is in flight");
            return;
        }

        {
            let mut state = self.ws_dispatch_state.lock();
            if !state.begin_modify(cmd.client_order_id, venue_order_id, order.instrument_id()) {
                reject("Another Polymarket modification or cancellation is already in flight");
                return;
            }
        }

        if self.order_contexts.get(&venue_order_id).is_none() {
            self.order_contexts
                .register_context(venue_order_id, OrderContext::from(&order));
        }

        if !self.fill_tracker.contains(&venue_order_id) {
            self.fill_tracker.restore_order(
                venue_order_id,
                venue_leg_qty,
                cached_venue_leg_filled,
                order.order_side(),
            );
        }

        self.shared_token_instruments.insert(
            ustr::Ustr::from(instrument.raw_symbol().as_str()),
            instrument.clone(),
        );

        let submitter = self.submitter.clone();
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let fill_tracker = self.fill_tracker.clone();
        let order_contexts = self.order_contexts.clone();
        let ws_dispatch_state = self.ws_dispatch_state.clone();
        let token_instruments = self.shared_token_instruments.clone();
        let pending_cancels = self.pending_cancels.clone();
        let account_id = self.core.account_id;
        let client_order_id = cmd.client_order_id;
        let instrument_id = order.instrument_id();
        let token_id = instrument.raw_symbol().to_string();
        let tick_decimals = u32::from(instrument.min_price_increment_precision());
        let size_precision = instrument.size_precision();
        let neg_risk = self.get_neg_risk(&instrument_id);
        let user_address = self
            .secrets
            .funder
            .clone()
            .unwrap_or_else(|| self.secrets.address.clone());
        let api_key = SecretString::from(self.secrets.credential.api_key_str().to_string());
        let load_ids = self.config.reconciliation_load_ids().map(Vec::from);
        let max_status_retries = self.config.max_retries;
        let status_retry_delay_initial_ms = self.config.retry_delay_initial_ms;
        let status_retry_delay_max_ms = self.config.retry_delay_max_ms;
        let spawn_failure_order = order.clone();

        let spawned = self.spawn_task("modify_order", async move {
            let venue_order_id_str = venue_order_id.to_string();
            let reject_modify =
                |reason: &str, cancellation_proven: bool, close_canceled_order: bool| {
                    reject_modify_and_finish(
                        &order,
                        venue_order_id,
                        reason,
                        cancellation_proven,
                        close_canceled_order,
                        &emitter,
                        clock,
                        &fill_tracker,
                        &ws_dispatch_state,
                        &pending_cancels,
                    );
                };

            let response = match submitter.cancel_order(&venue_order_id_str).await {
                Ok(response) => response,
                Err(e) => {
                    let reason = match classify_http_command_failure(&e) {
                        CommandFailure::Ambiguous(reason) => {
                            format!("Cancel outcome is unknown; replacement not submitted: {reason}")
                        }
                        CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                            format!("Cancel failed; replacement not submitted: {reason}")
                        }
                    };
                    reject_modify(&reason, false, true);
                    return Ok(());
                }
            };

            if !response
                .canceled
                .iter()
                .any(|order_id| order_id == &venue_order_id_str)
            {
                let reason = response
                    .not_canceled
                    .get(&venue_order_id_str)
                    .and_then(|reason| reason.as_deref())
                    .map_or_else(
                        || "cancel response omitted the order result".to_string(),
                        sanitize_error_text,
                    );
                reject_modify(
                    &format!("Cancel not confirmed; replacement not submitted: {reason}"),
                    false,
                    true,
                );
                return Ok(());
            }

            let cancel_ts = clock.get_time_ns();
            let cancel_confirmed = {
                let mut state = ws_dispatch_state.lock();
                let confirmed =
                    state.confirm_modify_cancel(client_order_id, venue_order_id, cancel_ts);
                if !confirmed {
                    pending_cancels.remove(&client_order_id);
                }
                confirmed
            };

            if !cancel_confirmed {
                emitter.emit_order_modify_rejected(
                    &order,
                    Some(venue_order_id),
                    "Polymarket modification state was lost after canceling the old venue leg",
                    cancel_ts,
                );

                if !fill_tracker.is_fully_filled(&venue_order_id) {
                    emitter.emit_order_canceled(&order, Some(venue_order_id), cancel_ts);
                }
                return Ok(());
            }

            let fill_context = FillContext {
                account_id,
                user_address: &user_address,
                api_key: api_key.expose_secret(),
                pusd: get_pusd_currency(),
                clock,
            };
            let mut status_retry_count = 0;
            let mut status_retry_delay_ms =
                status_retry_delay_initial_ms.min(status_retry_delay_max_ms);
            let report = loop {
                let report = match submitter.get_order(&venue_order_id_str).await {
                    Ok(Some(provider_order)) => match build_target_order_report(
                        &provider_order,
                        &token_instruments,
                        &fill_context,
                        TargetOrderReportScope::new(
                            instrument_id,
                            venue_order_id,
                            Some(client_order_id),
                            Some(&order),
                            Some(venue_leg_qty),
                        ),
                        clock.get_time_ns(),
                    ) {
                        Ok(report) => Some(report),
                        Err(e) => {
                            reject_modify(
                                &format!("Canceled order reconciliation failed: {e}"),
                                true,
                                true,
                            );
                            return Ok(());
                        }
                    },
                    Ok(None) => None,
                    Err(e) => {
                        reject_modify(
                            &format!("Failed to reconcile canceled order: {e}"),
                            true,
                            true,
                        );
                        return Ok(());
                    }
                };

                match report {
                    Some(report)
                        if matches!(
                            report.order_status,
                            OrderStatus::Canceled | OrderStatus::Filled
                        ) =>
                    {
                        break report;
                    }
                    Some(report) if status_retry_count == max_status_retries => {
                        reject_modify(
                            &format!(
                                "Canceled order reconciliation returned {:?}; replacement not submitted",
                                report.order_status
                            ),
                            true,
                            report.order_status != OrderStatus::Filled,
                        );
                        return Ok(());
                    }
                    None if status_retry_count == max_status_retries => {
                        reject_modify(
                            "Canceled order is unavailable for final fill reconciliation",
                            true,
                            true,
                        );
                        return Ok(());
                    }
                    Some(_) | None => {}
                }

                tokio::time::sleep(Duration::from_millis(status_retry_delay_ms)).await;
                status_retry_count += 1;
                status_retry_delay_ms = status_retry_delay_ms
                    .saturating_mul(2)
                    .min(status_retry_delay_max_ms);
            };

            let trades = match http_client.get_trades(GetTradesParams::default()).await {
                Ok(trades) => trades,
                Err(e) => {
                    reject_modify(
                        &format!("Failed to reconcile canceled order fills: {e}"),
                        true,
                        true,
                    );
                    return Ok(());
                }
            };

            let (mut confirmed_fills, discards) = match build_fill_reports_from_trades(
                &trades,
                &fill_context,
                &token_instruments,
                FillReportScope::new(Some(instrument_id), Some(venue_order_id))
                    .with_expected_order_side(Some(order.order_side())),
                clock.get_time_ns(),
                load_ids.as_deref(),
                None,
            ) {
                Ok(result) => result,
                Err(e) => {
                    reject_modify(
                        &format!("Canceled order fill reconciliation failed: {e}"),
                        true,
                        true,
                    );
                    return Ok(());
                }
            };

            if discards.has_pending_target {
                reject_modify(
                    "Canceled order still has unsettled fills; replacement not submitted",
                    true,
                    true,
                );
                return Ok(());
            }

            let confirmed_filled_dec = confirmed_filled_quantities(&confirmed_fills)
                .get(&venue_order_id)
                .copied()
                .unwrap_or(Decimal::ZERO);
            let confirmed_venue_leg_filled = match Quantity::from_decimal_dp(
                confirmed_filled_dec,
                size_precision,
            ) {
                Ok(quantity) if quantity.as_decimal() == confirmed_filled_dec => quantity,
                _ => {
                    reject_modify(
                        "Confirmed fill quantity is not exactly representable",
                        true,
                        true,
                    );
                    return Ok(());
                }
            };

            if report.filled_qty != confirmed_venue_leg_filled {
                reject_modify(
                    &format!(
                        "Canceled order fill quantity {} does not match confirmed trades {}",
                        report.filled_qty, confirmed_venue_leg_filled
                    ),
                    true,
                    true,
                );
                return Ok(());
            }

            let Some(final_filled_qty) = prior_filled_qty.checked_add(confirmed_venue_leg_filled)
            else {
                reject_modify(
                    "Cumulative fill quantity overflow",
                    true,
                    true,
                );
                return Ok(());
            };

            if final_filled_qty < order.filled_qty() {
                reject_modify(
                    "Confirmed cumulative fills are behind the cached order",
                    true,
                    true,
                );
                return Ok(());
            }

            for fill in &mut confirmed_fills {
                fill.client_order_id = Some(client_order_id);
            }

            {
                // Dispatch holds this state while applying WebSocket fills. Keep the restored
                // cumulative quantity and its dedup keys atomic against that path.
                let mut state = ws_dispatch_state.lock();
                fill_tracker.restore_order(
                    venue_order_id,
                    venue_leg_qty,
                    confirmed_venue_leg_filled,
                    order.order_side(),
                );

                for fill in &confirmed_fills {
                    state.record_reconciled_fill(fill.trade_id, fill.venue_order_id);
                }
            }

            for fill in confirmed_fills {
                emitter.send_fill_report(fill);
            }

            if report.order_status == OrderStatus::Filled {
                reject_modify(
                    "Order filled while cancellation was in flight; replacement not submitted",
                    true,
                    false,
                );
                return Ok(());
            }

            if pending_cancels.contains(&client_order_id) {
                reject_modify(
                    "Modification superseded by cancellation",
                    true,
                    true,
                );
                return Ok(());
            }

            let Some(replacement_qty) = target_total_qty.checked_sub(final_filled_qty) else {
                reject_modify(
                    &format!(
                        "Modify quantity {target_total_qty} is not greater than final filled quantity {final_filled_qty}"
                    ),
                    true,
                    final_filled_qty < order.quantity(),
                );
                return Ok(());
            };

            if replacement_qty.is_zero() {
                reject_modify(
                    &format!(
                        "Modify quantity {target_total_qty} equals final filled quantity {final_filled_qty}"
                    ),
                    true,
                    final_filled_qty < order.quantity(),
                );
                return Ok(());
            }

            if let Err(reason) =
                PolymarketOrderBuilder::validate_limit_expiration(&order, clock.get_time_ns())
            {
                reject_modify(&reason.to_string(), true, true);
                return Ok(());
            }

            let request = LimitOrderSubmitRequest {
                token_id,
                side: order.order_side(),
                price,
                quantity: replacement_qty,
                quote_quantity: false,
                time_in_force: order.time_in_force(),
                post_only: order.is_post_only(),
                neg_risk,
                expire_time: order.expire_time(),
                tick_decimals,
                size_precision,
            };

            let submission = match submitter.prepare_limit_order_submission(&request).await {
                Ok(submission) => submission,
                Err(e) => {
                    reject_modify(
                        &format!("Failed to prepare replacement order: {e}"),
                        true,
                        true,
                    );
                    return Ok(());
                }
            };

            let expected_venue_order_id = submission.expected_venue_order_id;
            let Some(logical_total_qty) =
                final_filled_qty.checked_add(submission.expected_base_qty)
            else {
                reject_modify(
                    "Replacement logical quantity overflow",
                    true,
                    true,
                );
                return Ok(());
            };

            if !ws_dispatch_state.lock().set_modify_replacement(
                client_order_id,
                expected_venue_order_id,
                logical_total_qty,
                submission.expected_base_qty,
                price,
            ) {
                reject_modify(
                    "Polymarket modification state was lost before replacement submission",
                    true,
                    true,
                );
                return Ok(());
            }

            if pending_cancels.contains(&client_order_id) {
                reject_modify(
                    "Modification superseded by cancellation",
                    true,
                    true,
                );
                return Ok(());
            }

            match submitter.post_limit_order_submission(submission).await {
                Ok(response) => {
                    if let Some(reason) = immediate_rejection_reason(&response, order.time_in_force())
                    {
                        reject_modify(reason, true, true);
                        return Ok(());
                    }

                    if response.success
                        && submit_response_venue_order_id(&response)
                            == Some(expected_venue_order_id)
                    {
                        let _ = confirm_modify_replacement(
                            &order,
                            expected_venue_order_id,
                            &emitter,
                            clock,
                            &fill_tracker,
                            &order_contexts,
                            &ws_dispatch_state,
                        );
                    } else if response.success {
                        log::warn!(
                            "Replacement outcome unknown for {client_order_id}; tracking expected venue order ID {expected_venue_order_id}"
                        );
                    } else {
                        let reason = response
                            .error_msg
                            .as_deref()
                            .map_or_else(
                                || "replacement order rejected".to_string(),
                                sanitize_error_text,
                            );
                        reject_modify(&reason, true, true);
                    }
                }
                Err(e) => match classify_http_command_failure(&e) {
                    CommandFailure::Ambiguous(reason) => {
                        log::warn!(
                            "Replacement outcome unknown for {client_order_id}: {reason}. Tracking expected venue order ID {expected_venue_order_id}"
                        );
                    }
                    CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                        reject_modify(&reason, true, true);
                    }
                },
            }

            Ok(())
        });

        if !spawned {
            reject_modify_and_finish(
                &spawn_failure_order,
                venue_order_id,
                "Polymarket execution client is shutting down",
                false,
                false,
                &self.emitter,
                self.clock,
                &self.fill_tracker,
                &self.ws_dispatch_state,
                &self.pending_cancels,
            );
        }
    }

    pub(super) fn calculate_commission_impl(
        &self,
        instrument: &InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
    ) -> anyhow::Result<Money> {
        calculate_commission(instrument, last_qty, last_px, liquidity_side)
    }
}

#[expect(clippy::too_many_arguments)]
fn reject_modify_and_finish(
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    reason: &str,
    cancellation_proven: bool,
    close_canceled_order: bool,
    emitter: &nautilus_live::ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &super::order_fill_tracker::OrderFillTrackerMap,
    ws_dispatch_state: &std::sync::Arc<
        parking_lot::Mutex<crate::websocket::dispatch::WsDispatchState>,
    >,
    pending_cancels: &super::pending::PendingCancelTracker,
) {
    let ts_event = clock.get_time_ns();
    let finish = {
        let mut state = ws_dispatch_state.lock();
        let finish = state.finish_modify_without_replacement(
            order.client_order_id(),
            venue_order_id,
            cancellation_proven,
            ts_event,
        );

        if cancellation_proven && finish.is_some() {
            pending_cancels.remove(&order.client_order_id());
        }
        finish
    };
    let Some(finish) = finish else {
        return;
    };

    let reason = sanitize_error_text(reason);
    emitter.emit_order_modify_rejected(order, order.venue_order_id(), &reason, ts_event);

    if let (venue_order_id, Some(ts_event)) = finish
        && close_canceled_order
        && !fill_tracker.is_fully_filled(&venue_order_id)
    {
        emitter.emit_order_canceled(order, Some(venue_order_id), ts_event);
    }
}

/// Calculates the venue commission for a fill of `last_qty` at `last_px`.
///
/// # Errors
///
/// Returns an error if the computed commission cannot be represented as [`Money`] in the
/// instrument's quote currency.
pub(super) fn calculate_commission(
    instrument: &InstrumentAny,
    last_qty: Quantity,
    last_px: Price,
    liquidity_side: LiquiditySide,
) -> anyhow::Result<Money> {
    let fee_rate = instrument_taker_fee(instrument);
    let fee_exponent = instrument_fee_exponent(instrument)?;

    let commission = compute_commission(
        fee_rate,
        fee_exponent,
        last_qty.as_decimal(),
        last_px.as_decimal(),
        liquidity_side,
    )?;

    Money::from_decimal(commission, instrument.quote_currency()).with_context(|| {
        format!(
            "failed to represent commission {commission} for {} as Money",
            instrument.id(),
        )
    })
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::stubs::binary_option;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_calculate_commission_returns_exact_money() {
        let instrument = InstrumentAny::BinaryOption(binary_option());

        let commission = calculate_commission(
            &instrument,
            Quantity::from("100"),
            Price::from("0.50"),
            LiquiditySide::Taker,
        )
        .expect("a representable commission succeeds");

        assert_eq!(commission.currency, instrument.quote_currency());
        assert_eq!(
            commission.as_decimal(),
            compute_commission(
                instrument_taker_fee(&instrument),
                instrument_fee_exponent(&instrument).unwrap(),
                dec!(100),
                dec!(0.50),
                LiquiditySide::Taker,
            )
            .unwrap()
        );
    }

    #[rstest]
    fn test_calculate_commission_is_zero_for_maker_liquidity() {
        let instrument = InstrumentAny::BinaryOption(binary_option());

        let commission = calculate_commission(
            &instrument,
            Quantity::from("100"),
            Price::from("0.50"),
            LiquiditySide::Maker,
        )
        .expect("a zero commission is representable");

        assert_eq!(commission.as_decimal(), dec!(0));
    }
}
