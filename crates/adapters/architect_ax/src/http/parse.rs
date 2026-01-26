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

//! Parsing functions to convert Ax HTTP responses to Nautilus domain types.

use anyhow::Context;
use nautilus_core::{UUID4, nanos::UnixNanos};
use nautilus_model::{
    data::{Bar, BarSpecification, BarType},
    enums::{
        AccountType, AggregationSource, BarAggregation, LiquiditySide, OrderSide, OrderType,
        PositionSideSpecified, PriceType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    instruments::{CryptoPerpetual, Instrument, any::InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use super::models::{AxBalancesResponse, AxCandle, AxFill, AxInstrument, AxOpenOrder, AxPosition};
use crate::common::{consts::AX_VENUE, enums::AxCandleWidth};

/// Converts a Decimal value to a Price.
///
/// # Errors
///
/// Returns an error if the Decimal cannot be converted to Price.
fn decimal_to_price(value: Decimal, field_name: &str) -> anyhow::Result<Price> {
    Price::from_decimal(value)
        .with_context(|| format!("Failed to convert {field_name} Decimal to Price"))
}

/// Converts a Decimal value to a Quantity.
///
/// # Errors
///
/// Returns an error if the Decimal cannot be converted to Quantity.
fn decimal_to_quantity(value: Decimal, field_name: &str) -> anyhow::Result<Quantity> {
    Quantity::from_decimal(value)
        .with_context(|| format!("Failed to convert {field_name} Decimal to Quantity"))
}

/// Converts a Decimal to a Price with specific precision.
fn decimal_to_price_dp(value: Decimal, precision: u8, field: &str) -> anyhow::Result<Price> {
    Price::from_decimal_dp(value, precision).with_context(|| {
        format!("Failed to construct Price for {field} with precision {precision}")
    })
}

/// Gets or creates a Currency from a currency code string.
#[must_use]
fn get_currency(code: &str) -> Currency {
    Currency::from(code)
}

/// Converts an Ax candle width to a Nautilus bar specification.
#[must_use]
pub fn candle_width_to_bar_spec(width: AxCandleWidth) -> BarSpecification {
    match width {
        AxCandleWidth::Seconds1 => {
            BarSpecification::new(1, BarAggregation::Second, PriceType::Last)
        }
        AxCandleWidth::Seconds5 => {
            BarSpecification::new(5, BarAggregation::Second, PriceType::Last)
        }
        AxCandleWidth::Minutes1 => {
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last)
        }
        AxCandleWidth::Minutes5 => {
            BarSpecification::new(5, BarAggregation::Minute, PriceType::Last)
        }
        AxCandleWidth::Minutes15 => {
            BarSpecification::new(15, BarAggregation::Minute, PriceType::Last)
        }
        AxCandleWidth::Hours1 => BarSpecification::new(1, BarAggregation::Hour, PriceType::Last),
        AxCandleWidth::Days1 => BarSpecification::new(1, BarAggregation::Day, PriceType::Last),
    }
}

/// Parses an Ax candle into a Nautilus Bar.
///
/// # Errors
///
/// Returns an error if any OHLCV field cannot be parsed.
pub fn parse_bar(
    candle: &AxCandle,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open = decimal_to_price_dp(candle.open, price_precision, "candle.open")?;
    let high = decimal_to_price_dp(candle.high, price_precision, "candle.high")?;
    let low = decimal_to_price_dp(candle.low, price_precision, "candle.low")?;
    let close = decimal_to_price_dp(candle.close, price_precision, "candle.close")?;

    // Ax provides volume as i64 contracts
    let volume = Quantity::new(candle.volume as f64, size_precision);

    let ts_event = UnixNanos::from(candle.tn.timestamp_nanos_opt().unwrap_or(0) as u64);

    let bar_spec = candle_width_to_bar_spec(candle.width);
    let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::External);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("Failed to construct Bar from Ax candle")
}

/// Parses an Ax perpetual futures instrument into a Nautilus CryptoPerpetual.
///
/// # Errors
///
/// Returns an error if any required field cannot be parsed or is invalid.
pub fn parse_perp_instrument(
    definition: &AxInstrument,
    maker_fee: Decimal,
    taker_fee: Decimal,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let raw_symbol_str = definition.symbol.as_str();
    let raw_symbol = Symbol::new(raw_symbol_str);
    let instrument_id = InstrumentId::new(raw_symbol, *AX_VENUE);

    // Extract base currency from symbol:
    // - Crypto: BTC-PERP → base=BTC
    // - FX: JPYUSD-PERP → base=JPY (strip quote currency suffix)
    let symbol_prefix = raw_symbol_str
        .split('-')
        .next()
        .context("Failed to extract symbol prefix")?;

    let quote_code = definition.quote_currency.as_str();
    let base_code = if symbol_prefix.ends_with(quote_code) && symbol_prefix.len() > quote_code.len()
    {
        &symbol_prefix[..symbol_prefix.len() - quote_code.len()]
    } else {
        symbol_prefix
    };
    let base_currency = get_currency(base_code);

    let quote_currency = get_currency(quote_code);
    let settlement_currency = quote_currency;

    let price_increment = decimal_to_price(definition.tick_size, "tick_size")?;
    let size_increment = decimal_to_quantity(definition.minimum_order_size, "minimum_order_size")?;

    let lot_size = Some(size_increment);
    let min_quantity = Some(size_increment);

    let margin_init = definition.initial_margin_pct;
    let margin_maint = definition.maintenance_margin_pct;

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        false, // Ax perps are linear/USDT-margined
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None,
        lot_size,
        None,
        min_quantity,
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

/// Parses an Ax balances response into a Nautilus [`AccountState`].
///
/// Ax provides a simple balance structure with symbol and amount.
/// The amount is treated as both total and free balance (no locked funds tracking).
///
/// # Errors
///
/// Returns an error if balance amount parsing fails.
pub fn parse_account_state(
    response: &AxBalancesResponse,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let mut balances = Vec::new();

    for balance in &response.balances {
        let symbol_str = balance.symbol.as_str().trim();
        if symbol_str.is_empty() {
            log::debug!("Skipping balance with empty symbol");
            continue;
        }

        let currency = Currency::from(symbol_str);

        let total = Money::from_decimal(balance.amount, currency)
            .with_context(|| format!("Failed to convert balance for {symbol_str}"))?;
        let locked = Money::new(0.0, currency);
        let free = total;

        balances.push(AccountBalance::new(total, locked, free));
    }

    if balances.is_empty() {
        let zero_currency = Currency::USD();
        let zero_money = Money::new(0.0, zero_currency);
        balances.push(AccountBalance::new(zero_money, zero_money, zero_money));
    }

    Ok(AccountState::new(
        account_id,
        AccountType::Margin,
        balances,
        vec![],
        true,
        UUID4::new(),
        ts_event,
        ts_init,
        None,
    ))
}

/// Parses an Ax open order into a Nautilus [`OrderStatusReport`].
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity fields cannot be parsed.
/// - Timestamp conversion fails.
pub fn parse_order_status_report(
    order: &AxOpenOrder,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&order.oid);
    let order_side = order.d.into();
    let order_status = order.o.into();
    let time_in_force = order.tif.into();

    // Ax only supports limit orders currently
    let order_type = OrderType::Limit;

    // Parse quantity (Ax uses i64 contracts)
    let quantity = Quantity::new(order.q as f64, instrument.size_precision());
    let filled_qty = Quantity::new(order.xq as f64, instrument.size_precision());

    // Parse price
    let price = decimal_to_price_dp(order.p, instrument.price_precision(), "order.p")?;

    // Ax timestamps are in Unix epoch seconds
    let ts_event = UnixNanos::from((order.ts as u64) * 1_000_000_000);

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
        ts_event,
        ts_event,
        ts_init,
        Some(UUID4::new()),
    );

    // Add client order ID if tag is present
    if let Some(ref tag) = order.tag
        && !tag.is_empty()
    {
        report = report.with_client_order_id(ClientOrderId::new(tag.as_str()));
    }

    report = report.with_price(price);

    // Calculate average price if there are fills
    if order.xq > 0 {
        let avg_px = price.as_f64();
        report = report.with_avg_px(avg_px)?;
    }

    Ok(report)
}

