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
use nautilus_core::{Params, UUID4, datetime::datetime_to_unix_nanos, nanos::UnixNanos};
use nautilus_model::{
    data::{Bar, BarSpecification, BarType, FundingRateUpdate, TradeTick},
    enums::{
        AccountType, AggregationSource, AggressorSide, AssetClass, BarAggregation, CurrencyType,
        LiquiditySide, OrderSide, OrderType, PositionSideSpecified, PriceType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    instruments::{FuturesContract, Instrument, PerpetualContract, any::InstrumentAny},
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
    parse::{
        ax_timestamp_ns_to_unix_nanos, ax_timestamp_s_to_unix_nanos,
        ax_timestamp_stn_to_unix_nanos, cid_to_client_order_id,
    },
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
        None,
        None, // AX doesn't provide next funding time
        ax_timestamp_ns_to_unix_nanos(ax_rate.timestamp_ns)?,
        ts_init,
    ))
}

/// Parses an Ax instrument into a Nautilus perpetual or dated futures contract.
///
/// # Errors
///
/// Returns an error if any required field cannot be parsed or is invalid.
pub fn parse_instrument(
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

    let underlying = match definition.product {
        Some(product) => {
            let trimmed = product.as_str().trim();
            anyhow::ensure!(
                !trimmed.is_empty() && trimmed == product.as_str(),
                "AX instrument product must be non-empty without surrounding whitespace, was '{product}'"
            );
            product
        }
        None => Ustr::from(symbol_prefix),
    };

    // Derive base code by stripping quote currency suffix if present
    // e.g. JPYUSD-PERP → base=JPY, BTC-PERP → base=BTC
    let quote_code = definition.quote_currency.as_str();
    let base_code = if symbol_prefix.ends_with(quote_code) && symbol_prefix.len() > quote_code.len()
    {
        &symbol_prefix[..symbol_prefix.len() - quote_code.len()]
    } else {
        symbol_prefix
    };

    let asset_class = AssetClass::from(definition.category);

    // Only resolve base currency for FX/crypto where the base code is a currency
    let base_currency = match asset_class {
        AssetClass::FX | AssetClass::Cryptocurrency => Some(get_currency(base_code)),
        _ => None,
    };

    let quote_currency = get_currency(quote_code);
    let settlement_currency = get_currency(definition.funding_settlement_currency.as_str());

    let price_increment = decimal_to_price(definition.tick_size, "tick_size")?;
    anyhow::ensure!(
        definition.minimum_order_size > Decimal::ZERO
            && definition.minimum_order_size.fract().is_zero(),
        "AX minimum_order_size must be a positive whole number, was {}",
        definition.minimum_order_size
    );
    let size_increment = decimal_to_quantity(Decimal::ONE, "size_increment")?;
    let lot_size = Some(size_increment);
    let min_quantity = Some(decimal_to_quantity(
        definition.minimum_order_size.normalize(),
        "minimum_order_size",
    )?);

    let (margin_init, margin_maint) = parse_margin_rates(
        definition.initial_margin_pct,
        definition.maintenance_margin_pct,
    )?;

    let mut info = Params::new();

    if let Some(ref desc) = definition.description {
        info.insert("description".to_string(), json!(desc));
    }

    if let Some(product) = definition.product {
        info.insert("product".to_string(), json!(product.as_str()));
    }

    info.insert(
        "initial_margin_pct".to_string(),
        json!(definition.initial_margin_pct.to_string()),
    );
    info.insert(
        "maintenance_margin_pct".to_string(),
        json!(definition.maintenance_margin_pct.to_string()),
    );
    info.insert(
        "quantity_increment_source".to_string(),
        json!("integer_contract_wire_quantity"),
    );

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

    if let Some(expiration) = definition.expiration {
        anyhow::ensure!(
            definition.quote_currency == definition.funding_settlement_currency,
            "AX dated contract {} has different quote and settlement currencies: {} and {}",
            definition.symbol,
            definition.quote_currency,
            definition.funding_settlement_currency
        );
        let expiration_ns = datetime_to_unix_nanos(Some(expiration))
            .context("Failed to convert AX contract expiration to Unix nanoseconds")?;
        let multiplier = decimal_to_quantity(definition.multiplier, "multiplier")?;
        info.insert("expiration".to_string(), json!(expiration.to_rfc3339()));
        info.insert(
            "activation_source".to_string(),
            json!("unavailable_from_ax"),
        );

        let instrument = FuturesContract::new_checked(
            instrument_id,
            raw_symbol,
            asset_class,
            None,
            underlying,
            UnixNanos::default(),
            expiration_ns,
            quote_currency,
            price_increment.precision,
            price_increment,
            multiplier,
            size_increment,
            None,
            min_quantity,
            None,
            None,
            Some(margin_init),
            Some(margin_maint),
            Some(maker_fee),
            Some(taker_fee),
            None,
            Some(info),
            ts_event,
            ts_init,
        )
        .context("Failed to construct AX dated futures contract")?;

        return Ok(InstrumentAny::FuturesContract(instrument));
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
        None,
        Some(info),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::PerpetualContract(instrument))
}

