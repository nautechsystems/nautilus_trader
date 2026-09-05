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

//! HTTP order submission and cancellation facade for the Polymarket execution client.
//!
//! Accepts Nautilus-native types, handles conversion to Polymarket types,
//! order building, signing, and HTTP posting, following the dYdX OrderSubmitter pattern.
//!
//! Uses [`RetryManager`] from `nautilus-network` with exponential backoff for
//! transient HTTP failures (timeouts, 5xx, rate limits).

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::VenueOrderId,
    types::{Price, Quantity},
};
use nautilus_network::retry::{RetryConfig, RetryError, RetryManager};
use rust_decimal::Decimal;
use thiserror::Error;

use super::{
    order_builder::PolymarketOrderBuilder,
    parse::{adjust_market_buy_amount, calculate_market_price},
    types::{LimitOrderSubmitRequest, SignedLimitOrderSubmission},
};
use crate::{
    common::enums::{PolymarketOrderSide, PolymarketOrderType},
    http::{
        clob::PolymarketClobHttpClient,
        error::{Error, Result as HttpResult, sanitize_error_text},
        models::{PolymarketOpenOrder, PolymarketOrder},
        query::{CancelMarketOrdersParams, CancelResponse, OrderResponse},
    },
};

/// Fee-adjustment context for market BUYs sized to the user's pUSD balance.
///
/// When supplied to [`OrderSubmitter::submit_market_order`] alongside
/// `OrderSide::Buy`, the submitter shrinks `amount` so `amount + fees`
/// fits within `user_pusd_balance`, mirroring the SDK behaviour. SELL
/// orders ignore this context.
#[derive(Debug, Clone)]
pub(crate) struct MarketBuyFeeContext {
    pub user_pusd_balance: Decimal,
    pub fee_rate: Decimal,
    pub fee_exponent: f64,
    pub builder_taker_fee_rate: Decimal,
}

#[derive(Debug, Clone)]
pub(crate) struct MarketOrderSubmitRequest {
    pub(crate) token_id: String,
    pub(crate) side: OrderSide,
    pub(crate) amount: Quantity,
    pub(crate) time_in_force: TimeInForce,
    pub(crate) neg_risk: bool,
    pub(crate) tick_size: Price,
    pub(crate) tick_decimals: u32,
    pub(crate) fee_context: Option<MarketBuyFeeContext>,
}

#[derive(Debug, Clone)]
pub(crate) struct MarketOrderSubmitResult {
    pub response: OrderResponse,
    pub expected_base_qty: Decimal,
    pub expected_venue_order_id: VenueOrderId,
}

#[derive(Debug, Clone, Error)]
#[error("submit outcome unknown for {expected_venue_order_id}: {reason}")]
pub(crate) struct UnknownSubmitError {
    pub reason: String,
    pub expected_venue_order_id: VenueOrderId,
    pub expected_base_qty: Option<Decimal>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SubmitResponseOutcome {
    Accepted,
    Rejected,
    Unknown,
}

#[derive(Debug, Error)]
#[error("{0}")]
pub(crate) struct InvalidMarketPriceError(String);

/// HTTP order submission and cancellation facade.
///
/// Provides a clean API accepting Nautilus-native types, internally handling:
/// - Side/TIF conversion to Polymarket types
/// - Order building and EIP-712 signing (via [`PolymarketOrderBuilder`])
/// - HTTP posting to the CLOB API with automatic retry on transient failures
///
/// Fees are set by the protocol at match time in CLOB V2 (no longer embedded
/// in the signed order), so the submitter does not pre-fetch fee rates.
#[derive(Debug, Clone)]
pub(crate) struct OrderSubmitter {
    http_client: PolymarketClobHttpClient,
    order_builder: Arc<PolymarketOrderBuilder>,
    retry_manager: Arc<RetryManager<Error>>,
}

impl OrderSubmitter {
    pub(crate) fn new(
        http_client: PolymarketClobHttpClient,
        order_builder: Arc<PolymarketOrderBuilder>,
        retry_config: RetryConfig,
    ) -> Self {
        Self {
            http_client,
            order_builder,
            retry_manager: Arc::new(RetryManager::new(retry_config)),
        }
    }