/// Parses an Ax fill into a Nautilus [`FillReport`].
///
/// Note: Ax fills don't include order ID, side, or liquidity information
/// in the fills endpoint response, so we use default values where necessary.
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity fields cannot be parsed.
/// - Fee parsing fails.
pub fn parse_fill_report(
    fill: &AxFill,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();

    let venue_order_id = VenueOrderId::new(&fill.order_id);
    let trade_id = TradeId::new_checked(&fill.trade_id).context("Invalid trade_id in Ax fill")?;

    // Ax doesn't provide order side in fills, infer from quantity sign
    let order_side = if fill.quantity >= 0 {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };

    let last_px = decimal_to_price_dp(fill.price, instrument.price_precision(), "fill.price")?;
    let last_qty = Quantity::new(
        fill.quantity.unsigned_abs() as f64,
        instrument.size_precision(),
    );

    // Parse fee (Ax returns positive fee, Nautilus uses negative for costs)
    let currency = Currency::USD();
    let commission = Money::from_decimal(-fill.fee, currency)
        .context("Failed to convert fill.fee Decimal to Money")?;

    let liquidity_side = if fill.is_taker {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    let ts_event = UnixNanos::from(
        fill.timestamp
            .timestamp_nanos_opt()
            .unwrap_or(0)
            .unsigned_abs(),
    );

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
        None,
        None,
        ts_event,
        ts_init,
        None,
    ))
}

