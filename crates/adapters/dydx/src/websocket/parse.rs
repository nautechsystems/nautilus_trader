// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Parsing utilities for dYdX WebSocket messages.
//!
//! Converts WebSocket-specific message formats into Nautilus domain types
//! by transforming them into HTTP-equivalent structures and delegating to
//! the HTTP parser for consistency.

use std::str::FromStr;

use anyhow::Context;
use chrono::Utc;
use dashmap::DashMap;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{OrderSide, OrderStatus},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
};
use rust_decimal::Decimal;

use crate::{http::models::Order, schemas::ws::DydxWsOrderSubaccountMessageContents};

/// Parses a WebSocket order update into an OrderStatusReport.
///
/// Converts the WebSocket order format to the HTTP Order format, then delegates
/// to the existing HTTP parser for consistency.
///
/// # Errors
///
/// Returns an error if:
/// - clob_pair_id cannot be parsed from string
/// - Instrument lookup fails for the clob_pair_id
/// - Field parsing fails (price, size, etc.)
/// - HTTP parser fails
pub fn parse_ws_order_report(
    ws_order: &DydxWsOrderSubaccountMessageContents,
    clob_pair_id_to_instrument: &DashMap<u32, InstrumentId>,
    instruments: &DashMap<InstrumentId, InstrumentAny>,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    // Parse clob_pair_id from string
    let clob_pair_id: u32 = ws_order.clob_pair_id.parse().context(format!(
        "Failed to parse clob_pair_id '{}'",
        ws_order.clob_pair_id
    ))?;

    // Lookup instrument by clob_pair_id
    let instrument_id = *clob_pair_id_to_instrument
        .get(&clob_pair_id)
        .ok_or_else(|| {
            let available: Vec<u32> = clob_pair_id_to_instrument
                .iter()
                .map(|entry| *entry.key())
                .collect();
            anyhow::anyhow!(
                "No instrument cached for clob_pair_id {clob_pair_id}. Available: {available:?}"
            )
        })?
        .value();

    let instrument = instruments
        .get(&instrument_id)
        .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found in cache"))?
        .value()
        .clone();

    // Convert WebSocket order to HTTP Order format
    let http_order = convert_ws_order_to_http(ws_order)?;

    // Delegate to existing HTTP parser
    let mut report = crate::http::parse::parse_order_status_report(
        &http_order,
        &instrument,
        account_id,
        ts_init,
    )?;

    // For untriggered conditional orders with an explicit trigger price we
    // surface `PendingUpdate` to match Nautilus semantics and existing dYdX
    // enum mapping.
    if matches!(
        ws_order.status,
        crate::common::enums::DydxOrderStatus::Untriggered
    ) && ws_order.trigger_price.is_some()
    {
        report.order_status = OrderStatus::PendingUpdate;
    }

    Ok(report)
}

/// Converts a WebSocket order message to HTTP Order format.
///
/// # Errors
///
/// Returns an error if any field parsing fails.
fn convert_ws_order_to_http(
    ws_order: &DydxWsOrderSubaccountMessageContents,
) -> anyhow::Result<Order> {
    // Parse numeric fields
    let clob_pair_id: u32 = ws_order
        .clob_pair_id
        .parse()
        .context("Failed to parse clob_pair_id")?;

    let size: Decimal = ws_order.size.parse().context("Failed to parse size")?;

    let total_filled: Decimal = ws_order
        .total_filled
        .parse()
        .context("Failed to parse total_filled")?;

    // Saturate to zero if total_filled exceeds size (edge case: rounding or partial fills)
    let remaining_size = (size - total_filled).max(Decimal::ZERO);

    let price: Decimal = ws_order.price.parse().context("Failed to parse price")?;

    let created_at_height: u64 = ws_order
        .created_at_height
        .parse()
        .context("Failed to parse created_at_height")?;

    let order_flags: u32 = ws_order
        .order_flags
        .parse()
        .context("Failed to parse order_flags")?;

    let client_metadata: u32 = ws_order
        .client_metadata
        .parse()
        .context("Failed to parse client_metadata")?;

    // Parse optional fields
    let good_til_block = ws_order
        .good_til_block
        .as_ref()
        .and_then(|s| s.parse::<u64>().ok());

    let good_til_block_time = ws_order
        .good_til_block_time
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    let trigger_price = ws_order
        .trigger_price
        .as_ref()
        .and_then(|s| Decimal::from_str(s).ok());

    // Parse updated_at (optional for BEST_EFFORT_OPENED orders)
    let updated_at = ws_order
        .updated_at
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    // Parse updated_at_height (optional for BEST_EFFORT_OPENED orders)
    let updated_at_height = ws_order
        .updated_at_height
        .as_ref()
        .and_then(|s| s.parse::<u64>().ok());

    // Convert order type to string using Display (gives PascalCase like "Limit", "Market")
    let order_type = ws_order.order_type.to_string();

    // Calculate total filled from size - remaining_size
    let total_filled = size.checked_sub(remaining_size).unwrap_or(Decimal::ZERO);

    Ok(Order {
        id: ws_order.id.clone(),
        subaccount_id: ws_order.subaccount_id.clone(),
        client_id: ws_order.client_id.clone(),
        clob_pair_id,
        side: ws_order.side,
        size,
        total_filled,
        price,
        status: ws_order.status,
        order_type,
        time_in_force: ws_order.time_in_force,
        reduce_only: ws_order.reduce_only,
        post_only: ws_order.post_only,
        order_flags,
        good_til_block,
        good_til_block_time,
        created_at_height: Some(created_at_height),
        client_metadata,
        trigger_price,
        condition_type: None, // Not provided in WebSocket messages
        conditional_order_trigger_subticks: None, // Not provided in WebSocket messages
        execution: None,      // Inferred from post_only flag by HTTP parser
        updated_at,
        updated_at_height,
        ticker: None,               // Not provided in WebSocket messages
        subaccount_number: 0,       // Default to 0 for WebSocket messages
        order_router_address: None, // Not provided in WebSocket messages
    })
}

