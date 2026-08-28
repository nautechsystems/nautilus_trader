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

//! HTTP response parsing utilities for the Derive execution client.

use anyhow::Context;
use nautilus_core::{Params, UUID4, UnixNanos, datetime::NANOSECONDS_IN_MILLISECOND};
use nautilus_model::{
    enums::{LiquiditySide, OrderType, PositionSideSpecified},
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use serde_json::Value;

use crate::{
    common::{
        consts::DERIVE_VENUE,
        enums::{
            DeriveLiquidityRole, DeriveOrderSide, DeriveOrderStatus, DeriveOrderType,
            DeriveTimeInForce, DeriveTriggerType, DeriveTxStatus,
        },
        parse::{
            derive_order_side_to_nautilus, derive_order_type_to_nautilus_for_order,
            derive_rejection_due_post_only, derive_status_to_nautilus, derive_tif_to_nautilus,
            derive_trigger_price_type_to_nautilus,
        },
    },
    http::models::{DeriveOrder, DerivePosition, DeriveSubaccount, DeriveTrade},
};

/// Builds an [`OrderStatusReport`] from a Derive order record.
///
/// `client_order_id` is sourced from the `label` field on the order when the
/// label is non-empty; callers that need a specific client_order_id should
/// override via `with_client_order_id` after this call.
/// Trailing zero padding is removed without changing the value.
///
/// # Errors
///
/// Returns an error when any decimal field cannot be converted to a Nautilus
/// `Price` or `Quantity`.
pub fn parse_derive_order_to_report(
    order: &DeriveOrder,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    parse_derive_order_to_report_with_precision(order, account_id, None, None, ts_init)
}

pub(crate) fn parse_derive_order_to_report_with_precision(
    order: &DeriveOrder,
    account_id: AccountId,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id =
        InstrumentId::new(Symbol::new(order.instrument_name.as_str()), *DERIVE_VENUE);
    let venue_order_id = VenueOrderId::new(order.order_id.as_str());
    let order_side = derive_order_side_to_nautilus(order.direction);
    let order_type = derive_order_type_to_nautilus_for_report(order);
    let post_only = matches!(order.time_in_force, DeriveTimeInForce::PostOnly);
    let time_in_force = derive_tif_to_nautilus(order.time_in_force);
    let order_status =
        derive_status_to_nautilus(order.order_status, order.filled_amount, order.amount);
    let quantity = quantity_from_decimal(order.amount, size_precision, "amount")?;
    let filled_qty = quantity_from_decimal(order.filled_amount, size_precision, "filled_amount")?;

    let ts_accepted = ms_to_nanos(order.creation_timestamp);
    let ts_last = ms_to_nanos(order.last_update_timestamp);

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_accepted,
        ts_last,
        ts_init,
        Some(UUID4::new()),
    );

    if !order.label.as_str().is_empty() {
        let client_order_id = ClientOrderId::new(order.label.as_str());
        report = report.with_client_order_id(client_order_id);
    }

    if order.limit_price > Decimal::ZERO
        && order_type_has_limit_price(order_type)
        && let Ok(price) = price_from_decimal(order.limit_price, price_precision, "limit_price")
    {
        report = report.with_price(price);
    }

    if let Some(trigger_price) = order.trigger_price
        && trigger_price > Decimal::ZERO
        && let Ok(price) = price_from_decimal(trigger_price, price_precision, "trigger_price")
    {
        report = report.with_trigger_price(price);
    }

    if let Some(trigger_price_type) = order.trigger_price_type {
        report =
            report.with_trigger_type(derive_trigger_price_type_to_nautilus(trigger_price_type));
    }

    if order.average_price > Decimal::ZERO {
        report.avg_px = Some(order.average_price);
    }
    report.post_only = post_only;
    let trigger_reject_message = order
        .trigger_reject_message
        .as_deref()
        .filter(|message| !message.is_empty())
        .map(str::to_string);
    let cancel_reason = trigger_reject_message
        .clone()
        .unwrap_or_else(|| order.cancel_reason.to_string());
    if order.order_status == DeriveOrderStatus::Cancelled
        || (order.order_status == DeriveOrderStatus::Rejected
            && (trigger_reject_message.is_some()
                || derive_rejection_due_post_only(None, &cancel_reason)))
    {
        report.cancel_reason = Some(cancel_reason);
    }
    Ok(report)
}

fn order_type_has_limit_price(order_type: OrderType) -> bool {
    matches!(
        order_type,
        OrderType::Limit | OrderType::StopLimit | OrderType::LimitIfTouched
    )
}

fn derive_order_type_to_nautilus_for_report(order: &DeriveOrder) -> OrderType {
    let order_type = derive_order_type_to_nautilus_for_order(order.order_type, order.trigger_type);
    if order_type != OrderType::LimitIfTouched {
        return order_type;
    }

    match (order.order_type, order.trigger_type, order.trigger_price) {
        (DeriveOrderType::Limit, Some(DeriveTriggerType::Takeprofit), Some(trigger_price))
            if !limit_if_touched_prices_are_valid(
                order.direction,
                order.limit_price,
                trigger_price,
            ) =>
        {
            OrderType::StopLimit
        }
        _ => order_type,
    }
}

fn limit_if_touched_prices_are_valid(
    direction: DeriveOrderSide,
    limit_price: Decimal,
    trigger_price: Decimal,
) -> bool {
    match direction {
        DeriveOrderSide::Buy => trigger_price <= limit_price,
        DeriveOrderSide::Sell => trigger_price >= limit_price,
    }
}

