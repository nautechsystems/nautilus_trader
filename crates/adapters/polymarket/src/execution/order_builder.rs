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
//! Amounts are converted from human-readable decimals to on-chain base units
//! (USDC 10^6 / CTF shares 10^6) by truncating to `USDC_DECIMALS` (6) decimal
//! places and extracting the mantissa as an integer.
//!
//! The builder produces signed [`PolymarketOrder`] structs ready for HTTP submission.

use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    orders::{Order, OrderAny},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        consts::{LOT_SIZE_SCALE, USDC_DECIMALS},
        enums::{PolymarketOrderSide, PolymarketOrderType, SignatureType},
    },
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
    #[allow(clippy::too_many_arguments)]
    pub fn build_limit_order(
        &self,
        token_id: &str,
        side: PolymarketOrderSide,
        price: Decimal,
        quantity: Decimal,
        expiration: &str,
        neg_risk: bool,
        tick_decimals: u32,
        fee_rate_bps: Decimal,
    ) -> anyhow::Result<PolymarketOrder> {
        let (maker_amount, taker_amount) =
            compute_maker_taker_amounts(price, quantity, side, tick_decimals);
        self.build_and_sign(
            token_id,
            side,
            maker_amount,
            taker_amount,
            expiration,
            neg_risk,
            fee_rate_bps,
        )
    }

    /// Builds and signs a market order for submission.
    ///
    /// `amount` semantics differ by side:
    /// - BUY: `amount` is USDC to spend
    /// - SELL: `amount` is shares to sell
    #[allow(clippy::too_many_arguments)]
    pub fn build_market_order(
        &self,
        token_id: &str,
        side: PolymarketOrderSide,
        price: Decimal,
        amount: Decimal,
        neg_risk: bool,
        tick_decimals: u32,
        fee_rate_bps: Decimal,
    ) -> anyhow::Result<PolymarketOrder> {
        let (maker_amount, taker_amount) =
            compute_market_maker_taker_amounts(price, amount, side, tick_decimals);
        // Market orders never expire
        self.build_and_sign(
            token_id,
            side,
            maker_amount,
            taker_amount,
            "0",
            neg_risk,
            fee_rate_bps,
        )
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

    #[allow(clippy::too_many_arguments)]
    fn build_and_sign(
        &self,
        token_id: &str,
        side: PolymarketOrderSide,
        maker_amount: Decimal,
        taker_amount: Decimal,
        expiration: &str,
        neg_risk: bool,
        fee_rate_bps: Decimal,
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
            fee_rate_bps,
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

fn to_fixed_decimal(d: Decimal) -> Decimal {
    let mantissa = d.normalize().trunc_with_scale(USDC_DECIMALS).mantissa();
    Decimal::from(mantissa)
}

/// Builds the maker/taker amounts for a Polymarket CLOB limit order.
///
/// The CLOB enforces precision constraints on both amounts:
/// - Direct amounts (quantity passed through): max `LOT_SIZE_SCALE` (2) decimal places
/// - Computed amounts (quantity * price): max `tick_decimals + LOT_SIZE_SCALE` decimal places
///
/// For BUY: paying USDC (maker, computed) to receive CTF shares (taker, direct)
/// For SELL: paying CTF shares (maker, direct) to receive USDC (taker, computed)
pub fn compute_maker_taker_amounts(
    price: Decimal,
    quantity: Decimal,
    side: PolymarketOrderSide,
    tick_decimals: u32,
) -> (Decimal, Decimal) {
    let precision = tick_decimals + LOT_SIZE_SCALE;
    let qty = quantity.trunc_with_scale(LOT_SIZE_SCALE);
    match side {
        PolymarketOrderSide::Buy => {
            let maker_amount = to_fixed_decimal((qty * price).trunc_with_scale(precision));
            let taker_amount = to_fixed_decimal(qty);
            (maker_amount, taker_amount)
        }
        PolymarketOrderSide::Sell => {
            let maker_amount = to_fixed_decimal(qty);
            let taker_amount = to_fixed_decimal((qty * price).trunc_with_scale(precision));
            (maker_amount, taker_amount)
        }
    }
}

/// Builds maker/taker amounts for a Polymarket market order.
///
/// Same precision constraints as limit orders. The direct amount is truncated to
/// `LOT_SIZE_SCALE` decimal places before conversion (market order amounts like
/// position sizes from fills may have more decimal places than the CLOB allows).
///
/// Unlike limit orders where quantity always means shares, market order semantics differ by side:
/// - BUY: `amount` is USDC to spend, compute shares received
/// - SELL: `amount` is shares to sell, compute USDC received
pub fn compute_market_maker_taker_amounts(
    price: Decimal,
    amount: Decimal,
    side: PolymarketOrderSide,
    tick_decimals: u32,
) -> (Decimal, Decimal) {
    let precision = tick_decimals + LOT_SIZE_SCALE;
    let amt = amount.trunc_with_scale(LOT_SIZE_SCALE);
    match side {
        PolymarketOrderSide::Buy => {
            let maker_amount = to_fixed_decimal(amt);
            let taker_amount = to_fixed_decimal((amt / price).trunc_with_scale(precision));
            (maker_amount, taker_amount)
        }
        PolymarketOrderSide::Sell => {
            let maker_amount = to_fixed_decimal(amt);
            let taker_amount = to_fixed_decimal((amt * price).trunc_with_scale(precision));
            (maker_amount, taker_amount)
        }
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
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::common::enums::PolymarketOrderSide;

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

    // Limit order amount tests
    // qty truncated to LOT_SIZE_SCALE=2 first, then computed side truncated to precision
    #[rstest]
    #[case(dec!(0.50), dec!(100), PolymarketOrderSide::Buy, 2, dec!(50_000_000), dec!(100_000_000))]
    #[case(dec!(0.50), dec!(100), PolymarketOrderSide::Sell, 2, dec!(100_000_000), dec!(50_000_000))]
    #[case(dec!(0.75), dec!(200), PolymarketOrderSide::Buy, 2, dec!(150_000_000), dec!(200_000_000))]
    // qty=23.456 → trunc(2)=23.45, maker=(23.45*0.567).trunc(5)=13.29615→13_296_150
    #[case(dec!(0.567), dec!(23.456), PolymarketOrderSide::Buy, 3, dec!(13_296_150), dec!(23_450_000))]
    #[case(dec!(0.55), dec!(10), PolymarketOrderSide::Buy, 1, dec!(5_500_000), dec!(10_000_000))]
    fn test_compute_maker_taker_amounts(
        #[case] price: Decimal,
        #[case] quantity: Decimal,
        #[case] side: PolymarketOrderSide,
        #[case] tick_decimals: u32,
        #[case] expected_maker: Decimal,
        #[case] expected_taker: Decimal,
    ) {
        let (maker, taker) = compute_maker_taker_amounts(price, quantity, side, tick_decimals);
        assert_eq!(maker, expected_maker);
        assert_eq!(taker, expected_taker);
    }

    // Market order amount tests
    // amount truncated to LOT_SIZE_SCALE=2 first, then computed side truncated to precision
    #[rstest]
    #[case(dec!(0.50), dec!(50), PolymarketOrderSide::Buy, 2, dec!(50_000_000), dec!(100_000_000))]
    #[case(dec!(0.50), dec!(100), PolymarketOrderSide::Sell, 2, dec!(100_000_000), dec!(50_000_000))]
    #[case(dec!(0.75), dec!(150), PolymarketOrderSide::Buy, 2, dec!(150_000_000), dec!(200_000_000))]
    // amt=23.696681 → trunc(2)=23.69, taker=(23.69*0.211).trunc(5)=4.99859→4_998_590
    #[case(dec!(0.211), dec!(23.696681), PolymarketOrderSide::Sell, 3, dec!(23_690_000), dec!(4_998_590))]
    fn test_compute_market_maker_taker_amounts(
        #[case] price: Decimal,
        #[case] amount: Decimal,
        #[case] side: PolymarketOrderSide,
        #[case] tick_decimals: u32,
        #[case] expected_maker: Decimal,
        #[case] expected_taker: Decimal,
    ) {
        let (maker, taker) = compute_market_maker_taker_amounts(price, amount, side, tick_decimals);
        assert_eq!(maker, expected_maker);
        assert_eq!(taker, expected_taker);
    }

    #[rstest]
    fn test_compute_maker_taker_qty_truncated_to_lot_size() {
        // qty=23.456 → trunc(2)=23.45, tick_decimals=3 → precision=5
        // BUY: maker=(23.45*0.567).trunc(5)=13.29615→13_296_150, taker=23.45→23_450_000
        let (maker, taker) =
            compute_maker_taker_amounts(dec!(0.567), dec!(23.456), PolymarketOrderSide::Buy, 3);
        assert_eq!(maker, dec!(13_296_150));
        assert_eq!(taker, dec!(23_450_000));
    }

    #[rstest]
    fn test_compute_maker_taker_zero_amounts() {
        let (maker, taker) =
            compute_maker_taker_amounts(dec!(0.50), dec!(0), PolymarketOrderSide::Buy, 2);
        assert_eq!(maker, dec!(0));
        assert_eq!(taker, dec!(0));
    }

    #[rstest]
    fn test_compute_maker_taker_tick_decimals_1() {
        let (maker, taker) =
            compute_maker_taker_amounts(dec!(0.3), dec!(50), PolymarketOrderSide::Buy, 1);
        assert_eq!(maker, dec!(15_000_000));
        assert_eq!(taker, dec!(50_000_000));
    }

    #[rstest]
    fn test_compute_maker_taker_tick_decimals_3_sell() {
        // qty=15.123 → trunc(2)=15.12
        // SELL: maker=15.12→15_120_000, taker=(15.12*0.789).trunc(5)=11.92968→11_929_680
        let (maker, taker) =
            compute_maker_taker_amounts(dec!(0.789), dec!(15.123), PolymarketOrderSide::Sell, 3);
        assert_eq!(maker, dec!(15_120_000));
        assert_eq!(taker, dec!(11_929_680));
    }

    #[rstest]
    fn test_compute_market_maker_taker_zero_amount() {
        let (maker, taker) =
            compute_market_maker_taker_amounts(dec!(0.50), dec!(0), PolymarketOrderSide::Buy, 2);
        assert_eq!(maker, dec!(0));
        assert_eq!(taker, dec!(0));
    }

    #[rstest]
    fn test_compute_market_sell_position_close() {
        // Reproduces the live bug: position=23.696681 shares, price=0.211, tick_decimals=3
        // Without truncation: maker=23_696_681 (6 decimals) → rejected by CLOB
        // With truncation: amt=23.69 → maker=23_690_000 (2 decimals) → accepted
        let (maker, taker) = compute_market_maker_taker_amounts(
            dec!(0.211),
            dec!(23.696681),
            PolymarketOrderSide::Sell,
            3,
        );
        assert_eq!(maker, dec!(23_690_000));
        // 23.69 * 0.211 = 4.99859, trunc(5) = 4.99859
        assert_eq!(taker, dec!(4_998_590));
        // Verify maker has max 2 decimal precision: 23_690_000 / 1_000_000 = 23.69
        assert_eq!(maker % dec!(10_000), dec!(0));
        // Verify taker has max 5 decimal precision: 4_998_590 / 1_000_000 = 4.99859
        assert_eq!(taker % dec!(10), dec!(0));
    }

    #[rstest]
    fn test_to_fixed_decimal_basic() {
        assert_eq!(to_fixed_decimal(dec!(13.29955)), dec!(13_299_550));
        assert_eq!(to_fixed_decimal(dec!(100)), dec!(100_000_000));
        assert_eq!(to_fixed_decimal(dec!(0)), dec!(0));
    }
}
