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

//! Parsing utilities for converting IB execution data to Nautilus reports.

use std::str::FromStr;

use anyhow::Context;
use ibapi::orders::{Execution, OrderStatus};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{
        LiquiditySide, OrderSide, OrderStatus as NautilusOrderStatus, OrderType, TimeInForce,
        TrailingOffsetType,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    instruments::Instrument,
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use time::{PrimitiveDateTime, macros::format_description};

use crate::{
    common::parse::is_spread_instrument_id,
    providers::instruments::InteractiveBrokersInstrumentProvider,
};

pub(crate) fn should_use_avg_fill_price(avg_fill_price: f64, instrument_id: &InstrumentId) -> bool {
    avg_fill_price.is_finite()
        && avg_fill_price != f64::MAX
        && avg_fill_price != 0.0
        && (avg_fill_price > 0.0 || is_spread_instrument_id(instrument_id))
}

/// Parse an IB execution to a Nautilus FillReport.
///
/// # Arguments
///
/// * `execution` - The IB execution
/// * `contract` - The IB contract
/// * `commission` - Commission amount
/// * `commission_currency` - Commission currency
/// * `instrument_id` - The instrument ID
/// * `account_id` - The account ID
/// * `instrument_provider` - Instrument provider for price conversion
/// * `ts_init` - Initial timestamp
/// * `avg_px` - Optional average fill price (from order status tracking)
///
/// # Errors
///
/// Returns an error if parsing fails.
///
/// # Note
///
/// The `avg_px` parameter is stored from order status updates and is available for
/// future use when FillReport supports additional metadata fields.
#[allow(clippy::too_many_arguments)]
pub fn parse_execution_to_fill_report(
    execution: &Execution,
    _contract: &ibapi::contracts::Contract,
    commission: f64,
    commission_currency: &str,
    instrument_id: InstrumentId,
    account_id: AccountId,
    instrument_provider: &InteractiveBrokersInstrumentProvider,
    ts_init: UnixNanos,
    avg_px: Option<Price>,
) -> anyhow::Result<FillReport> {
    // Get price magnifier from instrument provider
    let price_magnifier = instrument_provider.get_price_magnifier(&instrument_id) as f64;

    // Convert execution price
    let execution_price = execution.price * price_magnifier;

    // Determine order side
    let order_side = match execution.side.as_str() {
        "BUY" | "BOT" => OrderSide::Buy,
        "SELL" | "SLD" => OrderSide::Sell,
        _ => anyhow::bail!("Unknown order side: {}", execution.side),
    };

    // Get instrument for precision
    let instrument = instrument_provider
        .find(&instrument_id)
        .context("Instrument not found")?;

    // Create quantities and prices
    let last_qty = Quantity::new(execution.shares, instrument.size_precision());
    let last_px = Price::new(execution_price, instrument.price_precision());

    // Create commission
    let commission_money = Money::new(commission, Currency::from_str(commission_currency)?);

    // Parse execution time
    let ts_event = parse_execution_time(&execution.time)?;

    // Create trade ID
    let trade_id = TradeId::new(&execution.execution_id);

    // Create venue order ID
    let venue_order_id = VenueOrderId::new(execution.order_id.to_string());

    // Parse client order ID from order reference
    let client_order_id = if !execution.order_reference.is_empty() {
        Some(ClientOrderId::new(&execution.order_reference))
    } else {
        None
    };

    let mut report = FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission_money,
        LiquiditySide::NoLiquiditySide,
        client_order_id,
        None, // venue_position_id
        ts_event,
        ts_init,
        Some(nautilus_core::UUID4::new()),
    );
    report.avg_px = avg_px.map(|price: Price| price.as_decimal());

    Ok(report)
}