fn parse_margin_rates(
    initial_margin_pct: Decimal,
    maintenance_margin_pct: Decimal,
) -> anyhow::Result<(Decimal, Decimal)> {
    anyhow::ensure!(
        initial_margin_pct > Decimal::ZERO,
        "AX initial_margin_pct must be positive, was {initial_margin_pct}"
    );
    anyhow::ensure!(
        maintenance_margin_pct > Decimal::ZERO,
        "AX maintenance_margin_pct must be positive, was {maintenance_margin_pct}"
    );
    anyhow::ensure!(
        maintenance_margin_pct <= initial_margin_pct,
        "AX maintenance_margin_pct {maintenance_margin_pct} exceeds initial_margin_pct {initial_margin_pct}"
    );

    Ok((
        margin_percent_to_rate(initial_margin_pct, "initial_margin_pct")?,
        margin_percent_to_rate(maintenance_margin_pct, "maintenance_margin_pct")?,
    ))
}

fn margin_percent_to_rate(value: Decimal, field: &str) -> anyhow::Result<Decimal> {
    let normalized = value.normalize();
    let scale = normalized.scale();
    anyhow::ensure!(
        scale <= 26,
        "AX {field} scale must not exceed 26 for exact percent conversion, was {scale}"
    );
    Decimal::try_from_i128_with_scale(normalized.mantissa(), scale + 2)
        .with_context(|| format!("Failed to convert AX {field} percentage to a rate"))
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

        // The /balances endpoint does not include margin data, so locked
        // is always zero here. The /risk-snapshot endpoint provides
        // initial_margin_required_total which could be used, but that
        // requires an additional HTTP call on every account state refresh.
        let balance =
            AccountBalance::from_total_and_locked(balance.amount, Decimal::ZERO, currency)
                .with_context(|| format!("Failed to convert balance for {symbol_str}"))?;
        balances.push(balance);
    }

    if balances.is_empty() {
        let zero_currency = Currency::USD();
        let zero_money = Money::zero(zero_currency);
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
    cid_resolver: Option<&F>,
) -> anyhow::Result<OrderStatusReport>
where
    F: Fn(u64) -> Option<ClientOrderId>,
{
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&order.oid);
    let order_side = order.d.into();
    let order_status = order.o.into();
    let time_in_force = order.tif.into();

    // The current AX wire shape maps to a Nautilus limit order.
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
/// AX may omit an order ID for block trades and final settlement fills. Those
/// records receive a deterministic reconciliation ID derived from the trade ID.
/// The special-fill classifications are optional for regular fills with an
/// order ID.
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity fields cannot be parsed.
/// - Fee parsing fails.
/// - Fill classification is inconsistent.
/// - A fill is neither explicitly special nor linked to a valid order ID.
pub fn parse_fill_report(
    fill: &AxFill,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();

    let trade_id = TradeId::new_checked(&fill.trade_id).context("Invalid trade_id in Ax fill")?;
    let is_block_trade = fill.is_block_trade;
    let is_final_settlement = fill.is_final_settlement;
    anyhow::ensure!(
        !(is_final_settlement == Some(true) && is_block_trade == Some(false)),
        "AX final-settlement fill must also be classified as a block trade"
    );

    let is_special_fill = is_block_trade == Some(true) || is_final_settlement == Some(true);
    let venue_order_id = if is_special_fill {
        VenueOrderId::new_checked(format!("AX-FILL-{}", fill.trade_id))
            .context("Invalid synthetic venue order ID for AX fill")?
    } else {
        let order_id = fill
            .order_id
            .as_deref()
            .context("AX fill is missing order_id and explicit special-fill classification")?;
        anyhow::ensure!(
            !order_id.is_empty(),
            "AX regular fill has an empty order_id"
        );
        VenueOrderId::new_checked(order_id).context("Invalid order_id in AX fill")?
    };

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

    let ts_event = match fill.timestamp.timestamp_nanos_opt() {
        Some(nanos) => UnixNanos::from(nanos.unsigned_abs()),
        None => {
            log::warn!(
                "Timestamp overflow for fill {} (timestamp={}), defaulting to 0",
                fill.trade_id,
                fill.timestamp
            );
            UnixNanos::from(0u64)
        }
    };

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
            Quantity::zero(instrument.size_precision()),
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

    let ts_last = match position.timestamp.timestamp_nanos_opt() {
        Some(nanos) => UnixNanos::from(nanos.unsigned_abs()),
        None => {
            log::warn!(
                "Timestamp overflow for position {} (timestamp={}), defaulting to 0",
                position.symbol,
                position.timestamp
            );
            UnixNanos::from(0u64)
        }
    };

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

    let ts_event = ax_timestamp_stn_to_unix_nanos(trade.ts, trade.tn)?;

    // AX publishes no trade identifier, and `tn` is the nanosecond component of the timestamp
    // rather than a sequence number, so the composed timestamp is the only identity available.
    // It is not unique: a sweep across several levels reports multiple prints at one timestamp.
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
    use chrono::{DateTime, Utc};
    use nautilus_core::nanos::UnixNanos;
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::{AxCategory, AxInstrumentState, AxOrderSide, AxOrderStatus, AxTimeInForce},
        http::models::{AxFundingRatesResponse, AxInstrumentsResponse, AxOpenOrder},
    };

    fn create_eurusd_instrument() -> AxInstrument {
        AxInstrument {
            symbol: Ustr::from("EURUSD-PERP"),
            product: Some(Ustr::from("EURUSD")),
            state: AxInstrumentState::Open,
            multiplier: dec!(1),
            minimum_order_size: dec!(100),
            tick_size: dec!(0.0001),
            quote_currency: Ustr::from("USD"),
            funding_settlement_currency: Ustr::from("USD"),
            category: AxCategory::Fx,
            maintenance_margin_pct: dec!(4.0),
            initial_margin_pct: dec!(8.0),
            contract_mark_price: Some("Average price on AX at London 4pm".to_string()),
            contract_size: Some("1 Euro per contract".to_string()),
            description: Some("Euro / US Dollar FX Perpetual Future".to_string()),
            expiration: None,
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
            product: Some(Ustr::from("NVDA")),
            state: AxInstrumentState::Open,
            multiplier: dec!(1),
            minimum_order_size: dec!(1),
            tick_size: dec!(0.01),
            quote_currency: Ustr::from("USD"),
            funding_settlement_currency: Ustr::from("USD"),
            category: AxCategory::Equities,
            maintenance_margin_pct: dec!(10),
            initial_margin_pct: dec!(20),
            contract_mark_price: Some(
                "Average price on ArchitectX at 4pm New York Time".to_string(),
            ),
            contract_size: Some("1 share per contract".to_string()),
            description: Some("NVIDIA Corp US Equity Perpetual Future".to_string()),
            expiration: None,
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
            product: Some(Ustr::from("XAU")),
            state: AxInstrumentState::Open,
            multiplier: dec!(1),
            minimum_order_size: dec!(1),
            tick_size: dec!(0.1),
            quote_currency: Ustr::from("USD"),
            funding_settlement_currency: Ustr::from("USD"),
            category: AxCategory::Metals,
            maintenance_margin_pct: dec!(5),
            initial_margin_pct: dec!(10),
            contract_mark_price: Some("Average price on ArchitectX at London 4pm".to_string()),
            contract_size: Some("1 ounce per contract".to_string()),
            description: Some("Gold Metals Perpetual Future".to_string()),
            expiration: None,
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

    fn create_fill() -> AxFill {
        AxFill {
            trade_id: "T-01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            order_id: Some("O-01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()),
            fee: dec!(0.10),
            is_taker: true,
            is_block_trade: Some(false),
            is_final_settlement: Some(false),
            price: dec!(1.0845),
            quantity: 100,
            side: AxOrderSide::Buy,
            symbol: Ustr::from("EURUSD-PERP"),
            timestamp: DateTime::parse_from_rfc3339("2026-07-17T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            account_id: Ustr::from("account-1"),
            realized_pnl: None,
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
    fn test_parse_order_status_report_uses_cid_resolver() {
        let instrument = parse_instrument(
            &create_eurusd_instrument(),
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let order = AxOpenOrder {
            tn: 0,
            ts: 1_609_459_200,
            d: AxOrderSide::Buy,
            o: AxOrderStatus::Accepted,
            oid: "O-NEW".to_string(),
            p: dec!(1.0845),
            q: 100,
            rq: 100,
            s: Ustr::from("EURUSD-PERP"),
            tif: AxTimeInForce::Gtc,
            u: "user".to_string(),
            xq: 0,
            cid: Some(42),
            tag: None,
            po: true,
        };
        let expected_client_order_id = ClientOrderId::from("O-PERSISTED");
        let resolver = |cid| (cid == 42).then_some(expected_client_order_id);

        let report = parse_order_status_report(
            &order,
            AccountId::from("AX-001"),
            &instrument,
            UnixNanos::default(),
            Some(&resolver),
        )
        .unwrap();

        assert_eq!(report.client_order_id, Some(expected_client_order_id));
        assert_eq!(report.venue_order_id, VenueOrderId::from("O-NEW"));
    }

    #[rstest]
    #[case(Some(false), Some(false))]
    #[case(None, Some(false))]
    #[case(Some(false), None)]
    #[case(None, None)]
    fn test_parse_fill_report_uses_real_order_id_for_regular_fill(
        #[case] is_block_trade: Option<bool>,
        #[case] is_final_settlement: Option<bool>,
    ) {
        let instrument = parse_instrument(
            &create_eurusd_instrument(),
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let mut fill = create_fill();
        fill.is_block_trade = is_block_trade;
        fill.is_final_settlement = is_final_settlement;

        let report = parse_fill_report(
            &fill,
            AccountId::from("AX-001"),
            &instrument,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(
            report.venue_order_id.as_str(),
            "O-01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
    }

    #[rstest]
    #[case(None)]
    #[case(Some("O-01ARZ3NDEKTSV4RRFFQ69G5FAV"))]
    fn test_parse_fill_report_uses_stable_surrogate_for_block_fill(#[case] order_id: Option<&str>) {
        let instrument = parse_instrument(
            &create_eurusd_instrument(),
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let mut fill = create_fill();
        fill.order_id = order_id.map(str::to_string);
        fill.is_block_trade = Some(true);

        let report = parse_fill_report(
            &fill,
            AccountId::from("AX-001"),
            &instrument,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(
            report.venue_order_id.as_str(),
            "AX-FILL-T-01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
    }

    #[rstest]
    fn test_parse_fill_report_uses_stable_surrogate_for_final_settlement() {
        let instrument = parse_instrument(
            &create_eurusd_instrument(),
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let mut fill = create_fill();
        fill.order_id = None;
        fill.is_block_trade = Some(true);
        fill.is_final_settlement = Some(true);

        let report = parse_fill_report(
            &fill,
            AccountId::from("AX-001"),
            &instrument,
            UnixNanos::default(),
        )
        .unwrap();

        assert_eq!(
            report.venue_order_id.as_str(),
            "AX-FILL-T-01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
    }

    #[rstest]
    fn test_parse_fill_report_rejects_final_settlement_without_block_classification() {
        let instrument = parse_instrument(
            &create_eurusd_instrument(),
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let mut fill = create_fill();
        fill.is_block_trade = Some(false);
        fill.is_final_settlement = Some(true);

        let error = parse_fill_report(
            &fill,
            AccountId::from("AX-001"),
            &instrument,
            UnixNanos::default(),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "AX final-settlement fill must also be classified as a block trade"
        );
    }

    #[rstest]
    fn test_parse_fill_report_rejects_missing_identity() {
        let instrument = parse_instrument(
            &create_eurusd_instrument(),
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let mut fill = create_fill();
        fill.order_id = None;
        fill.is_block_trade = None;
        fill.is_final_settlement = None;

        let error = parse_fill_report(
            &fill,
            AccountId::from("AX-001"),
            &instrument,
            UnixNanos::default(),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "AX fill is missing order_id and explicit special-fill classification"
        );
    }

    #[rstest]
    fn test_parse_fill_report_rejects_regular_fill_without_order_id() {
        let instrument = parse_instrument(
            &create_eurusd_instrument(),
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let mut fill = create_fill();
        fill.order_id = None;

        let result = parse_fill_report(
            &fill,
            AccountId::from("AX-001"),
            &instrument,
            UnixNanos::default(),
        );

        assert!(result.is_err());
    }

    #[rstest]
    #[case("")]
    #[case(" ")]
    #[case("O-α")]
    fn test_parse_fill_report_rejects_invalid_regular_order_id(#[case] order_id: &str) {
        let instrument = parse_instrument(
            &create_eurusd_instrument(),
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let mut fill = create_fill();
        fill.order_id = Some(order_id.to_string());
        fill.is_block_trade = None;
        fill.is_final_settlement = None;

        let result = parse_fill_report(
            &fill,
            AccountId::from("AX-001"),
            &instrument,
            UnixNanos::default(),
        );

        assert!(result.is_err());
    }

    #[rstest]
    fn test_parse_fx_instrument() {
        let definition = create_eurusd_instrument();
        let maker_fee = Decimal::new(2, 5);
        let taker_fee = Decimal::new(2, 5);
        let ts_now = UnixNanos::default();

        let result = parse_instrument(&definition, maker_fee, taker_fee, ts_now, ts_now);
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
                assert_eq!(perp.size_increment.as_decimal(), Decimal::ONE);
                assert_eq!(perp.lot_size.as_decimal(), Decimal::ONE);
                assert_eq!(perp.min_quantity.unwrap().as_decimal(), dec!(100));
                assert_eq!(perp.margin_init, dec!(0.08));
                assert_eq!(perp.margin_maint, dec!(0.04));
                let info = perp.info.as_ref().unwrap();
                assert_eq!(info["initial_margin_pct"], json!("8.0"));
                assert_eq!(info["maintenance_margin_pct"], json!("4.0"));
                assert_eq!(
                    info["quantity_increment_source"],
                    json!("integer_contract_wire_quantity")
                );
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

        let result = parse_instrument(&definition, maker_fee, taker_fee, ts_now, ts_now);
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

        let result = parse_instrument(&definition, Decimal::ZERO, Decimal::ZERO, ts_now, ts_now);
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
    fn test_parse_current_dated_instruments() {
        let test_data = include_str!("../../test_data/http_get_dated_instruments.json");
        let response: AxInstrumentsResponse = serde_json::from_str(test_data).unwrap();
        let maker_fee = dec!(0.0002);
        let taker_fee = dec!(0.0005);
        let ts_now = UnixNanos::default();

        let instruments = response
            .instruments
            .iter()
            .map(|definition| {
                parse_instrument(definition, maker_fee, taker_fee, ts_now, ts_now).unwrap()
            })
            .collect::<Vec<_>>();

        assert_eq!(instruments.len(), 2);
        for (instrument, expected_symbol, expected_expiration) in [
            (&instruments[0], "XAU-2026-SEP", "2026-09-30T15:00:00Z"),
            (&instruments[1], "XAU-2026-DEC", "2026-12-31T16:00:00Z"),
        ] {
            let InstrumentAny::FuturesContract(future) = instrument else {
                panic!("Expected FuturesContract instrument");
            };
            let expected_expiration_ns = DateTime::parse_from_rfc3339(expected_expiration)
                .unwrap()
                .timestamp_nanos_opt()
                .unwrap() as u64;
            let info = future.info.as_ref().unwrap();

            assert_eq!(future.id.symbol.as_str(), expected_symbol);
            assert_eq!(future.id.venue, *AX_VENUE);
            assert_eq!(future.underlying, Ustr::from("XAU"));
            assert_eq!(future.asset_class, AssetClass::Commodity);
            assert_eq!(future.activation_ns, UnixNanos::default());
            assert_eq!(
                future.expiration_ns,
                UnixNanos::from(expected_expiration_ns)
            );
            assert_eq!(future.currency.code.as_str(), "USD");
            assert_eq!(future.price_increment.as_decimal(), dec!(0.1));
            assert_eq!(future.size_increment.as_decimal(), Decimal::ONE);
            assert_eq!(future.lot_size.as_decimal(), Decimal::ONE);
            assert_eq!(future.min_quantity.unwrap().as_decimal(), Decimal::ONE);
            assert_eq!(future.multiplier.as_decimal(), Decimal::ONE);
            assert_eq!(future.margin_init, dec!(0.125));
            assert_eq!(future.margin_maint, dec!(0.075));
            assert_eq!(future.maker_fee, maker_fee);
            assert_eq!(future.taker_fee, taker_fee);
            assert_eq!(info["product"], json!("XAU"));
            assert_eq!(
                info["expiration"],
                json!(expected_expiration.replace('Z', "+00:00"))
            );
            assert_eq!(info["activation_source"], json!("unavailable_from_ax"));
            assert_eq!(
                info["quantity_increment_source"],
                json!("integer_contract_wire_quantity")
            );
            assert_eq!(info["initial_margin_pct"], json!("12.5"));
            assert_eq!(info["maintenance_margin_pct"], json!("7.5"));
        }
    }

    #[rstest]
    fn test_parse_dated_instrument_keeps_numeric_fields_distinct() {
        let test_data = include_str!("../../test_data/http_get_dated_instruments.json");
        let mut response: AxInstrumentsResponse = serde_json::from_str(test_data).unwrap();
        let definition = &mut response.instruments[0];
        definition.multiplier = dec!(2.5);
        definition.minimum_order_size = dec!(5);

        let instrument = parse_instrument(
            definition,
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let InstrumentAny::FuturesContract(future) = instrument else {
            panic!("Expected FuturesContract instrument");
        };

        assert_eq!(future.size_increment.as_decimal(), Decimal::ONE);
        assert_eq!(future.lot_size.as_decimal(), Decimal::ONE);
        assert_eq!(future.min_quantity.unwrap().as_decimal(), dec!(5));
        assert_eq!(future.multiplier.as_decimal(), dec!(2.5));
    }

    #[rstest]
    fn test_parse_dated_instrument_without_product_uses_symbol_fallback() {
        let test_data = include_str!("../../test_data/http_get_dated_instruments.json");
        let mut response: AxInstrumentsResponse = serde_json::from_str(test_data).unwrap();
        let definition = &mut response.instruments[0];
        definition.product = None;

        let instrument = parse_instrument(
            definition,
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let InstrumentAny::FuturesContract(future) = instrument else {
            panic!("Expected FuturesContract instrument");
        };

        assert_eq!(future.underlying, Ustr::from("XAU"));
    }

    #[rstest]
    fn test_parse_instrument_rejects_blank_product() {
        let mut definition = create_xau_instrument();
        definition.product = Some(Ustr::from(" "));

        let result = parse_instrument(
            &definition,
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(result.is_err());
    }

    #[rstest]
    #[case(Decimal::ZERO)]
    #[case(dec!(-1))]
    #[case(dec!(1.5))]
    fn test_parse_instrument_rejects_invalid_minimum_order_size(
        #[case] minimum_order_size: Decimal,
    ) {
        let mut definition = create_xau_instrument();
        definition.minimum_order_size = minimum_order_size;

        let result = parse_instrument(
            &definition,
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(result.is_err());
    }

    #[rstest]
    fn test_parse_dated_instrument_rejects_currency_mismatch() {
        let test_data = include_str!("../../test_data/http_get_dated_instruments.json");
        let mut response: AxInstrumentsResponse = serde_json::from_str(test_data).unwrap();
        let definition = &mut response.instruments[0];
        definition.funding_settlement_currency = Ustr::from("EUR");

        let result = parse_instrument(
            definition,
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(result.is_err());
    }

    #[rstest]
    fn test_parse_settlement_differs_from_quote() {
        let mut definition = create_eurusd_instrument();
        definition.funding_settlement_currency = Ustr::from("EUR");
        let ts_now = UnixNanos::default();

        let result = parse_instrument(&definition, Decimal::ZERO, Decimal::ZERO, ts_now, ts_now);
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
    fn test_margin_percent_to_rate_preserves_exact_scale() {
        let value = Decimal::from_i128_with_scale(1, 26);
        let result = margin_percent_to_rate(value, "test").unwrap();

        assert_eq!(result.mantissa(), 1);
        assert_eq!(result.scale(), 28);
    }

    #[rstest]
    fn test_margin_percent_to_rate_normalizes_trailing_zero() {
        let value = Decimal::from_i128_with_scale(10, 27);
        let result = margin_percent_to_rate(value, "test").unwrap();

        assert_eq!(result.mantissa(), 1);
        assert_eq!(result.scale(), 28);
    }

    #[rstest]
    fn test_margin_percent_to_rate_rejects_unrepresentable_scale() {
        let value = Decimal::from_i128_with_scale(1, 27);
        let result = margin_percent_to_rate(value, "test");

        assert!(result.is_err());
    }

    #[rstest]
    #[case(Decimal::ZERO, dec!(1))]
    #[case(dec!(-1), dec!(1))]
    #[case(dec!(1), Decimal::ZERO)]
    #[case(dec!(1), dec!(-1))]
    #[case(dec!(4), dec!(8))]
    fn test_parse_margin_rates_rejects_invalid_values(
        #[case] initial_margin_pct: Decimal,
        #[case] maintenance_margin_pct: Decimal,
    ) {
        let result = parse_margin_rates(initial_margin_pct, maintenance_margin_pct);

        assert!(result.is_err());
    }

    #[rstest]
    fn test_parse_unknown_category_falls_back_to_alternative() {
        let mut definition = create_eurusd_instrument();
        definition.category = AxCategory::Unknown;
        let ts_now = UnixNanos::default();

        let result = parse_instrument(&definition, Decimal::ZERO, Decimal::ZERO, ts_now, ts_now);
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
        assert_eq!(eurusd.category, AxCategory::Fx);
        assert_eq!(eurusd.tick_size, dec!(0.0001));
        assert_eq!(eurusd.minimum_order_size, dec!(100));

        let xau = &response.instruments[1];
        assert_eq!(xau.symbol.as_str(), "XAU-PERP");
        assert_eq!(xau.category, AxCategory::Metals);

        let nvda = &response.instruments[2];
        assert_eq!(nvda.symbol.as_str(), "NVDA-PERP");
        assert_eq!(nvda.category, AxCategory::Equities);
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
            let result = parse_instrument(instrument, maker_fee, taker_fee, ts_now, ts_now);
            assert!(
                result.is_ok(),
                "Failed to parse {}: {:?}",
                instrument.symbol,
                result.err()
            );
        }
    }

    fn create_rest_trade() -> AxRestTrade {
        AxRestTrade {
            ts: 1_766_193_240,
            tn: 334_589_144,
            p: dec!(1.1719),
            q: 400,
            s: Ustr::from("EURUSD-PERP"),
            d: AxOrderSide::Buy,
        }
    }

    #[rstest]
    fn test_parse_trade_tick_derives_trade_id_from_composed_timestamp() {
        // AX publishes no trade identifier, and `tn` is the nanosecond component rather than a
        // sequence number, so the identity must come from the full composed timestamp
        let instrument = parse_instrument(
            &create_eurusd_instrument(),
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let trade = create_rest_trade();

        let tick = parse_trade_tick(&trade, &instrument, UnixNanos::from(7u64)).unwrap();

        assert_eq!(tick.instrument_id, instrument.id());
        assert_eq!(tick.trade_id.to_string(), "1766193240334589144");
        assert_eq!(tick.price, Price::from("1.1719"));
        assert_eq!(tick.size, Quantity::from(400));
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(tick.ts_event, UnixNanos::from(1_766_193_240_334_589_144u64));
        assert_eq!(tick.ts_init, UnixNanos::from(7u64));
    }

    #[rstest]
    fn test_parse_trade_tick_trade_id_collides_within_one_timestamp() {
        // Documents a venue limitation, not a desired property. AX publishes no trade identifier,
        // so a sweep across several levels produces prints sharing `ts` and `tn` that the composed
        // identity cannot separate. Sampling 100 sandbox trades for GBPUSD-PERP yielded 62 distinct
        // IDs. Replacing the derivation must update this test, the comment above
        // `parse_trade_tick`, and the adapter guide together.
        let instrument = parse_instrument(
            &create_eurusd_instrument(),
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let first = create_rest_trade();
        let mut second = create_rest_trade();
        second.p = dec!(1.1720);
        second.q = 100;

        let first_tick = parse_trade_tick(&first, &instrument, UnixNanos::default()).unwrap();
        let second_tick = parse_trade_tick(&second, &instrument, UnixNanos::default()).unwrap();

        assert_ne!(first_tick.price, second_tick.price);
        assert_ne!(first_tick.size, second_tick.size);
        assert_eq!(first_tick.trade_id, second_tick.trade_id);
    }

    #[rstest]
    fn test_parse_trade_tick_rejects_negative_timestamp() {
        let instrument = parse_instrument(
            &create_eurusd_instrument(),
            Decimal::ZERO,
            Decimal::ZERO,
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let mut trade = create_rest_trade();
        trade.ts = -1;

        let error = parse_trade_tick(&trade, &instrument, UnixNanos::default()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "AX timestamp must be non-negative, was -1"
        );
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
