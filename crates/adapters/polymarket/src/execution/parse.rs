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

//! Parsing functions for Polymarket execution reports.

use nautilus_core::{
    UUID4, UnixNanos,
    datetime::{NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND},
};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    instruments::InstrumentAny,
    reports::{FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        enums::{
            PolymarketEventType, PolymarketLiquiditySide, PolymarketOrderSide,
            PolymarketOrderStatus,
        },
        models::PolymarketMakerOrder,
    },
    http::models::{ClobBookLevel, PolymarketOpenOrder, PolymarketTradeReport},
};

/// Converts a [`PolymarketLiquiditySide`] to a Nautilus [`LiquiditySide`].
pub const fn parse_liquidity_side(side: PolymarketLiquiditySide) -> LiquiditySide {
    match side {
        PolymarketLiquiditySide::Maker => LiquiditySide::Maker,
        PolymarketLiquiditySide::Taker => LiquiditySide::Taker,
    }
}

/// Resolves the Nautilus order status from Polymarket status and event type.
///
/// Venue-initiated cancellations arrive as `status=Invalid, event_type=Cancellation`
/// (e.g. sport market resolution). These map to `Canceled`, not `Rejected`.
pub fn resolve_order_status(
    status: PolymarketOrderStatus,
    event_type: PolymarketEventType,
) -> OrderStatus {
    if status == PolymarketOrderStatus::Invalid && event_type == PolymarketEventType::Cancellation {
        OrderStatus::Canceled
    } else {
        OrderStatus::from(status)
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

    let ts_accepted = UnixNanos::from(order.created_at * NANOSECONDS_IN_SECOND);

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
/// derived from the Polymarket trade ID. Commission is computed from the
/// instrument's effective taker fee rate and the fill notional.
#[expect(clippy::too_many_arguments)]
pub fn parse_fill_report(
    trade: &PolymarketTradeReport,
    instrument_id: InstrumentId,
    account_id: AccountId,
    client_order_id: Option<ClientOrderId>,
    price_precision: u8,
    size_precision: u8,
    currency: Currency,
    taker_fee_rate: Decimal,
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

    let commission_value =
        compute_commission(taker_fee_rate, trade.size, trade.price, liquidity_side);
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
        avg_px: None,
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
/// share the same [`PolymarketMakerOrder`] type for maker fills. Maker fills never
/// pay commission per Polymarket's fee rules.
#[expect(clippy::too_many_arguments)]
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
    // Maker fills always pay zero commission per Polymarket docs:
    // https://docs.polymarket.com/trading/fees
    let commission_value =
        compute_commission(Decimal::ZERO, mo.matched_amount, mo.price, liquidity_side);

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
        avg_px: None,
        report_id: UUID4::new(),
        ts_event,
        ts_init,
        client_order_id: None,
        venue_position_id: None,
    }
}

/// Returns the effective taker fee rate for a Polymarket instrument.
///
/// Polymarket sets this from the Gamma market's `feeSchedule.rate`. When the
/// feeSchedule is unavailable (e.g. CLOB-only flow) the instrument's taker fee
/// defaults to zero and no commission is charged.
#[must_use]
pub fn instrument_taker_fee(instrument: &InstrumentAny) -> Decimal {
    match instrument {
        InstrumentAny::BinaryOption(bo) => bo.taker_fee,
        _ => Decimal::ZERO,
    }
}

/// Computes a USDC commission using Polymarket's fee formula.
///
/// `fee = C * feeRate * p * (1 - p)` where C is shares, feeRate is the effective
/// taker rate from the market's `feeSchedule`, and p is the share price. Fees peak
/// at p = 0.50 and decrease symmetrically toward the extremes. Only taker fills pay;
/// maker fills always return zero. Rounded to 5 decimal places (0.00001 USDC minimum).
///
/// The `fee_rate` here is the effective rate from `feeSchedule.rate` (e.g. 0.03 for
/// 3%), not the `fee_rate_bps` field on a trade or order. The latter is the maximum
/// fee cap used for order signing and is never the value actually charged.
///
/// # References
/// <https://docs.polymarket.com/trading/fees>
pub fn compute_commission(
    fee_rate: Decimal,
    size: Decimal,
    price: Decimal,
    liquidity_side: LiquiditySide,
) -> f64 {
    if liquidity_side != LiquiditySide::Taker || fee_rate.is_zero() {
        return 0.0;
    }

    let commission = size * fee_rate * price * (Decimal::ONE - price);
    let rounded = commission.round_dp(5);
    rounded.to_string().parse().unwrap_or(0.0)
}

/// USDC scale factor: the Polymarket API returns balances in micro-USDC (10^6 units).
const USDC_SCALE: Decimal = Decimal::from_parts(1_000_000, 0, 0, false, 0);

/// Converts a raw micro-USDC balance from the Polymarket API into an [`AccountBalance`].
///
/// The API returns balances as integer micro-USDC (e.g. `20000000` = 20 USDC).
/// This divides by 10^6 and constructs Money via `Money::from_decimal`, matching
/// the pattern used by dYdX, Deribit, OKX, and other adapters.
pub fn parse_balance_allowance(
    balance_raw: Decimal,
    currency: Currency,
) -> anyhow::Result<AccountBalance> {
    let balance_usdc = balance_raw / USDC_SCALE;
    AccountBalance::from_total_and_locked(balance_usdc, Decimal::ZERO, currency)
        .map_err(|e| anyhow::anyhow!("Failed to convert balance: {e}"))
}

/// Result of walking the order book to compute market order parameters.
#[derive(Debug)]
pub struct MarketPriceResult {
    /// The crossing price (worst level reached) for the signed CLOB order.
    pub crossing_price: Decimal,
    /// Expected base quantity (shares) computed by walking levels at actual prices.
    pub expected_base_qty: Decimal,
}

/// Calculates the market-crossing price and expected base quantity by walking the order book.
///
/// Sorts levels deterministically before walking:
/// - BUY (asks): ascending by price, best (lowest) ask first
/// - SELL (bids): descending by price, best (highest) bid first
///
/// This ensures correct results regardless of the CLOB API's response ordering.
///
/// For BUY: walks asks best-first, accumulates `size * price` (USDC) until >= amount.
///          Also accumulates the exact shares at each level for precise base qty.
/// For SELL: walks bids best-first, accumulates `size` (shares) until >= amount.
///
/// Returns the crossing price and expected base quantity. If insufficient liquidity,
/// uses all available levels. If the book side is empty, returns an error.
pub fn calculate_market_price(
    book_levels: &[ClobBookLevel],
    amount: Decimal,
    side: PolymarketOrderSide,
) -> anyhow::Result<MarketPriceResult> {
    if book_levels.is_empty() {
        anyhow::bail!("Empty order book: no liquidity available for market order");
    }

    // Parse and sort levels deterministically so we never depend on API ordering.
    // BUY: asks ascending (best/lowest first). SELL: bids descending (best/highest first).
    let mut parsed_levels: Vec<(Decimal, Decimal)> = book_levels
        .iter()
        .map(|l| {
            let price = Decimal::from_str_exact(&l.price).unwrap_or(Decimal::ZERO);
            let size = Decimal::from_str_exact(&l.size).unwrap_or(Decimal::ZERO);
            (price, size)
        })
        .filter(|(p, s)| !p.is_zero() && !s.is_zero())
        .collect();

    if parsed_levels.is_empty() {
        anyhow::bail!("Empty order book: no valid price levels for market order");
    }

    match side {
        PolymarketOrderSide::Buy => parsed_levels.sort_by_key(|a| a.0),
        PolymarketOrderSide::Sell => parsed_levels.sort_by(|a, b| b.0.cmp(&a.0)),
    }

    let mut remaining = amount;
    let mut last_price = Decimal::ZERO;
    let mut total_base_qty = Decimal::ZERO;

    for &(price, size) in &parsed_levels {
        last_price = price;

        match side {
            PolymarketOrderSide::Buy => {
                let level_usdc = size * price;
                let consumed_usdc = level_usdc.min(remaining);
                let shares_at_level = consumed_usdc / price;
                total_base_qty += shares_at_level;
                remaining -= consumed_usdc;
            }
            PolymarketOrderSide::Sell => {
                let consumed_shares = size.min(remaining);
                total_base_qty += consumed_shares;
                remaining -= consumed_shares;
            }
        }

        if remaining <= Decimal::ZERO {
            return Ok(MarketPriceResult {
                crossing_price: last_price,
                expected_base_qty: total_base_qty,
            });
        }
    }

    // Insufficient liquidity: return what we have (FOK will reject at venue)
    Ok(MarketPriceResult {
        crossing_price: last_price,
        expected_base_qty: total_base_qty,
    })
}

/// Parses a timestamp string into [`UnixNanos`].
///
/// Accepts millisecond integers ("1703875200000"), second integers ("1703875200"),
/// and RFC3339 strings ("2024-01-01T00:00:00Z").
pub fn parse_timestamp(ts_str: &str) -> Option<UnixNanos> {
    if let Ok(n) = ts_str.parse::<u64>() {
        return if n > 1_000_000_000_000 {
            Some(UnixNanos::from(n * NANOSECONDS_IN_MILLISECOND))
        } else {
            Some(UnixNanos::from(n * NANOSECONDS_IN_SECOND))
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
    #[case(dec!(20_000_000), 20.0)] // 20 USDC
    #[case(dec!(1_000_000), 1.0)] // 1 USDC
    #[case(dec!(500_000), 0.5)] // 0.5 USDC
    #[case(dec!(0), 0.0)] // zero
    #[case(dec!(123_456_789), 123.456789)] // fractional
    fn test_parse_balance_allowance(#[case] raw: Decimal, #[case] expected: f64) {
        let currency = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);
        let balance = parse_balance_allowance(raw, currency).unwrap();
        let total_f64: f64 = balance.total.as_decimal().to_string().parse().unwrap();
        assert!(
            (total_f64 - expected).abs() < 1e-8,
            "expected {expected}, was {total_f64}"
        );
        assert_eq!(balance.free, balance.total);
    }

    /// Polymarket fee formula: `fee = C * feeRate * p * (1 - p)`
    /// Rates are the category-specific taker rates from `feeSchedule.rate`.
    /// Reference: <https://docs.polymarket.com/trading/fees>
    #[rstest]
    #[case::crypto_p50("0.072", "0.50", 1.8)]
    #[case::crypto_p01("0.072", "0.01", 0.07128)]
    #[case::crypto_p05("0.072", "0.05", 0.342)]
    #[case::crypto_p10("0.072", "0.10", 0.648)]
    #[case::crypto_p30("0.072", "0.30", 1.512)]
    #[case::crypto_p70("0.072", "0.70", 1.512)]
    #[case::crypto_p90("0.072", "0.90", 0.648)]
    #[case::crypto_p99("0.072", "0.99", 0.07128)]
    #[case::sports_p50("0.03", "0.50", 0.75)]
    #[case::sports_p30("0.03", "0.30", 0.63)]
    #[case::sports_p70("0.03", "0.70", 0.63)]
    #[case::politics_p50("0.04", "0.50", 1.0)]
    #[case::politics_p30("0.04", "0.30", 0.84)]
    #[case::economics_p50("0.05", "0.50", 1.25)]
    #[case::economics_p30("0.05", "0.30", 1.05)]
    #[case::geopolitics_p50("0", "0.50", 0.0)]
    fn test_compute_commission_docs_table(
        #[case] fee_rate: &str,
        #[case] price: &str,
        #[case] expected: f64,
    ) {
        let commission = compute_commission(
            Decimal::from_str_exact(fee_rate).unwrap(),
            dec!(100),
            Decimal::from_str_exact(price).unwrap(),
            LiquiditySide::Taker,
        );
        assert!(
            (commission - expected).abs() < 1e-10,
            "at p={price}, fee_rate={fee_rate}: expected {expected}, was {commission}"
        );
    }

    #[rstest]
    fn test_compute_commission_issue_3860_strategy_buy() {
        // Issue #3860: strategy BUY fill
        // qty=15.463900, price=0.97, fee_rate=0.072
        // Expected: 15.4639 * 0.97 * 0.072 * (1 - 0.97) = 0.03240
        let commission = compute_commission(
            dec!(0.072),
            Decimal::from_str_exact("15.463900").unwrap(),
            dec!(0.97),
            LiquiditySide::Taker,
        );
        assert!(
            (commission - 0.03240).abs() < 1e-5,
            "expected 0.03240, was {commission}"
        );
    }

    #[rstest]
    fn test_compute_commission_issue_3860_reconciliation_sell() {
        // Issue #3860: reconciliation EXTERNAL SELL fill
        // qty=0.033400, price=0.98, fee_rate=0.072
        // Was 0.002357 with old generic formula (qty * price * fee_rate)
        // Correct: 0.0334 * 0.98 * 0.072 * (1 - 0.98) = 0.00005
        let commission = compute_commission(
            dec!(0.072),
            Decimal::from_str_exact("0.033400").unwrap(),
            dec!(0.98),
            LiquiditySide::Taker,
        );
        assert!(
            (commission - 0.00005).abs() < 1e-5,
            "expected 0.00005, was {commission}"
        );
    }

    #[rstest]
    fn test_compute_commission_maker_is_zero() {
        let commission = compute_commission(
            Decimal::from_str_exact("0.072").unwrap(),
            dec!(100),
            Decimal::from_str_exact("0.50").unwrap(),
            LiquiditySide::Maker,
        );
        assert_eq!(commission, 0.0);
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
        assert_eq!(
            report.ts_accepted,
            UnixNanos::from(1_703_875_200_000_000_000u64)
        );
        assert_eq!(
            report.ts_last,
            UnixNanos::from(1_703_875_200_000_000_000u64)
        );
        assert_eq!(report.ts_init, UnixNanos::from(1_000_000_000u64));
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
            Decimal::ZERO,
            UnixNanos::from(1_000_000_000u64),
        );

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(report.commission.as_f64(), 0.0);
    }

    #[rstest]
    fn test_parse_fill_report_forwards_taker_fee_rate() {
        let path = "test_data/http_trade_report.json";
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        let trade: PolymarketTradeReport =
            serde_json::from_str(&content).expect("Failed to parse test data");

        let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
        let account_id = AccountId::from("POLYMARKET-001");
        let currency = Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto);

        // Sports rate: 25 shares * 0.03 * 0.5 * 0.5 = 0.1875 USDC
        let report = parse_fill_report(
            &trade,
            instrument_id,
            account_id,
            None,
            4,
            6,
            currency,
            dec!(0.03),
            UnixNanos::from(1_000_000_000u64),
        );

        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert!((report.commission.as_f64() - 0.1875).abs() < 1e-10);
    }

    #[rstest]
    fn test_instrument_taker_fee_reads_binary_option() {
        use crate::http::parse::{create_instrument_from_def, parse_gamma_market};

        let path = "test_data/gamma_market_sports_market_money_line.json";
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        let market = serde_json::from_str(&content).expect("Failed to parse test data");
        let defs = parse_gamma_market(&market).unwrap();
        let instrument =
            create_instrument_from_def(&defs[0], UnixNanos::from(1_000_000_000u64)).unwrap();

        assert_eq!(instrument_taker_fee(&instrument), dec!(0.03));
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

    // Tests use various input orderings to prove the function sorts deterministically.

    #[rstest]
    fn test_calculate_market_price_buy_single_level() {
        let levels = vec![ClobBookLevel {
            price: "0.55".to_string(),
            size: "200.0".to_string(),
        }];
        let result = calculate_market_price(&levels, dec!(50), PolymarketOrderSide::Buy).unwrap();
        assert_eq!(result.crossing_price, dec!(0.55));
        // 50 USDC / 0.55 per share = ~90.909 shares
        assert!(result.expected_base_qty > dec!(90));
    }

    #[rstest]
    fn test_calculate_market_price_buy_walks_multiple_levels() {
        // Asks in arbitrary order, function sorts ascending for BUY
        let levels = vec![
            ClobBookLevel {
                price: "0.55".to_string(),
                size: "100.0".to_string(),
            },
            ClobBookLevel {
                price: "0.50".to_string(),
                size: "10.0".to_string(),
            },
            ClobBookLevel {
                price: "0.60".to_string(),
                size: "200.0".to_string(),
            },
        ];
        // Sorted ascending: 0.50/10, 0.55/100, 0.60/200
        // Walk: 0.50/10 → 5 USDC (10 shares), 0.55/100 → 15 USDC (27.27 shares)
        let result = calculate_market_price(&levels, dec!(20), PolymarketOrderSide::Buy).unwrap();
        assert_eq!(result.crossing_price, dec!(0.55));
        let expected = dec!(10) + dec!(15) / dec!(0.55);
        assert_eq!(result.expected_base_qty, expected);
    }

    #[rstest]
    fn test_calculate_market_price_buy_small_order_uses_best_ask() {
        // Asks in mixed order, function sorts to find best (0.20) first
        let levels = vec![
            ClobBookLevel {
                price: "0.50".to_string(),
                size: "50.0".to_string(),
            },
            ClobBookLevel {
                price: "0.999".to_string(),
                size: "100.0".to_string(),
            },
            ClobBookLevel {
                price: "0.20".to_string(),
                size: "72.0".to_string(),
            },
        ];
        // Sorted ascending: 0.20/72, 0.50/50, 0.999/100
        // 5 USDC at best ask 0.20: 72 * 0.20 = 14.4 USDC available, fills entirely
        let result = calculate_market_price(&levels, dec!(5), PolymarketOrderSide::Buy).unwrap();
        assert_eq!(result.crossing_price, dec!(0.20));
        assert_eq!(result.expected_base_qty, dec!(25)); // 5 / 0.20 = 25 shares
    }

    #[rstest]
    fn test_calculate_market_price_sell_walks_levels() {
        // Bids in ascending order, function sorts descending for SELL (best bid first)
        let levels = vec![
            ClobBookLevel {
                price: "0.48".to_string(),
                size: "100.0".to_string(),
            },
            ClobBookLevel {
                price: "0.50".to_string(),
                size: "50.0".to_string(),
            },
        ];
        // Sorted descending: 0.50/50, 0.48/100
        // Walk: 0.50 gives 50, need 30 more from 0.48 → fills
        let result = calculate_market_price(&levels, dec!(80), PolymarketOrderSide::Sell).unwrap();
        assert_eq!(result.crossing_price, dec!(0.48));
        assert_eq!(result.expected_base_qty, dec!(80));
    }

    #[rstest]
    fn test_calculate_market_price_empty_book() {
        let levels: Vec<ClobBookLevel> = vec![];
        let result = calculate_market_price(&levels, dec!(50), PolymarketOrderSide::Buy);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_calculate_market_price_all_zero_levels_returns_error() {
        let levels = vec![
            ClobBookLevel {
                price: "0".to_string(),
                size: "100.0".to_string(),
            },
            ClobBookLevel {
                price: "0.50".to_string(),
                size: "0".to_string(),
            },
        ];
        let result = calculate_market_price(&levels, dec!(50), PolymarketOrderSide::Buy);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_calculate_market_price_insufficient_liquidity_returns_worst() {
        let levels = vec![ClobBookLevel {
            price: "0.55".to_string(),
            size: "10.0".to_string(),
        }];
        // 10 * 0.55 = 5.5 USDC < 50 USDC needed, returns what's available
        let result = calculate_market_price(&levels, dec!(50), PolymarketOrderSide::Buy).unwrap();
        assert_eq!(result.crossing_price, dec!(0.55));
        assert_eq!(result.expected_base_qty, dec!(10)); // only 10 shares available
    }

    #[rstest]
    fn test_calculate_market_price_buy_order_independent_of_input_ordering() {
        let levels_ascending = vec![
            ClobBookLevel {
                price: "0.20".to_string(),
                size: "72.0".to_string(),
            },
            ClobBookLevel {
                price: "0.50".to_string(),
                size: "50.0".to_string(),
            },
            ClobBookLevel {
                price: "0.999".to_string(),
                size: "100.0".to_string(),
            },
        ];
        let levels_descending = vec![
            ClobBookLevel {
                price: "0.999".to_string(),
                size: "100.0".to_string(),
            },
            ClobBookLevel {
                price: "0.50".to_string(),
                size: "50.0".to_string(),
            },
            ClobBookLevel {
                price: "0.20".to_string(),
                size: "72.0".to_string(),
            },
        ];
        let levels_shuffled = vec![
            ClobBookLevel {
                price: "0.50".to_string(),
                size: "50.0".to_string(),
            },
            ClobBookLevel {
                price: "0.20".to_string(),
                size: "72.0".to_string(),
            },
            ClobBookLevel {
                price: "0.999".to_string(),
                size: "100.0".to_string(),
            },
        ];

        let r1 =
            calculate_market_price(&levels_ascending, dec!(20), PolymarketOrderSide::Buy).unwrap();
        let r2 =
            calculate_market_price(&levels_descending, dec!(20), PolymarketOrderSide::Buy).unwrap();
        let r3 =
            calculate_market_price(&levels_shuffled, dec!(20), PolymarketOrderSide::Buy).unwrap();

        assert_eq!(r1.crossing_price, r2.crossing_price);
        assert_eq!(r2.crossing_price, r3.crossing_price);
        assert_eq!(r1.expected_base_qty, r2.expected_base_qty);
        assert_eq!(r2.expected_base_qty, r3.expected_base_qty);
    }

    #[rstest]
    fn test_calculate_market_price_sell_order_independent_of_input_ordering() {
        let levels_a = vec![
            ClobBookLevel {
                price: "0.48".to_string(),
                size: "100.0".to_string(),
            },
            ClobBookLevel {
                price: "0.50".to_string(),
                size: "50.0".to_string(),
            },
        ];
        let levels_b = vec![
            ClobBookLevel {
                price: "0.50".to_string(),
                size: "50.0".to_string(),
            },
            ClobBookLevel {
                price: "0.48".to_string(),
                size: "100.0".to_string(),
            },
        ];

        let r1 = calculate_market_price(&levels_a, dec!(80), PolymarketOrderSide::Sell).unwrap();
        let r2 = calculate_market_price(&levels_b, dec!(80), PolymarketOrderSide::Sell).unwrap();

        assert_eq!(r1.crossing_price, r2.crossing_price);
        assert_eq!(r1.expected_base_qty, r2.expected_base_qty);
    }
}