    /// Fetches order book, calculates crossing price, builds and posts a market order.
    ///
    /// Converts Nautilus side to Polymarket side, walks the appropriate book side
    /// to find the crossing price, then builds and submits a FAK or FOK order.
    /// The book fetch is not retried (stale on retry); only the final POST is retried.
    ///
    /// The second return value is the order's signed base quantity (shares for
    /// BUY, the original `amount` for SELL). For BUY this is derived from the
    /// signed `taker_amount` so quote-to-base conversion matches what the venue
    /// can fill (single crossing price), not the multi-level book walk total.
    ///
    /// `request.fee_context`, when supplied with `OrderSide::Buy`, is used to shrink
    /// `amount` for taker fees before signing so balance-sized BUYs are not
    /// rejected by the venue. SELL ignores the context.
    pub(crate) async fn submit_market_order(
        &self,
        request: MarketOrderSubmitRequest,
    ) -> anyhow::Result<MarketOrderSubmitResult> {
        let MarketOrderSubmitRequest {
            token_id,
            side,
            amount,
            time_in_force,
            neg_risk,
            tick_size,
            tick_decimals,
            fee_context,
        } = request;
        let poly_side = PolymarketOrderSide::from(side);
        let order_type = PolymarketOrderType::from_market_time_in_force(time_in_force)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let amount_dec = amount.as_decimal();

        let book = self
            .http_client
            .get_book(&token_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch order book: {e}"))?;

        let levels = match poly_side {
            PolymarketOrderSide::Buy => &book.asks,
            PolymarketOrderSide::Sell => &book.bids,
        };

        let result = calculate_market_price(levels, amount_dec, poly_side)
            .map_err(|e| anyhow::anyhow!("Market price calculation failed: {e}"))?;
        let price = PolymarketOrderBuilder::normalize_market_price(
            result.crossing_price,
            tick_size,
            tick_decimals,
        )
        .map_err(InvalidMarketPriceError)?;

        // Fee-aware sizing applies to BUY only and only when a context is
        // provided. Run before signing so the on-chain `taker_amount` and
        // the emitted base quantity both reflect the venue-fillable amount.
        let signed_amount = match (poly_side, fee_context) {
            (PolymarketOrderSide::Buy, Some(ctx)) => adjust_market_buy_amount(
                amount_dec,
                ctx.user_pusd_balance,
                price,
                ctx.fee_rate,
                ctx.fee_exponent,
                ctx.builder_taker_fee_rate,
            )?,
            _ => amount_dec,
        };

        let poly_order = self
            .order_builder
            .build_market_order(
                &token_id,
                poly_side,
                price,
                signed_amount,
                neg_risk,
                tick_decimals,
            )
            .map_err(|e| anyhow::anyhow!("Failed to build market order: {e}"))?;

        // Wire amounts are mantissas at USDC_DECIMALS (10^6) scale. The share-denominated leg is
        // the exact base quantity signed for the venue: takerAmount for BUY and makerAmount for
        // SELL. Market SELL signing truncates shares to two decimal places.
        let signed_base_qty =
            signed_base_quantity(poly_order.maker_amount, poly_order.taker_amount, poly_side);
        let expected_venue_order_id = self
            .order_builder
            .expected_order_id(&poly_order, neg_risk)?;

        let http_client = self.http_client.clone();
        let saw_unknown_outcome = Arc::new(AtomicBool::new(false));

        let response = match self
            .retry_manager
            .invocation(
                "submit_market_order",
                || {
                    let http_client = http_client.clone();
                    let poly_order = poly_order.clone();
                    let saw_unknown_outcome = saw_unknown_outcome.clone();
                    async move {
                        let result = http_client.post_order(&poly_order, order_type, false).await;

                        if result.as_ref().is_err_and(Error::is_submit_outcome_unknown) {
                            saw_unknown_outcome.store(true, Ordering::Release);
                        }

                        result
                    }
                },
                |e| e.is_retryable(),
                |e| submit_retry_error(e, &saw_unknown_outcome),
            )
            .retry_delay(&Error::retry_after)
            .execute()
            .await
        {
            Ok(response) => {
                let outcome = submit_response_outcome(&response, time_in_force);
                let earlier_attempt_unknown = saw_unknown_outcome.load(Ordering::Acquire);

                if outcome == SubmitResponseOutcome::Unknown
                    || (earlier_attempt_unknown
                        && !submit_response_confirms_expected(
                            &response,
                            expected_venue_order_id,
                            time_in_force,
                        ))
                {
                    return Err(UnknownSubmitError {
                        reason: submit_response_unknown_reason(
                            &response,
                            earlier_attempt_unknown,
                            expected_venue_order_id,
                        ),
                        expected_venue_order_id,
                        expected_base_qty: Some(signed_base_qty),
                    }
                    .into());
                }

                response
            }
            Err(e)
                if submit_outcome_is_unknown(&e, saw_unknown_outcome.load(Ordering::Acquire)) =>
            {
                return Err(UnknownSubmitError {
                    reason: e.to_string(),
                    expected_venue_order_id,
                    expected_base_qty: Some(signed_base_qty),
                }
                .into());
            }
            Err(e) => return Err(e.into()),
        };

        Ok(MarketOrderSubmitResult {
            response,
            expected_base_qty: signed_base_qty,
            expected_venue_order_id,
        })
    }

    /// Cancels a single order with retry on transient failures.
    pub(crate) async fn cancel_order(&self, venue_order_id: &str) -> HttpResult<CancelResponse> {
        let http_client = self.http_client.clone();
        let order_id = venue_order_id.to_string();

        self.retry_manager
            .invocation(
                "cancel_order",
                || {
                    let http_client = http_client.clone();
                    let order_id = order_id.clone();
                    async move { http_client.cancel_order(&order_id).await }
                },
                |e| e.is_retryable(),
                |e| Error::transport(e.to_string()),
            )
            .retry_delay(&Error::retry_after)
            .execute()
            .await
    }

    /// Cancels all orders for one outcome token with retry on transient failures.
    pub(crate) async fn cancel_market_orders(&self, asset_id: &str) -> HttpResult<CancelResponse> {
        let http_client = self.http_client.clone();
        let asset_id = asset_id.to_string();

        self.retry_manager
            .invocation(
                "cancel_market_orders",
                || {
                    let http_client = http_client.clone();
                    let asset_id = asset_id.clone();
                    async move {
                        http_client
                            .cancel_market_orders(CancelMarketOrdersParams {
                                market: None,
                                asset_id: Some(asset_id),
                            })
                            .await
                    }
                },
                |e| e.is_retryable(),
                |e| Error::transport(e.to_string()),
            )
            .retry_delay(&Error::retry_after)
            .execute()
            .await
    }

    /// Cancels multiple orders with retry on transient failures.
    pub(crate) async fn cancel_orders(
        &self,
        venue_order_ids: &[&str],
    ) -> HttpResult<CancelResponse> {
        let order_ids: Vec<String> = venue_order_ids.iter().map(|s| s.to_string()).collect();
        if order_ids.is_empty() {
            return self.cancel_orders_chunk(&order_ids).await;
        }

        let mut response = CancelResponse::default();
        let mut offset = 0;

        while offset < order_ids.len() {
            let limit = self.http_client.cancel_batch_limit().await;
            let end = offset.saturating_add(limit).min(order_ids.len());
            match self.cancel_orders_chunk(&order_ids[offset..end]).await {
                Ok(chunk_response) => {
                    response.merge(chunk_response);
                    offset = end;
                }
                Err(e @ Error::BurstExceeded { .. }) => {
                    log::warn!("Cancellation batch limit changed, reselecting chunk: {e}");
                }
                Err(e) => return Err(e),
            }
        }
        Ok(response)
    }

    async fn cancel_orders_chunk(&self, order_ids: &[String]) -> HttpResult<CancelResponse> {
        let http_client = self.http_client.clone();
        let order_ids = order_ids.to_vec();

        self.retry_manager
            .invocation(
                "cancel_orders",
                || {
                    let http_client = http_client.clone();
                    let order_ids = order_ids.clone();
                    async move {
                        let refs: Vec<&str> = order_ids.iter().map(String::as_str).collect();
                        http_client.cancel_orders(&refs).await
                    }
                },
                |e| e.is_retryable(),
                |e| Error::transport(e.to_string()),
            )
            .retry_delay(&Error::retry_after)
            .execute()
            .await
    }

    /// Fetches a single order by its venue order ID from the CLOB REST API.
    ///
    /// Returns `Ok(None)` if the API returns an empty or `null` body (order not found / settled).
    pub(crate) async fn get_order(
        &self,
        order_id: &str,
    ) -> anyhow::Result<Option<PolymarketOpenOrder>> {
        let http_client = self.http_client.clone();
        let oid = order_id.to_string();

        self.retry_manager
            .invocation(
                "get_order",
                || {
                    let http_client = http_client.clone();
                    let oid = oid.clone();
                    async move { http_client.get_order_optional(&oid).await }
                },
                |e| e.is_retryable(),
                |e| Error::transport(e.to_string()),
            )
            .retry_delay(&Error::retry_after)
            .execute()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch order status: {e}"))
    }

    /// Prepares multiple limit order submissions in parallel.
    pub(crate) async fn prepare_limit_order_submissions(
        &self,
        requests: &[LimitOrderSubmitRequest],
    ) -> Vec<anyhow::Result<SignedLimitOrderSubmission>> {
        let futures = requests
            .iter()
            .map(|request| self.prepare_limit_order_submission(request));
        futures_util::future::join_all(futures).await
    }

    pub(crate) async fn prepare_limit_order_submission(
        &self,
        request: &LimitOrderSubmitRequest,
    ) -> anyhow::Result<SignedLimitOrderSubmission> {
        let order_type = PolymarketOrderType::try_from(request.time_in_force)
            .map_err(|e| anyhow::anyhow!("Unsupported time in force: {e}"))?;
        let side = PolymarketOrderSide::from(request.side);
        let expiration = limit_order_expiration(request.expire_time);

        let order = if request.quote_quantity {
            anyhow::ensure!(
                side == PolymarketOrderSide::Buy,
                "Limit SELL orders require quote_quantity=false (amount in shares)"
            );
            self.order_builder.build_limit_order_from_collateral(
                &request.token_id,
                request.price.as_decimal(),
                request.quantity.as_decimal(),
                &expiration,
                request.neg_risk,
                request.tick_decimals,
            )
        } else {
            self.order_builder.build_limit_order(
                &request.token_id,
                side,
                request.price.as_decimal(),
                request.quantity.as_decimal(),
                order_type,
                &expiration,
                request.neg_risk,
                request.tick_decimals,
            )
        }
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        let signed_base_qty = signed_base_quantity(order.maker_amount, order.taker_amount, side);
        let expected_base_qty =
            Quantity::from_decimal_dp(signed_base_qty, request.size_precision).map_err(|e| {
                anyhow::anyhow!(
                    "Signed limit order share quantity {signed_base_qty} is invalid at instrument size precision {}: {e}",
                    request.size_precision,
                )
            })?;
        anyhow::ensure!(
            expected_base_qty.as_decimal() == signed_base_qty,
            "Signed limit order share quantity {signed_base_qty} cannot be represented exactly at instrument size precision {}",
            request.size_precision,
        );

        let expected_venue_order_id = self
            .order_builder
            .expected_order_id(&order, request.neg_risk)?;

        Ok(SignedLimitOrderSubmission {
            order,
            order_type,
            post_only: request.post_only,
            expected_venue_order_id,
            expected_base_qty,
        })
    }

    pub(crate) async fn post_limit_order_submission(
        &self,
        submission: SignedLimitOrderSubmission,
    ) -> crate::http::error::Result<OrderResponse> {
        let http_client = self.http_client.clone();
        let saw_unknown_outcome = Arc::new(AtomicBool::new(false));

        let result = self
            .retry_manager
            .invocation(
                "submit_limit_order",
                || {
                    let http_client = http_client.clone();
                    let submission = submission.clone();
                    let saw_unknown_outcome = saw_unknown_outcome.clone();
                    async move {
                        let result = http_client
                            .post_order(
                                &submission.order,
                                submission.order_type,
                                submission.post_only,
                            )
                            .await;

                        if result.as_ref().is_err_and(Error::is_submit_outcome_unknown) {
                            saw_unknown_outcome.store(true, Ordering::Release);
                        }

                        result
                    }
                },
                |e| e.is_retryable(),
                |e| submit_retry_error(e, &saw_unknown_outcome),
            )
            .retry_delay(&Error::retry_after)
            .execute()
            .await;

        match result {
            Ok(response) => {
                let time_in_force = TimeInForce::from(submission.order_type);
                let earlier_attempt_unknown = saw_unknown_outcome.load(Ordering::Acquire);
                let outcome = submit_response_outcome(&response, time_in_force);

                if outcome == SubmitResponseOutcome::Unknown
                    || (earlier_attempt_unknown
                        && !submit_response_confirms_expected(
                            &response,
                            submission.expected_venue_order_id,
                            time_in_force,
                        ))
                {
                    Err(Error::decode(submit_response_unknown_reason(
                        &response,
                        earlier_attempt_unknown,
                        submission.expected_venue_order_id,
                    )))
                } else {
                    Ok(response)
                }
            }
            Err(e) if saw_unknown_outcome.load(Ordering::Acquire) => Err(Error::transport(
                format!("submit outcome unknown after an earlier attempt: {e}"),
            )),
            result => result,
        }
    }

    pub(crate) async fn post_limit_order_submissions(
        &self,
        submissions: Vec<SignedLimitOrderSubmission>,
    ) -> crate::http::error::Result<Vec<OrderResponse>> {
        let order_refs: Vec<(&PolymarketOrder, PolymarketOrderType, bool)> = submissions
            .iter()
            .map(|submission| {
                (
                    &submission.order,
                    submission.order_type,
                    submission.post_only,
                )
            })
            .collect();

        // Do not retry batch submits automatically.
        // A transport timeout can race with server-side acceptance and resubmit
        // the whole batch without an idempotency key we can verify here.
        self.http_client.post_orders(&order_refs).await
    }
}

fn submit_outcome_is_unknown(error: &Error, earlier_attempt_unknown: bool) -> bool {
    error.is_submit_outcome_unknown() || earlier_attempt_unknown
}

pub(super) fn submit_response_outcome(
    response: &OrderResponse,
    time_in_force: TimeInForce,
) -> SubmitResponseOutcome {
    if immediate_rejection_reason(response, time_in_force).is_some() {
        SubmitResponseOutcome::Rejected
    } else if response.success && submit_response_venue_order_id(response).is_some() {
        SubmitResponseOutcome::Accepted
    } else if response
        .error_msg
        .as_deref()
        .is_some_and(|reason| !reason.trim().is_empty())
    {
        SubmitResponseOutcome::Rejected
    } else {
        SubmitResponseOutcome::Unknown
    }
}

pub(super) fn submit_response_venue_order_id(response: &OrderResponse) -> Option<VenueOrderId> {
    response
        .order_id
        .as_deref()
        .and_then(|order_id| VenueOrderId::new_checked(order_id).ok())
}

fn submit_response_confirms_expected(
    response: &OrderResponse,
    expected_venue_order_id: VenueOrderId,
    time_in_force: TimeInForce,
) -> bool {
    submit_response_outcome(response, time_in_force) == SubmitResponseOutcome::Accepted
        && submit_response_venue_order_id(response) == Some(expected_venue_order_id)
}

pub(super) fn submit_response_unknown_reason(
    response: &OrderResponse,
    earlier_attempt_unknown: bool,
    expected_venue_order_id: VenueOrderId,
) -> String {
    if earlier_attempt_unknown {
        if submit_response_venue_order_id(response)
            .is_some_and(|venue_order_id| venue_order_id != expected_venue_order_id)
        {
            return "earlier attempt was ambiguous; final response returned an unexpected order ID"
                .to_string();
        }

        let final_response = response.error_msg.as_deref().map_or_else(
            || "no venue rejection reason".to_string(),
            sanitize_error_text,
        );

        format!("earlier attempt was ambiguous; final response: {final_response}")
    } else {
        "response contained neither a non-empty order ID nor a venue rejection reason".to_string()
    }
}

fn submit_retry_error(error: RetryError, saw_unknown_outcome: &AtomicBool) -> Error {
    match error {
        RetryError::OperationTimeout { .. } => {
            saw_unknown_outcome.store(true, Ordering::Release);
            Error::Timeout
        }
        error @ RetryError::ElapsedBudgetExceeded { .. } => {
            saw_unknown_outcome.store(true, Ordering::Release);
            Error::transport(error.to_string())
        }
        error @ RetryError::InvalidConfiguration { .. } => Error::bad_request(error.to_string()),
        error => Error::transport(error.to_string()),
    }
}

pub(super) fn immediate_rejection_reason(
    response: &OrderResponse,
    time_in_force: TimeInForce,
) -> Option<&str> {
    if !response.success || response.status.is_some() {
        return None;
    }

    let reason = response.error_msg.as_deref()?;
    match time_in_force {
        TimeInForce::Fok if is_fok_unfilled(reason) => Some(reason),
        TimeInForce::Ioc if is_fak_unfilled(reason) => Some(reason),
        _ => None,
    }
}

fn is_fok_unfilled(reason: &str) -> bool {
    reason.contains("couldn't be fully filled")
        && reason.contains("FOK orders are fully filled or killed")
}

const FAK_UNFILLED_REASON: &str = concat!(
    "no orders found to match with FAK order. ",
    "FAK orders are partially filled or killed if no match is found.",
);

fn is_fak_unfilled(reason: &str) -> bool {
    reason == FAK_UNFILLED_REASON
}

fn signed_base_quantity(
    maker_amount: Decimal,
    taker_amount: Decimal,
    side: PolymarketOrderSide,
) -> Decimal {
    let usdc_scale = Decimal::from(1_000_000u32);
    match side {
        PolymarketOrderSide::Buy => taker_amount / usdc_scale,
        PolymarketOrderSide::Sell => maker_amount / usdc_scale,
    }
}

// Converts a nanos expire time to the unix-seconds string expected by the
// Polymarket API. Returns `"0"` when there is no expiration.
fn limit_order_expiration(expire_time: Option<UnixNanos>) -> String {
    match expire_time {
        Some(ns) if !ns.is_zero() => ns.as_seconds().to_string(),
        _ => "0".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    #[case::none(None, "0")]
    #[case::zero(Some(UnixNanos::from(0u64)), "0")]
    #[case::one_second(Some(UnixNanos::from(1_000_000_000u64)), "1")]
    #[case::sub_second_truncates(Some(UnixNanos::from(1_500_000_000u64)), "1")]
    #[case::typical(Some(UnixNanos::from(1_735_689_600_000_000_000u64)), "1735689600")]
    fn test_limit_order_expiration(#[case] expire_time: Option<UnixNanos>, #[case] expected: &str) {
        assert_eq!(limit_order_expiration(expire_time), expected);
    }

    #[rstest]
    #[case::buy(dec!(4_800_000), dec!(5_202_897), PolymarketOrderSide::Buy, dec!(5.202897))]
    #[case::sell(dec!(5_200_000), dec!(4_992_000), PolymarketOrderSide::Sell, dec!(5.2))]
    fn test_signed_base_quantity_uses_share_denominated_wire_leg(
        #[case] maker_amount: Decimal,
        #[case] taker_amount: Decimal,
        #[case] side: PolymarketOrderSide,
        #[case] expected: Decimal,
    ) {
        assert_eq!(
            signed_base_quantity(maker_amount, taker_amount, side),
            expected
        );
    }

    #[rstest]
    fn test_rate_limit_is_definitive_unless_an_earlier_attempt_was_unknown() {
        let signer_limited = Error::RateLimit {
            endpoint: "/order",
            token_cost: 1,
            retry_after_ms: Some(2_000),
            message: "rate limit exceeded".to_string(),
            signer_limited: true,
        };
        let bare_rate_limit = Error::rate_limit("/order", 1, Some(2_000));

        assert!(!submit_outcome_is_unknown(&signer_limited, false));
        assert!(submit_outcome_is_unknown(&signer_limited, true));
        assert!(submit_outcome_is_unknown(&bare_rate_limit, false));
        assert!(submit_outcome_is_unknown(
            &Error::transport("connection reset"),
            false
        ));
        assert!(submit_outcome_is_unknown(&Error::Timeout, false));
    }

    #[rstest]
    #[case::accepted(
        TimeInForce::Gtc,
        true,
        Some("0xorder"),
        None,
        SubmitResponseOutcome::Accepted
    )]
    #[case::rejected(
        TimeInForce::Gtc,
        false,
        None,
        Some("rejected"),
        SubmitResponseOutcome::Rejected
    )]
    #[case::successful_rejection(
        TimeInForce::Gtc,
        true,
        Some(""),
        Some("rejected"),
        SubmitResponseOutcome::Rejected
    )]
    #[case::missing_id(TimeInForce::Gtc, true, None, None, SubmitResponseOutcome::Unknown)]
    #[case::empty_id(TimeInForce::Gtc, true, Some(""), None, SubmitResponseOutcome::Unknown)]
    #[case::whitespace_id_rejection(
        TimeInForce::Gtc,
        true,
        Some(" \t"),
        Some("rejected"),
        SubmitResponseOutcome::Rejected
    )]
    #[case::non_ascii_id(
        TimeInForce::Gtc,
        true,
        Some("é"),
        None,
        SubmitResponseOutcome::Unknown
    )]
    #[case::whitespace_reason(
        TimeInForce::Gtc,
        false,
        None,
        Some(" \n\t"),
        SubmitResponseOutcome::Unknown
    )]
    #[case::fok_unfilled(
        TimeInForce::Fok,
        true,
        Some("0xfok"),
        Some("order couldn't be fully filled. FOK orders are fully filled or killed."),
        SubmitResponseOutcome::Rejected
    )]
    #[case::fak_unfilled(
        TimeInForce::Ioc,
        true,
        Some("0xfak"),
        Some(FAK_UNFILLED_REASON),
        SubmitResponseOutcome::Rejected
    )]
    #[case::fak_unfilled_for_resting_order(
        TimeInForce::Gtc,
        true,
        Some("0xgtc"),
        Some(FAK_UNFILLED_REASON),
        SubmitResponseOutcome::Accepted
    )]
    #[case::fak_near_match(
        TimeInForce::Ioc,
        true,
        Some("0xfak"),
        Some("no orders found to match with FAK order"),
        SubmitResponseOutcome::Accepted
    )]
    fn test_submit_response_outcome(
        #[case] time_in_force: TimeInForce,
        #[case] success: bool,
        #[case] order_id: Option<&str>,
        #[case] error_msg: Option<&str>,
        #[case] expected: SubmitResponseOutcome,
    ) {
        let response = OrderResponse {
            success,
            order_id: order_id.map(str::to_string),
            status: None,
            making_amount: None,
            taking_amount: None,
            transaction_hashes: None,
            trade_ids: None,
            error_msg: error_msg.map(str::to_string),
        };

        assert_eq!(submit_response_outcome(&response, time_in_force), expected);
    }

    #[rstest]
    fn test_submit_response_matched_fok_confirms_expected_order_id() {
        let expected_venue_order_id = VenueOrderId::from("0xmatched-fok");
        let response = OrderResponse {
            success: true,
            order_id: Some(expected_venue_order_id.to_string()),
            status: Some(crate::http::query::OrderResponseStatus::Matched),
            making_amount: None,
            taking_amount: None,
            transaction_hashes: None,
            trade_ids: None,
            error_msg: Some(
                "order couldn't be fully filled. FOK orders are fully filled or killed."
                    .to_string(),
            ),
        };

        assert_eq!(
            submit_response_outcome(&response, TimeInForce::Fok),
            SubmitResponseOutcome::Accepted
        );
        assert!(submit_response_confirms_expected(
            &response,
            expected_venue_order_id,
            TimeInForce::Fok
        ));
    }

    #[rstest]
    fn test_submit_response_confirms_only_expected_order_id() {
        let expected_venue_order_id = VenueOrderId::from("0xexpected");
        let mut response = OrderResponse {
            success: true,
            order_id: Some(expected_venue_order_id.to_string()),
            status: None,
            making_amount: None,
            taking_amount: None,
            transaction_hashes: None,
            trade_ids: None,
            error_msg: None,
        };

        assert!(submit_response_confirms_expected(
            &response,
            expected_venue_order_id,
            TimeInForce::Gtc
        ));

        response.order_id = Some("0xother".to_string());
        assert!(!submit_response_confirms_expected(
            &response,
            expected_venue_order_id,
            TimeInForce::Gtc
        ));
    }

    #[rstest]
    #[case::operation_timeout(RetryError::OperationTimeout { timeout_ms: 500 }, true, true, true)]
    #[case::elapsed_budget(
        RetryError::ElapsedBudgetExceeded {
            attempt: 2,
            max_attempts: 3,
            last_error: None,
        },
        true,
        false,
        true
    )]
    #[case::invalid_configuration(
        RetryError::InvalidConfiguration {
            message: "invalid".to_string(),
        },
        false,
        false,
        false
    )]
    fn test_submit_retry_error_tracks_ambiguous_control_errors(
        #[case] retry_error: RetryError,
        #[case] expected_unknown: bool,
        #[case] expected_timeout: bool,
        #[case] expected_error_unknown: bool,
    ) {
        let saw_unknown_outcome = AtomicBool::new(false);

        let error = submit_retry_error(retry_error, &saw_unknown_outcome);

        assert_eq!(
            saw_unknown_outcome.load(Ordering::Acquire),
            expected_unknown
        );
        assert_eq!(matches!(error, Error::Timeout), expected_timeout);
        assert_eq!(error.is_submit_outcome_unknown(), expected_error_unknown);
    }
}