/// Builds a [`FillReport`] from a Derive trade record.
///
/// Quote-currency commission is reported in the same currency as the
/// instrument's settlement (USDC for perps and options). `client_order_id`
/// is sourced from the trade `label` when populated.
/// Trailing zero padding is removed without changing the value.
///
/// # Errors
///
/// Returns an error when any decimal field cannot be converted to a Nautilus
/// `Price`, `Quantity`, or `Money`.
pub fn parse_derive_trade_to_fill_report(
    trade: &DeriveTrade,
    account_id: AccountId,
    fee_currency: Currency,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<FillReport>> {
    parse_derive_trade_to_fill_report_with_precision(
        trade,
        account_id,
        fee_currency,
        None,
        None,
        ts_init,
    )
}

pub(crate) fn parse_derive_trade_to_fill_report_with_precision(
    trade: &DeriveTrade,
    account_id: AccountId,
    fee_currency: Currency,
    price_precision: Option<u8>,
    size_precision: Option<u8>,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<FillReport>> {
    // The venue ships pending settlements with an empty trade_id and tx_hash;
    // those rows would otherwise collapse identity-aware deduplication, so we
    // skip them and let a later poll observe the settled trade.
    if trade.trade_id.is_empty() || trade.tx_status == DeriveTxStatus::Reverted {
        return Ok(None);
    }

    let instrument_id =
        InstrumentId::new(Symbol::new(trade.instrument_name.as_str()), *DERIVE_VENUE);
    let venue_order_id = VenueOrderId::new(trade.order_id.as_str());
    let trade_id = TradeId::new(trade.trade_id.as_str());
    let order_side = derive_order_side_to_nautilus(trade.direction);
    let last_qty = quantity_from_decimal(trade.trade_amount, size_precision, "trade_amount")?;
    let last_px = price_from_decimal(trade.trade_price, price_precision, "trade_price")?;
    let commission = commission_from_decimal(trade.trade_fee, fee_currency)?;
    let liquidity_side = match trade.liquidity_role {
        DeriveLiquidityRole::Maker => LiquiditySide::Maker,
        DeriveLiquidityRole::Taker => LiquiditySide::Taker,
        DeriveLiquidityRole::Unknown => LiquiditySide::NoLiquiditySide,
    };

    let client_order_id = if trade.label.as_str().is_empty() {
        None
    } else {
        Some(ClientOrderId::new(trade.label.as_str()))
    };

    let ts_event = ms_to_nanos(trade.timestamp);

    Ok(Some(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        client_order_id,
        None,
        ts_event,
        ts_init,
        Some(UUID4::new()),
    )))
}

/// Builds a [`PositionStatusReport`] from a Derive position record.
///
/// Trailing zero padding is removed without changing the value.
///
/// # Errors
///
/// Returns an error when the position amount cannot be converted to a
/// Nautilus `Quantity`.
pub fn parse_derive_position_to_report(
    position: &DerivePosition,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    parse_derive_position_to_report_with_precision(position, account_id, None, ts_init)
}

pub(crate) fn parse_derive_position_to_report_with_precision(
    position: &DerivePosition,
    account_id: AccountId,
    size_precision: Option<u8>,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let instrument_id = InstrumentId::new(
        Symbol::new(position.instrument_name.as_str()),
        *DERIVE_VENUE,
    );
    let signed_amount = position.amount;
    let side = if signed_amount > Decimal::ZERO {
        PositionSideSpecified::Long
    } else if signed_amount < Decimal::ZERO {
        PositionSideSpecified::Short
    } else {
        PositionSideSpecified::Flat
    };
    let abs_amount = signed_amount.abs();
    let quantity = quantity_from_decimal(abs_amount, size_precision, "position.amount")?;

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        side,
        quantity,
        ts_init,
        ts_init,
        Some(UUID4::new()),
        None,
        Some(position.average_price),
    ))
}

/// Derives [`AccountBalance`], [`MarginBalance`], and supplemental info rows
/// from a [`DeriveSubaccount`] snapshot.
///
/// Each collateral row becomes one [`AccountBalance`] in the collateral's own
/// units with `total = amount` and `locked = 0`: the venue holds margin at the
/// subaccount level and reports no per-collateral reservation, while
/// `collaterals[].initial_margin` is USD credit contributed, not locked funds.
///
/// Portfolio requirements collapse into a single account-wide [`MarginBalance`]
/// in the subaccount currency: `initial = positions_initial_margin +
/// open_orders_margin` and `maintenance = positions_maintenance_margin`. The
/// subaccount's `initial_margin`/`maintenance_margin` are signed net health
/// values, not requirements, so they travel in the returned [`Params`] as
/// `net_initial_margin`/`net_maintenance_margin` alongside the requirement
/// split and the liquidation flag.
///
/// # Errors
///
/// Returns an error when a decimal field cannot be represented at the
/// currency precision used by [`Money`].
pub fn parse_derive_subaccount_to_balances(
    subaccount: &DeriveSubaccount,
) -> anyhow::Result<(Vec<AccountBalance>, Vec<MarginBalance>, Params)> {
    let mut balances = Vec::with_capacity(subaccount.collaterals.len());
    for collateral in &subaccount.collaterals {
        let currency = Currency::get_or_create_crypto(collateral.asset_name.as_str());
        let balance =
            AccountBalance::from_total_and_locked(collateral.amount, Decimal::ZERO, currency)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "failed to build collateral balance for {} (total={}): {e}",
                        collateral.asset_name,
                        collateral.amount,
                    )
                })?;
        balances.push(balance);
    }

    let currency = Currency::get_or_create_crypto(subaccount.currency.as_str());
    let initial_dec = subaccount.positions_initial_margin + subaccount.open_orders_margin;
    let maintenance_dec = subaccount.positions_maintenance_margin;
    let initial = Money::from_decimal(initial_dec, currency).with_context(|| {
        format!(
            "initial margin requirement {initial_dec} cannot be represented at {currency} precision",
        )
    })?;
    let maintenance =
        Money::from_decimal(maintenance_dec, currency).with_context(|| {
            format!(
                "maintenance margin requirement {maintenance_dec} cannot be represented at {currency} precision",
            )
        })?;
    let margins = vec![MarginBalance::new(initial, maintenance, None)];

    let mut info = Params::new();
    info.insert(
        "net_initial_margin".to_string(),
        Value::String(subaccount.initial_margin.to_string()),
    );
    info.insert(
        "net_maintenance_margin".to_string(),
        Value::String(subaccount.maintenance_margin.to_string()),
    );
    info.insert(
        "positions_initial_margin".to_string(),
        Value::String(subaccount.positions_initial_margin.to_string()),
    );
    info.insert(
        "positions_maintenance_margin".to_string(),
        Value::String(subaccount.positions_maintenance_margin.to_string()),
    );
    info.insert(
        "open_orders_margin".to_string(),
        Value::String(subaccount.open_orders_margin.to_string()),
    );
    info.insert(
        "is_under_liquidation".to_string(),
        Value::Bool(subaccount.is_under_liquidation),
    );

    Ok((balances, margins, info))
}

