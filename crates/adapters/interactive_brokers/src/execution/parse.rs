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

use ahash::AHashMap;
use anyhow::Context;
use ibapi::orders::{Execution, Liquidity, OrderData, OrderStatus};
use jiff::{
    Timestamp,
    civil::DateTime,
    tz::{AmbiguousOffset, Offset},
};
use nautilus_core::{UnixNanos, datetime::get_timezone};
use nautilus_model::{
    enums::{
        ContingencyType, LiquiditySide, OrderSide, OrderStatus as NautilusOrderStatus, OrderType,
        TimeInForce, TrailingOffsetType,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    instruments::Instrument,
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        enums::{IbAction, IbOrderStatus, IbOrderType, IbTimeInForce},
        spreads::is_spread_instrument_id,
    },
    execution::transform::ib_trigger_method_to_trigger_type,
    providers::instruments::InteractiveBrokersInstrumentProvider,
};

pub(super) fn parse_order_data_to_report(
    data: &OrderData,
    instrument_id: InstrumentId,
    account_id: AccountId,
    instrument_provider: &InteractiveBrokersInstrumentProvider,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    anyhow::ensure!(
        data.order.total_quantity.is_finite()
            && data.order.total_quantity >= 0.0
            && data.order.filled_quantity.is_finite()
            && data.order.filled_quantity >= 0.0,
        "IB order {} has invalid quantities",
        data.order_id
    );
    let mut report = parse_order_status_to_report(
        &OrderStatus {
            order_id: data.order_id,
            status: data.order_state.status.clone(),
            filled: data.order.filled_quantity,
            remaining: (data.order.total_quantity - data.order.filled_quantity).max(0.0),
            average_fill_price: None,
            perm_id: data.order.perm_id,
            parent_id: 0,
            last_fill_price: None,
            client_id: data.order.client_id,
            why_held: String::new(),
            market_cap_price: None,
        },
        Some(&data.order),
        instrument_id,
        account_id,
        instrument_provider,
        ts_init,
    )?;

    // Open orders carry no venue time; a completed order reports when it finished
    if !data.order_state.completed_time.is_empty() {
        match parse_execution_time(&data.order_state.completed_time) {
            Ok(completed) => report.ts_last = completed,
            Err(e) => tracing::debug!(
                "Cannot parse completed time of IB order {}: {e}",
                data.order_id
            ),
        }
    }

    Ok(report)
}

/// Links the contingencies IB reports across one batch of order status reports.
///
/// IB keeps bracket parents (`parent_id`) and OCA groups on its side; a status report parsed on
/// its own cannot name the related orders. Each report is matched to its IB order by index.
/// Reports whose OCA type IB does not model keep no contingency, and a contingency is only set
/// when at least one related order is present in the batch.
/// Sets the average price of filled order reports that lack one from their fill reports.
///
/// IB completed orders carry no average fill price, which reconciliation needs to rebuild a
/// filled order before applying its fills.
pub(crate) fn fill_missing_avg_px(
    order_reports: &mut [OrderStatusReport],
    fill_reports: &[FillReport],
) {
    for report in order_reports
        .iter_mut()
        .filter(|report| report.avg_px.is_none() && !report.filled_qty.is_zero())
    {
        let (notional, quantity) = fill_reports
            .iter()
            .filter(|fill| fill.venue_order_id == report.venue_order_id)
            .fold(
                (Decimal::ZERO, Decimal::ZERO),
                |(notional, quantity), fill| {
                    (
                        notional + fill.last_px.as_decimal() * fill.last_qty.as_decimal(),
                        quantity + fill.last_qty.as_decimal(),
                    )
                },
            );

        if !quantity.is_zero() {
            report.avg_px = Some(notional / quantity);
        }
    }
}

/// Returns whether an execution is the combo-level (BAG) execution of a spread order.
///
/// IB sends commission reports only for the leg executions of a combo; the combo-level
/// execution carries none, so its fill has zero commission.
pub(crate) fn is_combo_execution(exec_data: &ibapi::orders::ExecutionData) -> bool {
    matches!(
        exec_data.contract.security_type,
        ibapi::contracts::SecurityType::Spread
    )
}

pub(crate) fn link_order_contingencies(
    reports: &mut [OrderStatusReport],
    orders: &[ibapi::orders::Order],
) {
    debug_assert_eq!(reports.len(), orders.len());
    let order_ids: Vec<ClientOrderId> = reports
        .iter()
        .map(|report| {
            report
                .client_order_id
                .unwrap_or_else(|| ClientOrderId::from(report.venue_order_id.as_str()))
        })
        .collect();

    let mut oca_groups: AHashMap<(&str, &str), Vec<usize>> = AHashMap::new();

    for (index, order) in orders.iter().enumerate() {
        if !order.oca_group.is_empty() {
            oca_groups
                .entry((order.account.as_str(), order.oca_group.as_str()))
                .or_default()
                .push(index);
        }
    }

    let mut children: AHashMap<usize, Vec<usize>> = AHashMap::new();

    for (index, order) in orders.iter().enumerate() {
        let parent = orders.iter().position(|candidate| {
            order
                .parent_perm_id
                .is_some_and(|perm_id| perm_id != 0 && candidate.perm_id == perm_id)
                || (order.parent_id != 0
                    && candidate.order_id == order.parent_id
                    && candidate.client_id == order.client_id)
        });

        if let Some(parent) = parent.filter(|parent| *parent != index) {
            reports[index].parent_order_id = Some(order_ids[parent]);
            children.entry(parent).or_default().push(index);
        }
    }

    for (index, order) in orders.iter().enumerate() {
        if let Some(members) = oca_groups.get(&(order.account.as_str(), order.oca_group.as_str())) {
            let contingency_type = match order.oca_type {
                ibapi::orders::OcaType::CancelWithBlock => Some(ContingencyType::Oco),
                ibapi::orders::OcaType::ReduceWithBlock
                | ibapi::orders::OcaType::ReduceWithoutBlock => Some(ContingencyType::Ouo),
                _ => None,
            };
            let linked: Vec<ClientOrderId> = members
                .iter()
                .filter(|member| **member != index)
                .map(|member| order_ids[*member])
                .collect();

            if let Some(contingency_type) = contingency_type
                && !linked.is_empty()
            {
                reports[index].contingency_type = Some(contingency_type);
                reports[index].linked_order_ids = Some(linked);
                continue;
            }
        }

        if let Some(child_indices) = children.get(&index) {
            reports[index].contingency_type = Some(ContingencyType::Oto);
            reports[index].linked_order_ids = Some(
                child_indices
                    .iter()
                    .map(|child| order_ids[*child])
                    .collect(),
            );
        }
    }
}