/// Parses a WebSocket fill update into a FillReport.
///
/// Converts the WebSocket fill format to the HTTP Fill format, then delegates
/// to the existing HTTP parser for consistency.
///
/// # Errors
///
/// Returns an error if:
/// - Instrument lookup fails for the market symbol
/// - Field parsing fails (price, size, fee, etc.)
/// - HTTP parser fails
pub fn parse_ws_fill_report(
    ws_fill: &crate::schemas::ws::DydxWsFillSubaccountMessageContents,
    instruments: &DashMap<InstrumentId, InstrumentAny>,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    // Lookup instrument by market symbol
    let instrument = instruments
        .iter()
        .find(|entry| entry.value().id().symbol.as_str() == ws_fill.market.as_str())
        .ok_or_else(|| {
            let available: Vec<String> = instruments
                .iter()
                .map(|entry| entry.value().id().symbol.to_string())
                .collect();
            anyhow::anyhow!(
                "No instrument cached for market '{}'. Available: {:?}",
                ws_fill.market,
                available
            )
        })?
        .value()
        .clone();

    // Convert WebSocket fill to HTTP Fill format
    let http_fill = convert_ws_fill_to_http(ws_fill)?;

    // Delegate to existing HTTP parser
    crate::http::parse::parse_fill_report(&http_fill, &instrument, account_id, ts_init)
}

/// Converts a WebSocket fill message to HTTP Fill format.
///
/// # Errors
///
/// Returns an error if any field parsing fails.
fn convert_ws_fill_to_http(
    ws_fill: &crate::schemas::ws::DydxWsFillSubaccountMessageContents,
) -> anyhow::Result<crate::http::models::Fill> {
    use crate::http::models::Fill;

    // Parse numeric fields
    let price: Decimal = ws_fill.price.parse().context("Failed to parse price")?;

    let size: Decimal = ws_fill.size.parse().context("Failed to parse size")?;

    let fee: Decimal = ws_fill.fee.parse().context("Failed to parse fee")?;

    let created_at_height: u64 = ws_fill
        .created_at_height
        .parse()
        .context("Failed to parse created_at_height")?;

    let client_metadata: u32 = ws_fill
        .client_metadata
        .parse()
        .context("Failed to parse client_metadata")?;

    // Parse timestamp
    let created_at = chrono::DateTime::parse_from_rfc3339(&ws_fill.created_at)
        .context("Failed to parse created_at")?
        .with_timezone(&Utc);

    Ok(Fill {
        id: ws_fill.id.clone(),
        side: ws_fill.side,
        liquidity: ws_fill.liquidity,
        fill_type: ws_fill.fill_type,
        market: ws_fill.market.to_string(),
        market_type: ws_fill.market_type,
        price,
        size,
        fee,
        created_at,
        created_at_height,
        order_id: ws_fill.order_id.clone(),
        client_metadata,
    })
}

