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

use std::sync::Arc;

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    types::{Price, Quantity},
};
use nautilus_network::retry::{RetryConfig, RetryManager};
use rust_decimal::Decimal;

use super::{
    order_builder::PolymarketOrderBuilder,
    parse::{adjust_market_buy_amount, calculate_market_price},
    types::{LimitOrderSubmitRequest, SignedLimitOrderSubmission},
};
use crate::{
    common::enums::{PolymarketOrderSide, PolymarketOrderType},
    http::{
        clob::PolymarketClobHttpClient,
        error::Error,
        models::{PolymarketOpenOrder, PolymarketOrder},
        query::{CancelResponse, OrderResponse},
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
    pub fn new(
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

    /// Builds a signed limit order and posts it with retry on transient failures.
    #[expect(clippy::too_many_arguments)]
    pub async fn submit_limit_order(
        &self,
        token_id: &str,
        side: OrderSide,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        neg_risk: bool,
        expire_time: Option<UnixNanos>,
        tick_decimals: u32,
    ) -> anyhow::Result<OrderResponse> {
        let request = LimitOrderSubmitRequest {
            token_id: token_id.to_string(),
            side,
            price,
            quantity,
            time_in_force,
            post_only,
            neg_risk,
            expire_time,
            tick_decimals,
        };
        let submission = self.prepare_limit_order_submission(&request).await?;
        self.post_limit_order_submission(submission).await
    }

    /// Fetches order book, calculates crossing price, builds and posts a market order.
    ///
    /// Converts Nautilus side to Polymarket side, walks the appropriate book side
    /// to find the crossing price, then builds and submits a FOK order.
    /// The book fetch is not retried (stale on retry); only the final POST is retried.
    ///
    /// The second return value is the order's signed base quantity (shares for
    /// BUY, the original `amount` for SELL). For BUY this is derived from the
    /// signed `taker_amount` so quote-to-base conversion matches what the venue
    /// can fill (single crossing price), not the multi-level book walk total.
    ///
    /// `fee_context`, when supplied with `OrderSide::Buy`, is used to shrink
    /// `amount` for taker fees before signing so balance-sized BUYs are not
    /// rejected by the venue. SELL ignores the context.
    pub async fn submit_market_order(
        &self,
        token_id: &str,
        side: OrderSide,
        amount: Quantity,
        neg_risk: bool,
        tick_decimals: u32,
        fee_context: Option<MarketBuyFeeContext>,
    ) -> anyhow::Result<(OrderResponse, Decimal)> {
        let poly_side = PolymarketOrderSide::try_from(side)
            .map_err(|e| anyhow::anyhow!("Invalid order side: {e}"))?;
        let amount_dec = amount.as_decimal();

        let book = self
            .http_client
            .get_book(token_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch order book: {e}"))?;

        let levels = match poly_side {
            PolymarketOrderSide::Buy => &book.asks,
            PolymarketOrderSide::Sell => &book.bids,
        };

        let result = calculate_market_price(levels, amount_dec, poly_side)
            .map_err(|e| anyhow::anyhow!("Market price calculation failed: {e}"))?;

        // Fee-aware sizing applies to BUY only and only when a context is
        // provided. Run before signing so the on-chain `taker_amount` and
        // the emitted base quantity both reflect the venue-fillable amount.
        let signed_amount = match (poly_side, fee_context) {
            (PolymarketOrderSide::Buy, Some(ctx)) => adjust_market_buy_amount(
                amount_dec,
                ctx.user_pusd_balance,
                result.crossing_price,
                ctx.fee_rate,
                ctx.fee_exponent,
                ctx.builder_taker_fee_rate,
            )?,
            _ => amount_dec,
        };

        let poly_order = self
            .order_builder
            .build_market_order(
                token_id,
                poly_side,
                result.crossing_price,
                signed_amount,
                neg_risk,
                tick_decimals,
            )
            .map_err(|e| anyhow::anyhow!("Failed to build market order: {e}"))?;

        // Wire amounts are mantissas at USDC_DECIMALS (10^6) scale. For BUY,
        // the signed taker_amount is the exact share quantity the venue will
        // fill against; for SELL, the original `amount` is already in base
        // shares (book walk total is irrelevant since SELL is never quote-qty).
        let usdc_scale = Decimal::from(1_000_000u32);
        let signed_base_qty = match poly_side {
            PolymarketOrderSide::Buy => poly_order.taker_amount / usdc_scale,
            PolymarketOrderSide::Sell => amount_dec,
        };

        let http_client = self.http_client.clone();

        let response = self
            .retry_manager
            .execute_with_retry(
                "submit_market_order",
                || {
                    let http_client = http_client.clone();
                    let poly_order = poly_order.clone();
                    async move {
                        http_client
                            .post_order(&poly_order, PolymarketOrderType::FOK, false)
                            .await
                    }
                },
                |e| e.is_retryable(),
                Error::transport,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok((response, signed_base_qty))
    }

    /// Cancels a single order with retry on transient failures.
    pub async fn cancel_order(&self, venue_order_id: &str) -> anyhow::Result<CancelResponse> {
        let http_client = self.http_client.clone();
        let order_id = venue_order_id.to_string();
        self.retry_manager
            .execute_with_retry(
                "cancel_order",
                || {
                    let http_client = http_client.clone();
                    let order_id = order_id.clone();
                    async move { http_client.cancel_order(&order_id).await }
                },
                |e| e.is_retryable(),
                Error::transport,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Cancels multiple orders with retry on transient failures.
    pub async fn cancel_orders(&self, venue_order_ids: &[&str]) -> anyhow::Result<CancelResponse> {
        let http_client = self.http_client.clone();
        let order_ids: Vec<String> = venue_order_ids.iter().map(|s| s.to_string()).collect();

        self.retry_manager
            .execute_with_retry(
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
                Error::transport,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Fetches a single order by its venue order ID from the CLOB REST API.
    ///
    /// Returns `Ok(None)` if the API returns an empty or `null` body (order not found / settled).
    pub async fn get_order(&self, order_id: &str) -> anyhow::Result<Option<PolymarketOpenOrder>> {
        let http_client = self.http_client.clone();
        let oid = order_id.to_string();

        self.retry_manager
            .execute_with_retry(
                "get_order",
                || {
                    let http_client = http_client.clone();
                    let oid = oid.clone();
                    async move { http_client.get_order_optional(&oid).await }
                },
                |e| e.is_retryable(),
                Error::transport,
            )
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
        let side = PolymarketOrderSide::try_from(request.side)
            .map_err(|e| anyhow::anyhow!("Invalid order side: {e}"))?;
        let expiration = limit_order_expiration(request.expire_time);

        let order = self
            .order_builder
            .build_limit_order(
                &request.token_id,
                side,
                request.price.as_decimal(),
                request.quantity.as_decimal(),
                &expiration,
                request.neg_risk,
                request.tick_decimals,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(SignedLimitOrderSubmission {
            order,
            order_type,
            post_only: request.post_only,
        })
    }

    pub(crate) async fn post_limit_order_submission(
        &self,
        submission: SignedLimitOrderSubmission,
    ) -> anyhow::Result<OrderResponse> {
        let http_client = self.http_client.clone();

        self.retry_manager
            .execute_with_retry(
                "submit_limit_order",
                || {
                    let http_client = http_client.clone();
                    let submission = submission.clone();
                    async move {
                        http_client
                            .post_order(
                                &submission.order,
                                submission.order_type,
                                submission.post_only,
                            )
                            .await
                    }
                },
                |e| e.is_retryable(),
                Error::transport,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub(crate) async fn post_limit_order_submissions(
        &self,
        submissions: Vec<SignedLimitOrderSubmission>,
    ) -> anyhow::Result<Vec<OrderResponse>> {
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
        self.http_client
            .post_orders(&order_refs)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

// Converts a nanos expire time to the unix-seconds string expected by the
// Polymarket API. Returns `"0"` when there is no expiration.
fn limit_order_expiration(expire_time: Option<UnixNanos>) -> String {
    match expire_time {
        Some(ns) if ns.as_u64() > 0 => (ns.as_u64() / 1_000_000_000).to_string(),
        _ => "0".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

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
}
