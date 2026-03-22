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
//! order building, signing, and HTTP posting — following the dYdX OrderSubmitter pattern.
//!
//! Uses [`RetryManager`] from `nautilus-network` with exponential backoff for
//! transient HTTP failures (timeouts, 5xx, rate limits).

use std::sync::Arc;

use dashmap::DashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    types::{Price, Quantity},
};
use nautilus_network::retry::{RetryConfig, RetryManager};
use rust_decimal::Decimal;

use super::{order_builder::PolymarketOrderBuilder, parse::calculate_market_price};
use crate::{
    common::enums::{PolymarketOrderSide, PolymarketOrderType},
    http::{
        clob::PolymarketClobHttpClient,
        error::Error,
        query::{CancelResponse, OrderResponse},
    },
};

/// HTTP order submission and cancellation facade.
///
/// Provides a clean API accepting Nautilus-native types, internally handling:
/// - Side/TIF conversion to Polymarket types
/// - Expiration calculation
/// - Order building and EIP-712 signing (via [`PolymarketOrderBuilder`])
/// - HTTP posting to the CLOB API with automatic retry on transient failures
#[derive(Debug, Clone)]
pub(crate) struct OrderSubmitter {
    http_client: PolymarketClobHttpClient,
    order_builder: Arc<PolymarketOrderBuilder>,
    retry_manager: Arc<RetryManager<Error>>,
    fee_rate_cache: Arc<DashMap<String, Decimal>>,
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
            fee_rate_cache: Arc::new(DashMap::new()),
        }
    }

    /// Returns the fee rate in basis points for a token, fetching from the API on cache miss.
    async fn get_fee_rate_bps(&self, token_id: &str) -> anyhow::Result<Decimal> {
        if let Some(rate) = self.fee_rate_cache.get(token_id) {
            return Ok(*rate);
        }

        let response = self
            .http_client
            .get_fee_rate(token_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch fee rate: {e}"))?;

        self.fee_rate_cache
            .insert(token_id.to_string(), response.base_fee);
        Ok(response.base_fee)
    }

    /// Builds a signed limit order and posts it with retry on transient failures.
    ///
    /// Converts Nautilus types to Polymarket types, calculates expiration,
    /// builds and signs the order, then submits via HTTP.
    #[allow(clippy::too_many_arguments)]
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
        let poly_order_type = PolymarketOrderType::try_from(time_in_force)
            .map_err(|e| anyhow::anyhow!("Unsupported time in force: {e}"))?;
        let poly_side = PolymarketOrderSide::try_from(side)
            .map_err(|e| anyhow::anyhow!("Invalid order side: {e}"))?;

        let expiration = match expire_time {
            Some(ns) if ns.as_u64() > 0 => {
                let secs = ns.as_u64() / 1_000_000_000;
                secs.to_string()
            }
            _ => "0".to_string(),
        };

        let fee_rate_bps = self.get_fee_rate_bps(token_id).await?;

        let poly_order = self
            .order_builder
            .build_limit_order(
                token_id,
                poly_side,
                price.as_decimal(),
                quantity.as_decimal(),
                &expiration,
                neg_risk,
                tick_decimals,
                fee_rate_bps,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let http_client = self.http_client.clone();

        self.retry_manager
            .execute_with_retry(
                "submit_limit_order",
                || {
                    let http_client = http_client.clone();
                    let poly_order = poly_order.clone();
                    async move {
                        http_client
                            .post_order(&poly_order, poly_order_type, post_only)
                            .await
                    }
                },
                |e| e.is_retryable(),
                Error::transport,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Fetches order book, calculates crossing price, builds and posts a market order.
    ///
    /// Converts Nautilus side to Polymarket side, walks the appropriate book side
    /// to find the crossing price, then builds and submits a FOK order.
    /// The book fetch is not retried (stale on retry); only the final POST is retried.
    /// Returns `(OrderResponse, expected_base_qty)` on success.
    pub async fn submit_market_order(
        &self,
        token_id: &str,
        side: OrderSide,
        amount: Quantity,
        neg_risk: bool,
        tick_decimals: u32,
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

        let fee_rate_bps = self.get_fee_rate_bps(token_id).await?;

        let poly_order = self
            .order_builder
            .build_market_order(
                token_id,
                poly_side,
                result.crossing_price,
                amount_dec,
                neg_risk,
                tick_decimals,
                fee_rate_bps,
            )
            .map_err(|e| anyhow::anyhow!("Failed to build market order: {e}"))?;

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

        Ok((response, result.expected_base_qty))
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
}