pub(crate) fn should_use_avg_fill_price(avg_fill_price: f64, instrument_id: &InstrumentId) -> bool {
    avg_fill_price.is_finite()
        && avg_fill_price != f64::MAX
        && avg_fill_price != 0.0
        && (avg_fill_price > 0.0 || is_spread_instrument_id(instrument_id))
}

pub(crate) fn ib_venue_order_id(order_id: i32, perm_id: i64) -> VenueOrderId {
    if perm_id != 0 {
        VenueOrderId::new(format!("PERM-{perm_id}"))
    } else {
        VenueOrderId::new(order_id.to_string())
    }
}

pub(crate) fn normalized_order_ref(order_ref: &str) -> Option<&str> {
    if order_ref.is_empty() {
        return None;
    }

    Some(
        order_ref
            .rsplit_once(':')
            .map_or(order_ref, |(base, _)| base),
    )
}

/// Parse an IB execution to a Nautilus FillReport.
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
    let order_side = IbAction::from_str(execution.side.as_str())?.order_side();

    // Get instrument for precision
    let instrument = instrument_provider
        .find(&instrument_id)
        .context("Instrument not found")?;

    // Create quantities and prices
    let last_qty = Quantity::new(execution.shares, instrument.size_precision());
    let last_px = Price::new(execution_price, instrument.price_precision());

    // Clamp only IB's -1 pending sentinel to 0.0 to preserve rebates
    let commission_clamped = if commission == -1.0 { 0.0 } else { commission };
    let commission_money = Money::new(commission_clamped, Currency::from_str(commission_currency)?);

    // Parse execution time
    let ts_event = parse_execution_time(&execution.time)?;

    // Create trade ID
    let trade_id = TradeId::new(&execution.execution_id);

    let venue_order_id = ib_venue_order_id(execution.order_id, execution.perm_id);

    let client_order_id = normalized_order_ref(&execution.order_reference).map(ClientOrderId::new);
    let liquidity_side = execution_liquidity_side(&execution.last_liquidity);

    let mut report = FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission_money,
        liquidity_side,
        client_order_id,
        None, // venue_position_id
        ts_event,
        ts_init,
        Some(nautilus_core::UUID4::new()),
    );
    report.avg_px = avg_px.map(|price: Price| price.as_decimal());

    Ok(report)
}

fn execution_liquidity_side(liquidity: &Liquidity) -> LiquiditySide {
    match liquidity {
        Liquidity::AddedLiquidity => LiquiditySide::Maker,
        Liquidity::RemovedLiquidity => LiquiditySide::Taker,
        Liquidity::None => LiquiditySide::NoLiquiditySide,
        Liquidity::LiquidityRoutedOut => {
            tracing::warn!(
                "IB execution liquidity was routed out and has no maker/taker representation"
            );
            LiquiditySide::NoLiquiditySide
        }
        Liquidity::Unknown(code) => {
            tracing::warn!("IB execution used unknown liquidity code {code}");
            LiquiditySide::NoLiquiditySide
        }
    }
}

