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

//! Parsing functions for Polymarket execution reports and order building.

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        enums::{PolymarketLiquiditySide, PolymarketOrderSide},
        models::PolymarketMakerOrder,
    },
    http::models::{PolymarketOpenOrder, PolymarketTradeReport},
};

/// Converts a [`PolymarketLiquiditySide`] to a Nautilus [`LiquiditySide`].
pub const fn parse_liquidity_side(side: PolymarketLiquiditySide) -> LiquiditySide {
    match side {
        PolymarketLiquiditySide::Maker => LiquiditySide::Maker,
        PolymarketLiquiditySide::Taker => LiquiditySide::Taker,
    }
}

/// Determines the order side for a fill based on trader role and asset matching.
///
/// Polymarket uses a unified order book where complementary tokens (YES/NO) can match
/// across assets. A BUY YES can match with a BUY NO (cross-asset), not just SELL YES
/// (same-asset). For takers, the trade side is used directly. For makers, the side
/// depends on whether the match is cross-asset or same-asset.
pub fn determine_order_side(
    trader_side: PolymarketLiquiditySide,
    trade_side: PolymarketOrderSide,
    taker_asset_id: &str,
    maker_asset_id: &str,
) -> OrderSide {
    let order_side = OrderSide::from(trade_side);

    if trader_side == PolymarketLiquiditySide::Taker {
        return order_side;
    }

    let is_cross_asset = maker_asset_id != taker_asset_id;

    if is_cross_asset {
        order_side
    } else {
        match order_side {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
            other => other,
        }
    }
}

/// Creates a composite trade ID bounded to 36 characters.
///
/// When multiple orders are filled by a single market order, Polymarket sends one
/// trade message with a single ID for all fills. This creates a unique trade ID
/// per fill by combining the trade ID with part of the venue order ID.
///
/// Format: `{trade_id[..27]}-{venue_order_id[last 8]}` = 36 chars.
pub fn make_composite_trade_id(trade_id: &str, venue_order_id: &str) -> TradeId {
    let prefix_len = trade_id.len().min(27);
    let suffix_len = venue_order_id.len().min(8);
    let suffix_start = venue_order_id.len().saturating_sub(suffix_len);
    TradeId::from(
        format!(
            "{}-{}",
            &trade_id[..prefix_len],
            &venue_order_id[suffix_start..]
        )
        .as_str(),
    )
}

/// Parses a [`PolymarketOpenOrder`] into an [`OrderStatusReport`].
pub fn parse_order_status_report(
    order: &PolymarketOpenOrder,
    instrument_id: InstrumentId,
    account_id: AccountId,
    client_order_id: Option<ClientOrderId>,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> OrderStatusReport {
    let venue_order_id = VenueOrderId::from(order.id.as_str());
    let order_side = OrderSide::from(order.side);
    let time_in_force = TimeInForce::from(order.order_type);
    let order_status = OrderStatus::from(order.status);
    let quantity = Quantity::new(
        order.original_size.to_string().parse().unwrap_or(0.0),
        size_precision,
    );
    let filled_qty = Quantity::new(
        order.size_matched.to_string().parse().unwrap_or(0.0),
        size_precision,
    );
    let price = Price::new(
        order.price.to_string().parse().unwrap_or(0.0),
        price_precision,
    );

    let ts_accepted = UnixNanos::from(order.created_at * 1_000_000); // ms -> ns

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        order_side,
        OrderType::Limit,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_accepted, // ts_last
        ts_init,
        None, // report_id
    );
    report.price = Some(price);
    report
}

/// Parses a [`PolymarketTradeReport`] into a [`FillReport`].
///
/// Produces one fill report for the overall trade. The `trade_id` is
/// derived from the Polymarket trade ID. Commission is computed from
/// fee_rate_bps and the fill notional.
#[allow(clippy::too_many_arguments)]
pub fn parse_fill_report(
    trade: &PolymarketTradeReport,
    instrument_id: InstrumentId,
    account_id: AccountId,
    client_order_id: Option<ClientOrderId>,
    price_precision: u8,
    size_precision: u8,
    currency: Currency,
    ts_init: UnixNanos,
) -> FillReport {
    let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
    let trade_id = TradeId::from(trade.id.as_str());
    let order_side = OrderSide::from(trade.side);
    let last_qty = Quantity::new(
        trade.size.to_string().parse().unwrap_or(0.0),
        size_precision,
    );
    let last_px = Price::new(
        trade.price.to_string().parse().unwrap_or(0.0),
        price_precision,
    );
    let liquidity_side = parse_liquidity_side(trade.trader_side);

    let commission_value = compute_commission(trade.fee_rate_bps, trade.size, trade.price);
    let commission = Money::new(commission_value, currency);

    let ts_event = parse_timestamp(&trade.match_time).unwrap_or(ts_init);

    FillReport {
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        report_id: UUID4::new(),
        ts_event,
        ts_init,
        client_order_id,
        venue_position_id: None,
    }
}

