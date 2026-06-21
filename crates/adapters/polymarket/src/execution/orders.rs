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

use nautilus_common::{
    cache::InstrumentLookupError,
    messages::execution::{ModifyOrder, SubmitOrder, SubmitOrderList},
};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType, TimeInForce},
    identifiers::VenueOrderId,
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;

use super::{
    PolymarketExecutionClient,
    cancellations::execute_deferred_cancel,
    order_builder::PolymarketOrderBuilder,
    parse::{compute_commission, instrument_fee_exponent, instrument_taker_fee},
    reports::fetch_collateral_balance_pusd,
    responses::{
        check_fok_status, emit_market_order_submitted, handle_batch_order_responses,
        handle_order_response, handle_single_order_response, handle_unknown_submit_result,
        reject_submit_order,
    },
    submitter::{MarketBuyFeeContext, MarketOrderSubmitRequest, UnknownSubmitError},
    types::{BatchLimitOrderContext, LimitOrderSubmitRequest},
};
use crate::common::consts::BATCH_ORDER_LIMIT;

impl PolymarketExecutionClient {
    pub(super) fn submit_limit_order(&self, order: OrderAny) {
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
        let price = order.price().unwrap();
        let quantity = order.quantity();
        let tif = order.time_in_force();
        let post_only = order.is_post_only();
        let side = order.order_side();
        let expire_time = order.expire_time();
        let request = LimitOrderSubmitRequest {
            token_id,
            side,
            price,
            quantity,
            time_in_force: tif,
            post_only,
            neg_risk,
            expire_time,
            tick_decimals,
        };

        self.emitter.emit_order_submitted(&order);

        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let fill_tracker = self.fill_tracker.clone();
        let order_identities = self.order_identities.clone();
        let pending_submits = self.pending_submits.clone();
        let pending_fills = self.pending_fills.clone();
        let pending_order_reports = self.pending_order_reports.clone();
        let pending_cancels = self.pending_cancels.clone();
        let account_id = self.core.account_id;
        let size_precision = instrument.size_precision();
        let price_precision = instrument.price_precision();

        self.spawn_task("submit_limit_order", async move {
            let submission = match submitter.prepare_limit_order_submission(&request).await {
                Ok(submission) => submission,
                Err(e) => {
                    reject_submit_order(&order, &format!("{e}"), &emitter, clock, &pending_cancels);
                    return Ok(());
                }
            };

            let expected_venue_order_id = submission.expected_venue_order_id;
            match submitter.post_limit_order_submission(submission).await {
                Ok(response) => {
                    if let Some((order_id_str, venue_order_id)) = handle_order_response(
                        Ok(response),
                        &order,
                        &emitter,
                        clock,
                        &fill_tracker,
                        &order_identities,
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
                            &pending_cancels,
                            clock,
                        )
                        .await;
                    }
                }
                Err(e) if e.is_submit_outcome_unknown() => {
                    if let Some((order_id_str, venue_order_id)) = handle_unknown_submit_result(
                        &order,
                        expected_venue_order_id,
                        &e.to_string(),
                        None,
                        &emitter,
                        clock,
                        &fill_tracker,
                        &order_identities,
                        &pending_submits,
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
                            &pending_cancels,
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

    pub(super) fn submit_market_order(&self, order: OrderAny) {
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
        let time_in_force = order.time_in_force();
        let is_quote_qty = order.is_quote_quantity();

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
        let order_identities = self.order_identities.clone();
        let pending_submits = self.pending_submits.clone();
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
                .submit_market_order(MarketOrderSubmitRequest {
                    token_id,
                    side,
                    amount,
                    time_in_force,
                    neg_risk,
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
                    {
                        let venue_order_id = VenueOrderId::from(order_id.as_str());
                        if venue_order_id != result.expected_venue_order_id {
                            log::warn!(
                                "Market submit returned order ID {venue_order_id}, expected {}",
                                result.expected_venue_order_id
                            );
                        }
                    }

                    let fok_order_id = result
                        .response
                        .order_id
                        .as_ref()
                        .filter(|_| result.response.success && time_in_force == TimeInForce::Fok)
                        .cloned();

                    if let Some((order_id_str, venue_order_id)) = handle_order_response(
                        Ok(result.response),
                        &order,
                        &emitter,
                        clock,
                        &fill_tracker,
                        &order_identities,
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
                            &order_identities,
                            &pending_submits,
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
                                &pending_cancels,
                                clock,
                            )
                            .await;
                        }
                    } else {
                        let ts_now = clock.get_time_ns();
                        emitter.emit_order_rejected(&order, &format!("{e}"), ts_now, false);
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
                    &InstrumentLookupError::not_found(order.instrument_id()).to_string(),
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
                    &format!(
                        "Unsupported order type for Polymarket: {:?}",
                        order.order_type()
                    ),
                );
            }
        }
        Ok(())
    }

    pub(super) fn submit_order_list_command(&self, cmd: &SubmitOrderList) {
        let mut batch_orders = Vec::with_capacity(cmd.order_inits.len());
        let neg_risk_index = self.neg_risk_index.load();

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
                    neg_risk: Self::get_neg_risk_from_snapshot(
                        &neg_risk_index,
                        &order.instrument_id(),
                    ),
                    expire_time: order.expire_time(),
                    tick_decimals: instrument.price_precision() as u32,
                },
                size_precision: instrument.size_precision(),
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
        let order_identities = self.order_identities.clone();
        let pending_submits = self.pending_submits.clone();
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
                        &order_identities,
                        &pending_submits,
                        &pending_fills,
                        &pending_order_reports,
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
                                &submitter,
                                &emitter,
                                clock,
                                &fill_tracker,
                                &order_identities,
                                &pending_fills,
                                &pending_order_reports,
                                &pending_cancels,
                                &pending_tasks,
                                &stopping,
                                account_id,
                            )
                            .await;
                        }
                        Err(e) if e.is_submit_outcome_unknown() => {
                            for (batch_order, expected_venue_order_id) in
                                orders_chunk.into_iter().zip(expected_venue_order_ids)
                            {
                                if let Some((order_id_str, venue_order_id)) =
                                    handle_unknown_submit_result(
                                        &batch_order.order,
                                        expected_venue_order_id,
                                        &e.to_string(),
                                        None,
                                        &emitter,
                                        clock,
                                        &fill_tracker,
                                        &order_identities,
                                        &pending_submits,
                                        &pending_fills,
                                        &pending_order_reports,
                                        &pending_cancels,
                                        account_id,
                                        batch_order.size_precision,
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
    }

    pub(super) fn modify_order_command(&self, cmd: &ModifyOrder) {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .map(|o| o.clone());
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
    }

    pub(super) fn calculate_commission_impl(
        &self,
        instrument: &InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
    ) -> Money {
        let fee_rate = instrument_taker_fee(instrument);
        let commission = compute_commission(
            fee_rate,
            last_qty.as_decimal(),
            last_px.as_decimal(),
            liquidity_side,
        );

        Money::new(commission, instrument.quote_currency())
    }
}
