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
use nautilus_core::{Params, UUID4, nanos::UnixNanos};
use nautilus_model::{
    data::{Bar, BarSpecification, BarType, FundingRateUpdate, TradeTick},
    enums::{
        AccountType, AggregationSource, AggressorSide, AssetClass, BarAggregation, CurrencyType,
        LiquiditySide, OrderSide, OrderType, PositionSideSpecified, PriceType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    instruments::{Instrument, PerpetualContract, any::InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use serde_json::json;
use ustr::Ustr;

use super::models::{
    AxBalancesResponse, AxCandle, AxFill, AxFundingRate, AxInstrument, AxOpenOrder, AxPosition,
    AxRestTrade,
};
use crate::common::{
    consts::AX_VENUE,
    enums::AxCandleWidth,
    parse::{ax_timestamp_ns_to_unix_nanos, ax_timestamp_s_to_unix_nanos, cid_to_client_order_id},
};

fn decimal_to_price(value: Decimal, field_name: &str) -> anyhow::Result<Price> {
    Price::from_decimal(value)
        .with_context(|| format!("Failed to convert {field_name} Decimal to Price"))
}

fn decimal_to_quantity(value: Decimal, field_name: &str) -> anyhow::Result<Quantity> {
    Quantity::from_decimal(value)
        .with_context(|| format!("Failed to convert {field_name} Decimal to Quantity"))
}

fn decimal_to_price_dp(value: Decimal, precision: u8, field: &str) -> anyhow::Result<Price> {
    Price::from_decimal_dp(value, precision).with_context(|| {
        format!("Failed to construct Price for {field} with precision {precision}")
    })
}

fn get_currency(code: &str) -> Currency {
    Currency::try_from_str(code).unwrap_or_else(|| {
        // Create new currency with precision 0 (whole units for equity perps)
        let currency = Currency::new(code, 0, 0, code, CurrencyType::Crypto);
        if let Err(e) = Currency::register(currency, false) {
            log::warn!("Failed to register currency '{code}': {e}");
        }
        currency
    })
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

    let ts_event = ax_timestamp_s_to_unix_nanos(candle.ts)?;

    let bar_spec = candle_width_to_bar_spec(candle.width);
    let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::External);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("Failed to construct Bar from Ax candle")
}

/// Parses an Ax funding rate into a Nautilus [`FundingRateUpdate`].
///
/// # Errors
///
/// Returns an error if the timestamp is invalid.
pub fn parse_funding_rate(
    ax_rate: &AxFundingRate,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<FundingRateUpdate> {
    Ok(FundingRateUpdate::new(
        instrument_id,
        ax_rate.funding_rate,
        None, // AX doesn't provide next funding time
        ax_timestamp_ns_to_unix_nanos(ax_rate.timestamp_ns)?,
        ts_init,
    ))
}

/// Parses an Ax perpetual futures instrument into a Nautilus [`PerpetualContract`].
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

    let symbol_prefix = raw_symbol_str
        .split('-')
        .next()
        .context("Failed to extract symbol prefix")?;

    let underlying = Ustr::from(symbol_prefix);

    // Derive base code by stripping quote currency suffix if present
    // e.g. JPYUSD-PERP → base=JPY, BTC-PERP → base=BTC
    let quote_code = definition.quote_currency.as_str();
    let base_code = if symbol_prefix.ends_with(quote_code) && symbol_prefix.len() > quote_code.len()
    {
        &symbol_prefix[..symbol_prefix.len() - quote_code.len()]
    } else {
        symbol_prefix
    };

    let asset_class = match definition.category {
        Some(category) => AssetClass::from(category),
        None => match Currency::try_from_str(base_code) {
            Some(currency) => match currency.currency_type {
                CurrencyType::Fiat => AssetClass::FX,
                CurrencyType::Crypto => AssetClass::Cryptocurrency,
                CurrencyType::CommodityBacked => AssetClass::Commodity,
            },
            None => AssetClass::Alternative,
        },
    };

    // Only resolve base currency for FX/crypto where the base code is a currency
    let base_currency = match asset_class {
        AssetClass::FX | AssetClass::Cryptocurrency => Some(get_currency(base_code)),
        _ => None,
    };

    let quote_currency = get_currency(quote_code);
    let settlement_currency = get_currency(definition.funding_settlement_currency.as_str());

    let price_increment = decimal_to_price(definition.tick_size, "tick_size")?;
    let size_increment = decimal_to_quantity(definition.minimum_order_size, "minimum_order_size")?;

    let lot_size = Some(size_increment);
    let min_quantity = Some(size_increment);

    let margin_init = definition.initial_margin_pct;
    let margin_maint = definition.maintenance_margin_pct;

    let mut info = Params::new();

    if let Some(ref desc) = definition.description {
        info.insert("description".to_string(), json!(desc));
    }

    if let Some(ref s) = definition.contract_size {
        info.insert("contract_size".to_string(), json!(s));
    }

    if let Some(ref s) = definition.contract_mark_price {
        info.insert("contract_mark_price".to_string(), json!(s));
    }

    if let Some(ref s) = definition.price_quotation {
        info.insert("price_quotation".to_string(), json!(s));
    }

    if let Some(ref s) = definition.underlying_benchmark_price {
        info.insert("underlying_benchmark_price".to_string(), json!(s));
    }

    if let Some(ref s) = definition.price_bands {
        info.insert("price_bands".to_string(), json!(s));
    }

    if let Some(v) = definition.funding_rate_cap_upper_pct {
        info.insert(
            "funding_rate_cap_upper_pct".to_string(),
            json!(v.to_string()),
        );
    }

    if let Some(v) = definition.funding_rate_cap_lower_pct {
        info.insert(
            "funding_rate_cap_lower_pct".to_string(),
            json!(v.to_string()),
        );
    }

    if let Some(v) = definition.price_band_upper_deviation_pct {
        info.insert(
            "price_band_upper_deviation_pct".to_string(),
            json!(v.to_string()),
        );
    }

    if let Some(v) = definition.price_band_lower_deviation_pct {
        info.insert(
            "price_band_lower_deviation_pct".to_string(),
            json!(v.to_string()),
        );
    }

    let instrument = PerpetualContract::new(
        instrument_id,
        raw_symbol,
        underlying,
        asset_class,
        base_currency,
        quote_currency,
        settlement_currency,
        false,
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
        Some(info),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::PerpetualContract(instrument))
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
    let mut balances = Vec::with_capacity(response.balances.len());

    for balance in &response.balances {
        let symbol_str = balance.symbol.as_str().trim();
        if symbol_str.is_empty() {
            log::debug!("Skipping balance with empty symbol");
            continue;
        }

        let currency = get_currency(symbol_str);

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
/// The `cid_resolver` parameter is an optional function that resolves a `cid` (u64)
/// to a `ClientOrderId`. This is needed because orders submitted via WebSocket use
/// a hashed `cid` for correlation rather than storing the full `ClientOrderId` in the tag.
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity fields cannot be parsed.
/// - Timestamp conversion fails.
pub fn parse_order_status_report<F>(
    order: &AxOpenOrder,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
    cid_resolver: Option<F>,
) -> anyhow::Result<OrderStatusReport>
where
    F: Fn(u64) -> Option<ClientOrderId>,
{
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
    let ts_event = ax_timestamp_s_to_unix_nanos(order.ts)?;

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

    if let Some(cid) = order.cid {
        let client_order_id = cid_resolver
            .as_ref()
            .and_then(|resolver| resolver(cid))
            .unwrap_or_else(|| cid_to_client_order_id(cid));
        report = report.with_client_order_id(client_order_id);
    }

    report = report.with_price(price);

    // We don't set avg_px here since the order endpoint only provides the
    // limit price, not actual fill prices. True average would need to be
    // calculated from fill reports.

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

    // Use explicit side field from fill
    let order_side: OrderSide = fill.side.into();

    let last_px = decimal_to_price_dp(fill.price, instrument.price_precision(), "fill.price")?;
    let last_qty = Quantity::new(fill.quantity as f64, instrument.size_precision());

    let currency = Currency::USD();
    let commission = Money::from_decimal(fill.fee, currency)
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

    // Determine position side and quantity from signed_quantity sign
    let (position_side, quantity) = if position.signed_quantity > 0 {
        (
            PositionSideSpecified::Long,
            Quantity::new(position.signed_quantity as f64, instrument.size_precision()),
        )
    } else if position.signed_quantity < 0 {
        (
            PositionSideSpecified::Short,
            Quantity::new(
                position.signed_quantity.unsigned_abs() as f64,
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
    // Both signed_notional and signed_quantity are negative for shorts
    let avg_px_open = if position.signed_quantity != 0 {
        let qty_dec = Decimal::from(position.signed_quantity.abs());
        Some(position.signed_notional.abs() / qty_dec)
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

/// Parses an Ax REST trade into a Nautilus [`TradeTick`].
///
/// # Errors
///
/// Returns an error if any field cannot be parsed.
pub fn parse_trade_tick(
    trade: &AxRestTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = decimal_to_price_dp(trade.p, instrument.price_precision(), "trade.p")?;
    let size = Quantity::new(trade.q as f64, instrument.size_precision());
    let aggressor_side: AggressorSide = trade.d.into();

    // Combine seconds + nanoseconds into full timestamp
    let ts_event = UnixNanos::from(trade.ts as u64 * 1_000_000_000 + trade.tn as u64);

    // Use nanosecond timestamp as trade ID (unique per trade)
    let mut buf = itoa::Buffer::new();
    let trade_id =
        TradeId::new_checked(buf.format(ts_event.as_u64())).context("Failed to create TradeId")?;

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("Failed to construct TradeTick from Ax REST trade")
}

#[cfg(test)]
mod tests {
    use nautilus_core::nanos::UnixNanos;
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::{AxCategory, AxInstrumentState},
        http::models::{AxFundingRatesResponse, AxInstrumentsResponse},
    };

    fn create_eurusd_instrument() -> AxInstrument {
        AxInstrument {
            symbol: Ustr::from("EURUSD-PERP"),
            state: AxInstrumentState::Open,
            multiplier: dec!(1),
            minimum_order_size: dec!(100),
            tick_size: dec!(0.0001),
            quote_currency: Ustr::from("USD"),
            funding_settlement_currency: Ustr::from("USD"),
            category: Some(AxCategory::Fx),
            maintenance_margin_pct: dec!(4.0),
            initial_margin_pct: dec!(8.0),
            contract_mark_price: Some("Average price on AX at London 4pm".to_string()),
            contract_size: Some("1 Euro per contract".to_string()),
            description: Some("Euro / US Dollar FX Perpetual Future".to_string()),
            funding_calendar_schedule: None,
            funding_frequency: None,
            funding_rate_cap_lower_pct: Some(dec!(-1.0)),
            funding_rate_cap_upper_pct: Some(dec!(1.0)),
            price_band_lower_deviation_pct: Some(dec!(10)),
            price_band_upper_deviation_pct: Some(dec!(10)),
            price_bands: Some("+/- 10% from prior Contract Mark Price".to_string()),
            price_quotation: Some("U.S. dollars per Euro".to_string()),
            underlying_benchmark_price: Some("WMR London 4pm Closing Spot Rate".to_string()),
        }
    }

    fn create_nvda_instrument() -> AxInstrument {
        AxInstrument {
            symbol: Ustr::from("NVDA-PERP"),
            state: AxInstrumentState::Open,
            multiplier: dec!(1),
            minimum_order_size: dec!(1),
            tick_size: dec!(0.01),
            quote_currency: Ustr::from("USD"),
            funding_settlement_currency: Ustr::from("USD"),
            category: Some(AxCategory::Equities),
            maintenance_margin_pct: dec!(10),
            initial_margin_pct: dec!(20),
            contract_mark_price: Some(
                "Average price on ArchitectX at 4pm New York Time".to_string(),
            ),
            contract_size: Some("1 share per contract".to_string()),
            description: Some("NVIDIA Corp US Equity Perpetual Future".to_string()),
            funding_calendar_schedule: None,
            funding_frequency: None,
            funding_rate_cap_lower_pct: Some(dec!(-1)),
            funding_rate_cap_upper_pct: Some(dec!(1)),
            price_band_lower_deviation_pct: Some(dec!(10)),
            price_band_upper_deviation_pct: Some(dec!(10)),
            price_bands: Some("+/- 10% from prior Contract Mark Price".to_string()),
            price_quotation: Some("U.S. dollars per share".to_string()),
            underlying_benchmark_price: Some("Nasdaq Official Closing Price".to_string()),
        }
    }

    fn create_xau_instrument() -> AxInstrument {
        AxInstrument {
            symbol: Ustr::from("XAU-PERP"),
            state: AxInstrumentState::Open,
            multiplier: dec!(1),
            minimum_order_size: dec!(1),
            tick_size: dec!(0.1),
            quote_currency: Ustr::from("USD"),
            funding_settlement_currency: Ustr::from("USD"),
            category: Some(AxCategory::Metals),
            maintenance_margin_pct: dec!(5),
            initial_margin_pct: dec!(10),
            contract_mark_price: Some("Average price on ArchitectX at London 4pm".to_string()),
            contract_size: Some("1 ounce per contract".to_string()),
            description: Some("Gold Metals Perpetual Future".to_string()),
            funding_calendar_schedule: None,
            funding_frequency: None,
            funding_rate_cap_lower_pct: Some(dec!(-1)),
            funding_rate_cap_upper_pct: Some(dec!(1)),
            price_band_lower_deviation_pct: Some(dec!(10)),
            price_band_upper_deviation_pct: Some(dec!(10)),
            price_bands: Some("+/- 10% from prior Contract Mark Price".to_string()),
            price_quotation: Some("U.S. dollars per ounce".to_string()),
            underlying_benchmark_price: Some("XAU WMR Metals Daily Closing Rate".to_string()),
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
    fn test_get_currency_known() {
        let currency = get_currency("USD");
        assert_eq!(currency.code, Ustr::from("USD"));
        assert_eq!(currency.precision, 2);
    }

    #[rstest]
    fn test_get_currency_unknown_creates_new() {
        let currency = get_currency("NVDA");
        assert_eq!(currency.code, Ustr::from("NVDA"));
        assert_eq!(currency.precision, 0);
    }

    #[rstest]
    fn test_parse_fx_instrument() {
        let definition = create_eurusd_instrument();
        let maker_fee = Decimal::new(2, 5);
        let taker_fee = Decimal::new(2, 5);
        let ts_now = UnixNanos::default();

        let result = parse_perp_instrument(&definition, maker_fee, taker_fee, ts_now, ts_now);
        assert!(result.is_ok());

        let instrument = result.unwrap();
        match instrument {
            InstrumentAny::PerpetualContract(perp) => {
                assert_eq!(perp.id.symbol.as_str(), "EURUSD-PERP");
                assert_eq!(perp.id.venue, *AX_VENUE);
                assert_eq!(perp.underlying.as_str(), "EURUSD");
                assert_eq!(perp.asset_class, AssetClass::FX);
                assert_eq!(perp.base_currency.unwrap().code.as_str(), "EUR");
                assert_eq!(perp.quote_currency.code.as_str(), "USD");
                assert_eq!(perp.settlement_currency.code.as_str(), "USD");
                assert_eq!(perp.price_precision, 4);
                assert!(!perp.is_inverse);
            }
            _ => panic!("Expected PerpetualContract instrument"),
        }
    }

    #[rstest]
    fn test_parse_equity_instrument() {
        let definition = create_nvda_instrument();
        let maker_fee = Decimal::new(2, 5);
        let taker_fee = Decimal::new(2, 5);
        let ts_now = UnixNanos::default();

        let result = parse_perp_instrument(&definition, maker_fee, taker_fee, ts_now, ts_now);
        assert!(result.is_ok());

        let instrument = result.unwrap();
        match instrument {
            InstrumentAny::PerpetualContract(perp) => {
                assert_eq!(perp.id.symbol.as_str(), "NVDA-PERP");
                assert_eq!(perp.id.venue, *AX_VENUE);
                assert_eq!(perp.underlying.as_str(), "NVDA");
                assert_eq!(perp.asset_class, AssetClass::Equity);
                assert_eq!(perp.quote_currency.code.as_str(), "USD");
                assert_eq!(perp.settlement_currency.code.as_str(), "USD");
                assert_eq!(perp.price_precision, 2);
                assert!(!perp.is_inverse);
            }
            _ => panic!("Expected PerpetualContract instrument"),
        }
    }

    #[rstest]
    fn test_parse_metals_instrument() {
        let definition = create_xau_instrument();
        let ts_now = UnixNanos::default();

        let result =
            parse_perp_instrument(&definition, Decimal::ZERO, Decimal::ZERO, ts_now, ts_now);
        let instrument = result.unwrap();
        match instrument {
            InstrumentAny::PerpetualContract(perp) => {
                assert_eq!(perp.id.symbol.as_str(), "XAU-PERP");
                assert_eq!(perp.underlying.as_str(), "XAU");
                assert_eq!(perp.asset_class, AssetClass::Commodity);
                assert!(perp.base_currency.is_none());
                assert_eq!(perp.quote_currency.code.as_str(), "USD");
                assert_eq!(perp.price_precision, 1);
            }
            _ => panic!("Expected PerpetualContract instrument"),
        }
    }

    #[rstest]
    fn test_parse_settlement_differs_from_quote() {
        let mut definition = create_eurusd_instrument();
        definition.funding_settlement_currency = Ustr::from("EUR");
        let ts_now = UnixNanos::default();

        let result =
            parse_perp_instrument(&definition, Decimal::ZERO, Decimal::ZERO, ts_now, ts_now);
        let instrument = result.unwrap();
        match instrument {
            InstrumentAny::PerpetualContract(perp) => {
                assert_eq!(perp.quote_currency.code.as_str(), "USD");
                assert_eq!(perp.settlement_currency.code.as_str(), "EUR");
            }
            _ => panic!("Expected PerpetualContract instrument"),
        }
    }

    #[rstest]
    fn test_parse_unknown_category_falls_back_to_alternative() {
        let mut definition = create_eurusd_instrument();
        definition.category = Some(AxCategory::Unknown);
        let ts_now = UnixNanos::default();

        let result =
            parse_perp_instrument(&definition, Decimal::ZERO, Decimal::ZERO, ts_now, ts_now);
        let instrument = result.unwrap();
        match instrument {
            InstrumentAny::PerpetualContract(perp) => {
                assert_eq!(perp.asset_class, AssetClass::Alternative);
            }
            _ => panic!("Expected PerpetualContract instrument"),
        }
    }

    #[rstest]
    fn test_deserialize_instruments_from_test_data() {
        let test_data = include_str!("../../test_data/http_get_instruments.json");
        let response: AxInstrumentsResponse =
            serde_json::from_str(test_data).expect("Failed to deserialize test data");

        assert_eq!(response.instruments.len(), 3);

        let eurusd = &response.instruments[0];
        assert_eq!(eurusd.symbol.as_str(), "EURUSD-PERP");
        assert_eq!(eurusd.category, Some(AxCategory::Fx));
        assert_eq!(eurusd.tick_size, dec!(0.0001));
        assert_eq!(eurusd.minimum_order_size, dec!(100));

        let xau = &response.instruments[1];
        assert_eq!(xau.symbol.as_str(), "XAU-PERP");
        assert_eq!(xau.category, Some(AxCategory::Metals));

        let nvda = &response.instruments[2];
        assert_eq!(nvda.symbol.as_str(), "NVDA-PERP");
        assert_eq!(nvda.category, Some(AxCategory::Equities));
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

        assert_eq!(open_instruments.len(), 3);

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

    #[rstest]
    fn test_deserialize_and_parse_funding_rates() {
        let test_data = include_str!("../../test_data/http_get_funding_rates.json");
        let response: AxFundingRatesResponse =
            serde_json::from_str(test_data).expect("Failed to deserialize test data");

        assert_eq!(response.funding_rates.len(), 2);
        assert_eq!(response.funding_rates[0].symbol.as_str(), "JPYUSD-PERP");
        assert_eq!(response.funding_rates[0].funding_rate, dec!(0.001234560000));

        let instrument_id = InstrumentId::new(Symbol::new("JPYUSD-PERP"), *AX_VENUE);
        let ts_init = UnixNanos::from(1_000_000_000u64);

        let update =
            parse_funding_rate(&response.funding_rates[1], instrument_id, ts_init).unwrap();

        assert_eq!(update.instrument_id, instrument_id);
        assert_eq!(update.rate, dec!(0.003558290026));
        assert_eq!(update.next_funding_ns, None);
        assert_eq!(update.ts_event, UnixNanos::from(1770393600000000000u64));
        assert_eq!(update.ts_init, ts_init);
    }
}