/// Builds a [`FillReport`] from a [`PolymarketMakerOrder`] and trade-level context.
///
/// Used by both the WS stream handler and REST fill report generation since both
/// share the same [`PolymarketMakerOrder`] type for maker fills.
#[allow(clippy::too_many_arguments)]
pub fn build_maker_fill_report(
    mo: &PolymarketMakerOrder,
    trade_id: &str,
    trader_side: PolymarketLiquiditySide,
    trade_side: PolymarketOrderSide,
    taker_asset_id: &str,
    account_id: AccountId,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    currency: Currency,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> FillReport {
    let venue_order_id = VenueOrderId::from(mo.order_id.as_str());
    let fill_trade_id = make_composite_trade_id(trade_id, &mo.order_id);
    let order_side = determine_order_side(
        trader_side,
        trade_side,
        taker_asset_id,
        mo.asset_id.as_str(),
    );
    let last_qty = Quantity::new(
        mo.matched_amount.to_string().parse::<f64>().unwrap_or(0.0),
        size_precision,
    );
    let last_px = Price::new(
        mo.price.to_string().parse::<f64>().unwrap_or(0.0),
        price_precision,
    );
    let commission_value = compute_commission(mo.fee_rate_bps, mo.matched_amount, mo.price);

    FillReport {
        account_id,
        instrument_id,
        venue_order_id,
        trade_id: fill_trade_id,
        order_side,
        last_qty,
        last_px,
        commission: Money::new(commission_value, currency),
        liquidity_side,
        report_id: UUID4::new(),
        ts_event,
        ts_init,
        client_order_id: None,
        venue_position_id: None,
    }
}

/// Computes a USDC commission from fee basis points, size, and price.
///
/// `commission = size * price * fee_rate_bps / 10_000`
pub fn compute_commission(fee_rate_bps: Decimal, size: Decimal, price: Decimal) -> f64 {
    let bps = Decimal::new(10_000, 0);
    let commission = size * price * fee_rate_bps / bps;
    commission.to_string().parse().unwrap_or(0.0)
}

/// Builds the maker/taker amounts for a Polymarket CLOB order.
///
/// Returns `(maker_amount, taker_amount)` in on-chain base units (USDC 10^6 / CTF shares 10^6).
///
/// For BUY: paying USDC (maker) to receive CTF shares (taker)
///   - `maker_amount = qty * price * 10^6`
///   - `taker_amount = qty * 10^6`
///
/// For SELL: paying CTF shares (maker) to receive USDC (taker)
///   - `maker_amount = qty * 10^6`
///   - `taker_amount = qty * price * 10^6`
pub fn compute_maker_taker_amounts(
    price: Decimal,
    quantity: Decimal,
    side: PolymarketOrderSide,
) -> (Decimal, Decimal) {
    let scale = Decimal::new(1_000_000, 0);
    match side {
        PolymarketOrderSide::Buy => {
            let maker_amount = quantity * price * scale;
            let taker_amount = quantity * scale;
            (maker_amount, taker_amount)
        }
        PolymarketOrderSide::Sell => {
            let maker_amount = quantity * scale;
            let taker_amount = quantity * price * scale;
            (maker_amount, taker_amount)
        }
    }
}

/// Parses a timestamp string into [`UnixNanos`].
///
/// Accepts millisecond integers ("1703875200000"), second integers ("1703875200"),
/// and RFC3339 strings ("2024-01-01T00:00:00Z").
pub fn parse_timestamp(ts_str: &str) -> Option<UnixNanos> {
    if let Ok(n) = ts_str.parse::<u64>() {
        return if n > 1_000_000_000_000 {
            Some(UnixNanos::from(n * 1_000_000)) // milliseconds
        } else {
            Some(UnixNanos::from(n * 1_000_000_000)) // seconds
        };
    }
    let dt = chrono::DateTime::parse_from_rfc3339(ts_str).ok()?;
    Some(UnixNanos::from(dt.timestamp_nanos_opt()? as u64))
}

#[cfg(test)]
mod tests {
    use nautilus_model::enums::CurrencyType;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::common::enums::PolymarketOrderSide;

    #[rstest]
    #[case(dec!(0.50), dec!(100), PolymarketOrderSide::Buy, dec!(50_000_000), dec!(100_000_000))]
    #[case(dec!(0.50), dec!(100), PolymarketOrderSide::Sell, dec!(100_000_000), dec!(50_000_000))]
    #[case(dec!(0.75), dec!(200), PolymarketOrderSide::Buy, dec!(150_000_000), dec!(200_000_000))]
    fn test_compute_maker_taker_amounts(
        #[case] price: Decimal,
        #[case] quantity: Decimal,
        #[case] side: PolymarketOrderSide,
        #[case] expected_maker: Decimal,
        #[case] expected_taker: Decimal,
    ) {
        let (maker, taker) = compute_maker_taker_amounts(price, quantity, side);
        assert_eq!(maker, expected_maker);
        assert_eq!(taker, expected_taker);
    }

    #[rstest]
    fn test_compute_commission() {
        let commission = compute_commission(dec!(30), dec!(100), dec!(0.50));
        assert!((commission - 0.15).abs() < 1e-10);
    }

    #[rstest]
    fn test_compute_commission_zero_fee() {
        let commission = compute_commission(dec!(0), dec!(100), dec!(0.50));
        assert!((commission).abs() < 1e-10);
    }

    #[rstest]
    fn test_parse_timestamp_ms() {
        let ts = parse_timestamp("1703875200000").unwrap();
        assert_eq!(ts, UnixNanos::from(1_703_875_200_000_000_000u64));
    }

    #[rstest]
    fn test_parse_timestamp_secs() {
        let ts = parse_timestamp("1703875200").unwrap();
        assert_eq!(ts, UnixNanos::from(1_703_875_200_000_000_000u64));
    }

    #[rstest]
    fn test_parse_timestamp_rfc3339() {
        let ts = parse_timestamp("2024-01-01T00:00:00Z").unwrap();
        assert_eq!(ts, UnixNanos::from(1_704_067_200_000_000_000u64));
    }

    #[rstest]
    fn test_parse_liquidity_side_maker() {
        assert_eq!(
            parse_liquidity_side(PolymarketLiquiditySide::Maker),
            LiquiditySide::Maker
        );
    }

    #[rstest]
    fn test_parse_liquidity_side_taker() {
        assert_eq!(
            parse_liquidity_side(PolymarketLiquiditySide::Taker),
            LiquiditySide::Taker
        );
    }

    #[rstest]
    fn test_parse_order_status_report_from_fixture() {
        let path = "test_data/http_open_order.json";
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        let order: PolymarketOpenOrder =
            serde_json::from_str(&content).expect("Failed to parse test data");

        let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
        let account_id = AccountId::from("POLYMARKET-001");

        let report = parse_order_status_report(
            &order,
            instrument_id,
            account_id,
            None,
            4,
            6,
            UnixNanos::from(1_000_000_000u64),
        );

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(report.time_in_force, TimeInForce::Gtc);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert!(report.price.is_some());
    }

    #[rstest]
    fn test_parse_fill_report_from_fixture() {
        let path = "test_data/http_trade_report.json";
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        let trade: PolymarketTradeReport =
            serde_json::from_str(&content).expect("Failed to parse test data");

        let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
        let account_id = AccountId::from("POLYMARKET-001");
        let currency = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);

        let report = parse_fill_report(
            &trade,
            instrument_id,
            account_id,
            None,
            4,
            6,
            currency,
            UnixNanos::from(1_000_000_000u64),
        );

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
    }

    #[rstest]
    #[case(
        PolymarketLiquiditySide::Taker,
        PolymarketOrderSide::Buy,
        "token_a",
        "token_b",
        OrderSide::Buy
    )]
    #[case(
        PolymarketLiquiditySide::Taker,
        PolymarketOrderSide::Sell,
        "token_a",
        "token_b",
        OrderSide::Sell
    )]
    #[case(
        PolymarketLiquiditySide::Maker,
        PolymarketOrderSide::Buy,
        "token_a",
        "token_b",
        OrderSide::Buy
    )]
    #[case(
        PolymarketLiquiditySide::Maker,
        PolymarketOrderSide::Buy,
        "token_a",
        "token_a",
        OrderSide::Sell
    )]
    #[case(
        PolymarketLiquiditySide::Maker,
        PolymarketOrderSide::Sell,
        "token_a",
        "token_a",
        OrderSide::Buy
    )]
    fn test_determine_order_side(
        #[case] trader_side: PolymarketLiquiditySide,
        #[case] trade_side: PolymarketOrderSide,
        #[case] taker_asset: &str,
        #[case] maker_asset: &str,
        #[case] expected: OrderSide,
    ) {
        let result = determine_order_side(trader_side, trade_side, taker_asset, maker_asset);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_make_composite_trade_id_basic() {
        let trade_id = "trade-abc123";
        let venue_order_id = "order-xyz789";
        let result = make_composite_trade_id(trade_id, venue_order_id);
        assert_eq!(result.as_str(), "trade-abc123-r-xyz789");
    }

    #[rstest]
    fn test_make_composite_trade_id_truncates_long_ids() {
        let trade_id = "a]".repeat(30);
        let venue_order_id = "b".repeat(20);
        let result = make_composite_trade_id(&trade_id, &venue_order_id);
        assert!(result.as_str().len() <= 36);
    }

    #[rstest]
    fn test_make_composite_trade_id_short_venue_id() {
        let trade_id = "t123";
        let venue_order_id = "ab";
        let result = make_composite_trade_id(trade_id, venue_order_id);
        assert_eq!(result.as_str(), "t123-ab");
    }

    #[rstest]
    fn test_make_composite_trade_id_uniqueness() {
        let id_a = make_composite_trade_id("same-trade", "order-aaa");
        let id_b = make_composite_trade_id("same-trade", "order-bbb");
        assert_ne!(id_a, id_b);
    }
}
