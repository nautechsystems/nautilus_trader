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

//! Order builder for the Polymarket CLOB exchange.
//!
//! Centralizes all order construction logic:
//! - Limit and market order building
//! - Order validation (side, TIF, quote_quantity)
//! - EIP-712 signing
//! - Maker/taker amount computation
//!
//! The builder produces signed [`PolymarketOrder`] structs ready for HTTP submission.

use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    orders::{Order, OrderAny},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::parse::{compute_maker_taker_amounts, compute_market_maker_taker_amounts};
use crate::{
    common::enums::{PolymarketOrderSide, PolymarketOrderType, SignatureType},
    http::models::PolymarketOrder,
    signing::eip712::OrderSigner,
};

/// Builds signed Polymarket orders for submission to the CLOB exchange.
#[derive(Debug)]
pub struct PolymarketOrderBuilder {
    order_signer: OrderSigner,
    signer_address: String,
    maker_address: String,
    signature_type: SignatureType,
}

impl PolymarketOrderBuilder {
    /// Creates a new [`PolymarketOrderBuilder`].
    pub fn new(
        order_signer: OrderSigner,
        signer_address: String,
        maker_address: String,
        signature_type: SignatureType,
    ) -> Self {
        Self {
            order_signer,
            signer_address,
            maker_address,
            signature_type,
        }
    }

    /// Builds and signs a limit order for submission.
    pub fn build_limit_order(
        &self,
        token_id: &str,
        side: PolymarketOrderSide,
        price: Decimal,
        quantity: Decimal,
        expiration: &str,
        neg_risk: bool,
    ) -> anyhow::Result<PolymarketOrder> {
        let (maker_amount, taker_amount) = compute_maker_taker_amounts(price, quantity, side);
        self.build_and_sign(
            token_id,
            side,
            maker_amount,
            taker_amount,
            expiration,
            neg_risk,
        )
    }

    /// Builds and signs a market order for submission.
    ///
    /// `amount` semantics differ by side:
    /// - BUY: `amount` is USDC to spend
    /// - SELL: `amount` is shares to sell
    pub fn build_market_order(
        &self,
        token_id: &str,
        side: PolymarketOrderSide,
        price: Decimal,
        amount: Decimal,
        neg_risk: bool,
    ) -> anyhow::Result<PolymarketOrder> {
        let (maker_amount, taker_amount) = compute_market_maker_taker_amounts(price, amount, side);
        // Market orders never expire
        self.build_and_sign(token_id, side, maker_amount, taker_amount, "0", neg_risk)
    }

    /// Validates a limit order before building, returning a denial reason if invalid.
    pub fn validate_limit_order(order: &OrderAny) -> Result<(), String> {
        if order.is_reduce_only() {
            return Err("Reduce-only orders not supported on Polymarket".to_string());
        }

        if order.order_type() != OrderType::Limit {
            return Err(format!(
                "Unsupported order type for Polymarket: {:?}",
                order.order_type()
            ));
        }

        if order.is_quote_quantity() {
            return Err("Quote quantity not supported for limit orders".to_string());
        }

        if order.price().is_none() {
            return Err("Limit orders require a price".to_string());
        }

        if PolymarketOrderType::try_from(order.time_in_force()).is_err() {
            return Err(format!(
                "Unsupported time in force: {:?}",
                order.time_in_force()
            ));
        }

        if order.is_post_only()
            && !matches!(order.time_in_force(), TimeInForce::Gtc | TimeInForce::Gtd)
        {
            return Err("Post-only orders require GTC or GTD time in force".to_string());
        }

        Ok(())
    }

    /// Validates a market order before building, returning a denial reason if invalid.
    pub fn validate_market_order(order: &OrderAny) -> Result<(), String> {
        if order.is_reduce_only() {
            return Err("Reduce-only orders not supported on Polymarket".to_string());
        }

        if order.order_type() != OrderType::Market {
            return Err(format!(
                "Expected Market order, was {:?}",
                order.order_type()
            ));
        }

        // BUY market orders must use quote_quantity (amount in USDC)
        // SELL market orders must NOT use quote_quantity (amount in shares)
        match order.order_side() {
            OrderSide::Buy => {
                if !order.is_quote_quantity() {
                    return Err(
                        "Market BUY orders require quote_quantity=true (amount in USDC)"
                            .to_string(),
                    );
                }
            }
            OrderSide::Sell => {
                if order.is_quote_quantity() {
                    return Err(
                        "Market SELL orders require quote_quantity=false (amount in shares)"
                            .to_string(),
                    );
                }
            }
            _ => {
                return Err(format!("Invalid order side: {:?}", order.order_side()));
            }
        }

        Ok(())
    }

