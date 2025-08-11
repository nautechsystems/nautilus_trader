// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::str::FromStr;

use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime, uuid::UUID4};
use nautilus_model::{
    currencies::CURRENCY_MAP,
    data::trade::TradeTick,
    enums::CurrencyType,
    identifiers::{AccountId, ClientOrderId, OrderListId, Symbol, TradeId, VenueOrderId},
    instruments::{
        any::InstrumentAny, crypto_future::CryptoFuture, crypto_perpetual::CryptoPerpetual,
        currency_pair::CurrencyPair,
    },
    reports::{fill::FillReport, order::OrderStatusReport, position::PositionStatusReport},
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};
use rust_decimal::Decimal;
use uuid::Uuid;

use super::models::{Execution, Instrument, Order, Position, Trade};
use crate::{
    common::parse::{
        parse_aggressor_side, parse_contingency_type, parse_instrument_id, parse_liquidity_side,
        parse_optional_datetime_to_unix_nanos, parse_order_side, parse_order_status,
        parse_order_type, parse_position_side, parse_time_in_force,
    },
    enums::{ExecInstruction, InstrumentType},
};

#[must_use]
pub fn parse_instrument_any(instrument: &Instrument, ts_init: UnixNanos) -> Option<InstrumentAny> {
    match instrument.instrument_type {
        InstrumentType::Spot => Some(parse_spot_instrument(instrument, ts_init)),
        InstrumentType::PerpetualContract => Some(parse_perpetual_instrument(instrument, ts_init)),
        InstrumentType::Futures => Some(parse_futures_instrument(instrument, ts_init)),
        _ => None,
    }
}

#[must_use]
pub fn parse_spot_instrument(definition: &Instrument, ts_init: UnixNanos) -> InstrumentAny {
    let instrument_id = parse_instrument_id(&definition.symbol);
    let raw_symbol = Symbol::new(&definition.symbol);
    let base_currency = get_currency(definition.underlying.to_uppercase());
    let quote_currency = get_currency(definition.quote_currency.to_uppercase());

    let price_increment = Price::from(definition.tick_size.to_string());
    let size_increment = Quantity::from(1);

    let taker_fee = Decimal::from_str(definition.taker_fee.to_string().as_str())
        .expect("Invalid decimal value");
    let maker_fee = Decimal::from_str(definition.maker_fee.to_string().as_str())
        .expect("Invalid decimal value");

    let margin_init = definition
        .init_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);
    let margin_maint = definition
        .maint_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);

    let lot_size = Some(Quantity::from(definition.lot_size.to_string()));
    let max_quantity = Some(Quantity::from(definition.max_order_qty.to_string()));
    let min_quantity = Some(Quantity::from(definition.lot_size.to_string()));
    let max_notional: Option<Money> = None;
    let min_notional: Option<Money> = None;
    let max_price = Some(Price::from(definition.max_price.to_string()));
    let min_price = None;
    let ts_event = UnixNanos::from(definition.timestamp);

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None, // multiplier
        lot_size,
        max_quantity,
        min_quantity,
        max_notional,
        min_notional,
        max_price,
        min_price,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    InstrumentAny::CurrencyPair(instrument)
}

#[must_use]
pub fn parse_perpetual_instrument(definition: &Instrument, ts_init: UnixNanos) -> InstrumentAny {
    let instrument_id = parse_instrument_id(&definition.symbol);
    let raw_symbol = Symbol::new(&definition.symbol);
    let base_currency = get_currency(definition.underlying.to_uppercase());
    let quote_currency = get_currency(definition.quote_currency.to_uppercase());
    let settlement_currency = get_currency(definition.settl_currency.to_uppercase());
    let is_inverse = definition.is_inverse;

    let price_increment = Price::from(definition.tick_size.to_string());
    let size_increment = Quantity::from(1);

    let taker_fee = Decimal::from_str(definition.taker_fee.to_string().as_str())
        .expect("Invalid decimal value");
    let maker_fee = Decimal::from_str(definition.maker_fee.to_string().as_str())
        .expect("Invalid decimal value");

    let margin_init = definition
        .init_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);
    let margin_maint = definition
        .maint_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);

    // TODO: How to handle negative multipliers?
    let multiplier = Some(Quantity::from(definition.multiplier.abs().to_string()));
    let lot_size = Some(Quantity::from(definition.lot_size.to_string()));
    let max_quantity = Some(Quantity::from(definition.max_order_qty.to_string()));
    let min_quantity = Some(Quantity::from(definition.lot_size.to_string()));
    let max_notional: Option<Money> = None;
    let min_notional: Option<Money> = None;
    let max_price = Some(Price::from(definition.max_price.to_string()));
    let min_price = None;
    let ts_event = UnixNanos::from(definition.timestamp);

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
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    InstrumentAny::CryptoPerpetual(instrument)
}

