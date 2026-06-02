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
use nautilus_core::{UUID4, UnixNanos, datetime::NANOSECONDS_IN_MILLISECOND};
use nautilus_model::{
    enums::{LiquiditySide, OrderType, PositionSideSpecified},
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::DERIVE_VENUE,
        enums::{DeriveLiquidityRole, DeriveOrderStatus, DeriveTimeInForce, DeriveTxStatus},
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
    let instrument_id =
        InstrumentId::new(Symbol::new(order.instrument_name.as_str()), *DERIVE_VENUE);
    let venue_order_id = VenueOrderId::new(order.order_id.as_str());
    let order_side = derive_order_side_to_nautilus(order.direction);
    let order_type = derive_order_type_to_nautilus_for_order(order.order_type, order.trigger_type);
    let post_only = matches!(order.time_in_force, DeriveTimeInForce::PostOnly);
    let time_in_force = derive_tif_to_nautilus(order.time_in_force);
    let order_status =
        derive_status_to_nautilus(order.order_status, order.filled_amount, order.amount);
    let quantity = quantity_from_decimal(order.amount, "amount")?;
    let filled_qty = quantity_from_decimal(order.filled_amount, "filled_amount")?;

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
        && let Ok(price) = Price::from_decimal(order.limit_price.normalize())
    {
        report = report.with_price(price);
    }

    if let Some(trigger_price) = order.trigger_price
        && trigger_price > Decimal::ZERO
        && let Ok(price) = Price::from_decimal(trigger_price.normalize())
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

/// Builds a [`FillReport`] from a Derive trade record.
///
/// Quote-currency commission is reported in the same currency as the
/// instrument's settlement (USDC for perps and options). `client_order_id`
/// is sourced from the trade `label` when populated.
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
    let last_qty = quantity_from_decimal(trade.trade_amount, "trade_amount")?;
    let last_px = price_from_decimal(trade.trade_price, "trade_price")?;
    let commission = Money::new(
        trade
            .trade_fee
            .try_into()
            .with_context(|| format!("trade_fee {} out of f64 range", trade.trade_fee))?,
        fee_currency,
    );
    let liquidity_side = match trade.liquidity_role {
        DeriveLiquidityRole::Maker => LiquiditySide::Maker,
        DeriveLiquidityRole::Taker => LiquiditySide::Taker,
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
/// Returns `Ok(None)` when the position is flat (zero amount) and the caller
/// asked for non-flat reports only.
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
    let quantity = quantity_from_decimal(abs_amount, "position.amount")?;

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

/// Derives [`AccountBalance`] and [`MarginBalance`] rows from a
/// [`DeriveSubaccount`] snapshot.
///
/// Each collateral row becomes one [`AccountBalance`]; the subaccount's
/// initial/maintenance margin requirements collapse into a single
/// [`MarginBalance`] without an instrument scoping.
///
/// # Errors
///
/// Returns an error when a decimal field cannot be represented at the
/// currency precision used by [`Money`].
pub fn parse_derive_subaccount_to_balances(
    subaccount: &DeriveSubaccount,
) -> anyhow::Result<(Vec<AccountBalance>, Vec<MarginBalance>)> {
    let mut balances = Vec::with_capacity(subaccount.collaterals.len());
    for collateral in &subaccount.collaterals {
        let currency = Currency::get_or_create_crypto(collateral.asset_name.as_str());
        // `amount` is in collateral units (e.g. 2.5 ETH); `initial_margin` is
        // the USD value of the venue's margin requirement on this collateral.
        // To produce a single-unit `AccountBalance` we convert the USD margin
        // back into collateral units via `mark_price` (USDC's mark_price is 1
        // so this is a no-op there; ETH/BTC scale by the spot rate). When
        // mark_price is non-positive we cannot scale and report locked = 0
        // rather than mix units.
        let total_dec = collateral.amount;
        let locked_dec = if collateral.mark_price > Decimal::ZERO {
            (collateral.initial_margin.max(Decimal::ZERO) / collateral.mark_price)
                .max(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };
        // `from_total_and_locked` clamps `locked` into `[0, total]` and derives
        // `free` in fixed-point so the `total == locked + free` invariant holds
        // exactly at the currency precision, even when the venue's margin
        // formula overshoots `amount` by sub-precision dust.
        let balance = AccountBalance::from_total_and_locked(total_dec, locked_dec, currency)
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to build collateral balance for {} (total={total_dec}, locked={locked_dec}): {e}",
                    collateral.asset_name,
                )
            })?;
        balances.push(balance);
    }

    let currency = Currency::get_or_create_crypto(subaccount.currency.as_str());
    let initial = Money::from_decimal(subaccount.initial_margin, currency).with_context(|| {
        format!(
            "initial_margin {} cannot be represented at {} precision",
            subaccount.initial_margin, currency,
        )
    })?;
    let maintenance =
        Money::from_decimal(subaccount.maintenance_margin, currency).with_context(|| {
            format!(
                "maintenance_margin {} cannot be represented at {} precision",
                subaccount.maintenance_margin, currency,
            )
        })?;
    let margins = vec![MarginBalance::new(initial, maintenance, None)];

    Ok((balances, margins))
}