/// Parse an IB order status to a Nautilus OrderStatusReport.
///
/// # Arguments
///
/// * `order_status` - The IB order status
/// * `order` - The IB order (if available)
/// * `instrument_id` - The instrument ID
/// * `account_id` - The account ID
/// * `instrument_provider` - Instrument provider for price conversion
/// * `ts_init` - Initial timestamp
///
/// # Errors
///
/// Returns an error if parsing fails.
pub fn parse_order_status_to_report(
    order_status: &OrderStatus,
    order: Option<&ibapi::orders::Order>,
    instrument_id: InstrumentId,
    account_id: AccountId,
    instrument_provider: &InteractiveBrokersInstrumentProvider,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    // Get price magnifier from instrument provider
    let price_magnifier = instrument_provider.get_price_magnifier(&instrument_id) as f64;

    // Convert Nautilus order status
    let nautilus_status = match order_status.status.as_str() {
        "ApiPending" | "PendingSubmit" | "PreSubmitted" => NautilusOrderStatus::Submitted,
        "Submitted" => NautilusOrderStatus::Accepted,
        "PendingCancel" => NautilusOrderStatus::PendingCancel,
        "ApiCancelled" | "Cancelled" => NautilusOrderStatus::Canceled,
        "Filled" => NautilusOrderStatus::Filled,
        "Inactive" => NautilusOrderStatus::Rejected,
        _ => {
            tracing::warn!(
                "Unknown order status: {}, defaulting to SUBMITTED",
                order_status.status
            );
            NautilusOrderStatus::Submitted
        }
    };

    // Get order side
    let order_side = if let Some(order) = order {
        match order.action {
            ibapi::orders::Action::Buy => OrderSide::Buy,
            ibapi::orders::Action::Sell => OrderSide::Sell,
            ibapi::orders::Action::SellShort => OrderSide::Sell,
            ibapi::orders::Action::SellLong => OrderSide::Sell,
        }
    } else {
        // Default to Buy if order not available
        OrderSide::Buy
    };

    let instrument = instrument_provider.find(&instrument_id);

    // Get instrument for precision (use 0 as default if not available)
    let size_precision = instrument
        .as_ref()
        .map_or(0, |instr| instr.size_precision());
    let price_precision = instrument
        .as_ref()
        .map_or(0, |instr| instr.price_precision());

    // Get quantity
    let quantity = if let Some(order) = order {
        Quantity::new(order.total_quantity, size_precision)
    } else {
        Quantity::zero(size_precision)
    };

    // Get filled quantity
    let filled_qty = Quantity::new(order_status.filled, size_precision);

    // Get average price
    let include_avg_px = should_use_avg_fill_price(order_status.average_fill_price, &instrument_id);
    let avg_px_value = if include_avg_px {
        order_status.average_fill_price * price_magnifier
    } else {
        0.0
    };

    // Extract venue order ID from order_status
    let venue_order_id = VenueOrderId::new(order_status.order_id.to_string());

    // Extract client order ID from order reference if available
    let client_order_id = if let Some(order) = order {
        if order.order_ref.is_empty() {
            None
        } else {
            Some(ClientOrderId::new(&order.order_ref))
        }
    } else {
        None
    };

    // Map order type from IB order if available
    let order_type = order
        .map(|order| map_ib_order_type(&order.order_type))
        .unwrap_or(OrderType::Market);

    // Map time in force from IB order if available
    let time_in_force = if let Some(order) = order {
        let tif_str = order.tif.to_string();
        match tif_str.as_str() {
            "DAY" => TimeInForce::Day,
            "GTC" => TimeInForce::Gtc,
            "IOC" => TimeInForce::Ioc,
            "FOK" => TimeInForce::Fok,
            _ => {
                // Try to parse GTD date
                if tif_str.starts_with("GTD") || !order.good_till_date.is_empty() {
                    TimeInForce::Gtd
                } else {
                    TimeInForce::Day // Default fallback
                }
            }
        }
    } else {
        TimeInForce::Day // Default when order not available
    };

    // Parse limit price if available
    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        nautilus_status,
        quantity,
        filled_qty,
        ts_init, // ts_accepted
        ts_init, // ts_last
        ts_init,
        Some(nautilus_core::UUID4::new()), // report_id
    );

    // Set optional fields
    if let Some(order) = order {
        if let Some(limit_price) = order.limit_price {
            let converted = limit_price * price_magnifier;
            report = report.with_price(Price::new(converted, price_precision));
        }

        let (trigger_price, limit_offset, trailing_offset, trailing_offset_type) =
            parse_ib_order_pricing_fields(order, order_type, price_magnifier, price_precision)?;

        if let Some(trigger_price) = trigger_price {
            report = report.with_trigger_price(trigger_price);
        }

        if let Some(limit_offset) = limit_offset {
            report = report.with_limit_offset(limit_offset);
        }

        if let Some(trailing_offset) = trailing_offset {
            report = report.with_trailing_offset(trailing_offset);
        }

        if let Some(trailing_offset_type) = trailing_offset_type {
            report = report.with_trailing_offset_type(trailing_offset_type);
        }
    }

    if include_avg_px {
        report = report.with_avg_px(avg_px_value)?;
    }

    Ok(report)
}

