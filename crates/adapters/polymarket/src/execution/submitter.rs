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

use std::sync::Arc;

use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    types::{Price, Quantity},
};

use super::{order_builder::PolymarketOrderBuilder, parse::calculate_market_price};
use crate::{
    common::enums::{PolymarketOrderSide, PolymarketOrderType},
    http::{
        clob::PolymarketClobHttpClient,
        query::{CancelResponse, OrderResponse},
    },
};

/// HTTP order submission and cancellation facade.
///
/// Provides a clean API accepting Nautilus-native types, internally handling:
/// - Side/TIF conversion to Polymarket types
/// - Expiration calculation
/// - Order building and EIP-712 signing (via [`PolymarketOrderBuilder`])
/// - HTTP posting to the CLOB API
#[derive(Debug, Clone)]
pub(crate) struct OrderSubmitter {
    http_client: PolymarketClobHttpClient,
    order_builder: Arc<PolymarketOrderBuilder>,
}

impl OrderSubmitter {
    pub fn new(
        http_client: PolymarketClobHttpClient,
        order_builder: Arc<PolymarketOrderBuilder>,
    ) -> Self {
        Self {
            http_client,
            order_builder,
        }
    }

    /// Builds a signed limit order and posts it.
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

        let poly_order = self
            .order_builder
            .build_limit_order(
                token_id,
                poly_side,
                price.as_decimal(),
                quantity.as_decimal(),
                &expiration,
                neg_risk,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        self.http_client
            .post_order(&poly_order, poly_order_type, post_only)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Fetches order book, calculates crossing price, builds and posts a market order.
    ///
    /// Converts Nautilus side to Polymarket side, walks the appropriate book side
    /// to find the crossing price, then builds and submits a FOK order.
    pub async fn submit_market_order(
        &self,
        token_id: &str,
        side: OrderSide,
        amount: Quantity,
        neg_risk: bool,
    ) -> anyhow::Result<OrderResponse> {
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

        let amount_f64: f64 = amount_dec.to_string().parse().unwrap_or(0.0);
        let price = calculate_market_price(levels, amount_f64, poly_side)
            .map_err(|e| anyhow::anyhow!("Market price calculation failed: {e}"))?;

        let poly_order = self
            .order_builder
            .build_market_order(token_id, poly_side, price, amount_dec, neg_risk)
            .map_err(|e| anyhow::anyhow!("Failed to build market order: {e}"))?;

        self.http_client
            .post_order(&poly_order, PolymarketOrderType::FOK, false)
            .await
            .map_err(|e| anyhow::anyhow!("HTTP request failed: {e}"))
    }

    /// Cancels a single order.
    pub async fn cancel_order(&self, venue_order_id: &str) -> anyhow::Result<CancelResponse> {
        self.http_client
            .cancel_order(venue_order_id)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Cancels multiple orders.
    pub async fn cancel_orders(&self, venue_order_ids: &[&str]) -> anyhow::Result<CancelResponse> {
        self.http_client
            .cancel_orders(venue_order_ids)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}