/// Parses a WebSocket position into a PositionStatusReport.
///
/// Converts the WebSocket position format to the HTTP PerpetualPosition format,
/// then delegates to the existing HTTP parser for consistency.
///
/// # Errors
///
/// Returns an error if:
/// - Instrument lookup fails for the market symbol
/// - Field parsing fails (size, prices, etc.)
/// - HTTP parser fails
pub fn parse_ws_position_report(
    ws_position: &crate::schemas::ws::DydxPerpetualPosition,
    instruments: &DashMap<InstrumentId, InstrumentAny>,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    // Lookup instrument by market symbol
    let instrument = instruments
        .iter()
        .find(|entry| entry.value().id().symbol.as_str() == ws_position.market.as_str())
        .ok_or_else(|| {
            let available: Vec<String> = instruments
                .iter()
                .map(|entry| entry.value().id().symbol.to_string())
                .collect();
            anyhow::anyhow!(
                "No instrument cached for market '{}'. Available: {:?}",
                ws_position.market,
                available
            )
        })?
        .value()
        .clone();

    // Convert WebSocket position to HTTP PerpetualPosition format
    let http_position = convert_ws_position_to_http(ws_position)?;

    // Delegate to existing HTTP parser
    crate::http::parse::parse_position_status_report(
        &http_position,
        &instrument,
        account_id,
        ts_init,
    )
}