fn price_from_decimal(value: Decimal, precision: Option<u8>, field: &str) -> anyhow::Result<Price> {
    match precision {
        Some(precision) => Price::from_decimal_dp(value, precision),
        None => Price::from_decimal(value.normalize()),
    }
    .with_context(|| format!("invalid Derive {field}"))
}

fn quantity_from_decimal(
    value: Decimal,
    precision: Option<u8>,
    field: &str,
) -> anyhow::Result<Quantity> {
    match precision {
        Some(precision) => Quantity::from_decimal_dp(value, precision),
        None => Quantity::from_decimal(value.normalize()),
    }
    .with_context(|| format!("invalid Derive {field}"))
}

fn commission_from_decimal(value: Decimal, currency: Currency) -> anyhow::Result<Money> {
    Money::from_decimal(value, currency)
        .with_context(|| format!("trade_fee {value} cannot be represented at {currency} precision"))
}

fn ms_to_nanos(value: i64) -> UnixNanos {
    let clamped = u64::try_from(value.max(0)).unwrap_or(0);
    UnixNanos::from(clamped.saturating_mul(NANOSECONDS_IN_MILLISECOND))
}

#[cfg(test)]
mod tests {
    use nautilus_model::enums::{OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        common::{
            enums::{
                DeriveAssetType, DeriveInstrumentType, DeriveLiquidityRole, DeriveMarginType,
                DeriveOrderCancelReason, DeriveOrderSide, DeriveOrderStatus, DeriveOrderType,
                DeriveTimeInForce, DeriveTriggerPriceType, DeriveTriggerType, DeriveTxStatus,
            },
            parse::{
                derive_status_to_nautilus, order_side_to_derive, order_type_to_derive,
                time_in_force_to_derive,
            },
        },
        http::models::DeriveCollateral,
    };

    fn sample_order() -> DeriveOrder {
        DeriveOrder {
            amount: dec!(10),
            average_price: dec!(3500),
            cancel_reason: DeriveOrderCancelReason::Empty,
            creation_timestamp: 1_700_000_000_000,
            direction: DeriveOrderSide::Buy,
            filled_amount: dec!(4),
            instrument_name: "ETH-PERP".into(),
            is_transfer: false,
            label: "STRATEGY-1-O-1".into(),
            last_update_timestamp: 1_700_000_001_000,
            limit_price: dec!(3500),
            max_fee: dec!(1),
            mmp: false,
            nonce: 1,
            order_fee: dec!(0),
            order_id: "ord-1".to_string(),
            order_status: DeriveOrderStatus::Open,
            order_type: DeriveOrderType::Limit,
            quote_id: None,
            replaced_order_id: None,
            signature: "0x00".to_string(),
            signature_expiry_sec: 1_700_000_999,
            signer: "0xsigner".into(),
            subaccount_id: 30769,
            time_in_force: DeriveTimeInForce::Gtc,
            trigger_price: None,
            trigger_price_type: None,
            trigger_reject_message: None,
            trigger_type: None,
        }
    }

    fn sample_trade() -> DeriveTrade {
        DeriveTrade {
            direction: DeriveOrderSide::Sell,
            index_price: dec!(3500),
            instrument_name: "ETH-PERP".into(),
            is_transfer: false,
            label: "STRATEGY-1-O-2".into(),
            liquidity_role: DeriveLiquidityRole::Taker,
            mark_price: dec!(3500),
            order_id: "ord-2".to_string(),
            quote_id: None,
            realized_pnl: dec!(0),
            subaccount_id: 30769,
            timestamp: 1_700_000_002_000,
            trade_amount: dec!(2),
            trade_fee: dec!(0.5),
            trade_id: "tr-1".to_string(),
            trade_price: dec!(3505),
            tx_hash: Some("0xabc".to_string()),
            tx_status: DeriveTxStatus::Settled,
            wallet: Some("0xwallet".into()),
        }
    }

    #[rstest]
    fn test_order_side_round_trip() {
        assert_eq!(
            order_side_to_derive(OrderSide::Buy).unwrap(),
            DeriveOrderSide::Buy,
        );
        assert_eq!(
            order_side_to_derive(OrderSide::Sell).unwrap(),
            DeriveOrderSide::Sell,
        );
        assert!(order_side_to_derive(OrderSide::NoOrderSide).is_err());
    }