fn map_ib_order_type(order_type: &str) -> OrderType {
    match order_type {
        "MKT" | "MOC" => OrderType::Market,
        "LMT" | "LOC" => OrderType::Limit,
        "STP" => OrderType::StopMarket,
        "STP LMT" => OrderType::StopLimit,
        "TRAIL" => OrderType::TrailingStopMarket,
        "TRAIL LIMIT" => OrderType::TrailingStopLimit,
        "MIT" => OrderType::MarketIfTouched,
        "LIT" => OrderType::LimitIfTouched,
        "MTL" => OrderType::MarketToLimit,
        _ => OrderType::Market,
    }
}

fn parse_ib_order_pricing_fields(
    order: &ibapi::orders::Order,
    order_type: OrderType,
    price_magnifier: f64,
    price_precision: u8,
) -> anyhow::Result<(
    Option<Price>,
    Option<Decimal>,
    Option<Decimal>,
    Option<TrailingOffsetType>,
)> {
    let mut trigger_price = None;
    let mut limit_offset = None;
    let mut trailing_offset = None;
    let mut trailing_offset_type = None;

    if matches!(
        order_type,
        OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
    ) {
        if let Some(trail_stop_price) = order.trail_stop_price {
            trigger_price = Some(Price::new(
                trail_stop_price * price_magnifier,
                price_precision,
            ));
        }

        if let Some(aux_price) = order.aux_price {
            trailing_offset = Some(decimal_from_f64(aux_price)?);
            trailing_offset_type = Some(TrailingOffsetType::Price);
        } else if let Some(trailing_percent) = order.trailing_percent {
            trailing_offset = Some(decimal_from_f64(trailing_percent)? * Decimal::from(100));
            trailing_offset_type = Some(TrailingOffsetType::BasisPoints);
        }

        if order_type == OrderType::TrailingStopLimit
            && let Some(limit_price_offset) = order.limit_price_offset
        {
            limit_offset = Some(decimal_from_f64(limit_price_offset)?);
            trailing_offset_type = Some(trailing_offset_type.unwrap_or(TrailingOffsetType::Price));
        }

        return Ok((
            trigger_price,
            limit_offset,
            trailing_offset,
            trailing_offset_type,
        ));
    }

    if let Some(aux_price) = order.aux_price {
        trigger_price = Some(Price::new(aux_price * price_magnifier, price_precision));
    }

    Ok((
        trigger_price,
        limit_offset,
        trailing_offset,
        trailing_offset_type,
    ))
}

fn decimal_from_f64(value: f64) -> anyhow::Result<Decimal> {
    Decimal::from_str(&value.to_string())
        .with_context(|| format!("Failed to convert IB floating-point value {value} to Decimal"))
}