#[must_use]
pub fn parse_futures_instrument(definition: &Instrument, ts_init: UnixNanos) -> InstrumentAny {
    let instrument_id = parse_instrument_id(&definition.symbol);
    let raw_symbol = Symbol::new(&definition.symbol);
    let underlying = get_currency(definition.underlying.to_uppercase());
    let quote_currency = get_currency(definition.quote_currency.to_uppercase());
    let settlement_currency = get_currency(definition.settl_currency.to_uppercase());
    let is_inverse = definition.is_inverse;

    let activation_ns = UnixNanos::from(definition.listing);
    let expiration_ns = parse_optional_datetime_to_unix_nanos(&definition.expiry, "expiry");
    let price_increment = Price::from(definition.tick_size.to_string());
    let size_increment = Quantity::from(1);

    let taker_fee = Decimal::from_str(definition.taker_fee.to_string().as_str())
        .expect("Invalid decimal value");
    let maker_fee = Decimal::from_str(definition.maker_fee.to_string().as_str())
        .expect("Invalid decimal value");

    let margin_init = definition
        .init_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);
    let margin_maint = definition
        .maint_margin
        .as_ref()
        .and_then(|margin| Decimal::from_str(&margin.to_string()).ok())
        .unwrap_or(Decimal::ZERO);

    // TODO: How to handle negative multipliers?
    let multiplier = Some(Quantity::from(definition.multiplier.abs().to_string()));

    let lot_size = Some(Quantity::from(definition.lot_size.to_string()));
    let max_quantity = Some(Quantity::from(definition.max_order_qty.to_string()));
    let min_quantity = Some(Quantity::from(definition.lot_size.to_string()));
    let max_notional: Option<Money> = None;
    let min_notional: Option<Money> = None;
    let max_price = Some(Price::from(definition.max_price.to_string()));
    let min_price = None;
    let ts_event = UnixNanos::from(definition.timestamp);

    let instrument = CryptoFuture::new(
        instrument_id,
        raw_symbol,
        underlying,
        quote_currency,
        settlement_currency,
        is_inverse,
        activation_ns,
        expiration_ns,
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
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    InstrumentAny::CryptoFuture(instrument)
}

pub fn parse_trade(
    trade: Trade,
    price_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let instrument_id = parse_instrument_id(&trade.symbol);
    let price = Price::new(trade.price.unwrap_or(0.0), price_precision);
    let size = Quantity::from(trade.size.unwrap_or(0));
    let aggressor_side = parse_aggressor_side(&trade.side);
    let trade_id = TradeId::new(
        trade
            .trd_match_id
            .map(|uuid| uuid.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
    );
    let ts_event = UnixNanos::from(trade.timestamp);

    Ok(TradeTick::new(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    ))
}

pub fn parse_order_status_report(
    order: Order,
    price_precision: u8,
) -> anyhow::Result<OrderStatusReport> {
    let account_id = AccountId::new(order.account.unwrap_or(0).to_string());
    let instrument_id = parse_instrument_id(&order.symbol.expect("Order should have `symbol`"));
    let venue_order_id = VenueOrderId::new(order.order_id.to_string());
    let order_side = parse_order_side(&order.side);
    let order_type = parse_order_type(&order.ord_type.expect("Order should have `ord_type`"));
    let time_in_force = parse_time_in_force(
        &order
            .time_in_force
            .expect("Order should have time in force"),
    );
    let order_status =
        parse_order_status(&order.ord_status.expect("Order should have `ord_status`"));
    let quantity = Quantity::from(order.order_qty.expect("Order should have `order_qty`"));
    let filled_qty = Quantity::from(order.cum_qty.unwrap_or(0));
    let report_id = UUID4::new();
    let ts_accepted = UnixNanos::from(order.transact_time.unwrap()); // TODO: Tighten model
    let ts_last = UnixNanos::from(order.timestamp.unwrap()); // TODO: Tighten model
    let ts_init = get_atomic_clock_realtime().get_time_ns();

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None, // client_order_id - will be set later if present
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
        Some(report_id),
    );

    if let Some(cl_ord_id) = order.cl_ord_id {
        report = report.with_client_order_id(ClientOrderId::new(cl_ord_id));
    }

    if let Some(cl_ord_link_id) = order.cl_ord_link_id {
        report = report.with_order_list_id(OrderListId::new(cl_ord_link_id));
    }

    if let Some(price) = order.price {
        report = report.with_price(Price::new(price, price_precision));
    }

    if let Some(avg_px) = order.avg_px {
        report = report.with_avg_px(avg_px);
    }

    if let Some(trigger_price) = order.stop_px {
        report = report.with_trigger_price(Price::new(trigger_price, price_precision));
    }

    if let Some(exec_inst) = order.exec_inst {
        match exec_inst {
            ExecInstruction::ParticipateDoNotInitiate => report = report.with_post_only(true),
            ExecInstruction::ReduceOnly => report = report.with_reduce_only(true),
            _ => panic!("Not implemented, {exec_inst}"), // TODO: Implement
        };
    }

    if let Some(contingency_type) = order.contingency_type {
        report = report.with_contingency_type(parse_contingency_type(&contingency_type));
    }

    // if let Some(expire_time) = order.ex {
    //     report = report.with_trigger_price(Price::new(trigger_price, price_precision));
    // }

    Ok(report)
}