/// Converts a WebSocket position to HTTP PerpetualPosition format.
///
/// # Errors
///
/// Returns an error if any field parsing fails.
fn convert_ws_position_to_http(
    ws_position: &crate::schemas::ws::DydxPerpetualPosition,
) -> anyhow::Result<crate::http::models::PerpetualPosition> {
    use crate::http::models::PerpetualPosition;

    // Parse numeric fields
    let size: Decimal = ws_position.size.parse().context("Failed to parse size")?;

    let max_size: Decimal = ws_position
        .max_size
        .parse()
        .context("Failed to parse max_size")?;

    let entry_price: Decimal = ws_position
        .entry_price
        .parse()
        .context("Failed to parse entry_price")?;

    let exit_price: Option<Decimal> = ws_position
        .exit_price
        .as_ref()
        .map(|s| s.parse())
        .transpose()
        .context("Failed to parse exit_price")?;

    let realized_pnl: Decimal = ws_position
        .realized_pnl
        .parse()
        .context("Failed to parse realized_pnl")?;

    let unrealized_pnl: Decimal = ws_position
        .unrealized_pnl
        .parse()
        .context("Failed to parse unrealized_pnl")?;

    let sum_open: Decimal = ws_position
        .sum_open
        .parse()
        .context("Failed to parse sum_open")?;

    let sum_close: Decimal = ws_position
        .sum_close
        .parse()
        .context("Failed to parse sum_close")?;

    let net_funding: Decimal = ws_position
        .net_funding
        .parse()
        .context("Failed to parse net_funding")?;

    // Parse timestamps
    let created_at = chrono::DateTime::parse_from_rfc3339(&ws_position.created_at)
        .context("Failed to parse created_at")?
        .with_timezone(&Utc);

    let closed_at = ws_position
        .closed_at
        .as_ref()
        .map(|s| chrono::DateTime::parse_from_rfc3339(s))
        .transpose()
        .context("Failed to parse closed_at")?
        .map(|dt| dt.with_timezone(&Utc));

    // Determine side from size sign (HTTP format uses OrderSide, not PositionSide)
    let side = if size.is_sign_positive() {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };

    Ok(PerpetualPosition {
        market: ws_position.market.to_string(),
        status: ws_position.status,
        side,
        size,
        max_size,
        entry_price,
        exit_price,
        realized_pnl,
        created_at_height: 0, // Not provided in WebSocket messages
        created_at,
        sum_open,
        sum_close,
        net_funding,
        unrealized_pnl,
        closed_at,
    })
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderType, PositionSideSpecified},
        identifiers::{AccountId, InstrumentId, Symbol, Venue},
        instruments::CryptoPerpetual,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::common::enums::{DydxOrderStatus, DydxOrderType, DydxTimeInForce};

    fn create_test_instrument() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));

        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            Symbol::new("BTC-USD"),
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false,
            2,
            8,
            Price::new(0.01, 2),
            Quantity::new(0.001, 8),
            Some(Quantity::new(1.0, 0)),
            Some(Quantity::new(0.001, 8)),
            Some(Quantity::new(100000.0, 8)),
            Some(Quantity::new(0.001, 8)),
            None,
            None,
            Some(Price::new(1000000.0, 2)),
            Some(Price::new(0.01, 2)),
            Some(rust_decimal_macros::dec!(0.05)),
            Some(rust_decimal_macros::dec!(0.03)),
            Some(rust_decimal_macros::dec!(0.0002)),
            Some(rust_decimal_macros::dec!(0.0005)),
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[rstest]
    fn test_convert_ws_order_to_http_basic() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "order123".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "12345".to_string(),
            clob_pair_id: "1".to_string(),
            side: OrderSide::Buy,
            size: "1.5".to_string(),
            price: "50000.0".to_string(),
            status: DydxOrderStatus::PartiallyFilled,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            post_only: false,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("1000".to_string()),
            good_til_block_time: None,
            created_at_height: "900".to_string(),
            client_metadata: "0".to_string(),
            trigger_price: None,
            total_filled: "0.5".to_string(),
            updated_at: Some("2024-11-14T10:00:00Z".to_string()),
            updated_at_height: Some("950".to_string()),
        };

        let result = convert_ws_order_to_http(&ws_order);
        assert!(result.is_ok());

        let http_order = result.unwrap();
        assert_eq!(http_order.id, "order123");
        assert_eq!(http_order.clob_pair_id, 1);
        assert_eq!(http_order.size.to_string(), "1.5");
        assert_eq!(http_order.total_filled, rust_decimal_macros::dec!(0.5)); // 0.5 filled
        assert_eq!(http_order.status, DydxOrderStatus::PartiallyFilled);
    }

    #[rstest]
    fn test_parse_ws_order_report_success() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "order456".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "67890".to_string(),
            clob_pair_id: "1".to_string(),
            side: OrderSide::Sell,
            size: "2.0".to_string(),
            price: "51000.0".to_string(),
            status: DydxOrderStatus::Open,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            post_only: true,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("2000".to_string()),
            good_til_block_time: None,
            created_at_height: "1800".to_string(),
            client_metadata: "0".to_string(),
            trigger_price: None,
            total_filled: "0.0".to_string(),
            updated_at: None,
            updated_at_height: None,
        };

        let clob_pair_id_to_instrument = DashMap::new();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
        clob_pair_id_to_instrument.insert(1, instrument_id);

        let instruments = DashMap::new();
        let instrument = create_test_instrument();
        instruments.insert(instrument_id, instrument);

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_order_report(
            &ws_order,
            &clob_pair_id_to_instrument,
            &instruments,
            account_id,
            ts_init,
        );

        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.account_id, account_id);
        assert_eq!(report.order_side, OrderSide::Sell);
    }

    #[rstest]
    fn test_parse_ws_order_report_missing_instrument() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "order789".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "11111".to_string(),
            clob_pair_id: "99".to_string(), // Non-existent
            side: OrderSide::Buy,
            size: "1.0".to_string(),
            price: "50000.0".to_string(),
            status: DydxOrderStatus::Open,
            order_type: DydxOrderType::Market,
            time_in_force: DydxTimeInForce::Ioc,
            post_only: false,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("1000".to_string()),
            good_til_block_time: None,
            created_at_height: "900".to_string(),
            client_metadata: "0".to_string(),
            trigger_price: None,
            total_filled: "0.0".to_string(),
            updated_at: None,
            updated_at_height: None,
        };

        let clob_pair_id_to_instrument = DashMap::new();
        let instruments = DashMap::new();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_order_report(
            &ws_order,
            &clob_pair_id_to_instrument,
            &instruments,
            account_id,
            ts_init,
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No instrument cached")
        );
    }

    // ========== Fill Parsing Tests ==========

    #[rstest]
    fn test_convert_ws_fill_to_http() {
        use crate::{
            common::enums::{DydxFillType, DydxLiquidity, DydxTickerType},
            schemas::ws::DydxWsFillSubaccountMessageContents,
        };

        let ws_fill = DydxWsFillSubaccountMessageContents {
            id: "fill123".to_string(),
            subaccount_id: "sub1".to_string(),
            side: OrderSide::Buy,
            liquidity: DydxLiquidity::Maker,
            fill_type: DydxFillType::Limit,
            market: "BTC-USD".into(),
            market_type: DydxTickerType::Perpetual,
            price: "50000.5".to_string(),
            size: "0.1".to_string(),
            fee: "-2.5".to_string(), // Negative for maker rebate
            created_at: "2024-01-15T10:30:00Z".to_string(),
            created_at_height: "12345".to_string(),
            order_id: "order456".to_string(),
            client_metadata: "999".to_string(),
        };

        let result = convert_ws_fill_to_http(&ws_fill);
        assert!(result.is_ok());

        let http_fill = result.unwrap();
        assert_eq!(http_fill.id, "fill123");
        assert_eq!(http_fill.side, OrderSide::Buy);
        assert_eq!(http_fill.liquidity, DydxLiquidity::Maker);
        assert_eq!(http_fill.price, rust_decimal_macros::dec!(50000.5));
        assert_eq!(http_fill.size, rust_decimal_macros::dec!(0.1));
        assert_eq!(http_fill.fee, rust_decimal_macros::dec!(-2.5));
        assert_eq!(http_fill.created_at_height, 12345);
        assert_eq!(http_fill.order_id, "order456");
        assert_eq!(http_fill.client_metadata, 999);
    }

    #[rstest]
    fn test_parse_ws_fill_report_success() {
        use crate::{
            common::enums::{DydxFillType, DydxLiquidity, DydxTickerType},
            schemas::ws::DydxWsFillSubaccountMessageContents,
        };

        let instrument = create_test_instrument();
        let instrument_id = instrument.id();

        let instruments = DashMap::new();
        instruments.insert(instrument_id, instrument);

        let ws_fill = DydxWsFillSubaccountMessageContents {
            id: "fill789".to_string(),
            subaccount_id: "sub1".to_string(),
            side: OrderSide::Sell,
            liquidity: DydxLiquidity::Taker,
            fill_type: DydxFillType::Limit,
            market: "BTC-USD-PERP".into(),
            market_type: DydxTickerType::Perpetual,
            price: "49500.0".to_string(),
            size: "0.5".to_string(),
            fee: "12.375".to_string(), // Positive for taker fee
            created_at: "2024-01-15T11:00:00Z".to_string(),
            created_at_height: "12400".to_string(),
            order_id: "order999".to_string(),
            client_metadata: "888".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_fill_report(&ws_fill, &instruments, account_id, ts_init);
        assert!(result.is_ok());

        let fill_report = result.unwrap();
        assert_eq!(fill_report.instrument_id, instrument_id);
        assert_eq!(fill_report.venue_order_id.as_str(), "order999");
        assert_eq!(fill_report.last_qty.as_f64(), 0.5);
        assert_eq!(fill_report.last_px.as_f64(), 49500.0);
        // Commission should be negative (cost to trader) after negating positive fee
        assert!((fill_report.commission.as_f64() + 12.38).abs() < 0.01);
    }

    #[rstest]
    fn test_parse_ws_fill_report_missing_instrument() {
        use crate::{
            common::enums::{DydxFillType, DydxLiquidity, DydxTickerType},
            schemas::ws::DydxWsFillSubaccountMessageContents,
        };

        let instruments = DashMap::new(); // Empty - no instruments cached

        let ws_fill = DydxWsFillSubaccountMessageContents {
            id: "fill000".to_string(),
            subaccount_id: "sub1".to_string(),
            side: OrderSide::Buy,
            liquidity: DydxLiquidity::Maker,
            fill_type: DydxFillType::Limit,
            market: "ETH-USD-PERP".into(),
            market_type: DydxTickerType::Perpetual,
            price: "3000.0".to_string(),
            size: "1.0".to_string(),
            fee: "-1.5".to_string(),
            created_at: "2024-01-15T12:00:00Z".to_string(),
            created_at_height: "12500".to_string(),
            order_id: "order111".to_string(),
            client_metadata: "777".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_fill_report(&ws_fill, &instruments, account_id, ts_init);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No instrument cached for market")
        );
    }

    // ========== Position Parsing Tests ==========

    #[rstest]
    fn test_convert_ws_position_to_http() {
        use nautilus_model::enums::PositionSide;

        use crate::{common::enums::DydxPositionStatus, schemas::ws::DydxPerpetualPosition};

        let ws_position = DydxPerpetualPosition {
            market: "BTC-USD".into(),
            status: DydxPositionStatus::Open,
            side: PositionSide::Long,
            size: "1.5".to_string(),
            max_size: "2.0".to_string(),
            entry_price: "50000.0".to_string(),
            exit_price: None,
            realized_pnl: "100.0".to_string(),
            unrealized_pnl: "250.5".to_string(),
            created_at: "2024-01-15T10:00:00Z".to_string(),
            closed_at: None,
            sum_open: "5.0".to_string(),
            sum_close: "3.5".to_string(),
            net_funding: "-10.25".to_string(),
        };

        let result = convert_ws_position_to_http(&ws_position);
        assert!(result.is_ok());

        let http_position = result.unwrap();
        assert_eq!(http_position.market, "BTC-USD");
        assert_eq!(http_position.status, DydxPositionStatus::Open);
        assert_eq!(http_position.side, OrderSide::Buy); // Positive size = Buy
        assert_eq!(http_position.size, rust_decimal_macros::dec!(1.5));
        assert_eq!(http_position.max_size, rust_decimal_macros::dec!(2.0));
        assert_eq!(
            http_position.entry_price,
            rust_decimal_macros::dec!(50000.0)
        );
        assert_eq!(http_position.exit_price, None);
        assert_eq!(http_position.realized_pnl, rust_decimal_macros::dec!(100.0));
        assert_eq!(
            http_position.unrealized_pnl,
            rust_decimal_macros::dec!(250.5)
        );
        assert_eq!(http_position.sum_open, rust_decimal_macros::dec!(5.0));
        assert_eq!(http_position.sum_close, rust_decimal_macros::dec!(3.5));
        assert_eq!(http_position.net_funding, rust_decimal_macros::dec!(-10.25));
    }

    #[rstest]
    fn test_parse_ws_position_report_success() {
        use nautilus_model::enums::PositionSide;

        use crate::{common::enums::DydxPositionStatus, schemas::ws::DydxPerpetualPosition};

        let instrument = create_test_instrument();
        let instrument_id = instrument.id();

        let instruments = DashMap::new();
        instruments.insert(instrument_id, instrument);

        let ws_position = DydxPerpetualPosition {
            market: "BTC-USD-PERP".into(),
            status: DydxPositionStatus::Open,
            side: PositionSide::Long,
            size: "0.5".to_string(),
            max_size: "1.0".to_string(),
            entry_price: "49500.0".to_string(),
            exit_price: None,
            realized_pnl: "0.0".to_string(),
            unrealized_pnl: "125.0".to_string(),
            created_at: "2024-01-15T09:00:00Z".to_string(),
            closed_at: None,
            sum_open: "0.5".to_string(),
            sum_close: "0.0".to_string(),
            net_funding: "-2.5".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_position_report(&ws_position, &instruments, account_id, ts_init);
        assert!(result.is_ok());

        let position_report = result.unwrap();
        assert_eq!(position_report.instrument_id, instrument_id);
        assert_eq!(position_report.position_side, PositionSideSpecified::Long);
        assert_eq!(position_report.quantity.as_f64(), 0.5);
        // avg_px_open should be entry_price
        assert!(position_report.avg_px_open.is_some());
    }

    #[rstest]
    fn test_parse_ws_position_report_short() {
        use nautilus_model::enums::PositionSide;

        use crate::{common::enums::DydxPositionStatus, schemas::ws::DydxPerpetualPosition};

        let instrument = create_test_instrument();
        let instrument_id = instrument.id();

        let instruments = DashMap::new();
        instruments.insert(instrument_id, instrument);

        let ws_position = DydxPerpetualPosition {
            market: "BTC-USD-PERP".into(),
            status: DydxPositionStatus::Open,
            side: PositionSide::Short,
            size: "-0.25".to_string(), // Negative for short
            max_size: "0.5".to_string(),
            entry_price: "51000.0".to_string(),
            exit_price: None,
            realized_pnl: "50.0".to_string(),
            unrealized_pnl: "-75.25".to_string(),
            created_at: "2024-01-15T08:00:00Z".to_string(),
            closed_at: None,
            sum_open: "0.25".to_string(),
            sum_close: "0.0".to_string(),
            net_funding: "1.5".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_position_report(&ws_position, &instruments, account_id, ts_init);
        assert!(result.is_ok());

        let position_report = result.unwrap();
        assert_eq!(position_report.instrument_id, instrument_id);
        assert_eq!(position_report.position_side, PositionSideSpecified::Short);
        assert_eq!(position_report.quantity.as_f64(), 0.25); // Quantity is always positive
    }

    #[rstest]
    fn test_parse_ws_position_report_missing_instrument() {
        use nautilus_model::enums::PositionSide;

        use crate::{common::enums::DydxPositionStatus, schemas::ws::DydxPerpetualPosition};

        let instruments = DashMap::new(); // Empty - no instruments cached

        let ws_position = DydxPerpetualPosition {
            market: "ETH-USD-PERP".into(),
            status: DydxPositionStatus::Open,
            side: PositionSide::Long,
            size: "5.0".to_string(),
            max_size: "10.0".to_string(),
            entry_price: "3000.0".to_string(),
            exit_price: None,
            realized_pnl: "0.0".to_string(),
            unrealized_pnl: "500.0".to_string(),
            created_at: "2024-01-15T07:00:00Z".to_string(),
            closed_at: None,
            sum_open: "5.0".to_string(),
            sum_close: "0.0".to_string(),
            net_funding: "-5.0".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_position_report(&ws_position, &instruments, account_id, ts_init);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No instrument cached for market")
        );
    }

    #[rstest]
    #[case(DydxOrderStatus::Filled, "2.0")]
    #[case(DydxOrderStatus::Canceled, "0.0")]
    #[case(DydxOrderStatus::BestEffortCanceled, "0.5")]
    #[case(DydxOrderStatus::BestEffortOpened, "0.0")]
    #[case(DydxOrderStatus::Untriggered, "0.0")]
    fn test_parse_ws_order_various_statuses(
        #[case] status: DydxOrderStatus,
        #[case] total_filled: &str,
    ) {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: format!("order_{status:?}"),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "99999".to_string(),
            clob_pair_id: "1".to_string(),
            side: OrderSide::Buy,
            size: "2.0".to_string(),
            price: "50000.0".to_string(),
            status,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            post_only: false,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("1000".to_string()),
            good_til_block_time: None,
            created_at_height: "900".to_string(),
            client_metadata: "0".to_string(),
            trigger_price: None,
            total_filled: total_filled.to_string(),
            updated_at: Some("2024-11-14T10:00:00Z".to_string()),
            updated_at_height: Some("950".to_string()),
        };

        let clob_pair_id_to_instrument = DashMap::new();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
        clob_pair_id_to_instrument.insert(1, instrument_id);

        let instruments = DashMap::new();
        let instrument = create_test_instrument();
        instruments.insert(instrument_id, instrument);

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_order_report(
            &ws_order,
            &clob_pair_id_to_instrument,
            &instruments,
            account_id,
            ts_init,
        );

        assert!(
            result.is_ok(),
            "Failed to parse order with status {status:?}"
        );
        let report = result.unwrap();

        // Verify status conversion
        use nautilus_model::enums::OrderStatus;
        let expected_status = match status {
            DydxOrderStatus::Open
            | DydxOrderStatus::BestEffortOpened
            | DydxOrderStatus::Untriggered => OrderStatus::Accepted,
            DydxOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
            DydxOrderStatus::Filled => OrderStatus::Filled,
            DydxOrderStatus::Canceled | DydxOrderStatus::BestEffortCanceled => {
                OrderStatus::Canceled
            }
        };
        assert_eq!(report.order_status, expected_status);
    }

    #[rstest]
    fn test_parse_ws_order_with_trigger_price() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "conditional_order".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "88888".to_string(),
            clob_pair_id: "1".to_string(),
            side: OrderSide::Sell,
            size: "1.0".to_string(),
            price: "52000.0".to_string(),
            status: DydxOrderStatus::Untriggered,
            order_type: DydxOrderType::StopLimit,
            time_in_force: DydxTimeInForce::Gtt,
            post_only: false,
            reduce_only: true,
            order_flags: "32".to_string(), // Conditional flag
            good_til_block: None,
            good_til_block_time: Some("2024-12-31T23:59:59Z".to_string()),
            created_at_height: "1000".to_string(),
            client_metadata: "100".to_string(),
            trigger_price: Some("51500.0".to_string()),
            total_filled: "0.0".to_string(),
            updated_at: Some("2024-11-14T11:00:00Z".to_string()),
            updated_at_height: Some("1050".to_string()),
        };

        let clob_pair_id_to_instrument = DashMap::new();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
        clob_pair_id_to_instrument.insert(1, instrument_id);

        let instruments = DashMap::new();
        let instrument = create_test_instrument();
        instruments.insert(instrument_id, instrument);

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_order_report(
            &ws_order,
            &clob_pair_id_to_instrument,
            &instruments,
            account_id,
            ts_init,
        );

        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.order_status, OrderStatus::PendingUpdate);
        // Trigger price should be parsed and available in the report
        assert!(report.trigger_price.is_some());
    }

    #[rstest]
    fn test_parse_ws_order_market_type() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "market_order".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "77777".to_string(),
            clob_pair_id: "1".to_string(),
            side: OrderSide::Buy,
            size: "0.5".to_string(),
            price: "50000.0".to_string(), // Market orders still have a price
            status: DydxOrderStatus::Filled,
            order_type: DydxOrderType::Market,
            time_in_force: DydxTimeInForce::Ioc,
            post_only: false,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("1000".to_string()),
            good_til_block_time: None,
            created_at_height: "900".to_string(),
            client_metadata: "0".to_string(),
            trigger_price: None,
            total_filled: "0.5".to_string(),
            updated_at: Some("2024-11-14T10:01:00Z".to_string()),
            updated_at_height: Some("901".to_string()),
        };

        let clob_pair_id_to_instrument = DashMap::new();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
        clob_pair_id_to_instrument.insert(1, instrument_id);

        let instruments = DashMap::new();
        let instrument = create_test_instrument();
        instruments.insert(instrument_id, instrument);

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_order_report(
            &ws_order,
            &clob_pair_id_to_instrument,
            &instruments,
            account_id,
            ts_init,
        );

        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.order_type, OrderType::Market);
        assert_eq!(report.order_status, OrderStatus::Filled);
    }

    #[rstest]
    fn test_parse_ws_order_invalid_clob_pair_id() {
        let ws_order = DydxWsOrderSubaccountMessageContents {
            id: "bad_order".to_string(),
            subaccount_id: "dydx1test/0".to_string(),
            client_id: "12345".to_string(),
            clob_pair_id: "not_a_number".to_string(), // Invalid
            side: OrderSide::Buy,
            size: "1.0".to_string(),
            price: "50000.0".to_string(),
            status: DydxOrderStatus::Open,
            order_type: DydxOrderType::Limit,
            time_in_force: DydxTimeInForce::Gtt,
            post_only: false,
            reduce_only: false,
            order_flags: "0".to_string(),
            good_til_block: Some("1000".to_string()),
            good_til_block_time: None,
            created_at_height: "900".to_string(),
            client_metadata: "0".to_string(),
            trigger_price: None,
            total_filled: "0.0".to_string(),
            updated_at: None,
            updated_at_height: None,
        };

        let clob_pair_id_to_instrument = DashMap::new();
        let instruments = DashMap::new();
        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_order_report(
            &ws_order,
            &clob_pair_id_to_instrument,
            &instruments,
            account_id,
            ts_init,
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to parse clob_pair_id")
        );
    }

    #[rstest]
    fn test_parse_ws_position_closed() {
        use nautilus_model::enums::PositionSide;

        use crate::{common::enums::DydxPositionStatus, schemas::ws::DydxPerpetualPosition};

        let instrument = create_test_instrument();
        let instrument_id = instrument.id();

        let instruments = DashMap::new();
        instruments.insert(instrument_id, instrument);

        let ws_position = DydxPerpetualPosition {
            market: "BTC-USD-PERP".into(),
            status: DydxPositionStatus::Closed,
            side: PositionSide::Long,
            size: "0.0".to_string(), // Closed = zero size
            max_size: "2.0".to_string(),
            entry_price: "48000.0".to_string(),
            exit_price: Some("52000.0".to_string()),
            realized_pnl: "2000.0".to_string(),
            unrealized_pnl: "0.0".to_string(),
            created_at: "2024-01-10T09:00:00Z".to_string(),
            closed_at: Some("2024-01-15T14:00:00Z".to_string()),
            sum_open: "5.0".to_string(),
            sum_close: "5.0".to_string(), // Fully closed
            net_funding: "-25.5".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_position_report(&ws_position, &instruments, account_id, ts_init);
        assert!(result.is_ok());

        let position_report = result.unwrap();
        assert_eq!(position_report.instrument_id, instrument_id);
        // Closed position should have zero quantity
        assert_eq!(position_report.quantity.as_f64(), 0.0);
    }

    #[rstest]
    fn test_parse_ws_fill_with_maker_rebate() {
        use crate::{
            common::enums::{DydxFillType, DydxLiquidity, DydxTickerType},
            schemas::ws::DydxWsFillSubaccountMessageContents,
        };

        let instrument = create_test_instrument();
        let instrument_id = instrument.id();

        let instruments = DashMap::new();
        instruments.insert(instrument_id, instrument);

        let ws_fill = DydxWsFillSubaccountMessageContents {
            id: "fill_rebate".to_string(),
            subaccount_id: "sub1".to_string(),
            side: OrderSide::Buy,
            liquidity: DydxLiquidity::Maker,
            fill_type: DydxFillType::Limit,
            market: "BTC-USD-PERP".into(),
            market_type: DydxTickerType::Perpetual,
            price: "50000.0".to_string(),
            size: "1.0".to_string(),
            fee: "-15.0".to_string(), // Negative fee = rebate
            created_at: "2024-01-15T13:00:00Z".to_string(),
            created_at_height: "13000".to_string(),
            order_id: "order_maker".to_string(),
            client_metadata: "200".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_fill_report(&ws_fill, &instruments, account_id, ts_init);
        assert!(result.is_ok());

        let fill_report = result.unwrap();
        assert_eq!(fill_report.liquidity_side, LiquiditySide::Maker);
        // Commission should be positive (rebate) after negating dYdX's negative fee
        assert!(fill_report.commission.as_f64() > 0.0);
    }

    #[rstest]
    fn test_parse_ws_fill_taker_with_fee() {
        use crate::{
            common::enums::{DydxFillType, DydxLiquidity, DydxTickerType},
            schemas::ws::DydxWsFillSubaccountMessageContents,
        };

        let instrument = create_test_instrument();
        let instrument_id = instrument.id();

        let instruments = DashMap::new();
        instruments.insert(instrument_id, instrument);

        let ws_fill = DydxWsFillSubaccountMessageContents {
            id: "fill_taker".to_string(),
            subaccount_id: "sub2".to_string(),
            side: OrderSide::Sell,
            liquidity: DydxLiquidity::Taker,
            fill_type: DydxFillType::Limit,
            market: "BTC-USD-PERP".into(),
            market_type: DydxTickerType::Perpetual,
            price: "49800.0".to_string(),
            size: "0.75".to_string(),
            fee: "18.675".to_string(), // Positive fee for taker
            created_at: "2024-01-15T14:00:00Z".to_string(),
            created_at_height: "14000".to_string(),
            order_id: "order_taker".to_string(),
            client_metadata: "300".to_string(),
        };

        let account_id = AccountId::new("DYDX-001");
        let ts_init = UnixNanos::default();

        let result = parse_ws_fill_report(&ws_fill, &instruments, account_id, ts_init);
        assert!(result.is_ok());

        let fill_report = result.unwrap();
        assert_eq!(fill_report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(fill_report.order_side, OrderSide::Sell);
        // Commission should be negative (cost to trader) after negating positive fee
        assert!(fill_report.commission.as_f64() < 0.0);
    }
}