    #[rstest]
    fn test_order_type_rejects_unsupported() {
        assert_eq!(
            order_type_to_derive(OrderType::Limit).unwrap(),
            DeriveOrderType::Limit,
        );
        assert_eq!(
            order_type_to_derive(OrderType::Market).unwrap(),
            DeriveOrderType::Market,
        );
        assert!(order_type_to_derive(OrderType::StopMarket).is_err());
    }

    #[rstest]
    #[case(TimeInForce::Gtc, false, DeriveTimeInForce::Gtc)]
    #[case(TimeInForce::Gtc, true, DeriveTimeInForce::PostOnly)]
    #[case(TimeInForce::Ioc, false, DeriveTimeInForce::Ioc)]
    #[case(TimeInForce::Fok, false, DeriveTimeInForce::Fok)]
    fn test_time_in_force_maps_supported_values(
        #[case] tif: TimeInForce,
        #[case] post_only: bool,
        #[case] expected: DeriveTimeInForce,
    ) {
        assert_eq!(time_in_force_to_derive(tif, post_only).unwrap(), expected);
    }

    #[rstest]
    #[case(TimeInForce::Ioc)]
    #[case(TimeInForce::Fok)]
    fn test_time_in_force_rejects_post_only_immediate_values(#[case] tif: TimeInForce) {
        let err = time_in_force_to_derive(tif, true)
            .expect_err("post-only immediate TIF must be rejected");

        assert!(
            err.to_string()
                .contains("post-only Derive orders only support GTC"),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    #[case(TimeInForce::Gtd, false)]
    #[case(TimeInForce::Gtd, true)]
    #[case(TimeInForce::Day, false)]
    #[case(TimeInForce::Day, true)]
    #[case(TimeInForce::AtTheOpen, false)]
    #[case(TimeInForce::AtTheOpen, true)]
    #[case(TimeInForce::AtTheClose, false)]
    #[case(TimeInForce::AtTheClose, true)]
    fn test_time_in_force_rejects_unsupported(#[case] tif: TimeInForce, #[case] post_only: bool) {
        let err = time_in_force_to_derive(tif, post_only).expect_err("must reject unsupported TIF");

        assert!(
            err.to_string().contains("unsupported time in force"),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    fn test_derive_status_partial_fill_classification() {
        assert_eq!(
            derive_status_to_nautilus(DeriveOrderStatus::Open, dec!(0), dec!(10)),
            OrderStatus::Accepted,
        );
        assert_eq!(
            derive_status_to_nautilus(DeriveOrderStatus::Open, dec!(4), dec!(10)),
            OrderStatus::PartiallyFilled,
        );
        assert_eq!(
            derive_status_to_nautilus(DeriveOrderStatus::Filled, dec!(10), dec!(10)),
            OrderStatus::Filled,
        );
        assert_eq!(
            derive_status_to_nautilus(DeriveOrderStatus::Cancelled, dec!(0), dec!(10)),
            OrderStatus::Canceled,
        );
    }

    #[rstest]
    fn test_parse_order_report_assigns_partial_fill_status() {
        let account_id = AccountId::new("DERIVE-001");
        let report =
            parse_derive_order_to_report(&sample_order(), account_id, UnixNanos::from(1)).unwrap();
        assert_eq!(report.order_status, OrderStatus::PartiallyFilled);
        assert_eq!(report.quantity, Quantity::from("10"));
        assert_eq!(report.filled_qty, Quantity::from("4"));
        assert_eq!(report.client_order_id.unwrap().as_str(), "STRATEGY-1-O-1");
        assert_eq!(report.venue_order_id.as_str(), "ord-1");
    }

    #[rstest]
    fn test_parse_order_report_normalizes_without_instrument_precision() {
        let mut order = sample_order();
        order.amount = Decimal::from_str_exact("0.100000000000000000").unwrap();
        order.filled_amount = Decimal::from_str_exact("0.000000000000000000").unwrap();
        order.limit_price = Decimal::from_str_exact("0.100000000000000000").unwrap();
        order.average_price = Decimal::ZERO;
        order.order_status = DeriveOrderStatus::Cancelled;
        let account_id = AccountId::new("DERIVE-001");

        let report = parse_derive_order_to_report(&order, account_id, UnixNanos::from(1)).unwrap();

        assert_eq!(report.quantity, Quantity::from("0.1"));
        assert_eq!(report.filled_qty, Quantity::from("0"));
        assert_eq!(report.price, Some(Price::from("0.1")));
    }

    #[rstest]
    fn test_parse_order_report_uses_instrument_precision() {
        let mut order = sample_order();
        order.amount = Decimal::from_str_exact("25.000").unwrap();
        order.filled_amount = Decimal::from_str_exact("5.000").unwrap();
        order.limit_price = Decimal::from_str_exact("25.000").unwrap();

        let report = parse_derive_order_to_report_with_precision(
            &order,
            AccountId::new("DERIVE-001"),
            Some(2),
            Some(2),
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(report.quantity, Quantity::from("25.00"));
        assert_eq!(report.quantity.precision, 2);
        assert_eq!(report.filled_qty, Quantity::from("5.00"));
        assert_eq!(report.filled_qty.precision, 2);
        assert_eq!(report.price, Some(Price::from("25.00")));
        assert_eq!(report.price.unwrap().precision, 2);
    }

    #[rstest]
    fn test_parse_order_report_maps_untriggered_stop_market() {
        let mut order = sample_order();
        order.average_price = Decimal::ZERO;
        order.filled_amount = Decimal::ZERO;
        order.limit_price = dec!(3400);
        order.order_status = DeriveOrderStatus::Untriggered;
        order.order_type = DeriveOrderType::Market;
        order.trigger_price = Some(dec!(3450));
        order.trigger_price_type = Some(DeriveTriggerPriceType::Mark);
        order.trigger_type = Some(DeriveTriggerType::Stoploss);
        let account_id = AccountId::new("DERIVE-001");

        let report = parse_derive_order_to_report(&order, account_id, UnixNanos::from(1)).unwrap();

        assert_eq!(report.order_type, OrderType::StopMarket);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.price, None);
        assert_eq!(report.trigger_price, Some(Price::from("3450")));
        assert_eq!(report.trigger_type, Some(TriggerType::MarkPrice));
    }

    #[rstest]
    #[case(DeriveOrderSide::Buy, dec!(3700), dec!(3600))]
    #[case(DeriveOrderSide::Buy, dec!(3700), dec!(3700))]
    #[case(DeriveOrderSide::Sell, dec!(3700), dec!(3800))]
    #[case(DeriveOrderSide::Sell, dec!(3700), dec!(3700))]
    fn test_parse_order_report_maps_limit_if_touched_trigger(
        #[case] direction: DeriveOrderSide,
        #[case] limit_price: Decimal,
        #[case] trigger_price: Decimal,
    ) {
        let mut order = sample_order();
        order.average_price = Decimal::ZERO;
        order.direction = direction;
        order.filled_amount = Decimal::ZERO;
        order.limit_price = limit_price;
        order.order_status = DeriveOrderStatus::Untriggered;
        order.order_type = DeriveOrderType::Limit;
        order.trigger_price = Some(trigger_price);
        order.trigger_price_type = Some(DeriveTriggerPriceType::Index);
        order.trigger_type = Some(DeriveTriggerType::Takeprofit);
        let account_id = AccountId::new("DERIVE-001");

        let report = parse_derive_order_to_report(&order, account_id, UnixNanos::from(1)).unwrap();

        assert_eq!(report.order_type, OrderType::LimitIfTouched);
        assert_eq!(
            report.price,
            Some(Price::from_decimal(limit_price.normalize()).unwrap())
        );
        assert_eq!(
            report.trigger_price,
            Some(Price::from_decimal(trigger_price.normalize()).unwrap())
        );
        assert_eq!(report.trigger_type, Some(TriggerType::IndexPrice));
    }

    #[rstest]
    #[case(DeriveOrderSide::Buy, dec!(3700), dec!(3800))]
    #[case(DeriveOrderSide::Sell, dec!(3700), dec!(3600))]
    fn test_parse_order_report_maps_take_profit_limit_with_stop_shape(
        #[case] direction: DeriveOrderSide,
        #[case] limit_price: Decimal,
        #[case] trigger_price: Decimal,
    ) {
        let mut order = sample_order();
        order.average_price = Decimal::ZERO;
        order.direction = direction;
        order.filled_amount = Decimal::ZERO;
        order.limit_price = limit_price;
        order.order_status = DeriveOrderStatus::Untriggered;
        order.order_type = DeriveOrderType::Limit;
        order.trigger_price = Some(trigger_price);
        order.trigger_price_type = Some(DeriveTriggerPriceType::Index);
        order.trigger_type = Some(DeriveTriggerType::Takeprofit);
        let account_id = AccountId::new("DERIVE-001");

        let report = parse_derive_order_to_report(&order, account_id, UnixNanos::from(1)).unwrap();

        assert_eq!(report.order_type, OrderType::StopLimit);
        assert_eq!(
            report.price,
            Some(Price::from_decimal(limit_price.normalize()).unwrap())
        );
        assert_eq!(
            report.trigger_price,
            Some(Price::from_decimal(trigger_price.normalize()).unwrap())
        );
        assert_eq!(report.trigger_type, Some(TriggerType::IndexPrice));
    }

    #[rstest]
    fn test_parse_rejected_post_only_report_keeps_cross_market_reason() {
        let mut order = sample_order();
        order.cancel_reason = DeriveOrderCancelReason::PostOnlyCrossMarket;
        order.order_status = DeriveOrderStatus::Rejected;
        order.time_in_force = DeriveTimeInForce::PostOnly;
        let account_id = AccountId::new("DERIVE-001");

        let report = parse_derive_order_to_report(&order, account_id, UnixNanos::from(1)).unwrap();

        assert_eq!(report.order_status, OrderStatus::Rejected);
        assert!(report.post_only);
        assert_eq!(
            report.cancel_reason.as_deref(),
            Some("Post only order cannot cross the market")
        );
    }

    #[rstest]
    fn test_parse_rejected_trigger_report_uses_trigger_message() {
        let mut order = sample_order();
        order.cancel_reason = DeriveOrderCancelReason::TriggerFailed;
        order.order_status = DeriveOrderStatus::Rejected;
        order.trigger_reject_message = Some("trigger price moved through limit".to_string());
        let account_id = AccountId::new("DERIVE-001");

        let report = parse_derive_order_to_report(&order, account_id, UnixNanos::from(1)).unwrap();

        assert_eq!(report.order_status, OrderStatus::Rejected);
        assert_eq!(
            report.cancel_reason.as_deref(),
            Some("trigger price moved through limit")
        );
    }

    #[rstest]
    fn test_parse_trade_report_emits_taker_fill() {
        let account_id = AccountId::new("DERIVE-001");
        let usdc = Currency::USDC();
        let report = parse_derive_trade_to_fill_report(
            &sample_trade(),
            account_id,
            usdc,
            UnixNanos::from(2),
        )
        .unwrap()
        .unwrap();
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.last_qty, Quantity::from("2"));
        assert_eq!(report.last_px, Price::from("3505"));
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(report.commission.as_decimal(), dec!(0.5));
    }

    #[rstest]
    fn test_parse_trade_report_uses_instrument_precision() {
        let mut trade = sample_trade();
        trade.trade_amount = Decimal::from_str_exact("25.000").unwrap();
        trade.trade_price = Decimal::from_str_exact("25.000").unwrap();

        let report = parse_derive_trade_to_fill_report_with_precision(
            &trade,
            AccountId::new("DERIVE-001"),
            Currency::USDC(),
            Some(2),
            Some(3),
            UnixNanos::from(2),
        )
        .unwrap()
        .unwrap();

        assert_eq!(report.last_px, Price::from("25.00"));
        assert_eq!(report.last_px.precision, 2);
        assert_eq!(report.last_qty, Quantity::from("25.000"));
        assert_eq!(report.last_qty.precision, 3);
    }

    #[rstest]
    #[case(DeriveLiquidityRole::Taker, LiquiditySide::Taker)]
    #[case(DeriveLiquidityRole::Maker, LiquiditySide::Maker)]
    fn test_parse_trade_report_preserves_exact_decimal_commission(
        #[case] liquidity_role: DeriveLiquidityRole,
        #[case] expected_liquidity_side: LiquiditySide,
    ) {
        let mut trade = sample_trade();
        trade.trade_fee = dec!(0.12345678);
        trade.liquidity_role = liquidity_role;
        let account_id = AccountId::new("DERIVE-001");
        let usdc = Currency::USDC();
        let report =
            parse_derive_trade_to_fill_report(&trade, account_id, usdc, UnixNanos::from(2))
                .unwrap()
                .expect("exact USDC-precision fee must emit the fill");
        assert_eq!(report.commission.as_decimal(), dec!(0.12345678));
        assert_eq!(report.commission.currency, usdc);
        assert_eq!(report.liquidity_side, expected_liquidity_side);
    }

    #[rstest]
    #[case(dec!(0.000000025), dec!(0.00000002))]
    #[case(dec!(0.000000015), dec!(0.00000002))]
    fn test_parse_trade_report_rounds_half_unit_commission_from_decimal(
        #[case] trade_fee: Decimal,
        #[case] expected: Decimal,
    ) {
        // These half-unit values are where the old f64 path diverged
        let mut trade = sample_trade();
        trade.trade_fee = trade_fee;
        let account_id = AccountId::new("DERIVE-001");
        let report = parse_derive_trade_to_fill_report(
            &trade,
            account_id,
            Currency::USDC(),
            UnixNanos::from(2),
        )
        .unwrap()
        .expect("sub-precision fee must still emit the fill");
        assert_eq!(report.commission.as_decimal(), expected);
    }

    #[rstest]
    fn test_parse_trade_report_errors_on_out_of_range_commission() {
        let mut trade = sample_trade();
        trade.trade_fee = Decimal::MAX;
        let account_id = AccountId::new("DERIVE-001");
        let err = parse_derive_trade_to_fill_report(
            &trade,
            account_id,
            Currency::USDC(),
            UnixNanos::from(2),
        )
        .expect_err("out-of-range fee must error instead of panicking");
        assert!(
            err.to_string().contains("trade_fee"),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    fn test_parse_trade_report_skips_reverted_settlement() {
        let mut trade = sample_trade();
        trade.tx_status = DeriveTxStatus::Reverted;
        let account_id = AccountId::new("DERIVE-001");
        let usdc = Currency::USDC();
        let report =
            parse_derive_trade_to_fill_report(&trade, account_id, usdc, UnixNanos::from(2))
                .unwrap();
        assert!(report.is_none());
    }

    #[rstest]
    fn test_parse_trade_report_degrades_unknown_liquidity_role() {
        let mut trade = sample_trade();
        trade.liquidity_role = DeriveLiquidityRole::Unknown;
        let account_id = AccountId::new("DERIVE-001");
        let usdc = Currency::USDC();

        let report =
            parse_derive_trade_to_fill_report(&trade, account_id, usdc, UnixNanos::from(2))
                .unwrap()
                .expect("unknown liquidity role must still emit the fill");

        assert_eq!(report.liquidity_side, LiquiditySide::NoLiquiditySide);
    }

    #[rstest]
    fn test_parse_position_long_short_flat() {
        let account_id = AccountId::new("DERIVE-001");

        let mut long_pos = sample_position();
        long_pos.amount = dec!(3);
        let report =
            parse_derive_position_to_report(&long_pos, account_id, UnixNanos::from(3)).unwrap();
        assert_eq!(report.position_side, PositionSideSpecified::Long);
        assert_eq!(report.quantity, Quantity::from("3"));

        let mut short_pos = sample_position();
        short_pos.amount = dec!(-2);
        let report =
            parse_derive_position_to_report(&short_pos, account_id, UnixNanos::from(3)).unwrap();
        assert_eq!(report.position_side, PositionSideSpecified::Short);
        assert_eq!(report.quantity, Quantity::from("2"));

        let mut flat_pos = sample_position();
        flat_pos.amount = dec!(0);
        let report =
            parse_derive_position_to_report(&flat_pos, account_id, UnixNanos::from(3)).unwrap();
        assert_eq!(report.position_side, PositionSideSpecified::Flat);
    }

    #[rstest]
    fn test_parse_position_report_uses_instrument_precision() {
        let mut position = sample_position();
        position.amount = Decimal::from_str_exact("25.000").unwrap();

        let report = parse_derive_position_to_report_with_precision(
            &position,
            AccountId::new("DERIVE-001"),
            Some(3),
            UnixNanos::from(3),
        )
        .unwrap();

        assert_eq!(report.quantity, Quantity::from("25.000"));
        assert_eq!(report.quantity.precision, 3);
    }

    fn sample_position() -> DerivePosition {
        DerivePosition {
            amount: dec!(0),
            average_price: dec!(3500),
            creation_timestamp: 0,
            cumulative_funding: dec!(0),
            delta: dec!(0),
            gamma: dec!(0),
            index_price: dec!(3500),
            initial_margin: dec!(0),
            instrument_name: "ETH-PERP".into(),
            instrument_type: DeriveInstrumentType::Perp,
            leverage: None,
            liquidation_price: None,
            maintenance_margin: dec!(0),
            mark_price: dec!(3500),
            mark_value: dec!(0),
            net_settlements: dec!(0),
            open_orders_margin: dec!(0),
            pending_funding: dec!(0),
            realized_pnl: dec!(0),
            theta: dec!(0),
            unrealized_pnl: dec!(0),
            vega: dec!(0),
        }
    }

    #[rstest]
    fn test_parse_subaccount_emits_balances_margins_and_info() {
        let subaccount = sample_subaccount();
        let (balances, margins, info) = parse_derive_subaccount_to_balances(&subaccount).unwrap();
        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].total.as_decimal(), dec!(1000));
        assert_eq!(balances[0].locked.as_decimal(), dec!(0));
        assert_eq!(balances[0].free.as_decimal(), dec!(1000));
        assert_eq!(margins.len(), 1);
        assert_eq!(margins[0].initial.as_decimal(), dec!(0));
        assert_eq!(margins[0].maintenance.as_decimal(), dec!(0));
        assert_eq!(
            info.get("net_initial_margin"),
            Some(&serde_json::json!("100")),
        );
        assert_eq!(
            info.get("net_maintenance_margin"),
            Some(&serde_json::json!("50")),
        );
        assert_eq!(
            info.get("is_under_liquidation"),
            Some(&serde_json::json!(false)),
        );
    }

    #[rstest]
    fn test_parse_subaccount_preserves_multi_collateral_units() {
        // 2.5 ETH collateral reporting $1000 of credit stays 2.5 ETH total
        // with nothing locked: credit is not a reservation on the collateral,
        // so no mark-price conversion applies
        let mut subaccount = sample_subaccount();
        subaccount.collaterals = vec![
            DeriveCollateral {
                amount: dec!(2.5),
                asset_name: "ETH".into(),
                asset_type: DeriveAssetType::Erc20,
                cumulative_interest: dec!(0),
                currency: "ETH".into(),
                initial_margin: dec!(1000),
                maintenance_margin: dec!(500),
                mark_price: dec!(3500),
                mark_value: dec!(8750),
                pending_interest: dec!(0),
            },
            DeriveCollateral {
                amount: dec!(1000),
                asset_name: "USDC".into(),
                asset_type: DeriveAssetType::Erc20,
                cumulative_interest: dec!(0),
                currency: "USDC".into(),
                initial_margin: dec!(1000),
                maintenance_margin: dec!(1000),
                mark_price: dec!(1),
                mark_value: dec!(1000),
                pending_interest: dec!(0),
            },
        ];

        let (balances, _, _) = parse_derive_subaccount_to_balances(&subaccount).unwrap();
        assert_eq!(balances.len(), 2);
        assert_eq!(balances[0].total.as_decimal(), dec!(2.5));
        assert_eq!(balances[0].locked.as_decimal(), dec!(0));
        assert_eq!(balances[0].free.as_decimal(), dec!(2.5));
        assert_eq!(balances[1].total.as_decimal(), dec!(1000));
        assert_eq!(balances[1].locked.as_decimal(), dec!(0));
        assert_eq!(balances[1].free.as_decimal(), dec!(1000));
    }

    #[rstest]
    fn test_parse_subaccount_aggregates_requirements_and_keeps_health_in_info() {
        // Requirements aggregate position IM with open-order margin; the
        // signed net health values must not appear as requirements
        let mut subaccount = sample_subaccount();
        subaccount.positions_initial_margin = dec!(350);
        subaccount.positions_maintenance_margin = dec!(175);
        subaccount.open_orders_margin = dec!(40);
        subaccount.initial_margin = dec!(610);
        subaccount.maintenance_margin = dec!(825);

        let (balances, margins, info) = parse_derive_subaccount_to_balances(&subaccount).unwrap();
        assert_eq!(balances[0].locked.as_decimal(), dec!(0));
        assert_eq!(margins.len(), 1);
        assert_eq!(margins[0].initial.as_decimal(), dec!(390));
        assert_eq!(margins[0].maintenance.as_decimal(), dec!(175));
        assert_eq!(
            info.get("positions_initial_margin"),
            Some(&serde_json::json!("350")),
        );
        assert_eq!(
            info.get("positions_maintenance_margin"),
            Some(&serde_json::json!("175")),
        );
        assert_eq!(
            info.get("open_orders_margin"),
            Some(&serde_json::json!("40")),
        );
        assert_eq!(
            info.get("net_initial_margin"),
            Some(&serde_json::json!("610")),
        );
        assert_eq!(
            info.get("net_maintenance_margin"),
            Some(&serde_json::json!("825")),
        );
    }

    #[rstest]
    fn test_parse_subaccount_funded_positionless_fixture_reports_no_locked() {
        let (balances, margins, info) =
            parse_subaccount_fixture("common/http_subaccount_usdc.json");
        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].total.as_decimal(), dec!(1000));
        assert_eq!(balances[0].locked.as_decimal(), dec!(0));
        assert_eq!(balances[0].free.as_decimal(), dec!(1000));
        assert_eq!(margins.len(), 1);
        assert_eq!(margins[0].initial.as_decimal(), dec!(0));
        assert_eq!(margins[0].maintenance.as_decimal(), dec!(0));
        // Net health equals the collateral credit with no requirements,
        // matching observed mainnet responses for funded positionless accounts
        assert_eq!(
            info.get("net_initial_margin"),
            Some(&serde_json::json!("1000")),
        );
        assert_eq!(
            info.get("net_maintenance_margin"),
            Some(&serde_json::json!("1000")),
        );
    }

    #[rstest]
    fn test_parse_subaccount_positions_margin_fixture_maps_requirements() {
        let (balances, margins, info) =
            parse_subaccount_fixture("common/http_subaccount_positions_margin.json");
        assert_eq!(balances[0].locked.as_decimal(), dec!(0));
        assert_eq!(margins[0].initial.as_decimal(), dec!(350));
        assert_eq!(margins[0].maintenance.as_decimal(), dec!(175));
        assert_eq!(
            info.get("net_initial_margin"),
            Some(&serde_json::json!("650")),
        );
        assert_eq!(
            info.get("net_maintenance_margin"),
            Some(&serde_json::json!("825")),
        );
    }

    #[rstest]
    fn test_parse_subaccount_open_orders_margin_fixture_maps_reservation() {
        let (_, margins, info) =
            parse_subaccount_fixture("common/http_subaccount_open_orders_margin.json");
        assert_eq!(margins[0].initial.as_decimal(), dec!(40));
        assert_eq!(margins[0].maintenance.as_decimal(), dec!(0));
        assert_eq!(
            info.get("open_orders_margin"),
            Some(&serde_json::json!("40")),
        );
    }

    #[rstest]
    fn test_parse_subaccount_negative_health_fixture_preserves_signs() {
        let (balances, margins, info) =
            parse_subaccount_fixture("common/http_subaccount_negative_health.json");
        assert_eq!(balances[0].locked.as_decimal(), dec!(0));
        assert_eq!(margins[0].initial.as_decimal(), dec!(390));
        assert_eq!(margins[0].maintenance.as_decimal(), dec!(175));
        assert_eq!(
            info.get("net_initial_margin"),
            Some(&serde_json::json!("-50")),
        );
        assert_eq!(
            info.get("net_maintenance_margin"),
            Some(&serde_json::json!("-20")),
        );
        assert_eq!(
            info.get("is_under_liquidation"),
            Some(&serde_json::json!(true)),
        );
    }

    #[rstest]
    fn test_parse_subaccount_with_no_collateral_emits_margins_only() {
        let mut subaccount = sample_subaccount();
        subaccount.collaterals = vec![];
        subaccount.positions_initial_margin = dec!(350);
        subaccount.positions_maintenance_margin = dec!(175);

        let (balances, margins, _) = parse_derive_subaccount_to_balances(&subaccount).unwrap();
        assert!(balances.is_empty());
        assert_eq!(margins.len(), 1);
        assert_eq!(margins[0].initial.as_decimal(), dec!(350));
        assert_eq!(margins[0].maintenance.as_decimal(), dec!(175));
    }

    #[rstest]
    fn test_parse_subaccount_errors_on_unrepresentable_amount() {
        let mut subaccount = sample_subaccount();
        subaccount.collaterals[0].amount = Decimal::MAX;

        let err = parse_derive_subaccount_to_balances(&subaccount)
            .expect_err("out-of-range collateral amount must error instead of panicking");
        assert!(
            err.to_string().contains("collateral balance"),
            "unexpected error: {err}",
        );
    }

    fn parse_subaccount_fixture(
        filename: &str,
    ) -> (Vec<AccountBalance>, Vec<MarginBalance>, Params) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(filename);
        let content =
            std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("failed to read {filename}"));
        let subaccount: DeriveSubaccount = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("failed to parse {filename}: {e}"));
        parse_derive_subaccount_to_balances(&subaccount).expect("subaccount maps")
    }

    fn sample_subaccount() -> DeriveSubaccount {
        DeriveSubaccount {
            collaterals: vec![DeriveCollateral {
                amount: dec!(1000),
                asset_name: "USDC".into(),
                asset_type: DeriveAssetType::Erc20,
                cumulative_interest: dec!(0),
                currency: "USDC".into(),
                initial_margin: dec!(100),
                maintenance_margin: dec!(50),
                mark_price: dec!(1),
                mark_value: dec!(1000),
                pending_interest: dec!(0),
            }],
            collaterals_initial_margin: dec!(100),
            collaterals_maintenance_margin: dec!(50),
            collaterals_value: dec!(1000),
            currency: "USDC".into(),
            initial_margin: dec!(100),
            is_under_liquidation: false,
            label: None,
            maintenance_margin: dec!(50),
            margin_type: DeriveMarginType::Sm,
            open_orders: vec![],
            open_orders_margin: dec!(0),
            positions: vec![],
            positions_initial_margin: dec!(0),
            positions_maintenance_margin: dec!(0),
            positions_value: dec!(0),
            subaccount_id: 30769,
            subaccount_value: dec!(1000),
        }
    }
}
