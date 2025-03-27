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

use nautilus_core::{UUID4, nanos::UnixNanos};
use nautilus_execution::reports::{
    fill::FillReport, order::OrderStatusReport, position::PositionStatusReport,
};
use nautilus_model::{
    enums::{
        AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, Symbol, TradeId, VenueOrderId},
    instruments::{CryptoPerpetual, CurrencyPair, any::InstrumentAny},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use super::models::{
    CoinbaseIntxBalance, CoinbaseIntxFill, CoinbaseIntxInstrument, CoinbaseIntxOrder,
    CoinbaseIntxPosition,
};
use crate::common::{
    enums::{CoinbaseIntxInstrumentType, CoinbaseIntxOrderEventType, CoinbaseIntxOrderStatus},
    parse::{get_currency, parse_instrument_id, parse_notional, parse_position_side},
};

/// Parses a Coinbase International Spot instrument into an InstrumentAny::CurrencyPair.
pub fn parse_spot_instrument(
    definition: &CoinbaseIntxInstrument,
    margin_init: Option<Decimal>,
    margin_maint: Option<Decimal>,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = parse_instrument_id(definition.symbol);
    let raw_symbol = Symbol::from_ustr_unchecked(definition.symbol);

    let base_currency = get_currency(&definition.base_asset_name);
    let quote_currency = get_currency(&definition.quote_asset_name);

    let price_increment = Price::from(&definition.quote_increment);
    let size_increment = Quantity::from(&definition.base_increment);

    let lot_size = None;
    let max_quantity = None;
    let min_quantity = None;
    let max_notional = None;
    let min_notional = parse_notional(&definition.min_notional_value, quote_currency)?;
    let max_price = None;
    let min_price = None;

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        lot_size,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
        margin_init,
        margin_maint,
        maker_fee,
        taker_fee,
        UnixNanos::from(definition.quote.timestamp),
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses a Coinbase International perpetual instrument into an InstrumentAny::CryptoPerpetual.
pub fn parse_perp_instrument(
    definition: &CoinbaseIntxInstrument,
    margin_init: Option<Decimal>,
    margin_maint: Option<Decimal>,
    maker_fee: Option<Decimal>,
    taker_fee: Option<Decimal>,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = parse_instrument_id(definition.symbol);
    let raw_symbol = Symbol::from_ustr_unchecked(definition.symbol);

    let base_currency = get_currency(&definition.base_asset_name);
    let quote_currency = get_currency(&definition.quote_asset_name);
    let settlement_currency = quote_currency;

    let price_increment = Price::from(&definition.quote_increment);
    let size_increment = Quantity::from(&definition.base_increment);

    let multiplier = Some(Quantity::from(&definition.base_asset_multiplier));

    let lot_size = None;
    let max_quantity = None;
    let min_quantity = None;
    let max_notional = None;
    let min_notional = parse_notional(&definition.min_notional_value, quote_currency)?;
    let max_price = None;
    let min_price = None;

    let is_inverse = false;

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        is_inverse,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        multiplier,
        lot_size,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
        margin_init,
        margin_maint,
        maker_fee,
        taker_fee,
        UnixNanos::from(definition.quote.timestamp),
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

#[must_use]
pub fn parse_instrument_any(
    instrument: &CoinbaseIntxInstrument,
    ts_init: UnixNanos,
) -> Option<InstrumentAny> {
    let result = match instrument.instrument_type {
        CoinbaseIntxInstrumentType::Spot => {
            parse_spot_instrument(instrument, None, None, None, None, ts_init).map(Some)
        }
        CoinbaseIntxInstrumentType::Perp => {
            parse_perp_instrument(instrument, None, None, None, None, ts_init).map(Some)
        }
        CoinbaseIntxInstrumentType::Index => Ok(None), // Not yet implemented
    };

    match result {
        Ok(instrument) => instrument,
        Err(e) => {
            tracing::warn!(
                "Failed to parse instrument {}: {e}",
                instrument.instrument_id,
            );
            None
        }
    }
}

pub fn parse_account_state(
    coinbase_balances: Vec<CoinbaseIntxBalance>,
    account_id: AccountId,
    ts_event: UnixNanos,
) -> anyhow::Result<AccountState> {
    let mut balances = Vec::new();
    for b in coinbase_balances {
        let currency = Currency::from(b.asset_name);
        let total = Money::new(b.quantity.parse::<f64>()?, currency);
        let locked = Money::new(b.hold.parse::<f64>()?, currency);
        let free = total - locked;
        let balance = AccountBalance::new(total, locked, free);
        balances.push(balance);
    }
    let margins = vec![]; // TBD

    let account_type = AccountType::Margin;
    let is_reported = true;
    let event_id = UUID4::new();

    Ok(AccountState::new(
        account_id,
        account_type,
        balances,
        margins,
        is_reported,
        event_id,
        ts_event,
        ts_event,
        None,
    ))
}

fn parse_order_status(coinbase_order: &CoinbaseIntxOrder) -> OrderStatus {
    let exec_qty = coinbase_order
        .exec_qty
        .parse::<Decimal>()
        .expect("Invalid value for `exec_qty`");

    match coinbase_order.order_status {
        CoinbaseIntxOrderStatus::Working => {
            if exec_qty > Decimal::ZERO {
                return OrderStatus::PartiallyFilled;
            }

            match coinbase_order.event_type {
                CoinbaseIntxOrderEventType::New => OrderStatus::Accepted,
                CoinbaseIntxOrderEventType::PendingNew => OrderStatus::Submitted,
                CoinbaseIntxOrderEventType::PendingCancel => OrderStatus::PendingCancel,
                CoinbaseIntxOrderEventType::PendingReplace => OrderStatus::PendingUpdate,
                CoinbaseIntxOrderEventType::StopTriggered => OrderStatus::Triggered,
                CoinbaseIntxOrderEventType::Replaced => OrderStatus::Accepted,
                // Safety fallback
                _ => {
                    tracing::debug!(
                        "Unexpected order status and last event type: {:?} {:?}",
                        coinbase_order.order_status,
                        coinbase_order.event_type
                    );
                    OrderStatus::Accepted
                }
            }
        }
        CoinbaseIntxOrderStatus::Done => {
            if exec_qty > Decimal::ZERO {
                return OrderStatus::Filled;
            }

            match coinbase_order.event_type {
                CoinbaseIntxOrderEventType::Canceled => OrderStatus::Canceled,
                CoinbaseIntxOrderEventType::Rejected => OrderStatus::Rejected,
                CoinbaseIntxOrderEventType::Expired => OrderStatus::Expired,
                // Safety fallback
                _ => {
                    tracing::debug!(
                        "Unexpected order status and last event type: {:?} {:?}",
                        coinbase_order.order_status,
                        coinbase_order.event_type
                    );
                    OrderStatus::Canceled
                }
            }
        }
    }
}

fn parse_price(value: &str, precision: u8) -> Price {
    Price::new(
        value.parse::<f64>().expect("Invalid value for `Price`"),
        precision,
    )
}

fn parse_quantity(value: &str, precision: u8) -> Quantity {
    Quantity::new(
        value.parse::<f64>().expect("Invalid value for `Quantity`"),
        precision,
    )
}

#[must_use]
pub fn parse_order_status_report(
    coinbase_order: CoinbaseIntxOrder,
    account_id: AccountId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> OrderStatusReport {
    let filled_qty = parse_quantity(&coinbase_order.exec_qty, size_precision);
    let order_status: OrderStatus = parse_order_status(&coinbase_order);

    let instrument_id = parse_instrument_id(coinbase_order.symbol);
    let client_order_id = ClientOrderId::new(coinbase_order.client_order_id);
    let venue_order_id = VenueOrderId::new(coinbase_order.order_id);
    let order_side: OrderSide = coinbase_order.side.into();
    let order_type: OrderType = coinbase_order.order_type.into();
    let time_in_force: TimeInForce = coinbase_order.tif.into();
    let quantity = parse_quantity(&coinbase_order.size, size_precision);
    let ts_accepted = UnixNanos::from(coinbase_order.submit_time.unwrap_or_default());
    let ts_last = UnixNanos::from(coinbase_order.event_time.unwrap_or_default());

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
        ts_accepted,
        ts_init,
        ts_last,
        None, // Will generate a UUID4
    );

    if let Some(price) = coinbase_order.price {
        let price = parse_price(&price, price_precision);
        report = report.with_price(price);
    };

    if let Some(stop_price) = coinbase_order.stop_price {
        let stop_price = parse_price(&stop_price, price_precision);
        report = report.with_trigger_price(stop_price);
        report = report.with_trigger_type(TriggerType::Default); // TBD
    };

    if let Some(expire_time) = coinbase_order.expire_time {
        report = report.with_expire_time(expire_time.into());
    };

    if let Some(avg_price) = coinbase_order.avg_price {
        let avg_px = avg_price
            .parse::<f64>()
            .expect("Invalid value for `avg_px`");
        report = report.with_avg_px(avg_px);
    };

    if let Some(text) = coinbase_order.text {
        report = report.with_cancel_reason(text)
    }

    report = report.with_post_only(coinbase_order.post_only);
    report = report.with_reduce_only(coinbase_order.close_only);

    report
}

#[must_use]
pub fn parse_fill_report(
    coinbase_fill: CoinbaseIntxFill,
    account_id: AccountId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> FillReport {
    let instrument_id = parse_instrument_id(coinbase_fill.symbol);
    let client_order_id = ClientOrderId::new(coinbase_fill.client_order_id);
    let venue_order_id = VenueOrderId::new(coinbase_fill.order_id);
    let trade_id = TradeId::from(coinbase_fill.fill_id);
    let order_side: OrderSide = coinbase_fill.side.into();
    let last_px = parse_price(&coinbase_fill.fill_price, price_precision);
    let last_qty = parse_quantity(&coinbase_fill.fill_qty, size_precision);
    let commission = Money::from(&format!(
        "{} {}",
        coinbase_fill.fee, coinbase_fill.fee_asset
    ));
    let liquidity = LiquiditySide::Maker; // TBD
    let ts_event = UnixNanos::from(coinbase_fill.event_time);

    FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity,
        Some(client_order_id),
        None, // Position ID not applicable on Coinbase Intx
        ts_event,
        ts_init,
        None, // Will generate a UUID4
    )
}

#[must_use]
pub fn parse_position_status_report(
    coinbase_position: CoinbaseIntxPosition,
    account_id: AccountId,
    size_precision: u8,
    ts_init: UnixNanos,
) -> PositionStatusReport {
    let instrument_id = parse_instrument_id(coinbase_position.symbol);
    let net_size = coinbase_position
        .net_size
        .parse::<f64>()
        .expect("Invalid value for `net_size`");
    let position_side = parse_position_side(Some(net_size));
    let quantity = Quantity::new(net_size.abs(), size_precision);

    PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        None, // Position ID not applicable on Coinbase Intx
        ts_init,
        ts_init,
        None, // Will generate a UUID4
    )
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::types::Money;
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    #[rstest]
    fn test_parse_spot_instrument() {
        let json_data = load_test_json("http_get_instruments_BTC-USDC.json");
        let parsed: CoinbaseIntxInstrument = serde_json::from_str(&json_data).unwrap();

        let ts_init = UnixNanos::default();
        let instrument = parse_spot_instrument(&parsed, None, None, None, None, ts_init).unwrap();

        if let InstrumentAny::CurrencyPair(pair) = instrument {
            assert_eq!(pair.id.to_string(), "BTC-USDC.COINBASE_INTX");
            assert_eq!(pair.raw_symbol.to_string(), "BTC-USDC");
            assert_eq!(pair.base_currency.to_string(), "BTC");
            assert_eq!(pair.quote_currency.to_string(), "USDC");
            assert_eq!(pair.price_increment.to_string(), "0.01");
            assert_eq!(pair.size_increment.to_string(), "0.00001");
            assert_eq!(
                pair.min_notional,
                Some(Money::new(10.0, pair.quote_currency))
            );
            assert_eq!(pair.ts_event, UnixNanos::from(parsed.quote.timestamp));
            assert_eq!(pair.ts_init, ts_init);
            assert_eq!(pair.lot_size, None);
            assert_eq!(pair.max_quantity, None);
            assert_eq!(pair.min_quantity, None);
            assert_eq!(pair.max_notional, None);
            assert_eq!(pair.max_price, None);
            assert_eq!(pair.min_price, None);
            assert_eq!(pair.margin_init, Decimal::ZERO);
            assert_eq!(pair.margin_maint, Decimal::ZERO);
            assert_eq!(pair.maker_fee, Decimal::ZERO);
            assert_eq!(pair.taker_fee, Decimal::ZERO);
        } else {
            panic!("Expected `CurrencyPair` variant");
        }
    }

    #[rstest]
    fn test_parse_perp_instrument() {
        let json_data = load_test_json("http_get_instruments_BTC-PERP.json");
        let parsed: CoinbaseIntxInstrument = serde_json::from_str(&json_data).unwrap();

        let ts_init = UnixNanos::default();
        let instrument = parse_perp_instrument(&parsed, None, None, None, None, ts_init).unwrap();

        if let InstrumentAny::CryptoPerpetual(perp) = instrument {
            assert_eq!(perp.id.to_string(), "BTC-PERP.COINBASE_INTX");
            assert_eq!(perp.raw_symbol.to_string(), "BTC-PERP");
            assert_eq!(perp.base_currency.to_string(), "BTC");
            assert_eq!(perp.quote_currency.to_string(), "USDC");
            assert_eq!(perp.settlement_currency.to_string(), "USDC");
            assert_eq!(perp.is_inverse, false);
            assert_eq!(perp.price_increment.to_string(), "0.1");
            assert_eq!(perp.size_increment.to_string(), "0.0001");
            assert_eq!(perp.multiplier.to_string(), "1.0");
            assert_eq!(
                perp.min_notional,
                Some(Money::new(10.0, perp.quote_currency))
            );
            assert_eq!(perp.ts_event, UnixNanos::from(parsed.quote.timestamp));
            assert_eq!(perp.ts_init, ts_init);
            assert_eq!(perp.lot_size, Quantity::from(1));
            assert_eq!(perp.max_quantity, None);
            assert_eq!(perp.min_quantity, None);
            assert_eq!(perp.max_notional, None);
            assert_eq!(perp.max_price, None);
            assert_eq!(perp.min_price, None);
            assert_eq!(perp.margin_init, Decimal::ZERO);
            assert_eq!(perp.margin_maint, Decimal::ZERO);
            assert_eq!(perp.maker_fee, Decimal::ZERO);
            assert_eq!(perp.taker_fee, Decimal::ZERO);
        } else {
            panic!("Expected `CryptoPerpetual` variant");
        }
    }
}