/// Parses an Ax position into a Nautilus [`PositionStatusReport`].
///
/// # Errors
///
/// Returns an error if:
/// - Position quantity parsing fails.
/// - Timestamp conversion fails.
pub fn parse_position_status_report(
    position: &AxPosition,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let instrument_id = instrument.id();

    // Determine position side and quantity from open_quantity sign
    let (position_side, quantity) = if position.open_quantity > 0 {
        (
            PositionSideSpecified::Long,
            Quantity::new(position.open_quantity as f64, instrument.size_precision()),
        )
    } else if position.open_quantity < 0 {
        (
            PositionSideSpecified::Short,
            Quantity::new(
                position.open_quantity.unsigned_abs() as f64,
                instrument.size_precision(),
            ),
        )
    } else {
        (
            PositionSideSpecified::Flat,
            Quantity::new(0.0, instrument.size_precision()),
        )
    };

    // Calculate average entry price from notional / quantity
    let avg_px_open = if position.open_quantity != 0 {
        let qty_dec = Decimal::from(position.open_quantity.abs());
        Some(position.open_notional / qty_dec)
    } else {
        None
    };

    let ts_last = UnixNanos::from(
        position
            .timestamp
            .timestamp_nanos_opt()
            .unwrap_or(0)
            .unsigned_abs(),
    );

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        ts_last,
        ts_init,
        None,
        None,
        avg_px_open,
    ))
}

#[cfg(test)]
mod tests {
    use nautilus_core::nanos::UnixNanos;
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::{common::enums::AxInstrumentState, http::models::AxInstrumentsResponse};

    fn create_test_instrument() -> AxInstrument {
        AxInstrument {
            symbol: Ustr::from("BTC-PERP"),
            state: AxInstrumentState::Open,
            multiplier: dec!(1.0),
            minimum_order_size: dec!(0.001),
            tick_size: dec!(0.5),
            quote_currency: Ustr::from("USD"),
            finding_settlement_currency: Ustr::from("USD"),
            maintenance_margin_pct: dec!(0.005),
            initial_margin_pct: dec!(0.01),
            contract_mark_price: Some("45000.50".to_string()),
            contract_size: Some("1 BTC per contract".to_string()),
            description: Some("Bitcoin Perpetual Futures".to_string()),
            funding_calendar_schedule: Some("0,8,16".to_string()),
            funding_frequency: Some("8h".to_string()),
            funding_rate_cap_lower_pct: Some(dec!(-0.0075)),
            funding_rate_cap_upper_pct: Some(dec!(0.0075)),
            price_band_lower_deviation_pct: Some(dec!(0.05)),
            price_band_upper_deviation_pct: Some(dec!(0.05)),
            price_bands: Some("dynamic".to_string()),
            price_quotation: Some("USD".to_string()),
            underlying_benchmark_price: Some("CME CF BRR".to_string()),
        }
    }

