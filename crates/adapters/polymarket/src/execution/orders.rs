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
use nautilus_common::messages::execution::{ModifyOrder, SubmitOrder, SubmitOrderList};
use nautilus_live::execution::failure::CommandFailure;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType},
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
    parse::{compute_commission, instrument_fee_exponent, instrument_taker_fee},
    reports::fetch_collateral_balance_pusd,
    responses::{
        check_fok_status, emit_market_order_submitted, emit_signed_base_quantity_update,
        fok_check_order_id, handle_batch_order_responses, handle_order_response,
        handle_single_order_response, handle_unknown_submit_result, reject_submit_order,
    },
    submitter::{
        InvalidMarketPriceError, MarketBuyFeeContext, MarketOrderSubmitRequest, UnknownSubmitError,
    },
    types::{BatchLimitOrderContext, LimitOrderSubmitRequest, classify_http_command_failure},
};
use crate::{common::consts::BATCH_ORDER_LIMIT, http::error::Error as HttpError};

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

        let submitter = self.submitter.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let fill_tracker = self.fill_tracker.clone();
        let order_identities = self.order_identities.clone();
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
                false,
                side,
                quantity,
                submission.expected_base_qty,
                size_precision,
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
                        &order_identities,
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
                            &order_identities,
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
                            &order_identities,
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
                        &order_identities,
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
                            &order_identities,
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
                    time_in_force: order.time_in_force(),
                    post_only: order.is_post_only(),
                    neg_risk: Self::get_neg_risk_from_snapshot(
                        &neg_risk_index,
                        &order.instrument_id(),
                    ),
                    expire_time: order.expire_time(),
                    tick_decimals: u32::from(instrument.min_price_increment_precision()),
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
                            false,
                            batch_order.request.side,
                            batch_order.request.quantity,
                            submission.expected_base_qty,
                            batch_order.size_precision,
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
                        &order_identities,
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
                                &order_identities,
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
                                            &order_identities,
                                            &pending_submits,
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
    ) -> anyhow::Result<Money> {
        calculate_commission(instrument, last_qty, last_px, liquidity_side)
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
    let fee_exponent = instrument_fee_exponent(instrument);

    let commission = compute_commission(
        fee_rate,
        fee_exponent,
        last_qty.as_decimal(),
        last_px.as_decimal(),
        liquidity_side,
    );

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
                instrument_fee_exponent(&instrument),
                dec!(100),
                dec!(0.50),
                LiquiditySide::Taker,
            )
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