pub fn parse_fill_report(exec: Execution, price_precision: u8) -> anyhow::Result<FillReport> {
    let account_id = AccountId::new(exec.account.unwrap_or(0).to_string());
    let instrument_id = parse_instrument_id(&exec.symbol.expect("Fill should have a `symbol`"));
    let venue_order_id = VenueOrderId::new(
        exec.order_id
            .expect("Fill should have an `order_id`")
            .to_string(),
    );
    let trade_id = TradeId::new(
        exec.trd_match_id
            .expect("Fill should have a `trd_match_id`")
            .to_string(),
    );
    let order_side = parse_order_side(&exec.side);
    let last_qty = Quantity::from(exec.last_qty.expect("Fill should have a `last_qty`"));
    let last_px = Price::new(
        exec.last_px.expect("Fill should have a `last_px`"),
        price_precision,
    );
    let settlement_currency = exec.settl_currency.unwrap_or("XBT".to_string());
    let commission = Money::new(
        exec.commission.unwrap_or(0.0),
        Currency::from(settlement_currency),
    );
    let liquidity_side = parse_liquidity_side(&exec.last_liquidity_ind);
    let client_order_id = exec.cl_ord_id.map(ClientOrderId::new);
    let venue_position_id = None; // Not applicable on BitMEX
    let ts_event = UnixNanos::from(
        exec.transact_time
            .expect("Fill should have `transact_time`"),
    );
    let ts_init = get_atomic_clock_realtime().get_time_ns();

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
        client_order_id,
        venue_position_id,
        ts_event,
        ts_init,
        None,
    ))
}

pub fn parse_position_report(position: Position) -> anyhow::Result<PositionStatusReport> {
    let account_id = AccountId::new(position.account.to_string());
    let instrument_id = parse_instrument_id(&position.symbol);
    let position_side = parse_position_side(position.current_qty);
    let quantity = Quantity::from(position.current_qty.map(|qty| qty.abs()).unwrap_or(0_i64));
    let venue_position_id = None; // Not applicable on BitMEX
    let ts_last = parse_optional_datetime_to_unix_nanos(&position.timestamp, "timestamp");
    let ts_init = get_atomic_clock_realtime().get_time_ns();

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        venue_position_id,
        ts_last,
        ts_init,
        None,
    ))
}