    #[rstest]
    fn test_decimal_to_price() {
        let price = decimal_to_price(dec!(100.50), "test_field").unwrap();
        assert_eq!(price.as_f64(), 100.50);
    }

    #[rstest]
    fn test_decimal_to_quantity() {
        let qty = decimal_to_quantity(dec!(1.5), "test_field").unwrap();
        assert_eq!(qty.as_f64(), 1.5);
    }

    #[rstest]
    fn test_get_currency() {
        let currency = get_currency("USD");
        assert_eq!(currency.code, Ustr::from("USD"));
    }

    #[rstest]
    fn test_parse_perp_instrument() {
        let definition = create_test_instrument();
        let maker_fee = Decimal::new(2, 4);
        let taker_fee = Decimal::new(5, 4);
        let ts_now = UnixNanos::default();

        let result = parse_perp_instrument(&definition, maker_fee, taker_fee, ts_now, ts_now);
        assert!(result.is_ok());

        let instrument = result.unwrap();
        match instrument {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.symbol.as_str(), "BTC-PERP");
                assert_eq!(perp.id.venue, *AX_VENUE);
                assert_eq!(perp.base_currency.code.as_str(), "BTC");
                assert_eq!(perp.quote_currency.code.as_str(), "USD");
                assert!(!perp.is_inverse);
            }
            _ => panic!("Expected CryptoPerpetual instrument"),
        }
    }

    #[rstest]
    fn test_deserialize_instruments_from_test_data() {
        let test_data = include_str!("../../test_data/http_get_instruments.json");
        let response: AxInstrumentsResponse =
            serde_json::from_str(test_data).expect("Failed to deserialize test data");

        assert_eq!(response.instruments.len(), 3);

        let btc = &response.instruments[0];
        assert_eq!(btc.symbol.as_str(), "BTC-PERP");
        assert_eq!(btc.state, AxInstrumentState::Open);
        assert_eq!(btc.tick_size, dec!(0.5));
        assert_eq!(btc.minimum_order_size, dec!(0.001));
        assert!(btc.contract_mark_price.is_some());

        let eth = &response.instruments[1];
        assert_eq!(eth.symbol.as_str(), "ETH-PERP");
        assert_eq!(eth.state, AxInstrumentState::Open);

        // SOL-PERP is suspended with null optional fields
        let sol = &response.instruments[2];
        assert_eq!(sol.symbol.as_str(), "SOL-PERP");
        assert_eq!(sol.state, AxInstrumentState::Suspended);
        assert!(sol.contract_mark_price.is_none());
        assert!(sol.funding_frequency.is_none());
    }

    #[rstest]
    fn test_parse_all_instruments_from_test_data() {
        let test_data = include_str!("../../test_data/http_get_instruments.json");
        let response: AxInstrumentsResponse =
            serde_json::from_str(test_data).expect("Failed to deserialize test data");

        let maker_fee = Decimal::new(2, 4);
        let taker_fee = Decimal::new(5, 4);
        let ts_now = UnixNanos::default();

        let open_instruments: Vec<_> = response
            .instruments
            .iter()
            .filter(|i| i.state == AxInstrumentState::Open)
            .collect();

        assert_eq!(open_instruments.len(), 2);

        for instrument in open_instruments {
            let result = parse_perp_instrument(instrument, maker_fee, taker_fee, ts_now, ts_now);
            assert!(
                result.is_ok(),
                "Failed to parse {}: {:?}",
                instrument.symbol,
                result.err()
            );
        }
    }
}