/// Parse an IB order status to a Nautilus OrderStatusReport.
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

    let mut nautilus_status = match IbOrderStatus::from_str(order_status.status.as_str()) {
        Ok(status) => status.nautilus_status(),
        _ => {
            tracing::warn!(
                "Unknown order status: {}, defaulting to SUBMITTED",
                order_status.status.as_str()
            );
            NautilusOrderStatus::Submitted
        }
    };

    // Get order side
    let order_side = if let Some(order) = order {
        IbAction::from(order.action).order_side()
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
    let total_quantity = order.map_or(0.0, |order| order.total_quantity);
    let quantity = Quantity::new(
        total_quantity.max(order_status.filled + order_status.remaining),
        size_precision,
    );

    // Get filled quantity
    let filled_qty = Quantity::new(order_status.filled, size_precision);

    // Get average price
    let average_fill_price = order_status.average_fill_price.unwrap_or(0.0);
    let include_avg_px = should_use_avg_fill_price(average_fill_price, &instrument_id);
    let avg_px_value = if include_avg_px {
        average_fill_price * price_magnifier
    } else {
        0.0
    };

    if order_status.filled > 0.0
        && (order_status.remaining > 0.0
            || order.is_some_and(|order| order.total_quantity > order_status.filled))
    {
        nautilus_status = NautilusOrderStatus::PartiallyFilled;
    }

    let venue_order_id = ib_venue_order_id(order_status.order_id, order_status.perm_id);

    let client_order_id = order
        .and_then(|order| normalized_order_ref(&order.order_ref))
        .map(ClientOrderId::new);

    // Map order type from IB order if available
    let order_type = order.map_or(OrderType::Market, |order| {
        map_ib_order_type(&order.order_type, order.limit_price)
    });

    // A GTD order needs its expiry to be rebuilt from the report; IB still enforces the date,
    // so an unparsable one degrades to GTC rather than failing the whole report set
    let expire_time = order
        .filter(|order| !order.good_till_date.is_empty())
        .and_then(|order| match parse_execution_time(&order.good_till_date) {
            Ok(expire_time) => Some(expire_time),
            Err(e) => {
                tracing::warn!(
                    "Reporting IB order {} as GTC: cannot parse goodTillDate: {e}",
                    order_status.order_id
                );
                None
            }
        });

    // Map time in force from IB order if available
    let time_in_force = if let Some(order) = order {
        let ib_time_in_force = IbTimeInForce::from(order.tif.clone());

        if expire_time.is_some() {
            TimeInForce::Gtd
        } else if ib_time_in_force == IbTimeInForce::GoodTilDate {
            TimeInForce::Gtc
        } else {
            ib_time_in_force.nautilus_time_in_force()
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
        order_side.into(),
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

    if let Some(expire_time) = expire_time {
        report = report.with_expire_time(expire_time);
    }

    // Set optional fields
    if let Some(order) = order {
        // IB reports a zero limit price on market-type orders, which reconciliation would
        // otherwise take as the fill price of an order it has no fills for
        if let Some(limit_price) = order.limit_price
            && matches!(
                order_type,
                OrderType::Limit
                    | OrderType::StopLimit
                    | OrderType::LimitIfTouched
                    | OrderType::TrailingStopLimit
                    | OrderType::MarketToLimit
            )
        {
            let converted = limit_price * price_magnifier;
            report = report.with_price(Price::new(converted, price_precision));
        }

        let (trigger_price, limit_offset, trailing_offset, trailing_offset_type) =
            parse_ib_order_pricing_fields(order, order_type, price_magnifier, price_precision)?;

        if let Some(trigger_price) = trigger_price {
            report = report.with_trigger_price(trigger_price);
        }

        if matches!(
            order_type,
            OrderType::LimitIfTouched
                | OrderType::MarketIfTouched
                | OrderType::StopLimit
                | OrderType::StopMarket
                | OrderType::TrailingStopLimit
                | OrderType::TrailingStopMarket
        ) {
            report =
                report.with_trigger_type(ib_trigger_method_to_trigger_type(order.trigger_method));
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
        report = report.with_avg_px(decimal_from_f64(avg_px_value)?);
    }

    if let Some(display_size) = order
        .and_then(|order| order.display_size)
        .filter(|size| *size > 0 && f64::from(*size) < quantity.as_f64())
    {
        report = report.with_display_qty(Quantity::new(f64::from(display_size), size_precision));
    }

    Ok(report)
}

fn map_ib_order_type(order_type: &str, limit_price: Option<f64>) -> OrderType {
    if order_type == "IBALGO" && limit_price.is_some_and(|price| price != 0.0) {
        OrderType::Limit
    } else {
        IbOrderType::from_str(order_type)
            .map_or(OrderType::Market, IbOrderType::nautilus_order_type)
    }
}

#[allow(clippy::type_complexity)] // The tuple mirrors the four optional IB pricing fields.
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
/// - "20230223 00:43:36 MET"
/// - "20230223 00:43:36 America/New_York"
/// - "20230223 00:43:36" (assumed UTC)
/// - "20250225-15:15:00" (assumed UTC)
///
/// Timezones are resolved through Jiff's bundled IANA tz database, so any
/// region abbreviation or name that IB stamps the execution with (e.g. `MET`,
/// `EST`, `America/New_York`) is honored, matching the v1 pandas-based parser.
/// This matters because some IB accounts (e.g. European paper accounts) report a
/// server timezone such as `MET` that the gateway cannot be coerced out of.
///
/// # Errors
///
/// Returns an error if the timestamp is malformed, the timezone is
/// unrecognized, or the local time is non-existent (a DST spring-forward gap).
/// DST fall-back folds resolve to the earliest matching instant.
pub fn parse_execution_time(time_str: &str) -> anyhow::Result<UnixNanos> {
    const NAIVE_FORMAT: &str = "%Y%m%d %H:%M:%S";

    // Hyphenated, space-less form (e.g. "20250225-15:15:00") is always UTC.
    if !time_str.contains(' ') {
        let normalized = time_str.replace('-', " ");
        let dt = DateTime::strptime(NAIVE_FORMAT, &normalized).map_err(|e| {
            anyhow::anyhow!("Failed to parse execution timestamp '{time_str}': {e}")
        })?;
        return datetime_to_unix_nanos(Offset::UTC.to_timestamp(dt)?, time_str);
    }

    // Split into at most three parts: date, time, and optional timezone token.
    // The timezone token itself never contains a space, so `splitn(3, ' ')`
    // correctly groups IANA names such as "America/New_York".
    let mut parts = time_str.splitn(3, ' ');
    let (Some(date), Some(time)) = (parts.next(), parts.next()) else {
        anyhow::bail!("Invalid execution time format: {time_str}");
    };
    let tz_str = parts.next().unwrap_or("").trim();

    let naive_str = format!("{date} {time}");
    let dt = DateTime::strptime(NAIVE_FORMAT, &naive_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse execution timestamp '{time_str}': {e}"))?;

    let utc = if tz_str.is_empty() {
        Offset::UTC.to_timestamp(dt)?
    } else {
        localize_with_zone(dt, tz_str, time_str)?
    };

    datetime_to_unix_nanos(utc, time_str)
}

/// Localize a naive timestamp against an IB timezone token and convert to UTC.
///
/// `Z` is normalized to `UTC`; everything else is resolved through the IANA tz
/// database. Error and fold behavior is documented on [`parse_execution_time`].
fn localize_with_zone(dt: DateTime, tz_str: &str, time_str: &str) -> anyhow::Result<Timestamp> {
    let tz_name = if tz_str.eq_ignore_ascii_case("Z") {
        "UTC"
    } else {
        tz_str
    };

    let zone = get_timezone(tz_name).map_err(|_| {
        anyhow::anyhow!(
            "Unrecognized execution timezone '{tz_str}' in '{time_str}'. Configure TWS / IB Gateway to emit a standard timezone (e.g. UTC)"
        )
    })?;
    let ambiguous = zone.to_ambiguous_timestamp(dt);
    match ambiguous.offset() {
        AmbiguousOffset::Unambiguous { .. } => Ok(ambiguous.unambiguous()?),
        // Fall-back fold: take the earliest instant (worst case ~1h skew).
        AmbiguousOffset::Fold { .. } => Ok(ambiguous.earlier()?),
        AmbiguousOffset::Gap { .. } => {
            anyhow::bail!("Execution timestamp '{time_str}' is non-existent in timezone '{tz_str}'")
        }
    }
}

fn datetime_to_unix_nanos(dt: Timestamp, time_str: &str) -> anyhow::Result<UnixNanos> {
    let nanos: u64 = dt
        .as_nanosecond()
        .try_into()
        .map_err(|_| anyhow::anyhow!("Execution timestamp '{time_str}' was before Unix epoch"))?;
    Ok(UnixNanos::new(nanos))
}

#[cfg(test)]
mod tests {
    use ibapi::{
        contracts::Contract,
        orders::{Action, ExecutionSide, Liquidity, Order, OrderStatusKind},
    };
    use nautilus_model::{
        enums::{TrailingOffsetType, TriggerType},
        identifiers::{Symbol, Venue},
        instruments::{InstrumentAny, stubs::equity_aapl},
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
    fn test_fill_missing_avg_px_uses_fill_reports() {
        let instrument_id = InstrumentId::from("ESZ6.XCME");
        let account_id = AccountId::from("IB-DU001");
        let order_report = |venue_order_id: &str, avg_px: Option<Decimal>| {
            let mut report = OrderStatusReport::new(
                account_id,
                instrument_id,
                None,
                VenueOrderId::new(venue_order_id),
                Some(OrderSide::Buy),
                OrderType::Market,
                TimeInForce::Day,
                NautilusOrderStatus::Filled,
                Quantity::from(3),
                Quantity::from(3),
                UnixNanos::default(),
                UnixNanos::default(),
                UnixNanos::default(),
                None,
            );
            report.avg_px = avg_px;
            report
        };
        let fill = |venue_order_id: &str, trade_id: &str, qty: u64, px: &str| {
            FillReport::new(
                account_id,
                instrument_id,
                VenueOrderId::new(venue_order_id),
                TradeId::new(trade_id),
                OrderSide::Buy,
                Quantity::from(qty),
                Price::from(px),
                Money::from("0.00 USD"),
                LiquiditySide::Taker,
                None,
                None,
                UnixNanos::default(),
                UnixNanos::default(),
                None,
            )
        };
        let mut reports = vec![
            order_report("PERM-1", None),
            order_report("PERM-2", Some(Decimal::from(7))),
            order_report("PERM-3", None),
        ];
        let fills = vec![
            fill("PERM-1", "T-1", 1, "2.55"),
            fill("PERM-1", "T-2", 2, "2.40"),
            fill("PERM-2", "T-3", 3, "9.00"),
        ];

        fill_missing_avg_px(&mut reports, &fills);

        assert_eq!(reports[0].avg_px, Some(Decimal::from_str("2.45").unwrap()));
        assert_eq!(reports[1].avg_px, Some(Decimal::from(7)));
        assert_eq!(reports[2].avg_px, None);
    }

    #[rstest]
    #[case::combo(ibapi::contracts::SecurityType::Spread, true)]
    #[case::leg(ibapi::contracts::SecurityType::FuturesOption, false)]
    fn test_is_combo_execution(
        #[case] security_type: ibapi::contracts::SecurityType,
        #[case] expected: bool,
    ) {
        let exec_data = ibapi::orders::ExecutionData {
            contract: ibapi::contracts::Contract {
                security_type,
                ..Default::default()
            },
            ..Default::default()
        };

        assert_eq!(is_combo_execution(&exec_data), expected);
    }

    #[rstest]
    fn test_ibalgo_with_zero_limit_price_maps_to_market() {
        assert_eq!(map_ib_order_type("IBALGO", Some(0.0)), OrderType::Market);
    }

    #[rstest]
    fn test_parse_execution_time_hyphenated_format() {
        let time_str = "20250225-15:15:00";
        let result = parse_execution_time(time_str);
        assert!(result.is_ok());
        let timestamp = result.unwrap();
        assert!(timestamp.as_i64() > 0);
    }

    #[rstest]
    fn test_parse_execution_time_with_met_timezone() {
        // Regression for European paper accounts that IB stamps with `MET`.
        // MET (CET) in February observes standard time (UTC+1).
        let met = parse_execution_time("20230223 00:43:36 MET").unwrap();
        let utc = parse_execution_time("20230223 00:43:36 Universal").unwrap();
        // Local 00:43:36 MET == 2023-02-22 23:43:36 UTC, i.e. 1 hour before UTC.
        assert_eq!(
            met.as_i64(),
            utc.as_i64() - 3_600_000_000_000,
            "MET (CET) should be 1h ahead of UTC in February"
        );
        assert!(met.as_i64() > 0);
    }

    #[rstest]
    fn test_parse_execution_time_applies_dst_for_regional_timezone() {
        // Same zone, two seasons: EST (UTC-5) in winter vs EDT (UTC-4) in summer.
        // Equal offsets would mean DST is NOT being applied - a real regression.
        let winter = parse_execution_time("20230223 00:43:36 America/New_York").unwrap();
        let summer = parse_execution_time("20230715 00:43:36 America/New_York").unwrap();
        let winter_utc = parse_execution_time("20230223 00:43:36 Universal").unwrap();
        let summer_utc = parse_execution_time("20230715 00:43:36 Universal").unwrap();
        assert_eq!(winter.as_i64(), winter_utc.as_i64() + 5 * 3_600_000_000_000); // EST
        assert_eq!(summer.as_i64(), summer_utc.as_i64() + 4 * 3_600_000_000_000); // EDT
    }

    #[rstest]
    fn test_parse_execution_time_dst_fall_back_fold_resolves_to_earliest() {
        // CME US/Central account (bebop23's case): on 2023-11-05 fall-back night
        // 01:30 America/Chicago occurs twice. Resolve to earliest (CDT, 06:30 UTC),
        // don't drop the fill.
        let fold = parse_execution_time("20231105 01:30:00 America/Chicago").unwrap();
        assert_eq!(
            fold.as_i64(),
            parse_execution_time("20231105 06:30:00 Universal")
                .unwrap()
                .as_i64()
        );
        assert_ne!(
            fold.as_i64(),
            parse_execution_time("20231105 07:30:00 Universal")
                .unwrap()
                .as_i64()
        );
    }

    #[rstest]
    fn test_parse_execution_time_dst_spring_forward_gap_errors() {
        // 02:30 America/Chicago never exists on 2023-03-12 spring-forward night.
        let gap = parse_execution_time("20230312 02:30:00 America/Chicago");
        assert!(gap.is_err());
    }

    #[rstest]
    fn test_parse_execution_time_fixed_offset_zone_without_dst() {
        // Asia/Tokyo is JST (UTC+9) year-round - guards the no-DST path.
        let tokyo = parse_execution_time("20230223 00:43:36 Asia/Tokyo").unwrap();
        let utc = parse_execution_time("20230223 00:43:36 Universal").unwrap();
        assert_eq!(tokyo.as_i64(), utc.as_i64() - 9 * 3_600_000_000_000);
    }

    #[rstest]
    fn test_parse_execution_time_with_unrecognized_timezone_errors() {
        let time_str = "20230223 00:43:36 Mars/Olympus";
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
            status: OrderStatusKind::Submitted,
            filled: 0.0,
            remaining: 100.0,
            average_fill_price: Some(0.0),
            perm_id: 0,
            parent_id: 0,
            last_fill_price: Some(0.0),
            client_id: 0,
            why_held: String::new(),
            market_cap_price: Some(0.0),
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
                "Unexpected error: {error_msg}"
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
            status: OrderStatusKind::Filled,
            filled: 100.0,
            remaining: 0.0,
            average_fill_price: Some(150.25),
            perm_id: 0,
            parent_id: 0,
            last_fill_price: Some(150.25),
            client_id: 0,
            why_held: String::new(),
            market_cap_price: Some(0.0),
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
                "Unexpected error: {error_msg}"
            );
        }
    }

    #[rstest]
    fn test_parse_order_status_to_report_spread_allows_negative_avg_fill_price() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = InstrumentId::new(
            Symbol::from("(1)SPY C400___((1))SPY C410"),
            Venue::from("SMART"),
        );
        let account_id = AccountId::from("IB-001");

        let order_status = OrderStatus {
            order_id: 12345,
            status: OrderStatusKind::Filled,
            filled: 1.0,
            remaining: 0.0,
            average_fill_price: Some(-2.25),
            perm_id: 0,
            parent_id: 0,
            last_fill_price: Some(-2.25),
            client_id: 0,
            why_held: String::new(),
            market_cap_price: Some(0.0),
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
            status: OrderStatusKind::Inactive,
            filled: 0.0,
            remaining: 100.0,
            average_fill_price: Some(0.0),
            perm_id: 0,
            parent_id: 0,
            last_fill_price: Some(0.0),
            client_id: 0,
            why_held: String::new(),
            market_cap_price: Some(0.0),
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
    fn test_parse_order_status_to_report_partial_fill_and_perm_fallback() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let account_id = AccountId::from("IB-001");

        let order_status = OrderStatus {
            order_id: 0,
            status: OrderStatusKind::Submitted,
            filled: 3.0,
            remaining: 7.0,
            average_fill_price: Some(150.25),
            perm_id: 123_456,
            parent_id: 0,
            last_fill_price: Some(150.25),
            client_id: 0,
            why_held: String::new(),
            market_cap_price: Some(0.0),
        };
        let order = Order {
            action: Action::Buy,
            total_quantity: 10.0,
            order_type: "LMT".to_string(),
            limit_price: Some(150.25),
            order_ref: "O-20260527-001:123".to_string(),
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

        assert_eq!(report.order_status, NautilusOrderStatus::PartiallyFilled);
        assert_eq!(report.venue_order_id.to_string(), "PERM-123456");
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("O-20260527-001"))
        );
    }

    #[rstest]
    fn test_parse_order_status_to_report_recovers_completed_order_quantity() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let order_status = OrderStatus {
            status: OrderStatusKind::Filled,
            filled: 3.0,
            remaining: 0.0,
            ..Default::default()
        };
        let order = Order {
            total_quantity: 0.0,
            filled_quantity: 3.0,
            ..Default::default()
        };

        let report = parse_order_status_to_report(
            &order_status,
            Some(&order),
            instrument_id,
            AccountId::from("IB-001"),
            &instrument_provider,
            UnixNanos::new(0),
        )
        .unwrap();

        assert_eq!(report.quantity, Quantity::from(3));
        assert_eq!(report.filled_qty, Quantity::from(3));
    }

    #[rstest]
    fn test_parse_order_status_to_report_sets_stop_trigger_type() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let order_status = OrderStatus {
            status: OrderStatusKind::Cancelled,
            remaining: 1.0,
            ..Default::default()
        };
        let order = Order {
            total_quantity: 1.0,
            order_type: "STP".to_string(),
            aux_price: Some(100.0),
            trigger_method: ibapi::orders::conditions::TriggerMethod::Last,
            ..Default::default()
        };

        let report = parse_order_status_to_report(
            &order_status,
            Some(&order),
            instrument_id,
            AccountId::from("IB-001"),
            &instrument_provider,
            UnixNanos::new(0),
        )
        .unwrap();

        assert_eq!(report.order_type, OrderType::StopMarket);
        assert_eq!(report.trigger_price, Some(Price::new(100.0, 0)));
        assert_eq!(report.trigger_type, Some(TriggerType::LastPrice));
    }

    #[rstest]
    #[case(
        "GTD",
        "20260925 20:00:00 UTC",
        TimeInForce::Gtd,
        Some(1_790_366_400_000_000_000)
    )]
    #[case(
        "GTC",
        "20260925-20:00:00",
        TimeInForce::Gtd,
        Some(1_790_366_400_000_000_000)
    )]
    #[case("GTD", "not a date", TimeInForce::Gtc, None)]
    #[case("DAY", "", TimeInForce::Day, None)]
    fn test_parse_order_status_to_report_sets_gtd_expire_time(
        #[case] tif: &str,
        #[case] good_till_date: &str,
        #[case] expected_time_in_force: TimeInForce,
        #[case] expected_expire_time: Option<u64>,
    ) {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let order_status = OrderStatus {
            status: OrderStatusKind::Filled,
            filled: 1.0,
            ..Default::default()
        };
        let order = Order {
            total_quantity: 1.0,
            order_type: "LMT".to_string(),
            limit_price: Some(101.0),
            tif: ibapi::orders::TimeInForce::from(tif),
            good_till_date: good_till_date.to_string(),
            ..Default::default()
        };

        let report = parse_order_status_to_report(
            &order_status,
            Some(&order),
            instrument_id,
            AccountId::from("IB-001"),
            &instrument_provider,
            UnixNanos::new(0),
        )
        .unwrap();

        assert_eq!(report.time_in_force, expected_time_in_force);
        assert_eq!(
            report.expire_time,
            expected_expire_time.map(UnixNanos::from)
        );
    }

    #[rstest]
    #[case("MKT", Some(0.0), OrderType::Market, None)]
    #[case("STP", Some(0.0), OrderType::StopMarket, None)]
    #[case("LMT", Some(101.0), OrderType::Limit, Some(Price::new(101.0, 0)))]
    fn test_parse_order_status_to_report_sets_price_only_for_limit_types(
        #[case] ib_order_type: &str,
        #[case] limit_price: Option<f64>,
        #[case] expected_order_type: OrderType,
        #[case] expected_price: Option<Price>,
    ) {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let order_status = OrderStatus {
            status: OrderStatusKind::Filled,
            filled: 1.0,
            ..Default::default()
        };
        let order = Order {
            total_quantity: 1.0,
            order_type: ib_order_type.to_string(),
            limit_price,
            aux_price: Some(100.0),
            ..Default::default()
        };

        let report = parse_order_status_to_report(
            &order_status,
            Some(&order),
            instrument_id,
            AccountId::from("IB-001"),
            &instrument_provider,
            UnixNanos::new(0),
        )
        .unwrap();

        assert_eq!(report.order_type, expected_order_type);
        assert_eq!(report.price, expected_price);
    }

    #[rstest]
    fn test_ib_venue_order_id_prefers_perm_id_and_falls_back_to_order_id() {
        assert_eq!(ib_venue_order_id(123, 456).to_string(), "PERM-456");
        assert_eq!(ib_venue_order_id(123, 0).to_string(), "123");
    }

    #[rstest]
    fn test_normalized_order_ref_strips_ib_suffix() {
        assert_eq!(normalized_order_ref("O-001:123"), Some("O-001"));
        assert_eq!(normalized_order_ref("O-001"), Some("O-001"));
        assert_eq!(normalized_order_ref(""), None);
    }

    #[rstest]
    #[case(Liquidity::AddedLiquidity, LiquiditySide::Maker)]
    #[case(Liquidity::RemovedLiquidity, LiquiditySide::Taker)]
    #[case(Liquidity::None, LiquiditySide::NoLiquiditySide)]
    #[case(Liquidity::LiquidityRoutedOut, LiquiditySide::NoLiquiditySide)]
    #[case(Liquidity::Unknown(4), LiquiditySide::NoLiquiditySide)]
    fn test_execution_liquidity_side(
        #[case] liquidity: Liquidity,
        #[case] expected: LiquiditySide,
    ) {
        assert_eq!(execution_liquidity_side(&liquidity), expected);
    }

    struct PricingExpectation {
        order_type: OrderType,
        price: Option<Price>,
        trigger_price: Option<Price>,
        limit_offset: Option<Decimal>,
        trailing_offset: Option<Decimal>,
        trailing_offset_type: Option<TrailingOffsetType>,
    }

    #[rstest]
    #[case(
        "MKT",
        None,
        None,
        None,
        None,
        PricingExpectation {
            order_type: OrderType::Market,
            price: None,
            trigger_price: None,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
        }
    )]
    #[case(
        "LMT",
        Some(185.0),
        None,
        None,
        None,
        PricingExpectation {
            order_type: OrderType::Limit,
            price: Some(Price::new(185.0, 0)),
            trigger_price: None,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
        }
    )]
    #[case(
        "IBALGO",
        Some(185.0),
        None,
        None,
        None,
        PricingExpectation {
            order_type: OrderType::Limit,
            price: Some(Price::new(185.0, 0)),
            trigger_price: None,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
        }
    )]
    #[case(
        "IBALGO",
        None,
        None,
        None,
        None,
        PricingExpectation {
            order_type: OrderType::Market,
            price: None,
            trigger_price: None,
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
        }
    )]
    #[case(
        "MIT",
        None,
        Some(180.0),
        None,
        None,
        PricingExpectation {
            order_type: OrderType::MarketIfTouched,
            price: None,
            trigger_price: Some(Price::new(180.0, 0)),
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
        }
    )]
    #[case(
        "LIT",
        Some(179.0),
        Some(180.0),
        None,
        None,
        PricingExpectation {
            order_type: OrderType::LimitIfTouched,
            price: Some(Price::new(179.0, 0)),
            trigger_price: Some(Price::new(180.0, 0)),
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
        }
    )]
    #[case(
        "STP",
        None,
        Some(180.0),
        None,
        None,
        PricingExpectation {
            order_type: OrderType::StopMarket,
            price: None,
            trigger_price: Some(Price::new(180.0, 0)),
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
        }
    )]
    #[case(
        "STP LMT",
        Some(179.0),
        Some(180.0),
        None,
        None,
        PricingExpectation {
            order_type: OrderType::StopLimit,
            price: Some(Price::new(179.0, 0)),
            trigger_price: Some(Price::new(180.0, 0)),
            limit_offset: None,
            trailing_offset: None,
            trailing_offset_type: None,
        }
    )]
    #[case(
        "TRAIL LIMIT",
        None,
        Some(2.5),
        Some(185.0),
        Some(0.25),
        PricingExpectation {
            order_type: OrderType::TrailingStopLimit,
            price: None,
            trigger_price: Some(Price::new(185.0, 0)),
            limit_offset: Some(Decimal::from_str("0.25").unwrap()),
            trailing_offset: Some(Decimal::from_str("2.5").unwrap()),
            trailing_offset_type: Some(TrailingOffsetType::Price),
        },
    )]
    fn test_parse_order_status_to_report_maps_pricing_fields_by_order_type(
        #[case] ib_order_type: &str,
        #[case] limit_price: Option<f64>,
        #[case] aux_price: Option<f64>,
        #[case] trail_stop_price: Option<f64>,
        #[case] limit_price_offset: Option<f64>,
        #[case] expected: PricingExpectation,
    ) {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let account_id = AccountId::from("IB-001");

        let order_status = OrderStatus {
            order_id: 12345,
            status: OrderStatusKind::Submitted,
            filled: 0.0,
            remaining: 5.0,
            average_fill_price: Some(0.0),
            perm_id: 0,
            parent_id: 0,
            last_fill_price: Some(0.0),
            client_id: 0,
            why_held: String::new(),
            market_cap_price: Some(0.0),
        };

        let order = Order {
            action: Action::Buy,
            total_quantity: 5.0,
            order_type: ib_order_type.to_string(),
            limit_price,
            aux_price,
            trail_stop_price,
            limit_price_offset,
            tif: ibapi::orders::TimeInForce::GoodTillCanceled,
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

        assert_eq!(report.order_type, expected.order_type);
        assert_eq!(report.price, expected.price);
        assert_eq!(report.trigger_price, expected.trigger_price);
        assert_eq!(report.limit_offset, expected.limit_offset);
        assert_eq!(report.trailing_offset, expected.trailing_offset);
        assert_eq!(report.trailing_offset_type, expected.trailing_offset_type);
    }

    #[rstest]
    fn test_parse_order_status_to_report_maps_trailing_percent_to_basis_points() {
        let instrument_provider = create_test_instrument_provider();
        let instrument_id = create_test_instrument_id();
        let account_id = AccountId::from("IB-001");

        let order_status = OrderStatus {
            order_id: 12345,
            status: OrderStatusKind::Submitted,
            filled: 0.0,
            remaining: 5.0,
            average_fill_price: Some(0.0),
            perm_id: 0,
            parent_id: 0,
            last_fill_price: Some(0.0),
            client_id: 0,
            why_held: String::new(),
            market_cap_price: Some(0.0),
        };

        let order = Order {
            action: Action::Buy,
            total_quantity: 5.0,
            order_type: "TRAIL".to_string(),
            trail_stop_price: Some(185.0),
            trailing_percent: Some(2.5),
            tif: ibapi::orders::TimeInForce::GoodTillCanceled,
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
        assert_eq!(
            report.trailing_offset_type,
            Some(TrailingOffsetType::BasisPoints),
        );
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
            side: ExecutionSide::Bought,
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
                    "Unexpected error: {error_msg}"
                );
            }
            Ok(fill) => {
                assert_eq!(fill.order_side, OrderSide::Buy);
                assert_eq!(fill.trade_id.to_string(), "EXEC-001");
            }
        }
    }

    #[rstest]
    fn test_parse_execution_to_fill_report_clamps_only_pending_commission_sentinel() {
        let instrument_provider = create_test_instrument_provider();
        let instrument = equity_aapl();
        let instrument_id = instrument.id();
        instrument_provider.insert_test_instrument(InstrumentAny::from(instrument), 265598, 1);
        let account_id = AccountId::from("IB-001");
        let contract = Contract::default();

        for (commission, expected) in [(-1.0, 0.0), (-0.25, -0.25)] {
            let execution = Execution {
                order_id: 12345,
                client_id: 0,
                execution_id: format!("EXEC-{commission}"),
                time: String::from("20230223 00:43:36 Universal"),
                account_number: String::new(),
                exchange: String::new(),
                side: ExecutionSide::Bought,
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

            let report = parse_execution_to_fill_report(
                &execution,
                &contract,
                commission,
                "USD",
                instrument_id,
                account_id,
                &instrument_provider,
                UnixNanos::new(0),
                None,
            )
            .unwrap();

            assert_eq!(report.commission, Money::new(expected, Currency::USD()));
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
            side: ExecutionSide::Sold,
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
                    "Unexpected error: {error_msg}"
                );
            }
            Ok(fill) => {
                assert_eq!(fill.order_side, OrderSide::Sell);
            }
        }
    }

    fn contingency_report(
        client_order_id: Option<&str>,
        venue_order_id: &str,
    ) -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("IB-001"),
            create_test_instrument_id(),
            client_order_id.map(ClientOrderId::from),
            VenueOrderId::from(venue_order_id),
            OrderSide::Sell.into(),
            OrderType::StopMarket,
            TimeInForce::Gtc,
            NautilusOrderStatus::Accepted,
            Quantity::from(1),
            Quantity::from(0),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        )
    }

    fn contingency_order(order_id: i32, perm_id: i64, parent_id: i32, oca: &str) -> Order {
        Order {
            order_id,
            perm_id,
            client_id: 7,
            parent_id,
            account: "DU123".to_string(),
            oca_group: oca.to_string(),
            oca_type: ibapi::orders::OcaType::ReduceWithBlock,
            ..Default::default()
        }
    }

    #[rstest]
    fn test_link_order_contingencies_restores_bracket_and_oca_group() {
        let mut reports = vec![
            contingency_report(Some("O-ENTRY"), "PERM-10"),
            contingency_report(Some("O-STOP"), "PERM-11"),
            contingency_report(None, "PERM-12"),
            contingency_report(Some("O-ALONE"), "PERM-13"),
        ];
        let orders = vec![
            contingency_order(1, 10, 0, ""),
            contingency_order(2, 11, 1, "EXIT-1"),
            contingency_order(3, 12, 1, "EXIT-1"),
            contingency_order(4, 13, 99, ""),
        ];

        link_order_contingencies(&mut reports, &orders);

        let links = reports
            .iter()
            .map(|report| {
                (
                    report.contingency_type,
                    report.linked_order_ids.clone(),
                    report.parent_order_id,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            links,
            vec![
                (
                    Some(ContingencyType::Oto),
                    Some(vec![
                        ClientOrderId::from("O-STOP"),
                        ClientOrderId::from("PERM-12")
                    ]),
                    None,
                ),
                (
                    Some(ContingencyType::Ouo),
                    Some(vec![ClientOrderId::from("PERM-12")]),
                    Some(ClientOrderId::from("O-ENTRY")),
                ),
                (
                    Some(ContingencyType::Ouo),
                    Some(vec![ClientOrderId::from("O-STOP")]),
                    Some(ClientOrderId::from("O-ENTRY")),
                ),
                (None, None, None),
            ]
        );
    }

    #[rstest]
    #[case::cancel(ibapi::orders::OcaType::CancelWithBlock, Some(ContingencyType::Oco))]
    #[case::reduce_without_block(
        ibapi::orders::OcaType::ReduceWithoutBlock,
        Some(ContingencyType::Ouo)
    )]
    #[case::unset(ibapi::orders::OcaType::None, None)]
    fn test_link_order_contingencies_maps_oca_type(
        #[case] oca_type: ibapi::orders::OcaType,
        #[case] expected: Option<ContingencyType>,
    ) {
        let mut reports = vec![
            contingency_report(Some("O-A"), "PERM-20"),
            contingency_report(Some("O-B"), "PERM-21"),
        ];
        let mut orders = vec![
            contingency_order(1, 20, 0, "GROUP"),
            contingency_order(2, 21, 0, "GROUP"),
        ];

        for order in &mut orders {
            order.oca_type = oca_type;
        }

        link_order_contingencies(&mut reports, &orders);

        assert_eq!(
            reports
                .iter()
                .map(|report| (report.contingency_type, report.linked_order_ids.clone()))
                .collect::<Vec<_>>(),
            vec![
                (expected, expected.map(|_| vec![ClientOrderId::from("O-B")])),
                (expected, expected.map(|_| vec![ClientOrderId::from("O-A")])),
            ]
        );
    }

    #[rstest]
    #[case::iceberg(Some(2), Some(Quantity::from(2)))]
    #[case::full_size(Some(5), None)]
    #[case::unset(Some(0), None)]
    fn test_parse_order_status_to_report_sets_display_qty(
        #[case] display_size: Option<i32>,
        #[case] expected: Option<Quantity>,
    ) {
        let instrument_provider = create_test_instrument_provider();
        let order = Order {
            total_quantity: 5.0,
            order_type: "LMT".to_string(),
            limit_price: Some(101.0),
            display_size,
            ..Default::default()
        };
        let order_status = OrderStatus {
            status: OrderStatusKind::Submitted,
            remaining: 5.0,
            ..Default::default()
        };

        let report = parse_order_status_to_report(
            &order_status,
            Some(&order),
            create_test_instrument_id(),
            AccountId::from("IB-001"),
            &instrument_provider,
            UnixNanos::new(0),
        )
        .unwrap();

        assert_eq!(report.display_qty, expected);
    }

    #[rstest]
    #[case::completed("20260924 12:02:04 UTC", 1_790_251_324_000_000_000)]
    #[case::open("", 7)]
    fn test_parse_order_data_to_report_uses_completed_time(
        #[case] completed_time: &str,
        #[case] expected_ts_last: u64,
    ) {
        let instrument_provider = create_test_instrument_provider();
        let mut data = OrderData {
            order_id: 1,
            order: Order {
                total_quantity: 1.0,
                order_type: "MKT".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        data.order_state.completed_time = completed_time.to_string();

        let report = parse_order_data_to_report(
            &data,
            create_test_instrument_id(),
            AccountId::from("IB-001"),
            &instrument_provider,
            UnixNanos::new(7),
        )
        .unwrap();

        assert_eq!(report.ts_last, UnixNanos::from(expected_ts_last));
    }
}
