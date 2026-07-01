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

//! Parse functions for converting Binance Futures venue types to Nautilus reports.
//!
//! Pure functions that take venue message + precision + account_id + ts_init
//! and return Nautilus report types.

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{
        AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce,
        TrailingOffsetType, TriggerType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId},
    reports::{FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use super::messages::{
    AlgoOrderUpdateData, BinanceFuturesAccountUpdateMsg, BinanceFuturesOrderUpdateMsg,
    OrderUpdateData,
};
use crate::{
    common::{
        consts::BINANCE_NAUTILUS_FUTURES_BROKER_ID,
        encoder::decode_broker_id,
        enums::{
            BinanceAlgoStatus, BinanceFuturesOrderType, BinanceOrderStatus, BinanceSide,
            BinanceTimeInForce, BinanceWorkingType,
        },
        parse::{
            parse_required_decimal, parse_required_price_at_precision,
            parse_required_quantity_at_precision,
        },
    },
    futures::conversions::normalize_futures_asset,
};

/// Converts a Binance Futures order update to a Nautilus order status report.
///
/// # Errors
///
/// Returns an error if report construction fails.
pub fn parse_futures_order_update_to_order_status(
    msg: &BinanceFuturesOrderUpdateMsg,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    account_id: AccountId,
    treat_expired_as_canceled: bool,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let order = &msg.order;
    let ts_event = UnixNanos::from_millis(msg.event_time as u64);

    let client_order_id = ClientOrderId::new(decode_broker_id(
        &order.client_order_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ));
    let venue_order_id = VenueOrderId::new(order.order_id.to_string());

    let order_side = parse_side(order.side);
    let order_status = parse_order_status(order.order_status, treat_expired_as_canceled);
    let order_type = parse_futures_order_type(order.order_type);
    let time_in_force = parse_time_in_force(order.time_in_force);

    let quantity =
        parse_required_quantity_at_precision(&order.original_qty, size_precision, "original_qty")?;
    let filled_qty = parse_required_quantity_at_precision(
        &order.cumulative_filled_qty,
        size_precision,
        "cumulative_filled_qty",
    )?;
    let price = parse_required_price_at_precision(
        &order.original_price,
        price_precision,
        "original_price",
    )?;

    let avg_px = if filled_qty.as_decimal() > Decimal::ZERO {
        parse_optional_positive_price_at_precision(&order.average_price, price_precision)
    } else {
        None
    };

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_event,
        ts_event,
        ts_init,
        None, // report_id
    );

    report.price = Some(price);
    report.post_only = order.order_type == BinanceFuturesOrderType::Limit
        && order.time_in_force == BinanceTimeInForce::Gtx;

    if let Some(stop_price) =
        parse_optional_positive_price_at_precision(&order.stop_price, price_precision)
    {
        report.trigger_price = Some(stop_price);
    }

    if let Some(offset) = order
        .callback_rate
        .as_deref()
        .and_then(parse_trailing_offset_basis_points)
    {
        report.trailing_offset = Some(offset);
        report.trailing_offset_type = TrailingOffsetType::BasisPoints;
    }

    if let Some(avg) = avg_px {
        report.avg_px = Some(avg.as_decimal());
    }

    Ok(report)
}

/// Resolves the commission for a Binance fill event.
///
/// Uses the venue-provided commission fields (N/n) when present. Falls back to
/// estimating `taker_fee * qty * price` when the venue omits them, matching the
/// Python adapter behavior for exchange-generated fills (liquidation, ADL).
/// Returns zero USDT when neither source is available.
///
/// # Errors
///
/// Returns an error if venue commission or fee fallback values cannot be
/// parsed or converted to `Money`.
pub fn resolve_commission(
    order: &OrderUpdateData,
    last_qty: Quantity,
    last_px: Price,
    taker_fee: Option<Decimal>,
    quote_currency: Option<Currency>,
    bnfcr_currency: Currency,
) -> anyhow::Result<Money> {
    if order.commission.is_some() || order.commission_asset.is_some() {
        let raw_commission = order.commission.as_deref().unwrap_or("0");
        let amount = parse_required_decimal(raw_commission, "commission")?;
        let currency = order.commission_asset.as_ref().map_or(bnfcr_currency, |a| {
            normalize_futures_asset(a.as_str(), bnfcr_currency)
        });
        Money::from_decimal(amount, currency)
            .map_err(|e| anyhow::anyhow!("invalid commission='{raw_commission}': {e}"))
    } else if let Some(fee) = taker_fee {
        let currency = quote_currency.unwrap_or_else(Currency::USDT);
        let notional = last_qty
            .as_decimal()
            .checked_mul(last_px.as_decimal())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid fee notional for last_qty='{last_qty}' and last_px='{last_px}': multiplication overflow",
                )
            })?;
        let amount = fee.checked_mul(notional).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid fee amount for taker_fee='{fee}' and notional='{notional}': multiplication overflow"
            )
        })?;
        Money::from_decimal(amount, currency)
            .map_err(|e| anyhow::anyhow!("invalid fee amount='{amount}': {e}"))
    } else {
        Ok(Money::zero(Currency::USDT()))
    }
}