/// Returns the currency either from the internal currency map or creates a default crypto.
fn get_currency(code: String) -> Currency {
    CURRENCY_MAP
        .lock()
        .unwrap()
        .get(&code)
        .copied()
        .unwrap_or(Currency::new(&code, 8, 0, &code, CurrencyType::Crypto))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        common::testing::load_test_json,
        enums::{LiquidityIndicator, OrderStatus, Side},
        http::models::{TradeBin, Wallet},
    };

    #[rstest]
    fn test_perp_instrument_deserialization() {
        let json_data = load_test_json("http_get_instrument_xbtusd.json");
        let instrument: Instrument = serde_json::from_str(&json_data).unwrap();

        assert_eq!(instrument.symbol, "XBTUSD");
        assert_eq!(instrument.root_symbol, "XBT");
        assert_eq!(instrument.state, "Open");
        assert!(instrument.is_inverse);
        assert_eq!(instrument.maker_fee, 0.0005);
        assert_eq!(
            instrument.timestamp.to_rfc3339(),
            "2024-11-24T23:33:19.034+00:00"
        );
    }

    #[rstest]
    fn test_parse_orders() {
        let json_data = load_test_json("http_get_orders.json");
        let orders: Vec<Order> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(orders.len(), 2);

        // Test first order (New)
        let order1 = &orders[0];
        assert_eq!(order1.symbol, Some("XBTUSD".to_string()));
        assert_eq!(order1.side, Some(Side::Buy));
        assert_eq!(order1.order_qty, Some(100));
        assert_eq!(order1.price, Some(98000.0));
        assert_eq!(order1.ord_status, Some(OrderStatus::New));
        assert_eq!(order1.leaves_qty, Some(100));
        assert_eq!(order1.cum_qty, Some(0));

        // Test second order (Filled)
        let order2 = &orders[1];
        assert_eq!(order2.symbol, Some("XBTUSD".to_string()));
        assert_eq!(order2.side, Some(Side::Sell));
        assert_eq!(order2.order_qty, Some(200));
        assert_eq!(order2.ord_status, Some(OrderStatus::Filled));
        assert_eq!(order2.leaves_qty, Some(0));
        assert_eq!(order2.cum_qty, Some(200));
        assert_eq!(order2.avg_px, Some(98950.5));
    }

    #[rstest]
    fn test_parse_executions() {
        let json_data = load_test_json("http_get_executions.json");
        let executions: Vec<Execution> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(executions.len(), 2);

        // Test first execution (Maker)
        let exec1 = &executions[0];
        assert_eq!(exec1.symbol, Some("XBTUSD".to_string()));
        assert_eq!(exec1.side, Some(Side::Sell));
        assert_eq!(exec1.last_qty, Some(100));
        assert_eq!(exec1.last_px, Some(98950.0));
        assert_eq!(exec1.last_liquidity_ind, Some(LiquidityIndicator::Maker));
        assert_eq!(exec1.commission, Some(0.00075));

        // Test second execution (Taker)
        let exec2 = &executions[1];
        assert_eq!(exec2.last_liquidity_ind, Some(LiquidityIndicator::Taker));
        assert_eq!(exec2.last_px, Some(98951.0));
    }

    #[rstest]
    fn test_parse_positions() {
        let json_data = load_test_json("http_get_positions.json");
        let positions: Vec<Position> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(positions.len(), 1);

        let position = &positions[0];
        assert_eq!(position.account, 1234567);
        assert_eq!(position.symbol, "XBTUSD");
        assert_eq!(position.current_qty, Some(100));
        assert_eq!(position.avg_entry_price, Some(98390.88));
        assert_eq!(position.unrealised_pnl, Some(1350));
        assert_eq!(position.realised_pnl, Some(-227));
        assert_eq!(position.is_open, Some(true));
    }

    #[rstest]
    fn test_parse_trades() {
        let json_data = load_test_json("http_get_trades.json");
        let trades: Vec<Trade> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(trades.len(), 3);

        // Test first trade
        let trade1 = &trades[0];
        assert_eq!(trade1.symbol, "XBTUSD");
        assert_eq!(trade1.side, Some(Side::Buy));
        assert_eq!(trade1.size, Some(100));
        assert_eq!(trade1.price, Some(98950.0));

        // Test third trade (Sell side)
        let trade3 = &trades[2];
        assert_eq!(trade3.side, Some(Side::Sell));
        assert_eq!(trade3.size, Some(50));
        assert_eq!(trade3.price, Some(98949.5));
    }

    #[rstest]
    fn test_parse_wallet() {
        let json_data = load_test_json("http_get_wallet.json");
        let wallets: Vec<Wallet> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(wallets.len(), 1);

        let wallet = &wallets[0];
        assert_eq!(wallet.account, 1234567);
        assert_eq!(wallet.currency, "XBt");
        assert_eq!(wallet.amount, Some(1000123456));
        assert_eq!(wallet.delta_amount, Some(123456));
    }

    #[rstest]
    fn test_parse_trade_bins() {
        let json_data = load_test_json("http_get_trade_bins.json");
        let bins: Vec<TradeBin> = serde_json::from_str(&json_data).unwrap();

        assert_eq!(bins.len(), 3);

        // Test first bin
        let bin1 = &bins[0];
        assert_eq!(bin1.symbol, "XBTUSD");
        assert_eq!(bin1.open, Some(98900.0));
        assert_eq!(bin1.high, Some(98980.5));
        assert_eq!(bin1.low, Some(98890.0));
        assert_eq!(bin1.close, Some(98950.0));
        assert_eq!(bin1.volume, Some(150000));
        assert_eq!(bin1.trades, Some(45));

        // Test last bin
        let bin3 = &bins[2];
        assert_eq!(bin3.close, Some(98970.0));
        assert_eq!(bin3.volume, Some(78000));
    }
}
