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
//! (pUSD 10^6 / CTF shares 10^6) by truncating to `USDC_DECIMALS` (6) decimal
//! places and extracting the mantissa as an integer.
//!
//! The builder produces signed [`PolymarketOrder`] structs ready for HTTP submission.

use std::sync::atomic::{AtomicU64, Ordering};

use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    orders::{Order, OrderAny},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        consts::{LOT_SIZE_SCALE, POLYMARKET_NAUTILUS_BUILDER_CODE, USDC_DECIMALS},
        enums::{PolymarketOrderSide, PolymarketOrderType, SignatureType},
    },
    http::models::PolymarketOrder,
    signing::eip712::OrderSigner,
};

/// Zero `bytes32` used for the `metadata` field (reserved for future use).
pub const ZERO_BYTES32: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";

/// Builds signed Polymarket orders for submission to the CLOB V2 exchange.
///
/// `last_timestamp_ms` backs a strictly-monotonic millisecond clock so that
/// bursts of submissions landing in the same wall-clock millisecond still
/// produce distinct `timestamp` values (the V2 per-address uniqueness field).
#[derive(Debug)]
pub struct PolymarketOrderBuilder {
    order_signer: OrderSigner,
    signer_address: String,
    maker_address: String,
    signature_type: SignatureType,
    last_timestamp_ms: AtomicU64,
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
            last_timestamp_ms: AtomicU64::new(0),
        }
    }

    // Returns a strictly-monotonic millisecond timestamp: the current wall
    // time in ms, or `last_seen + 1` if that would be larger. Thread-safe.
    fn next_timestamp_ms(&self) -> u64 {
        let now_ms = get_atomic_clock_realtime().get_time_ns().as_u64() / 1_000_000;

        loop {
            let prev = self.last_timestamp_ms.load(Ordering::Relaxed);
            let candidate = prev.saturating_add(1).max(now_ms);

            if self
                .last_timestamp_ms
                .compare_exchange_weak(prev, candidate, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return candidate;
            }
        }
    }

    /// Builds and signs a limit order for submission.
    ///
    /// `expiration` is a unix-seconds timestamp (`"0"` for non-GTD orders).
    /// It is carried in the wire body but excluded from the EIP-712 signed hash.
    #[expect(clippy::too_many_arguments)]
    pub fn build_limit_order(
        &self,
        token_id: &str,
        side: PolymarketOrderSide,
        price: Decimal,
        quantity: Decimal,
        expiration: &str,
        neg_risk: bool,
        tick_decimals: u32,
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
        )
    }

    /// Builds and signs a market order for submission.
    ///
    /// `amount` semantics differ by side:
    /// - BUY: `amount` is pUSD to spend
    /// - SELL: `amount` is shares to sell
    ///
    /// Market orders never set an expiration.
    pub fn build_market_order(
        &self,
        token_id: &str,
        side: PolymarketOrderSide,
        price: Decimal,
        amount: Decimal,
        neg_risk: bool,
        tick_decimals: u32,
    ) -> anyhow::Result<PolymarketOrder> {
        let (maker_amount, taker_amount) =
            compute_market_maker_taker_amounts(price, amount, side, tick_decimals);
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

        if PolymarketOrderSide::try_from(order.order_side()).is_err() {
            return Err(format!("Invalid order side: {:?}", order.order_side()));
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

        // BUY market orders must use quote_quantity (amount in pUSD)
        // SELL market orders must NOT use quote_quantity (amount in shares)
        match order.order_side() {
            OrderSide::Buy => {
                if !order.is_quote_quantity() {
                    return Err(
                        "Market BUY orders require quote_quantity=true (amount in pUSD)"
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
        let timestamp_ms = self.next_timestamp_ms();

        let mut poly_order = PolymarketOrder {
            salt,
            maker: self.maker_address.clone(),
            signer: self.signer_address.clone(),
            token_id: Ustr::from(token_id),
            maker_amount,
            taker_amount,
            side,
            signature_type: self.signature_type,
            expiration: expiration.to_string(),
            timestamp: timestamp_ms.to_string(),
            metadata: ZERO_BYTES32.to_string(),
            builder: POLYMARKET_NAUTILUS_BUILDER_CODE.to_string(),
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
/// For BUY: paying pUSD (maker, computed) to receive CTF shares (taker, direct)
/// For SELL: paying CTF shares (maker, direct) to receive pUSD (taker, computed)
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
/// - BUY: `amount` is pUSD to spend, compute shares received
/// - SELL: `amount` is shares to sell, compute pUSD received
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
    fn test_validate_limit_order_no_order_side_denied() {
        let order = OrderAny::Limit(LimitOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("TEST.POLYMARKET"),
            ClientOrderId::from("O-NO-SIDE"),
            OrderSide::NoOrderSide,
            Quantity::from("10"),
            Price::from("0.50"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
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
        ));
        let err = PolymarketOrderBuilder::validate_limit_order(&order).unwrap_err();
        assert!(err.contains("Invalid order side"));
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

    fn make_test_builder() -> PolymarketOrderBuilder {
        use crate::{common::credential::EvmPrivateKey, signing::eip712::OrderSigner};
        let pk = EvmPrivateKey::new(
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
        )
        .unwrap();
        let signer = OrderSigner::new(&pk).unwrap();
        let addr = format!("{:#x}", signer.address());
        PolymarketOrderBuilder::new(signer, addr.clone(), addr, SignatureType::Eoa)
    }

    #[rstest]
    fn test_next_timestamp_ms_is_strictly_monotonic() {
        let builder = make_test_builder();
        let mut prev = builder.next_timestamp_ms();
        for _ in 0..1_000 {
            let next = builder.next_timestamp_ms();
            assert!(
                next > prev,
                "timestamp not strictly monotonic: {prev} >= {next}"
            );
            prev = next;
        }
    }

    #[rstest]
    fn test_build_orders_produce_unique_timestamps() {
        let builder = make_test_builder();
        let mut timestamps = ahash::AHashSet::new();

        for _ in 0..50 {
            let order = builder
                .build_limit_order(
                    "71321045679252212594626385532706912750332728571942532289631379312455583992563",
                    PolymarketOrderSide::Buy,
                    dec!(0.50),
                    dec!(10),
                    "0",
                    false,
                    2,
                )
                .unwrap();
            assert!(
                timestamps.insert(order.timestamp.clone()),
                "duplicate timestamp {} in burst",
                order.timestamp,
            );
        }
    }

    #[rstest]
    fn test_built_order_carries_nautilus_builder_code() {
        let builder = make_test_builder();
        let order = builder
            .build_limit_order(
                "71321045679252212594626385532706912750332728571942532289631379312455583992563",
                PolymarketOrderSide::Buy,
                dec!(0.50),
                dec!(10),
                "0",
                false,
                2,
            )
            .unwrap();
        assert_eq!(order.builder, POLYMARKET_NAUTILUS_BUILDER_CODE);
    }

    #[rstest]
    fn test_build_limit_order_expiration_passthrough() {
        let builder = make_test_builder();
        let order = builder
            .build_limit_order(
                "71321045679252212594626385532706912750332728571942532289631379312455583992563",
                PolymarketOrderSide::Buy,
                dec!(0.50),
                dec!(10),
                "1735689600",
                false,
                2,
            )
            .unwrap();
        assert_eq!(order.expiration, "1735689600");
    }

    #[rstest]
    fn test_validate_limit_order_gtd_with_expire_accepted() {
        // LimitOrder::new enforces GTD + expire_time upstream, so validate_limit_order
        // only needs to accept the valid case and let the lower layer reject mismatched
        // orders. Locking this behavior prevents a future regression where our
        // validator rejects GTD outright (which would break V2 GTD flows, as the V2
        // wire body carries expiration unsigned).
        let order = make_limit(false, false, false, TimeInForce::Gtd);
        assert!(PolymarketOrderBuilder::validate_limit_order(&order).is_ok());
    }

    #[rstest]
    fn test_build_market_buy_order_wire_shape() {
        // Market BUY: `amount` is quote pUSD, so maker_amount = quote spend
        // and taker_amount = computed shares.
        let builder = make_test_builder();
        let order = builder
            .build_market_order(
                "71321045679252212594626385532706912750332728571942532289631379312455583992563",
                PolymarketOrderSide::Buy,
                dec!(0.50),
                dec!(10),
                false,
                2,
            )
            .unwrap();

        assert_eq!(order.expiration, "0");
        assert_eq!(order.builder, POLYMARKET_NAUTILUS_BUILDER_CODE);
        assert_eq!(order.metadata, ZERO_BYTES32);
        assert_eq!(order.side, PolymarketOrderSide::Buy);
        assert!(!order.timestamp.is_empty());
        assert!(!order.signature.is_empty());
        // Wire amounts are micro-pUSD / micro-share mantissas (10^6 scale).
        // Quote-denominated BUY: 10 pUSD spend -> 20 shares at price 0.50.
        assert_eq!(order.maker_amount, dec!(10_000_000));
        assert_eq!(order.taker_amount, dec!(20_000_000));
    }

    #[rstest]
    fn test_build_market_sell_order_wire_shape() {
        // Market SELL: `amount` is shares, so maker_amount = shares
        // and taker_amount = computed pUSD proceeds.
        let builder = make_test_builder();
        let order = builder
            .build_market_order(
                "71321045679252212594626385532706912750332728571942532289631379312455583992563",
                PolymarketOrderSide::Sell,
                dec!(0.50),
                dec!(20),
                false,
                2,
            )
            .unwrap();

        assert_eq!(order.expiration, "0");
        assert_eq!(order.side, PolymarketOrderSide::Sell);
        assert_eq!(order.maker_amount, dec!(20_000_000));
        assert_eq!(order.taker_amount, dec!(10_000_000));
        assert!(!order.signature.is_empty());
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

    // SDK parity vectors lifted from `polymarket-rs-clob-client-v2`'s
    // `tests/order.rs::lifecycle::limit::{buy,sell}::should_succeed_*`. They
    // pin (price, size, tick_size) to specific signed maker/taker amounts;
    // any drift in our truncation or scale logic is caught against the
    // reference SDK directly. Covers all four documented tick sizes on both
    // sides plus a handful of decimal-accuracy edge cases.
    #[rstest]
    // tick=0.1 (decimals=1)
    #[case::buy_tick_tenth(
        dec!(0.5), dec!(21.04), PolymarketOrderSide::Buy, 1,
        dec!(10_520_000), dec!(21_040_000),
    )]
    #[case::sell_tick_tenth(
        dec!(0.5), dec!(21.04), PolymarketOrderSide::Sell, 1,
        dec!(21_040_000), dec!(10_520_000),
    )]
    // tick=0.01 (decimals=2)
    #[case::buy_tick_hundredth(
        dec!(0.56), dec!(21.04), PolymarketOrderSide::Buy, 2,
        dec!(11_782_400), dec!(21_040_000),
    )]
    #[case::sell_tick_hundredth(
        dec!(0.56), dec!(21.04), PolymarketOrderSide::Sell, 2,
        dec!(21_040_000), dec!(11_782_400),
    )]
    #[case::buy_decimal_accuracy_24(
        dec!(0.24), dec!(15), PolymarketOrderSide::Buy, 2,
        dec!(3_600_000), dec!(15_000_000),
    )]
    #[case::buy_decimal_accuracy_82(
        dec!(0.82), dec!(101), PolymarketOrderSide::Buy, 2,
        dec!(82_820_000), dec!(101_000_000),
    )]
    #[case::buy_decimal_accuracy_18233(
        dec!(0.58), dec!(18233.33), PolymarketOrderSide::Buy, 2,
        dec!(10_575_331_400), dec!(18_233_330_000),
    )]
    // tick=0.001 (decimals=3)
    #[case::buy_tick_thousandth(
        dec!(0.056), dec!(21.04), PolymarketOrderSide::Buy, 3,
        dec!(1_178_240), dec!(21_040_000),
    )]
    #[case::sell_tick_thousandth(
        dec!(0.056), dec!(21.04), PolymarketOrderSide::Sell, 3,
        dec!(21_040_000), dec!(1_178_240),
    )]
    // tick=0.0001 (decimals=4)
    #[case::buy_tick_ten_thousandth(
        dec!(0.0056), dec!(21.04), PolymarketOrderSide::Buy, 4,
        dec!(117_824), dec!(21_040_000),
    )]
    #[case::sell_tick_ten_thousandth(
        dec!(0.0056), dec!(21.04), PolymarketOrderSide::Sell, 4,
        dec!(21_040_000), dec!(117_824),
    )]
    fn test_compute_maker_taker_amounts_sdk_parity(
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