/// Converts a Binance Futures order update (Trade type) to a Nautilus fill report.
///
/// # Errors
///
/// Returns an error if report construction fails.
#[expect(clippy::too_many_arguments)]
pub fn parse_futures_order_update_to_fill(
    msg: &BinanceFuturesOrderUpdateMsg,
    account_id: AccountId,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    taker_fee: Option<Decimal>,
    quote_currency: Option<Currency>,
    bnfcr_currency: Currency,
    venue_position_id: Option<PositionId>,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let order = &msg.order;
    let ts_event = UnixNanos::from_millis(msg.event_time as u64);

    let client_order_id = ClientOrderId::new(decode_broker_id(
        &order.client_order_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ));
    let venue_order_id = VenueOrderId::new(order.order_id.to_string());
    let trade_id = TradeId::new(order.trade_id.to_string());

    let order_side = parse_side(order.side);

    let liquidity_side = if order.is_maker {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    let last_qty = parse_required_quantity_at_precision(
        &order.last_filled_qty,
        size_precision,
        "last_filled_qty",
    )?;
    let last_px = parse_required_price_at_precision(
        &order.last_filled_price,
        price_precision,
        "last_filled_price",
    )?;
    let commission = resolve_commission(
        order,
        last_qty,
        last_px,
        taker_fee,
        quote_currency,
        bnfcr_currency,
    )?;

    Ok(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        Some(client_order_id),
        venue_position_id,
        ts_event,
        ts_init,
        None, // report_id
    ))
}

/// Converts a Binance Futures algo order update to a Nautilus order status report.
///
/// Returns `None` for algo statuses that don't map to an order status report
/// (e.g. New, Triggering, Triggered, Finished, Unknown).
///
/// # Errors
///
/// Returns an error if report quantity, limit price, or trigger price parsing fails.
pub fn parse_futures_algo_update_to_order_status(
    algo_data: &AlgoOrderUpdateData,
    event_time: i64,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<Option<OrderStatusReport>> {
    let ts_event = UnixNanos::from_millis(event_time as u64);

    let client_order_id = ClientOrderId::new(decode_broker_id(
        &algo_data.client_algo_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ));

    let venue_order_id = algo_data
        .actual_order_id
        .as_ref()
        .filter(|id| !id.is_empty())
        .map_or_else(
            || VenueOrderId::new(algo_data.algo_id.to_string()),
            |id| VenueOrderId::new(id.clone()),
        );

    let order_status = match algo_data.algo_status {
        BinanceAlgoStatus::Canceled | BinanceAlgoStatus::Expired => OrderStatus::Canceled,
        BinanceAlgoStatus::Rejected => OrderStatus::Rejected,
        _ => return Ok(None),
    };

    let order_side = parse_side(algo_data.side);
    let order_type = parse_futures_order_type(algo_data.order_type);
    let time_in_force = parse_time_in_force(algo_data.time_in_force);

    let quantity =
        parse_required_quantity_at_precision(&algo_data.quantity, size_precision, "quantity")?;
    let trigger_price = parse_algo_trigger_price(algo_data, price_precision)?;
    let price = parse_algo_limit_price(algo_data, price_precision)?;

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        Quantity::zero(size_precision),
        ts_event,
        ts_event,
        ts_init,
        None, // report_id
    );

    if let Some(price) = price {
        report.price = Some(price);
    }

    if let Some(trigger_price) = trigger_price {
        report.trigger_price = Some(trigger_price);
        report.trigger_type = Some(parse_working_type(algo_data.working_type));
    }

    Ok(Some(report))
}

/// Converts a Binance Futures account update to a Nautilus account state.
pub fn parse_futures_account_update(
    msg: &BinanceFuturesAccountUpdateMsg,
    account_id: AccountId,
    bnfcr_currency: Currency,
    ts_init: UnixNanos,
) -> Option<AccountState> {
    let ts_event = UnixNanos::from_millis(msg.event_time as u64);

    let balances: Vec<AccountBalance> = msg
        .account
        .balances
        .iter()
        .filter_map(|b| {
            if b.wallet_balance.is_zero() {
                return None;
            }

            let currency = normalize_futures_asset(b.asset, bnfcr_currency);
            AccountBalance::from_total_and_free(b.wallet_balance, b.cross_wallet_balance, currency)
                .ok()
        })
        .collect();

    if balances.is_empty() {
        return None;
    }

    Some(AccountState::new(
        account_id,
        AccountType::Margin,
        balances,
        vec![], // Margins handled separately
        true,   // is_reported
        UUID4::new(),
        ts_event,
        ts_init,
        None, // base_currency
    ))
}

/// Returns the decoded client order ID from an [`OrderUpdateData`].
pub fn decode_order_client_id(order: &OrderUpdateData) -> ClientOrderId {
    ClientOrderId::new(decode_broker_id(
        &order.client_order_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ))
}