    fn build_and_sign(
        &self,
        token_id: &str,
        side: PolymarketOrderSide,
        maker_amount: Decimal,
        taker_amount: Decimal,
        expiration: &str,
        neg_risk: bool,
    ) -> anyhow::Result<PolymarketOrder> {
        let salt = generate_salt();

        let mut poly_order = PolymarketOrder {
            salt,
            maker: self.maker_address.clone(),
            signer: self.signer_address.clone(),
            taker: "0x0000000000000000000000000000000000000000".to_string(),
            token_id: Ustr::from(token_id),
            maker_amount,
            taker_amount,
            expiration: expiration.to_string(),
            nonce: "0".to_string(),
            fee_rate_bps: Decimal::ZERO,
            side,
            signature_type: self.signature_type,
            signature: String::new(),
        };

        let signature = self
            .order_signer
            .sign_order(&poly_order, neg_risk)
            .map_err(|e| anyhow::anyhow!("EIP-712 signing failed: {e}"))?;
        poly_order.signature = signature;

        Ok(poly_order)
    }
}

/// Generates a random salt for order construction.
///
/// # Panics
///
/// Cannot panic: UUID v4 always produces 16 bytes, so the 8-byte slice conversion is infallible.
pub fn generate_salt() -> u64 {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    u64::from_le_bytes(bytes[..8].try_into().unwrap()) & ((1u64 << 53) - 1)
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{OrderSide, TimeInForce},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
        orders::{LimitOrder, MarketOrder, OrderAny},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn make_limit(
        reduce_only: bool,
        quote_quantity: bool,
        post_only: bool,
        tif: TimeInForce,
    ) -> OrderAny {
        let expire_time = if tif == TimeInForce::Gtd {
            Some(UnixNanos::from(2_000_000_000_000_000_000u64))
        } else {
            None
        };
        OrderAny::Limit(LimitOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("TEST.POLYMARKET"),
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            Quantity::from("10"),
            Price::from("0.50"),
            tif,
            expire_time,
            post_only,
            reduce_only,
            quote_quantity,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
        ))
    }

    fn make_market(side: OrderSide, quote_quantity: bool) -> OrderAny {
        OrderAny::Market(MarketOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("TEST.POLYMARKET"),
            ClientOrderId::from("O-001"),
            side,
            Quantity::from("10"),
            TimeInForce::Ioc,
            UUID4::new(),
            UnixNanos::default(),
            false,
            quote_quantity,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ))
    }

    #[rstest]
    fn test_validate_limit_order_valid() {
        let order = make_limit(false, false, false, TimeInForce::Gtc);
        assert!(PolymarketOrderBuilder::validate_limit_order(&order).is_ok());
    }

    #[rstest]
    fn test_validate_limit_order_reduce_only_denied() {
        let order = make_limit(true, false, false, TimeInForce::Gtc);
        let err = PolymarketOrderBuilder::validate_limit_order(&order).unwrap_err();
        assert!(err.contains("Reduce-only"));
    }

    #[rstest]
    fn test_validate_limit_order_quote_quantity_denied() {
        let order = make_limit(false, true, false, TimeInForce::Gtc);
        let err = PolymarketOrderBuilder::validate_limit_order(&order).unwrap_err();
        assert!(err.contains("Quote quantity"));
    }

    #[rstest]
    fn test_validate_limit_order_post_only_ioc_denied() {
        let order = make_limit(false, false, true, TimeInForce::Ioc);
        let err = PolymarketOrderBuilder::validate_limit_order(&order).unwrap_err();
        assert!(err.contains("Post-only"));
    }

    #[rstest]
    fn test_validate_limit_order_post_only_gtc_allowed() {
        let order = make_limit(false, false, true, TimeInForce::Gtc);
        assert!(PolymarketOrderBuilder::validate_limit_order(&order).is_ok());
    }

    #[rstest]
    fn test_validate_market_order_buy_with_quote_qty() {
        let order = make_market(OrderSide::Buy, true);
        assert!(PolymarketOrderBuilder::validate_market_order(&order).is_ok());
    }

    #[rstest]
    fn test_validate_market_order_buy_without_quote_qty_denied() {
        let order = make_market(OrderSide::Buy, false);
        let err = PolymarketOrderBuilder::validate_market_order(&order).unwrap_err();
        assert!(err.contains("quote_quantity=true"));
    }

    #[rstest]
    fn test_validate_market_order_sell_without_quote_qty() {
        let order = make_market(OrderSide::Sell, false);
        assert!(PolymarketOrderBuilder::validate_market_order(&order).is_ok());
    }

    #[rstest]
    fn test_validate_market_order_sell_with_quote_qty_denied() {
        let order = make_market(OrderSide::Sell, true);
        let err = PolymarketOrderBuilder::validate_market_order(&order).unwrap_err();
        assert!(err.contains("quote_quantity=false"));
    }

    #[rstest]
    fn test_validate_market_order_wrong_type_denied() {
        // Passing a limit order to validate_market_order should fail with type mismatch
        let limit = make_limit(false, false, false, TimeInForce::Gtc);
        let err = PolymarketOrderBuilder::validate_market_order(&limit).unwrap_err();
        assert!(err.contains("Expected Market order"));
    }

    #[rstest]
    fn test_generate_salt_uniqueness() {
        let s1 = generate_salt();
        let s2 = generate_salt();
        assert_ne!(s1, s2);
    }

    #[rstest]
    fn test_generate_salt_within_53_bits() {
        for _ in 0..100 {
            let s = generate_salt();
            assert!(s < (1u64 << 53));
        }
    }
}