fn price_from_decimal(value: Decimal, field: &str) -> anyhow::Result<Price> {
    Price::from_decimal(value.normalize()).with_context(|| format!("invalid Derive {field}"))
}

fn quantity_from_decimal(value: Decimal, field: &str) -> anyhow::Result<Quantity> {
    Quantity::from_decimal(value.normalize()).with_context(|| format!("invalid Derive {field}"))
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
    fn test_parse_order_report_normalizes_trailing_decimal_zeros() {
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
    fn test_parse_order_report_maps_limit_if_touched_trigger() {
        let mut order = sample_order();
        order.average_price = Decimal::ZERO;
        order.filled_amount = Decimal::ZERO;
        order.limit_price = dec!(3700);
        order.order_status = DeriveOrderStatus::Untriggered;
        order.order_type = DeriveOrderType::Limit;
        order.trigger_price = Some(dec!(3600));
        order.trigger_price_type = Some(DeriveTriggerPriceType::Index);
        order.trigger_type = Some(DeriveTriggerType::Takeprofit);
        let account_id = AccountId::new("DERIVE-001");

        let report = parse_derive_order_to_report(&order, account_id, UnixNanos::from(1)).unwrap();

        assert_eq!(report.order_type, OrderType::LimitIfTouched);
        assert_eq!(report.price, Some(Price::from("3700")));
        assert_eq!(report.trigger_price, Some(Price::from("3600")));
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
    fn test_parse_subaccount_emits_balances_and_margins() {
        let subaccount = sample_subaccount();
        let (balances, margins) = parse_derive_subaccount_to_balances(&subaccount).unwrap();
        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].total.as_decimal(), dec!(1000));
        assert_eq!(balances[0].locked.as_decimal(), dec!(100));
        assert_eq!(balances[0].free.as_decimal(), dec!(900));
        assert_eq!(margins.len(), 1);
        assert_eq!(margins[0].initial.as_decimal(), dec!(100));
        assert_eq!(margins[0].maintenance.as_decimal(), dec!(50));
    }

    #[rstest]
    fn test_parse_subaccount_converts_non_usdc_locked_to_collateral_units() {
        // 2.5 ETH collateral at $3500 mark with a $1000 USD margin requirement
        // should report locked = 1000/3500 ETH (~0.2857), not "1000 ETH".
        // Pre-fix code mixed units and reported locked as the raw 1000.
        let mut subaccount = sample_subaccount();
        subaccount.collaterals = vec![DeriveCollateral {
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
        }];

        let (balances, _) = parse_derive_subaccount_to_balances(&subaccount).unwrap();
        assert_eq!(balances[0].total.as_decimal(), dec!(2.5));
        // Locked is computed in collateral units (USD margin / mark_price).
        let locked = balances[0].locked.as_decimal();
        let expected_locked = dec!(1000) / dec!(3500);
        assert!(
            (locked - expected_locked).abs() < dec!(0.000001),
            "locked {locked} should be near {expected_locked} ETH"
        );
        let free = balances[0].free.as_decimal();
        let expected_free = dec!(2.5) - expected_locked;
        assert!(
            (free - expected_free).abs() < dec!(0.000001),
            "free {free} should be near {expected_free} ETH"
        );
    }

    #[rstest]
    fn test_parse_subaccount_reports_zero_locked_when_mark_price_non_positive() {
        // A zero mark_price (venue corner case during onboarding) cannot
        // convert USD into collateral units; report locked = 0 rather than
        // mix units or panic on divide-by-zero.
        let mut subaccount = sample_subaccount();
        subaccount.collaterals[0].mark_price = dec!(0);
        subaccount.collaterals[0].initial_margin = dec!(50);

        let (balances, _) = parse_derive_subaccount_to_balances(&subaccount).unwrap();
        assert_eq!(balances[0].locked.as_decimal(), dec!(0));
        assert_eq!(
            balances[0].free.as_decimal(),
            balances[0].total.as_decimal()
        );
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