/// Returns the decoded client order ID from an [`AlgoOrderUpdateData`].
pub fn decode_algo_client_id(algo: &AlgoOrderUpdateData) -> ClientOrderId {
    ClientOrderId::new(decode_broker_id(
        &algo.client_algo_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ))
}

fn parse_optional_positive_price_at_precision(raw: &str, precision: u8) -> Option<Price> {
    let decimal = parse_required_decimal(raw, "optional_price").ok()?;
    if decimal <= Decimal::ZERO {
        return None;
    }

    Price::from_decimal_dp(decimal, precision).ok()
}

fn parse_positive_price_at_precision(
    raw: &str,
    precision: u8,
    field: &str,
) -> anyhow::Result<Option<Price>> {
    let decimal = parse_required_decimal(raw, field)?;
    if decimal <= Decimal::ZERO {
        return Ok(None);
    }

    Price::from_decimal_dp(decimal, precision)
        .map(Some)
        .map_err(|e| anyhow::anyhow!("invalid {field} precision: {e}"))
}

fn parse_algo_trigger_price(
    algo_data: &AlgoOrderUpdateData,
    price_precision: u8,
) -> anyhow::Result<Option<Price>> {
    let trigger_price = parse_positive_price_at_precision(
        &algo_data.trigger_price,
        price_precision,
        "trigger_price",
    )?;

    if trigger_price.is_none() && requires_algo_trigger_price(algo_data.order_type) {
        anyhow::bail!(
            "missing positive trigger_price for Binance algo order type {:?}",
            algo_data.order_type
        );
    }

    Ok(trigger_price)
}

fn parse_algo_limit_price(
    algo_data: &AlgoOrderUpdateData,
    price_precision: u8,
) -> anyhow::Result<Option<Price>> {
    let price = parse_positive_price_at_precision(&algo_data.price, price_precision, "price")?;

    if price.is_none() && requires_algo_limit_price(algo_data.order_type) {
        anyhow::bail!(
            "missing positive price for Binance algo order type {:?}",
            algo_data.order_type
        );
    }

    Ok(price)
}

fn parse_trailing_offset_basis_points(raw: &str) -> Option<Decimal> {
    let rate = parse_required_decimal(raw, "callback_rate").ok()?;
    if rate <= Decimal::ZERO {
        return None;
    }

    rate.checked_mul(Decimal::from(100))
}

fn parse_working_type(working_type: BinanceWorkingType) -> TriggerType {
    match working_type {
        BinanceWorkingType::ContractPrice => TriggerType::LastPrice,
        BinanceWorkingType::MarkPrice => TriggerType::MarkPrice,
        BinanceWorkingType::Unknown => TriggerType::Default,
    }
}

fn requires_algo_trigger_price(order_type: BinanceFuturesOrderType) -> bool {
    matches!(
        order_type,
        BinanceFuturesOrderType::Stop
            | BinanceFuturesOrderType::StopMarket
            | BinanceFuturesOrderType::TakeProfit
            | BinanceFuturesOrderType::TakeProfitMarket
    )
}

fn requires_algo_limit_price(order_type: BinanceFuturesOrderType) -> bool {
    matches!(
        order_type,
        BinanceFuturesOrderType::Stop | BinanceFuturesOrderType::TakeProfit
    )
}

fn parse_side(side: BinanceSide) -> OrderSide {
    match side {
        BinanceSide::Buy => OrderSide::Buy,
        BinanceSide::Sell => OrderSide::Sell,
    }
}

fn parse_order_status(status: BinanceOrderStatus, treat_expired_as_canceled: bool) -> OrderStatus {
    match status {
        BinanceOrderStatus::New | BinanceOrderStatus::PendingNew => OrderStatus::Accepted,
        BinanceOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
        BinanceOrderStatus::Filled
        | BinanceOrderStatus::NewAdl
        | BinanceOrderStatus::NewInsurance => OrderStatus::Filled,
        BinanceOrderStatus::Canceled | BinanceOrderStatus::PendingCancel => OrderStatus::Canceled,
        BinanceOrderStatus::Rejected => OrderStatus::Rejected,
        BinanceOrderStatus::Expired | BinanceOrderStatus::ExpiredInMatch => {
            if treat_expired_as_canceled {
                OrderStatus::Canceled
            } else {
                OrderStatus::Expired
            }
        }
        BinanceOrderStatus::Unknown => OrderStatus::Accepted,
    }
}

fn parse_futures_order_type(order_type: BinanceFuturesOrderType) -> OrderType {
    match order_type {
        BinanceFuturesOrderType::Limit => OrderType::Limit,
        BinanceFuturesOrderType::Market => OrderType::Market,
        BinanceFuturesOrderType::Stop => OrderType::StopLimit,
        BinanceFuturesOrderType::StopMarket => OrderType::StopMarket,
        BinanceFuturesOrderType::TakeProfit => OrderType::LimitIfTouched,
        BinanceFuturesOrderType::TakeProfitMarket => OrderType::MarketIfTouched,
        BinanceFuturesOrderType::TrailingStopMarket => OrderType::TrailingStopMarket,
        BinanceFuturesOrderType::Liquidation
        | BinanceFuturesOrderType::Adl
        | BinanceFuturesOrderType::Unknown => OrderType::Market,
    }
}