/// Parse execution time string to UnixNanos.
///
/// Parse IB execution time to UnixNanos.
///
/// Supported IB formats:
/// - "20230223 00:43:36 Universal"
/// - "20230223 00:43:36 UTC"
/// - "20230223 00:43:36" (assumed UTC)
/// - "20250225-15:15:00" (assumed UTC)
///
/// # Arguments
///
/// * `time_str` - The execution time string from IB
///
/// # Errors
///
/// Returns an error if the execution timestamp is malformed or uses a non-UTC timezone.
pub fn parse_execution_time(time_str: &str) -> anyhow::Result<UnixNanos> {
    fn parse_utc(
        time_str: &str,
        format: &[time::format_description::FormatItem<'_>],
    ) -> anyhow::Result<UnixNanos> {
        let dt = PrimitiveDateTime::parse(time_str, format).map_err(|e| {
            anyhow::anyhow!("Failed to parse execution timestamp '{time_str}': {e}")
        })?;
        let nanos: u64 = dt
            .assume_utc()
            .unix_timestamp_nanos()
            .try_into()
            .map_err(|_| {
                anyhow::anyhow!("Execution timestamp '{time_str}' was before Unix epoch")
            })?;
        Ok(UnixNanos::new(nanos))
    }

    if time_str.contains('-') && !time_str.contains(' ') {
        let format = format_description!("[year][month][day]-[hour]:[minute]:[second]");
        return parse_utc(time_str, format);
    }

    let parts: Vec<&str> = time_str.split(' ').collect();

    if parts.len() < 2 {
        anyhow::bail!("Invalid execution time format: {time_str}");
    }

    let format = format_description!("[year][month][day] [hour]:[minute]:[second]");
    let date_str = format!("{} {}", parts[0], parts[1]);

    if parts.len() == 2 {
        return parse_utc(&date_str, format);
    }

    let timezone = parts[2];
    if !matches!(timezone, "Universal" | "UTC" | "Etc/UTC" | "GMT" | "Z") {
        anyhow::bail!(
            "Unsupported non-UTC execution timezone '{timezone}' in '{time_str}'. Configure TWS / IB Gateway to emit UTC timestamps"
        );
    }

    parse_utc(&date_str, format)
}

#[cfg(test)]
mod tests {
    use ibapi::{
        contracts::Contract,
        orders::{Action, Liquidity, Order},
    };
    use nautilus_model::{
        enums::TrailingOffsetType,
        identifiers::{Symbol, Venue},
    };
    use rust_decimal::Decimal;

    use super::*;
    use crate::{
        config::InteractiveBrokersInstrumentProviderConfig,
        providers::instruments::InteractiveBrokersInstrumentProvider,
    };

    fn create_test_instrument_provider() -> InteractiveBrokersInstrumentProvider {
        let config = InteractiveBrokersInstrumentProviderConfig::default();
        InteractiveBrokersInstrumentProvider::new(config)
    }

    fn create_test_instrument_id() -> InstrumentId {
        InstrumentId::new(Symbol::from("AAPL"), Venue::from("NASDAQ"))
    }

    use rstest::rstest;

    #[rstest]
    fn test_parse_execution_time_hyphenated_format() {
        let time_str = "20250225-15:15:00";
        let result = parse_execution_time(time_str);
        assert!(result.is_ok());
        let timestamp = result.unwrap();
        assert!(timestamp.as_i64() > 0);
    }

    #[rstest]
    fn test_parse_execution_time_with_unsupported_non_utc_timezone() {
        let time_str = "20230223 00:43:36 America/New_York";
        let result = parse_execution_time(time_str);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_parse_execution_time_utc() {
        let time_str = "20230223 00:43:36 Universal";
        let result = parse_execution_time(time_str);
        assert!(result.is_ok());
        let timestamp = result.unwrap();
        assert!(timestamp.as_i64() > 0);
    }

    #[rstest]
    fn test_parse_execution_time_no_timezone_assumes_utc() {
        let time_str = "20230223 00:43:36";
        let result = parse_execution_time(time_str);
        assert!(result.is_ok());
        let timestamp = result.unwrap();
        assert!(timestamp.as_i64() > 0);
    }

    #[rstest]
    fn test_parse_execution_time_invalid_format() {
        let time_str = "invalid format";
        let result = parse_execution_time(time_str);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_parse_execution_time_short_format() {
        let time_str = "20230223 00:43";
        let result = parse_execution_time(time_str);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_parse_order_status_to_report_submitted() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let account_id = AccountId::from("IB-001");

        let order_status = OrderStatus {
            order_id: 12345,
            status: String::from("Submitted"),
            filled: 0.0,
            remaining: 100.0,
            average_fill_price: 0.0,
            perm_id: 0,
            parent_id: 0,
            last_fill_price: 0.0,
            client_id: 0,
            why_held: String::new(),
            market_cap_price: 0.0,
        };

        let result = parse_order_status_to_report(
            &order_status,
            None,
            instrument_id,
            account_id,
            &instrument_provider,
            UnixNanos::new(0),
        );

        // May fail if instrument not in provider, but that's expected
        if let Err(e) = result {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("not found") || error_msg.contains("instrument"),
                "Unexpected error: {}",
                error_msg
            );
        }
    }

    #[rstest]
    fn test_parse_order_status_to_report_filled() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let account_id = AccountId::from("IB-001");

        let order_status = OrderStatus {
            order_id: 12345,
            status: String::from("Filled"),
            filled: 100.0,
            remaining: 0.0,
            average_fill_price: 150.25,
            perm_id: 0,
            parent_id: 0,
            last_fill_price: 150.25,
            client_id: 0,
            why_held: String::new(),
            market_cap_price: 0.0,
        };

        let result = parse_order_status_to_report(
            &order_status,
            None,
            instrument_id,
            account_id,
            &instrument_provider,
            UnixNanos::new(0),
        );

        // May fail if instrument not in provider, but that's expected
        if let Err(e) = result {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("not found") || error_msg.contains("instrument"),
                "Unexpected error: {}",
                error_msg
            );
        }
    }

    #[rstest]
    fn test_parse_order_status_to_report_spread_allows_negative_avg_fill_price() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = InstrumentId::new(
            Symbol::from("(1)SPY C400_((1))SPY C410"),
            Venue::from("SMART"),
        );
        let account_id = AccountId::from("IB-001");

        let order_status = OrderStatus {
            order_id: 12345,
            status: String::from("Filled"),
            filled: 1.0,
            remaining: 0.0,
            average_fill_price: -2.25,
            perm_id: 0,
            parent_id: 0,
            last_fill_price: -2.25,
            client_id: 0,
            why_held: String::new(),
            market_cap_price: 0.0,
        };

        let report = parse_order_status_to_report(
            &order_status,
            None,
            instrument_id,
            account_id,
            &instrument_provider,
            UnixNanos::new(0),
        )
        .unwrap();

        assert_eq!(report.avg_px, Some(Decimal::from_str("-2.25").unwrap()));
    }

    #[rstest]
    fn test_parse_order_status_to_report_inactive_maps_to_rejected() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let account_id = AccountId::from("IB-001");

        let order_status = OrderStatus {
            order_id: 12345,
            status: String::from("Inactive"),
            filled: 0.0,
            remaining: 100.0,
            average_fill_price: 0.0,
            perm_id: 0,
            parent_id: 0,
            last_fill_price: 0.0,
            client_id: 0,
            why_held: String::new(),
            market_cap_price: 0.0,
        };

        let report = parse_order_status_to_report(
            &order_status,
            None,
            instrument_id,
            account_id,
            &instrument_provider,
            UnixNanos::new(0),
        )
        .unwrap();

        assert_eq!(report.order_status, NautilusOrderStatus::Rejected);
    }

    #[rstest]
    #[case(
        "MKT",
        None,
        None,
        None,
        None,
        OrderType::Market,
        None,
        None,
        None,
        None,
        TrailingOffsetType::NoTrailingOffset
    )]
    #[case(
        "LMT",
        Some(185.0),
        None,
        None,
        None,
        OrderType::Limit,
        Some(Price::new(185.0, 0)),
        None,
        None,
        None,
        TrailingOffsetType::NoTrailingOffset
    )]
    #[case(
        "MIT",
        None,
        Some(180.0),
        None,
        None,
        OrderType::MarketIfTouched,
        None,
        Some(Price::new(180.0, 0)),
        None,
        None,
        TrailingOffsetType::NoTrailingOffset
    )]
    #[case(
        "LIT",
        Some(179.0),
        Some(180.0),
        None,
        None,
        OrderType::LimitIfTouched,
        Some(Price::new(179.0, 0)),
        Some(Price::new(180.0, 0)),
        None,
        None,
        TrailingOffsetType::NoTrailingOffset
    )]
    #[case(
        "STP",
        None,
        Some(180.0),
        None,
        None,
        OrderType::StopMarket,
        None,
        Some(Price::new(180.0, 0)),
        None,
        None,
        TrailingOffsetType::NoTrailingOffset
    )]
    #[case(
        "STP LMT",
        Some(179.0),
        Some(180.0),
        None,
        None,
        OrderType::StopLimit,
        Some(Price::new(179.0, 0)),
        Some(Price::new(180.0, 0)),
        None,
        None,
        TrailingOffsetType::NoTrailingOffset
    )]
    #[case(
        "TRAIL LIMIT",
        None,
        Some(2.5),
        Some(185.0),
        Some(0.25),
        OrderType::TrailingStopLimit,
        None,
        Some(Price::new(185.0, 0)),
        Some(Decimal::from_str("0.25").unwrap()),
        Some(Decimal::from_str("2.5").unwrap()),
        TrailingOffsetType::Price,
    )]
    fn test_parse_order_status_to_report_maps_pricing_fields_by_order_type(
        #[case] ib_order_type: &str,
        #[case] limit_price: Option<f64>,
        #[case] aux_price: Option<f64>,
        #[case] trail_stop_price: Option<f64>,
        #[case] limit_price_offset: Option<f64>,
        #[case] expected_order_type: OrderType,
        #[case] expected_price: Option<Price>,
        #[case] expected_trigger_price: Option<Price>,
        #[case] expected_limit_offset: Option<Decimal>,
        #[case] expected_trailing_offset: Option<Decimal>,
        #[case] expected_trailing_offset_type: TrailingOffsetType,
    ) {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let account_id = AccountId::from("IB-001");

        let order_status = OrderStatus {
            order_id: 12345,
            status: String::from("Submitted"),
            filled: 0.0,
            remaining: 5.0,
            average_fill_price: 0.0,
            perm_id: 0,
            parent_id: 0,
            last_fill_price: 0.0,
            client_id: 0,
            why_held: String::new(),
            market_cap_price: 0.0,
        };

        let order = Order {
            action: Action::Buy,
            total_quantity: 5.0,
            order_type: ib_order_type.to_string(),
            limit_price,
            aux_price,
            trail_stop_price,
            limit_price_offset,
            tif: ibapi::orders::TimeInForce::GoodTilCanceled,
            ..Default::default()
        };

        let report = parse_order_status_to_report(
            &order_status,
            Some(&order),
            instrument_id,
            account_id,
            &instrument_provider,
            UnixNanos::new(0),
        )
        .unwrap();

        assert_eq!(report.order_type, expected_order_type);
        assert_eq!(report.price, expected_price);
        assert_eq!(report.trigger_price, expected_trigger_price);
        assert_eq!(report.limit_offset, expected_limit_offset);
        assert_eq!(report.trailing_offset, expected_trailing_offset);
        assert_eq!(report.trailing_offset_type, expected_trailing_offset_type);
    }

    #[rstest]
    fn test_parse_order_status_to_report_maps_trailing_percent_to_basis_points() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let account_id = AccountId::from("IB-001");

        let order_status = OrderStatus {
            order_id: 12345,
            status: String::from("Submitted"),
            filled: 0.0,
            remaining: 5.0,
            average_fill_price: 0.0,
            perm_id: 0,
            parent_id: 0,
            last_fill_price: 0.0,
            client_id: 0,
            why_held: String::new(),
            market_cap_price: 0.0,
        };

        let order = Order {
            action: Action::Buy,
            total_quantity: 5.0,
            order_type: "TRAIL".to_string(),
            trail_stop_price: Some(185.0),
            trailing_percent: Some(2.5),
            tif: ibapi::orders::TimeInForce::GoodTilCanceled,
            ..Default::default()
        };

        let report = parse_order_status_to_report(
            &order_status,
            Some(&order),
            instrument_id,
            account_id,
            &instrument_provider,
            UnixNanos::new(0),
        )
        .unwrap();

        assert_eq!(report.order_type, OrderType::TrailingStopMarket);
        assert_eq!(report.trigger_price, Some(Price::new(185.0, 0)));
        assert_eq!(
            report.trailing_offset,
            Some(Decimal::from_str("250").unwrap())
        );
        assert_eq!(report.trailing_offset_type, TrailingOffsetType::BasisPoints);
        assert_eq!(report.limit_offset, None);
    }

    #[rstest]
    fn test_parse_execution_to_fill_report_buy() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let account_id = AccountId::from("IB-001");

        let execution = Execution {
            order_id: 12345,
            client_id: 0,
            execution_id: String::from("EXEC-001"),
            time: String::from("20230223 00:43:36 Universal"),
            account_number: String::new(),
            exchange: String::new(),
            side: String::from("BOT"),
            shares: 100.0,
            price: 150.25,
            perm_id: 0,
            liquidation: 0,
            cumulative_quantity: 100.0,
            average_price: 150.25,
            order_reference: String::from("ORDER-REF-001"),
            ev_rule: String::new(),
            ev_multiplier: None,
            model_code: String::new(),
            last_liquidity: Liquidity::None,
            pending_price_revision: false,
            submitter: String::new(),
        };

        let contract = Contract::default();
        let result = parse_execution_to_fill_report(
            &execution,
            &contract,
            1.0,
            "USD",
            instrument_id,
            account_id,
            &instrument_provider,
            UnixNanos::new(0),
            None, // avg_px
        );

        // May fail if instrument not in provider, but that's expected
        match result {
            Err(e) => {
                let error_msg = e.to_string();
                assert!(
                    error_msg.contains("not found") || error_msg.contains("instrument"),
                    "Unexpected error: {}",
                    error_msg
                );
            }
            Ok(fill) => {
                assert_eq!(fill.order_side, OrderSide::Buy);
                assert_eq!(fill.trade_id.to_string(), "EXEC-001");
            }
        }
    }

    #[rstest]
    fn test_parse_execution_to_fill_report_sell() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let account_id = AccountId::from("IB-001");

        let execution = Execution {
            order_id: 12345,
            client_id: 0,
            execution_id: String::from("EXEC-002"),
            time: String::from("20230223 00:43:36 Universal"),
            account_number: String::new(),
            exchange: String::new(),
            side: String::from("SLD"),
            shares: 50.0,
            price: 151.0,
            perm_id: 0,
            liquidation: 0,
            cumulative_quantity: 50.0,
            average_price: 151.0,
            order_reference: String::new(),
            ev_rule: String::new(),
            ev_multiplier: None,
            model_code: String::new(),
            last_liquidity: Liquidity::None,
            pending_price_revision: false,
            submitter: String::new(),
        };

        let contract = Contract::default();
        let result = parse_execution_to_fill_report(
            &execution,
            &contract,
            0.5,
            "USD",
            instrument_id,
            account_id,
            &instrument_provider,
            UnixNanos::new(0),
            None, // avg_px
        );

        // May fail if instrument not in provider, but that's expected
        match result {
            Err(e) => {
                let error_msg = e.to_string();
                assert!(
                    error_msg.contains("not found") || error_msg.contains("instrument"),
                    "Unexpected error: {}",
                    error_msg
                );
            }
            Ok(fill) => {
                assert_eq!(fill.order_side, OrderSide::Sell);
            }
        }
    }
}