fn parse_time_in_force(tif: BinanceTimeInForce) -> TimeInForce {
    match tif {
        BinanceTimeInForce::Gtc | BinanceTimeInForce::Gtx => TimeInForce::Gtc,
        BinanceTimeInForce::Ioc | BinanceTimeInForce::Rpi => TimeInForce::Ioc,
        BinanceTimeInForce::Fok => TimeInForce::Fok,
        BinanceTimeInForce::Gtd => TimeInForce::Gtd,
        BinanceTimeInForce::Unknown => TimeInForce::Gtc,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde::de::DeserializeOwned;

    use super::*;
    use crate::{
        common::{
            consts::BINANCE_NAUTILUS_FUTURES_BROKER_ID,
            encoder::encode_broker_id,
            enums::{BinancePriceMatch, BinanceSelfTradePreventionMode},
            testing::load_fixture_string,
        },
        futures::websocket::streams::messages::{
            BinanceFuturesAccountUpdateMsg, BinanceFuturesAlgoUpdateMsg,
            BinanceFuturesOrderUpdateMsg,
        },
    };

    const PRICE_PRECISION: u8 = 2;
    const SIZE_PRECISION: u8 = 3;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("ETHUSDT-PERP.BINANCE")
    }

    fn account_id() -> AccountId {
        AccountId::from("BINANCE-FUTURES-001")
    }

    fn load_user_data_fixture<T: DeserializeOwned>(filename: &str) -> T {
        let path = format!("futures/user_data_json/{filename}");
        serde_json::from_str(&load_fixture_string(&path))
            .unwrap_or_else(|e| panic!("Failed to parse fixture {path}: {e}"))
    }

    #[rstest]
    fn test_parse_order_update_to_order_status_new() {
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_new.json");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = parse_futures_order_update_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            false,
            ts_init,
        )
        .unwrap();

        assert_eq!(report.account_id, account_id());
        assert_eq!(report.instrument_id, instrument_id());
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.order_type, OrderType::TrailingStopMarket);
        assert_eq!(report.venue_order_id, VenueOrderId::new("8886774"));
        assert_eq!(report.client_order_id, Some(ClientOrderId::from("TEST")));
    }

    #[rstest]
    fn test_parse_order_update_to_order_status_rejects_invalid_quantity() {
        let mut msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_new.json");
        msg.order.original_qty = "not-a-number".to_string();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = parse_futures_order_update_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            false,
            ts_init,
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("original_qty"));
    }

    #[rstest]
    fn test_parse_order_update_to_order_status_rejects_invalid_filled_quantity() {
        let mut msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_new.json");
        msg.order.cumulative_filled_qty = "not-a-number".to_string();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = parse_futures_order_update_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            false,
            ts_init,
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("cumulative_filled_qty"));
    }

    #[rstest]
    fn test_parse_order_update_to_order_status_rejects_invalid_price() {
        let mut msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_new.json");
        msg.order.original_price = "not-a-number".to_string();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = parse_futures_order_update_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            false,
            ts_init,
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("original_price"));
    }

    #[rstest]
    fn test_parse_order_update_to_order_status_skips_invalid_optional_fields() {
        let mut msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_trade.json");
        msg.order.average_price = "not-a-number".to_string();
        msg.order.stop_price = "not-a-number".to_string();
        msg.order.callback_rate = Some("not-a-number".to_string());
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let report = parse_futures_order_update_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            false,
            ts_init,
        )
        .unwrap();

        assert!(report.avg_px.is_none());
        assert!(report.trigger_price.is_none());
        assert_eq!(report.trailing_offset, None);
    }

    #[rstest]
    fn test_parse_order_update_to_fill_report() {
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_trade.json");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        assert_eq!(
            msg.order.stp_mode,
            Some(BinanceSelfTradePreventionMode::ExpireTaker),
        );

        let report = parse_futures_order_update_to_fill(
            &msg,
            account_id(),
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            None,
            None,
            Currency::USDT(),
            None,
            ts_init,
        )
        .unwrap();

        assert_eq!(report.account_id, account_id());
        assert_eq!(report.instrument_id, instrument_id());
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
        assert_eq!(report.trade_id, TradeId::new("12345678"));
        assert_eq!(report.client_order_id, Some(ClientOrderId::from("TEST")));
        assert_eq!(report.last_qty, Quantity::new(0.001, SIZE_PRECISION));
        assert_eq!(report.last_px, Price::new(7100.50, PRICE_PRECISION));
    }

    #[rstest]
    fn test_parse_order_update_to_fill_rejects_invalid_price() {
        let mut msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_trade.json");
        msg.order.last_filled_price = "not-a-number".to_string();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = parse_futures_order_update_to_fill(
            &msg,
            account_id(),
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            None,
            None,
            Currency::USDT(),
            None,
            ts_init,
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("last_filled_price"));
    }

    #[rstest]
    fn test_parse_order_update_to_fill_rejects_invalid_quantity() {
        let mut msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_trade.json");
        msg.order.last_filled_qty = "not-a-number".to_string();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = parse_futures_order_update_to_fill(
            &msg,
            account_id(),
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            None,
            None,
            Currency::USDT(),
            None,
            ts_init,
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("last_filled_qty"));
    }

    #[rstest]
    fn test_parse_order_update_to_fill_rejects_invalid_commission() {
        let mut msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_trade.json");
        msg.order.commission = Some("not-a-number".to_string());
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let result = parse_futures_order_update_to_fill(
            &msg,
            account_id(),
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            None,
            None,
            Currency::USDT(),
            None,
            ts_init,
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("commission"));
    }

    #[rstest]
    fn test_parse_account_update() {
        let msg: BinanceFuturesAccountUpdateMsg = load_user_data_fixture("account_update.json");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let state =
            parse_futures_account_update(&msg, account_id(), Currency::USDT(), ts_init).unwrap();

        assert_eq!(state.account_id, account_id());
        assert_eq!(state.account_type, AccountType::Margin);
        assert!(state.is_reported);
        assert_eq!(state.balances.len(), 1);
    }

    // Credits Trading Mode (EU) reports the wallet in BNFCR, which is absent from the
    // currency table; resolving it to the configured `bnfcr_currency` keeps the balance
    // denominated in the stablecoin the contracts settle in instead of panicking.
    #[rstest]
    #[case(Currency::USDT())]
    #[case(Currency::USDC())]
    fn test_parse_account_update_maps_bnfcr_to_configured_currency(
        #[case] bnfcr_currency: Currency,
    ) {
        let msg: BinanceFuturesAccountUpdateMsg =
            load_user_data_fixture("account_update_bnfcr.json");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let state =
            parse_futures_account_update(&msg, account_id(), bnfcr_currency, ts_init).unwrap();

        assert_eq!(state.balances.len(), 1);
        assert_eq!(state.balances[0].total.currency, bnfcr_currency);
        assert_eq!(
            state.balances[0].total.as_decimal(),
            Decimal::from_str_exact("5001.28983031").unwrap()
        );
    }

    // Regression for the #3867 bug class: WS balances whose `wb` and `cw` have more decimal
    // places than the asset's currency precision used to trip the invariant when Money::new
    // rounded each side independently.
    #[rstest]
    fn test_parse_account_update_precision_drift() {
        let json = r#"{
            "e": "ACCOUNT_UPDATE",
            "E": 1700000000000,
            "T": 1700000000000,
            "a": {
                "m": "ORDER",
                "B": [{
                    "a": "USDT",
                    "wb": "10.000000034999",
                    "cw": "9.999999994999"
                }],
                "P": []
            }
        }"#;
        let msg: BinanceFuturesAccountUpdateMsg = serde_json::from_str(json).unwrap();
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let state =
            parse_futures_account_update(&msg, account_id(), Currency::USDT(), ts_init).unwrap();

        assert_eq!(state.balances.len(), 1);
        let balance = &state.balances[0];
        assert_eq!(balance.total.raw, balance.locked.raw + balance.free.raw);
    }

    #[rstest]
    fn test_parse_algo_update_to_order_status_canceled() {
        let msg: BinanceFuturesAlgoUpdateMsg = load_user_data_fixture("algo_update_canceled.json");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        assert_eq!(
            msg.algo_order.stp_mode,
            Some(BinanceSelfTradePreventionMode::ExpireMaker),
        );
        assert_eq!(msg.algo_order.price_match, Some(BinancePriceMatch::None));

        let report = parse_futures_algo_update_to_order_status(
            &msg.algo_order,
            msg.event_time,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            ts_init,
        )
        .unwrap()
        .unwrap();

        assert_eq!(report.account_id, account_id());
        assert_eq!(report.instrument_id, instrument_id());
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::new("Q5xaq5EGKgXXa0fD7fs0Ip")),
        );
        assert_eq!(report.venue_order_id, VenueOrderId::new("2148719"));
        assert_eq!(report.order_side, OrderSide::Sell);
        assert_eq!(report.order_type, OrderType::LimitIfTouched);
        assert_eq!(report.time_in_force, TimeInForce::Gtc);
        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.quantity, Quantity::new(0.01, SIZE_PRECISION));
        assert_eq!(report.filled_qty, Quantity::new(0.0, SIZE_PRECISION));
        assert_eq!(report.price, Some(Price::from("750.00")));
        assert_eq!(report.trigger_price, Some(Price::from("750.00")));
        assert_eq!(report.trigger_type, Some(TriggerType::LastPrice));
        assert_eq!(
            report.ts_accepted,
            UnixNanos::from(1_750_515_742_303_000_000u64)
        );
        assert_eq!(
            report.ts_last,
            UnixNanos::from(1_750_515_742_303_000_000u64)
        );
        assert_eq!(report.ts_init, ts_init);
    }

    #[rstest]
    fn test_parse_algo_update_to_order_status_new_returns_none() {
        let msg: BinanceFuturesAlgoUpdateMsg = load_user_data_fixture("algo_update_new.json");
        let report = parse_futures_algo_update_to_order_status(
            &msg.algo_order,
            msg.event_time,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            UnixNanos::default(),
        );

        assert!(report.unwrap().is_none());
    }

    #[rstest]
    fn test_parse_algo_update_to_order_status_rejects_invalid_trigger_price() {
        let mut msg: BinanceFuturesAlgoUpdateMsg =
            load_user_data_fixture("algo_update_canceled.json");
        msg.algo_order.trigger_price = "not-a-number".to_string();

        let result = parse_futures_algo_update_to_order_status(
            &msg.algo_order,
            msg.event_time,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            UnixNanos::default(),
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("trigger_price"));
    }

    #[rstest]
    fn test_parse_algo_update_to_order_status_rejects_missing_trigger_price() {
        let mut msg: BinanceFuturesAlgoUpdateMsg =
            load_user_data_fixture("algo_update_canceled.json");
        msg.algo_order.trigger_price = "0".to_string();

        let result = parse_futures_algo_update_to_order_status(
            &msg.algo_order,
            msg.event_time,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            UnixNanos::default(),
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("missing positive trigger_price"));
    }

    #[rstest]
    fn test_parse_algo_update_to_order_status_rejects_missing_limit_price() {
        let mut msg: BinanceFuturesAlgoUpdateMsg =
            load_user_data_fixture("algo_update_canceled.json");
        msg.algo_order.price = "0".to_string();

        let result = parse_futures_algo_update_to_order_status(
            &msg.algo_order,
            msg.event_time,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            UnixNanos::default(),
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("missing positive price"));
    }

    #[rstest]
    fn test_parse_algo_update_to_order_status_rejects_invalid_quantity() {
        let mut msg: BinanceFuturesAlgoUpdateMsg =
            load_user_data_fixture("algo_update_canceled.json");
        msg.algo_order.quantity = "not-a-number".to_string();

        let result = parse_futures_algo_update_to_order_status(
            &msg.algo_order,
            msg.event_time,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            UnixNanos::default(),
        );

        let error = result.unwrap_err().to_string();
        assert!(error.contains("quantity"));
    }

    #[rstest]
    fn test_decode_order_client_id() {
        let mut msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_new.json");
        let original = ClientOrderId::from("O-20200101-000000-000-000-1");
        msg.order.client_order_id = encode_broker_id(&original, BINANCE_NAUTILUS_FUTURES_BROKER_ID);

        let decoded = decode_order_client_id(&msg.order);

        assert_eq!(decoded, original);
    }

    #[rstest]
    fn test_decode_algo_client_id() {
        let mut msg: BinanceFuturesAlgoUpdateMsg =
            load_user_data_fixture("algo_update_canceled.json");
        let original = ClientOrderId::from("O-20200101-000000-000-000-2");
        msg.algo_order.client_algo_id =
            encode_broker_id(&original, BINANCE_NAUTILUS_FUTURES_BROKER_ID);

        let decoded = decode_algo_client_id(&msg.algo_order);

        assert_eq!(decoded, original);
    }

    #[rstest]
    fn test_parse_liquidation_fill() {
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_calculated.json");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        assert!(msg.order.is_liquidation());
        assert!(msg.order.is_exchange_generated());

        let fill = parse_futures_order_update_to_fill(
            &msg,
            account_id(),
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            None,
            None,
            Currency::USDT(),
            None,
            ts_init,
        )
        .unwrap();

        assert_eq!(fill.account_id, account_id());
        assert_eq!(fill.instrument_id, instrument_id());
        assert_eq!(
            fill.client_order_id,
            Some(ClientOrderId::new("autoclose-1234567890"))
        );
        assert_eq!(fill.venue_order_id, VenueOrderId::new("8886999"));
        assert_eq!(fill.trade_id, TradeId::new("12345999"));
        assert_eq!(fill.order_side, OrderSide::Sell);
        assert_eq!(fill.last_qty, Quantity::new(0.014, SIZE_PRECISION));
        assert_eq!(fill.last_px, Price::new(9910.12, PRICE_PRECISION));
        assert_eq!(
            fill.commission,
            Money::new(0.06937084, Currency::from("USDT"))
        );
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    }

    #[rstest]
    fn test_parse_liquidation_status_report() {
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_calculated.json");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let status = parse_futures_order_update_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            false,
            ts_init,
        )
        .unwrap();

        assert_eq!(status.account_id, account_id());
        assert_eq!(status.instrument_id, instrument_id());
        assert_eq!(
            status.client_order_id,
            Some(ClientOrderId::new("autoclose-1234567890"))
        );
        assert_eq!(status.venue_order_id, VenueOrderId::new("8886999"));
        assert_eq!(status.order_side, OrderSide::Sell);
        assert_eq!(status.order_status, OrderStatus::Filled);
        assert_eq!(status.quantity, Quantity::new(0.014, SIZE_PRECISION));
        assert_eq!(status.filled_qty, Quantity::new(0.014, SIZE_PRECISION));
    }

    #[rstest]
    fn test_parse_adl_fill_with_new_adl_status() {
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_adl.json");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        assert!(msg.order.is_adl());
        assert!(msg.order.is_exchange_generated());
        assert!(!msg.order.is_liquidation());

        let fill = parse_futures_order_update_to_fill(
            &msg,
            account_id(),
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            None,
            None,
            Currency::USDT(),
            None,
            ts_init,
        )
        .unwrap();

        assert_eq!(
            fill.client_order_id,
            Some(ClientOrderId::new("adl_autoclose_12345"))
        );
        assert_eq!(fill.venue_order_id, VenueOrderId::new("8887001"));
        assert_eq!(fill.order_side, OrderSide::Buy);
        assert_eq!(fill.last_qty, Quantity::new(0.005, SIZE_PRECISION));
        assert_eq!(fill.last_px, Price::new(42000.00, PRICE_PRECISION));
        assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    }

    #[rstest]
    fn test_parse_adl_status_report_maps_new_adl_to_filled() {
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_adl.json");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let status = parse_futures_order_update_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            false,
            ts_init,
        )
        .unwrap();

        assert_eq!(status.order_status, OrderStatus::Filled);
        assert_eq!(status.filled_qty, Quantity::new(0.005, SIZE_PRECISION));
    }

    #[rstest]
    fn test_parse_settlement_fill_with_trade_exec_type() {
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_settlement.json");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        assert!(msg.order.is_settlement());
        assert!(msg.order.is_exchange_generated());
        assert!(!msg.order.is_liquidation());
        assert!(!msg.order.is_adl());

        let fill = parse_futures_order_update_to_fill(
            &msg,
            account_id(),
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            None,
            None,
            Currency::USDT(),
            None,
            ts_init,
        )
        .unwrap();

        assert_eq!(
            fill.client_order_id,
            Some(ClientOrderId::new("settlement_autoclose-9999"))
        );
        assert_eq!(fill.venue_order_id, VenueOrderId::new("8887002"));
        assert_eq!(fill.order_side, OrderSide::Sell);
        assert_eq!(fill.last_qty, Quantity::new(0.010, SIZE_PRECISION));
        assert_eq!(fill.last_px, Price::new(50000.00, PRICE_PRECISION));
    }

    #[rstest]
    fn test_parse_order_status_new_adl_maps_to_filled() {
        let result = parse_order_status(BinanceOrderStatus::NewAdl, false);
        assert_eq!(result, OrderStatus::Filled);
    }

    #[rstest]
    fn test_parse_order_status_new_insurance_maps_to_filled() {
        let result = parse_order_status(BinanceOrderStatus::NewInsurance, false);
        assert_eq!(result, OrderStatus::Filled);
    }

    #[rstest]
    #[case(BinanceOrderStatus::Expired, false, OrderStatus::Expired)]
    #[case(BinanceOrderStatus::Expired, true, OrderStatus::Canceled)]
    #[case(BinanceOrderStatus::ExpiredInMatch, false, OrderStatus::Expired)]
    #[case(BinanceOrderStatus::ExpiredInMatch, true, OrderStatus::Canceled)]
    fn test_parse_order_status_expired_respects_treat_as_canceled(
        #[case] status: BinanceOrderStatus,
        #[case] treat_expired_as_canceled: bool,
        #[case] expected: OrderStatus,
    ) {
        let result = parse_order_status(status, treat_expired_as_canceled);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_is_exchange_generated_autoclose() {
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_calculated.json");
        assert!(msg.order.is_exchange_generated());
        assert!(msg.order.is_liquidation());
    }

    #[rstest]
    fn test_is_exchange_generated_adl_autoclose() {
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_adl.json");
        assert!(msg.order.is_exchange_generated());
        assert!(msg.order.is_adl());
    }

    #[rstest]
    fn test_is_exchange_generated_settlement_autoclose() {
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_settlement.json");
        assert!(msg.order.is_exchange_generated());
        assert!(msg.order.is_settlement());
    }

    #[rstest]
    fn test_is_exchange_generated_delivery_autoclose() {
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_delivery.json");
        assert!(msg.order.is_exchange_generated());
        assert!(msg.order.is_settlement());
        assert!(!msg.order.is_liquidation());
        assert!(!msg.order.is_adl());
    }

    #[rstest]
    fn test_normal_order_is_not_exchange_generated() {
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_trade.json");
        assert!(!msg.order.is_exchange_generated());
        assert!(!msg.order.is_liquidation());
        assert!(!msg.order.is_adl());
        assert!(!msg.order.is_settlement());
    }

    #[rstest]
    fn test_parse_insurance_fill_with_new_insurance_status() {
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_insurance.json");

        assert!(msg.order.is_liquidation());
        assert!(msg.order.is_exchange_generated());
        assert_eq!(msg.order.order_status, BinanceOrderStatus::NewInsurance);

        let fill = parse_futures_order_update_to_fill(
            &msg,
            account_id(),
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            None,
            None,
            Currency::USDT(),
            None,
            UnixNanos::from(1_000_000_000u64),
        )
        .unwrap();

        assert_eq!(
            fill.client_order_id,
            Some(ClientOrderId::new("autoclose-insurance-5678"))
        );
        assert_eq!(fill.order_side, OrderSide::Sell);
        assert_eq!(fill.last_qty, Quantity::new(0.020, SIZE_PRECISION));
        assert_eq!(fill.last_px, Price::new(45000.00, PRICE_PRECISION));
    }

    #[rstest]
    fn test_parse_insurance_status_maps_new_insurance_to_filled() {
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_insurance.json");

        let status = parse_futures_order_update_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            false,
            UnixNanos::from(1_000_000_000u64),
        )
        .unwrap();

        assert_eq!(status.order_status, OrderStatus::Filled);
    }

    #[rstest]
    fn test_parse_settlement_status_report() {
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_settlement.json");

        let status = parse_futures_order_update_to_order_status(
            &msg,
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            account_id(),
            false,
            UnixNanos::from(1_000_000_000u64),
        )
        .unwrap();

        assert_eq!(status.order_status, OrderStatus::Filled);
        assert_eq!(status.order_side, OrderSide::Sell);
        assert_eq!(status.quantity, Quantity::new(0.010, SIZE_PRECISION));
        assert_eq!(status.filled_qty, Quantity::new(0.010, SIZE_PRECISION));
    }

    #[rstest]
    fn test_pending_liquidation_has_zero_fill_qty() {
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_calculated_pending.json");

        assert!(msg.order.is_exchange_generated());
        assert!(msg.order.is_liquidation());

        let last_qty = parse_required_decimal(&msg.order.last_filled_qty, "last_filled_qty")
            .expect("last_filled_qty should parse");
        assert!(last_qty.is_zero());
    }

    #[rstest]
    #[case::venue_provided(Some("USDT"), Some("0.06937084"), None, None, 0.06937084, "USDT")]
    #[case::fallback_from_taker_fee(
        None, None,
        Some("0.0004"), Some("USDT"),
        0.055496, "USDT"  // 0.0004 * 0.014 * 9910.12 ≈ 0.05549...
    )]
    #[case::no_commission_no_fee(None, None, None, None, 0.0, "USDT")]
    fn test_resolve_commission(
        #[case] commission_asset: Option<&str>,
        #[case] commission_amount: Option<&str>,
        #[case] taker_fee_str: Option<&str>,
        #[case] quote_currency_str: Option<&str>,
        #[case] expected_amount: f64,
        #[case] expected_currency: &str,
    ) {
        let mut msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_calculated.json");
        msg.order.commission_asset = commission_asset.map(ustr::Ustr::from);
        msg.order.commission = commission_amount.map(String::from);

        let last_qty = Quantity::from_decimal_dp(
            Decimal::from_str_exact(&msg.order.last_filled_qty).unwrap(),
            SIZE_PRECISION,
        )
        .unwrap();
        let last_px = Price::from_decimal_dp(
            Decimal::from_str_exact(&msg.order.last_filled_price).unwrap(),
            PRICE_PRECISION,
        )
        .unwrap();
        let taker_fee = taker_fee_str.map(|s| Decimal::from_str_exact(s).unwrap());
        let quote_currency = quote_currency_str.map(Currency::from);

        let commission = resolve_commission(
            &msg.order,
            last_qty,
            last_px,
            taker_fee,
            quote_currency,
            Currency::USDT(),
        )
        .unwrap();

        assert_eq!(commission.currency, Currency::from(expected_currency));
        let diff = (commission.as_f64() - expected_amount).abs();
        assert!(
            diff < 1e-4,
            "expected {expected_amount}, was {}",
            commission.as_f64()
        );
    }

    #[rstest]
    #[case::with_venue_position_id(
        Some(Decimal::from_str_exact("0.0004").unwrap()),
        Some(Currency::from("USDT")),
        Some(PositionId::new("ETHUSDT-PERP.BINANCE-LONG")),
    )]
    #[case::without_extras(None, None, None)]
    fn test_parse_fill_with_optional_params(
        #[case] taker_fee: Option<Decimal>,
        #[case] quote_currency: Option<Currency>,
        #[case] venue_position_id: Option<PositionId>,
    ) {
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_calculated.json");
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let fill = parse_futures_order_update_to_fill(
            &msg,
            account_id(),
            instrument_id(),
            PRICE_PRECISION,
            SIZE_PRECISION,
            taker_fee,
            quote_currency,
            Currency::USDT(),
            venue_position_id,
            ts_init,
        )
        .unwrap();

        assert_eq!(fill.venue_position_id, venue_position_id);
        assert_eq!(fill.account_id, account_id());
        assert_eq!(fill.instrument_id, instrument_id());
    }
}
