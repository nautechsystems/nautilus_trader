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

//! Conversion helpers that translate Bybit API schemas into Nautilus instruments.

use std::{convert::TryFrom, str::FromStr};

use anyhow::Context;
use nautilus_core::{datetime::NANOSECONDS_IN_MILLISECOND, nanos::UnixNanos};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{
        AccountType, AggressorSide, AssetClass, BarAggregation, CurrencyType, LiquiditySide,
        OptionKind, OrderSide, OrderStatus, OrderType, PositionSideSpecified, TimeInForce,
    },
    events::account::state::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, Venue, VenueOrderId},
    instruments::{
        Instrument, any::InstrumentAny, crypto_future::CryptoFuture,
        crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair,
        option_contract::OptionContract,
    },
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        enums::{BybitContractType, BybitOptionType, BybitProductType},
        symbol::BybitSymbol,
    },
    http::models::{
        BybitExecution, BybitFeeRate, BybitInstrumentInverse, BybitInstrumentLinear,
        BybitInstrumentOption, BybitInstrumentSpot, BybitKline, BybitPosition, BybitTrade,
        BybitWalletBalance,
    },
};

const BYBIT_MINUTE_INTERVALS: &[u64] = &[1, 3, 5, 15, 30, 60, 120, 240, 360, 720];
const BYBIT_HOUR_INTERVALS: &[u64] = &[1, 2, 4, 6, 12];

/// Extracts the raw symbol from a Bybit symbol by removing the product type suffix.
///
/// # Examples
/// ```ignore
/// assert_eq!(extract_raw_symbol("ETHUSDT-LINEAR"), "ETHUSDT");
/// assert_eq!(extract_raw_symbol("BTCUSDT-SPOT"), "BTCUSDT");
/// assert_eq!(extract_raw_symbol("ETHUSDT"), "ETHUSDT"); // No suffix
/// ```
#[must_use]
pub fn extract_raw_symbol(symbol: &str) -> &str {
    symbol.rsplit_once('-').map_or(symbol, |(prefix, _)| prefix)
}

/// Constructs a full Bybit symbol from a raw symbol and product type.
///
/// Returns a `Ustr` for efficient string interning and comparisons.
///
/// # Examples
/// ```ignore
/// let symbol = make_bybit_symbol("ETHUSDT", BybitProductType::Linear);
/// assert_eq!(symbol.as_str(), "ETHUSDT-LINEAR");
/// ```
#[must_use]
pub fn make_bybit_symbol(raw_symbol: &str, product_type: BybitProductType) -> Ustr {
    let suffix = match product_type {
        BybitProductType::Spot => "-SPOT",
        BybitProductType::Linear => "-LINEAR",
        BybitProductType::Inverse => "-INVERSE",
        BybitProductType::Option => "-OPTION",
    };
    Ustr::from(&format!("{raw_symbol}{suffix}"))
}

/// Converts a Nautilus bar aggregation and step to a Bybit kline interval string.
///
/// Bybit supported intervals: 1, 3, 5, 15, 30, 60, 120, 240, 360, 720 (minutes), D, W, M
///
/// # Errors
///
/// Returns an error if the aggregation type or step is not supported by Bybit.
pub fn bar_spec_to_bybit_interval(
    aggregation: BarAggregation,
    step: u64,
) -> anyhow::Result<String> {
    match aggregation {
        BarAggregation::Minute => {
            if !BYBIT_MINUTE_INTERVALS.contains(&step) {
                anyhow::bail!(
                    "Bybit only supports the following minute intervals: {:?}",
                    BYBIT_MINUTE_INTERVALS
                );
            }
            Ok(step.to_string())
        }
        BarAggregation::Hour => {
            if !BYBIT_HOUR_INTERVALS.contains(&step) {
                anyhow::bail!(
                    "Bybit only supports the following hour intervals: {:?}",
                    BYBIT_HOUR_INTERVALS
                );
            }
            Ok((step * 60).to_string())
        }
        BarAggregation::Day => {
            if step != 1 {
                anyhow::bail!("Bybit only supports 1 DAY interval bars");
            }
            Ok("D".to_string())
        }
        BarAggregation::Week => {
            if step != 1 {
                anyhow::bail!("Bybit only supports 1 WEEK interval bars");
            }
            Ok("W".to_string())
        }
        BarAggregation::Month => {
            if step != 1 {
                anyhow::bail!("Bybit only supports 1 MONTH interval bars");
            }
            Ok("M".to_string())
        }
        _ => {
            anyhow::bail!("Bybit does not support {:?} bars", aggregation);
        }
    }
}

fn default_margin() -> Decimal {
    Decimal::new(1, 1)
}

/// Parses a spot instrument definition returned by Bybit into a Nautilus currency pair.
pub fn parse_spot_instrument(
    definition: &BybitInstrumentSpot,
    fee_rate: &BybitFeeRate,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let base_currency = get_currency(definition.base_coin.as_str());
    let quote_currency = get_currency(definition.quote_coin.as_str());

    let symbol = BybitSymbol::new(format!("{}-SPOT", definition.symbol))?;
    let instrument_id = symbol.to_instrument_id();
    let raw_symbol = Symbol::new(symbol.raw_symbol());

    let price_increment = parse_price(&definition.price_filter.tick_size, "priceFilter.tickSize")?;
    let size_increment = parse_quantity(
        &definition.lot_size_filter.base_precision,
        "lotSizeFilter.basePrecision",
    )?;
    let lot_size = Some(size_increment);
    let max_quantity = Some(parse_quantity(
        &definition.lot_size_filter.max_order_qty,
        "lotSizeFilter.maxOrderQty",
    )?);
    let min_quantity = Some(parse_quantity(
        &definition.lot_size_filter.min_order_qty,
        "lotSizeFilter.minOrderQty",
    )?);

    let maker_fee = parse_decimal(&fee_rate.maker_fee_rate, "makerFeeRate")?;
    let taker_fee = parse_decimal(&fee_rate.taker_fee_rate, "takerFeeRate")?;

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None,
        lot_size,
        max_quantity,
        min_quantity,
        None,
        None,
        None,
        None,
        Some(default_margin()),
        Some(default_margin()),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses a linear contract definition (perpetual or dated future) into a Nautilus instrument.
pub fn parse_linear_instrument(
    definition: &BybitInstrumentLinear,
    fee_rate: &BybitFeeRate,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let base_currency = get_currency(definition.base_coin.as_str());
    let quote_currency = get_currency(definition.quote_coin.as_str());
    let settlement_currency = resolve_settlement_currency(
        definition.settle_coin.as_str(),
        base_currency,
        quote_currency,
    )?;

    let symbol = BybitSymbol::new(format!("{}-LINEAR", definition.symbol))?;
    let instrument_id = symbol.to_instrument_id();
    let raw_symbol = Symbol::new(symbol.raw_symbol());

    let price_increment = parse_price(&definition.price_filter.tick_size, "priceFilter.tickSize")?;
    let size_increment = parse_quantity(
        &definition.lot_size_filter.qty_step,
        "lotSizeFilter.qtyStep",
    )?;
    let lot_size = Some(size_increment);
    let max_quantity = Some(parse_quantity(
        &definition.lot_size_filter.max_order_qty,
        "lotSizeFilter.maxOrderQty",
    )?);
    let min_quantity = Some(parse_quantity(
        &definition.lot_size_filter.min_order_qty,
        "lotSizeFilter.minOrderQty",
    )?);
    let max_price = Some(parse_price(
        &definition.price_filter.max_price,
        "priceFilter.maxPrice",
    )?);
    let min_price = Some(parse_price(
        &definition.price_filter.min_price,
        "priceFilter.minPrice",
    )?);

    let maker_fee = parse_decimal(&fee_rate.maker_fee_rate, "makerFeeRate")?;
    let taker_fee = parse_decimal(&fee_rate.taker_fee_rate, "takerFeeRate")?;

    match definition.contract_type {
        BybitContractType::LinearPerpetual => {
            let instrument = CryptoPerpetual::new(
                instrument_id,
                raw_symbol,
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
                max_quantity,
                min_quantity,
                None,
                None,
                max_price,
                min_price,
                Some(default_margin()),
                Some(default_margin()),
                Some(maker_fee),
                Some(taker_fee),
                ts_event,
                ts_init,
            );
            Ok(InstrumentAny::CryptoPerpetual(instrument))
        }
        BybitContractType::LinearFutures => {
            let activation_ns = parse_millis_timestamp(&definition.launch_time, "launchTime")?;
            let expiration_ns = parse_millis_timestamp(&definition.delivery_time, "deliveryTime")?;
            let instrument = CryptoFuture::new(
                instrument_id,
                raw_symbol,
                base_currency,
                quote_currency,
                settlement_currency,
                false,
                activation_ns,
                expiration_ns,
                price_increment.precision,
                size_increment.precision,
                price_increment,
                size_increment,
                None,
                lot_size,
                max_quantity,
                min_quantity,
                None,
                None,
                max_price,
                min_price,
                Some(default_margin()),
                Some(default_margin()),
                Some(maker_fee),
                Some(taker_fee),
                ts_event,
                ts_init,
            );
            Ok(InstrumentAny::CryptoFuture(instrument))
        }
        other => Err(anyhow::anyhow!(
            "unsupported linear contract variant: {other:?}"
        )),
    }
}

/// Parses an inverse contract definition into a Nautilus instrument.
pub fn parse_inverse_instrument(
    definition: &BybitInstrumentInverse,
    fee_rate: &BybitFeeRate,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let base_currency = get_currency(definition.base_coin.as_str());
    let quote_currency = get_currency(definition.quote_coin.as_str());
    let settlement_currency = resolve_settlement_currency(
        definition.settle_coin.as_str(),
        base_currency,
        quote_currency,
    )?;

    let symbol = BybitSymbol::new(format!("{}-INVERSE", definition.symbol))?;
    let instrument_id = symbol.to_instrument_id();
    let raw_symbol = Symbol::new(symbol.raw_symbol());

    let price_increment = parse_price(&definition.price_filter.tick_size, "priceFilter.tickSize")?;
    let size_increment = parse_quantity(
        &definition.lot_size_filter.qty_step,
        "lotSizeFilter.qtyStep",
    )?;
    let lot_size = Some(size_increment);
    let max_quantity = Some(parse_quantity(
        &definition.lot_size_filter.max_order_qty,
        "lotSizeFilter.maxOrderQty",
    )?);
    let min_quantity = Some(parse_quantity(
        &definition.lot_size_filter.min_order_qty,
        "lotSizeFilter.minOrderQty",
    )?);
    let max_price = Some(parse_price(
        &definition.price_filter.max_price,
        "priceFilter.maxPrice",
    )?);
    let min_price = Some(parse_price(
        &definition.price_filter.min_price,
        "priceFilter.minPrice",
    )?);

    let maker_fee = parse_decimal(&fee_rate.maker_fee_rate, "makerFeeRate")?;
    let taker_fee = parse_decimal(&fee_rate.taker_fee_rate, "takerFeeRate")?;

    match definition.contract_type {
        BybitContractType::InversePerpetual => {
            let instrument = CryptoPerpetual::new(
                instrument_id,
                raw_symbol,
                base_currency,
                quote_currency,
                settlement_currency,
                true,
                price_increment.precision,
                size_increment.precision,
                price_increment,
                size_increment,
                None,
                lot_size,
                max_quantity,
                min_quantity,
                None,
                None,
                max_price,
                min_price,
                Some(default_margin()),
                Some(default_margin()),
                Some(maker_fee),
                Some(taker_fee),
                ts_event,
                ts_init,
            );
            Ok(InstrumentAny::CryptoPerpetual(instrument))
        }
        BybitContractType::InverseFutures => {
            let activation_ns = parse_millis_timestamp(&definition.launch_time, "launchTime")?;
            let expiration_ns = parse_millis_timestamp(&definition.delivery_time, "deliveryTime")?;
            let instrument = CryptoFuture::new(
                instrument_id,
                raw_symbol,
                base_currency,
                quote_currency,
                settlement_currency,
                true,
                activation_ns,
                expiration_ns,
                price_increment.precision,
                size_increment.precision,
                price_increment,
                size_increment,
                None,
                lot_size,
                max_quantity,
                min_quantity,
                None,
                None,
                max_price,
                min_price,
                Some(default_margin()),
                Some(default_margin()),
                Some(maker_fee),
                Some(taker_fee),
                ts_event,
                ts_init,
            );
            Ok(InstrumentAny::CryptoFuture(instrument))
        }
        other => Err(anyhow::anyhow!(
            "unsupported inverse contract variant: {other:?}"
        )),
    }
}

/// Parses a Bybit option contract definition into a Nautilus option instrument.
pub fn parse_option_instrument(
    definition: &BybitInstrumentOption,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let quote_currency = get_currency(definition.quote_coin.as_str());

    let symbol = BybitSymbol::new(format!("{}-OPTION", definition.symbol))?;
    let instrument_id = symbol.to_instrument_id();
    let raw_symbol = Symbol::new(symbol.raw_symbol());

    let price_increment = parse_price(&definition.price_filter.tick_size, "priceFilter.tickSize")?;
    let max_price = Some(parse_price(
        &definition.price_filter.max_price,
        "priceFilter.maxPrice",
    )?);
    let min_price = Some(parse_price(
        &definition.price_filter.min_price,
        "priceFilter.minPrice",
    )?);
    let lot_size = parse_quantity(
        &definition.lot_size_filter.qty_step,
        "lotSizeFilter.qtyStep",
    )?;
    let max_quantity = Some(parse_quantity(
        &definition.lot_size_filter.max_order_qty,
        "lotSizeFilter.maxOrderQty",
    )?);
    let min_quantity = Some(parse_quantity(
        &definition.lot_size_filter.min_order_qty,
        "lotSizeFilter.minOrderQty",
    )?);

    let option_kind = match definition.options_type {
        BybitOptionType::Call => OptionKind::Call,
        BybitOptionType::Put => OptionKind::Put,
    };

    let strike_price = extract_strike_from_symbol(&definition.symbol)?;
    let activation_ns = parse_millis_timestamp(&definition.launch_time, "launchTime")?;
    let expiration_ns = parse_millis_timestamp(&definition.delivery_time, "deliveryTime")?;

    let instrument = OptionContract::new(
        instrument_id,
        raw_symbol,
        AssetClass::Cryptocurrency,
        None,
        Ustr::from(definition.base_coin.as_str()),
        option_kind,
        strike_price,
        quote_currency,
        activation_ns,
        expiration_ns,
        price_increment.precision,
        price_increment,
        Quantity::from(1_u32),
        lot_size,
        max_quantity,
        min_quantity,
        max_price,
        min_price,
        Some(Decimal::ZERO),
        Some(Decimal::ZERO),
        Some(Decimal::ZERO),
        Some(Decimal::ZERO),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::OptionContract(instrument))
}

/// Parses a REST trade payload into a [`TradeTick`].
pub fn parse_trade_tick(
    trade: &BybitTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price =
        parse_price_with_precision(&trade.price, instrument.price_precision(), "trade.price")?;
    let size =
        parse_quantity_with_precision(&trade.size, instrument.size_precision(), "trade.size")?;
    let aggressor: AggressorSide = trade.side.into();
    let trade_id = TradeId::new_checked(trade.exec_id.as_str())
        .context("invalid exec_id in Bybit trade payload")?;
    let ts_event = parse_millis_timestamp(&trade.time, "trade.time")?;

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor,
        trade_id,
        ts_event,
        ts_init,
    )
    .context("failed to construct TradeTick from Bybit trade payload")
}

/// Parses a kline entry into a [`Bar`].
pub fn parse_kline_bar(
    kline: &BybitKline,
    instrument: &InstrumentAny,
    bar_type: BarType,
    timestamp_on_close: bool,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open = parse_price_with_precision(&kline.open, price_precision, "kline.open")?;
    let high = parse_price_with_precision(&kline.high, price_precision, "kline.high")?;
    let low = parse_price_with_precision(&kline.low, price_precision, "kline.low")?;
    let close = parse_price_with_precision(&kline.close, price_precision, "kline.close")?;
    let volume = parse_quantity_with_precision(&kline.volume, size_precision, "kline.volume")?;

    let mut ts_event = parse_millis_timestamp(&kline.start, "kline.start")?;
    if timestamp_on_close {
        let interval_ns = bar_type
            .spec()
            .timedelta()
            .num_nanoseconds()
            .context("bar specification produced non-integer interval")?;
        let interval_ns = u64::try_from(interval_ns)
            .context("bar interval overflowed the u64 range for nanoseconds")?;
        let updated = ts_event
            .as_u64()
            .checked_add(interval_ns)
            .context("bar timestamp overflowed when adjusting to close time")?;
        ts_event = UnixNanos::from(updated);
    }
    let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("failed to construct Bar from Bybit kline entry")
}

/// Parses a Bybit execution into a Nautilus FillReport.
///
/// # Errors
///
/// This function returns an error if:
/// - Required price or quantity fields cannot be parsed.
/// - The execution timestamp cannot be parsed.
/// - Numeric conversions fail.
pub fn parse_fill_report(
    execution: &BybitExecution,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(execution.order_id.as_str());
    let trade_id = TradeId::new_checked(execution.exec_id.as_str())
        .context("invalid execId in Bybit execution payload")?;

    let order_side: OrderSide = execution.side.into();

    let last_px = parse_price_with_precision(
        &execution.exec_price,
        instrument.price_precision(),
        "execution.execPrice",
    )?;

    let last_qty = parse_quantity_with_precision(
        &execution.exec_qty,
        instrument.size_precision(),
        "execution.execQty",
    )?;

    // Parse commission (Bybit returns positive fee, Nautilus uses negative for costs)
    let fee_f64 = execution
        .exec_fee
        .parse::<f64>()
        .with_context(|| format!("Failed to parse execFee='{}'", execution.exec_fee))?;
    let commission = Money::new(-fee_f64, Currency::from(execution.fee_currency.as_str()));

    // Determine liquidity side from is_maker flag
    let liquidity_side = if execution.is_maker {
        LiquiditySide::Maker
    } else {
        LiquiditySide::Taker
    };

    let ts_event = parse_millis_timestamp(&execution.exec_time, "execution.execTime")?;

    // Parse client_order_id if present
    let client_order_id = if execution.order_link_id.is_empty() {
        None
    } else {
        Some(ClientOrderId::new(execution.order_link_id.as_str()))
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
        client_order_id,
        None, // venue_position_id not provided by Bybit executions
        ts_event,
        ts_init,
        None, // Will generate a new UUID4
    ))
}

/// Parses a Bybit position into a Nautilus PositionStatusReport.
///
/// # Errors
///
/// This function returns an error if:
/// - Position quantity or price fields cannot be parsed.
/// - The position timestamp cannot be parsed.
/// - Numeric conversions fail.
pub fn parse_position_status_report(
    position: &BybitPosition,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let instrument_id = instrument.id();

    // Parse position size
    let size_f64 = position
        .size
        .parse::<f64>()
        .with_context(|| format!("Failed to parse position size '{}'", position.size))?;

    // Determine position side and quantity
    let (position_side, quantity) = match position.side {
        crate::common::enums::BybitPositionSide::Buy => {
            let qty = Quantity::new(size_f64, instrument.size_precision());
            (PositionSideSpecified::Long, qty)
        }
        crate::common::enums::BybitPositionSide::Sell => {
            let qty = Quantity::new(size_f64, instrument.size_precision());
            (PositionSideSpecified::Short, qty)
        }
        crate::common::enums::BybitPositionSide::Flat => {
            let qty = Quantity::new(0.0, instrument.size_precision());
            (PositionSideSpecified::Flat, qty)
        }
    };

    // Parse average entry price
    let avg_px_open = if position.avg_price.is_empty() || position.avg_price == "0" {
        None
    } else {
        Some(Decimal::from_str(&position.avg_price)?)
    };

    // Parse timestamps
    let ts_last = parse_millis_timestamp(&position.updated_time, "position.updatedTime")?;

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        ts_last,
        ts_init,
        None, // Will generate a new UUID4
        None, // venue_position_id not used for now
        avg_px_open,
    ))
}

/// Parses a Bybit wallet balance into a Nautilus account state.
///
/// # Errors
///
/// Returns an error if:
/// - Balance data cannot be parsed.
/// - Currency is invalid.
pub fn parse_account_state(
    wallet_balance: &BybitWalletBalance,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let mut balances = Vec::new();

    // Parse each coin balance
    for coin in &wallet_balance.coin {
        let currency = Currency::from_str(&coin.coin)?;

        let wallet_balance_f64 = if coin.wallet_balance.is_empty() {
            0.0
        } else {
            coin.wallet_balance.parse::<f64>()?
        };

        // TODO: extract this logic to a function
        let spot_borrow_f64 = if let Some(ref spot_borrow) = coin.spot_borrow {
            if spot_borrow.is_empty() {
                0.0
            } else {
                spot_borrow.parse::<f64>()?
            }
        } else {
            0.0
        };

        let total_f64 = wallet_balance_f64 - spot_borrow_f64;

        let locked_f64 = if coin.locked.is_empty() {
            0.0
        } else {
            coin.locked.parse::<f64>()?
        };

        let total = Money::new(total_f64, currency);
        let locked = Money::new(locked_f64, currency);

        // Calculate free balance
        let free = if total.raw >= locked.raw {
            Money::from_raw(total.raw - locked.raw, currency)
        } else {
            Money::new(0.0, currency)
        };

        balances.push(AccountBalance::new(total, locked, free));
    }

    let mut margins = Vec::new();

    // Parse margin balances for each coin with position margin data
    for coin in &wallet_balance.coin {
        let currency = Currency::from_str(&coin.coin)?;

        let initial_margin_f64 = match &coin.total_position_im {
            Some(im) if !im.is_empty() => im.parse::<f64>()?,
            _ => 0.0,
        };

        let maintenance_margin_f64 = match &coin.total_position_mm {
            Some(mm) if !mm.is_empty() => mm.parse::<f64>()?,
            _ => 0.0,
        };

        // Only create margin balance if there are actual margin requirements
        if initial_margin_f64 > 0.0 || maintenance_margin_f64 > 0.0 {
            let initial_margin = Money::new(initial_margin_f64, currency);
            let maintenance_margin = Money::new(maintenance_margin_f64, currency);

            // Create a synthetic instrument_id for account-level margins
            let margin_instrument_id = InstrumentId::new(
                Symbol::from_str_unchecked(format!("ACCOUNT-{}", coin.coin)),
                Venue::new("BYBIT"),
            );

            margins.push(MarginBalance::new(
                initial_margin,
                maintenance_margin,
                margin_instrument_id,
            ));
        }
    }

    let account_type = AccountType::Margin;
    let is_reported = true;
    let event_id = nautilus_core::uuid::UUID4::new();

    // Use current time as ts_event since Bybit doesn't provide this in wallet balance
    let ts_event = ts_init;

    Ok(AccountState::new(
        account_id,
        account_type,
        balances,
        margins,
        is_reported,
        event_id,
        ts_event,
        ts_init,
        None,
    ))
}

pub(crate) fn parse_price_with_precision(
    value: &str,
    precision: u8,
    field: &str,
) -> anyhow::Result<Price> {
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("Failed to parse {field}='{value}' as f64"))?;
    Price::new_checked(parsed, precision).with_context(|| {
        format!("Failed to construct Price for {field} with precision {precision}")
    })
}

pub(crate) fn parse_quantity_with_precision(
    value: &str,
    precision: u8,
    field: &str,
) -> anyhow::Result<Quantity> {
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("Failed to parse {field}='{value}' as f64"))?;
    Quantity::new_checked(parsed, precision).with_context(|| {
        format!("Failed to construct Quantity for {field} with precision {precision}")
    })
}

pub(crate) fn parse_price(value: &str, field: &str) -> anyhow::Result<Price> {
    Price::from_str(value)
        .map_err(|err| anyhow::anyhow!("Failed to parse {field}='{value}': {err}"))
}

pub(crate) fn parse_quantity(value: &str, field: &str) -> anyhow::Result<Quantity> {
    Quantity::from_str(value)
        .map_err(|err| anyhow::anyhow!("Failed to parse {field}='{value}': {err}"))
}

pub(crate) fn parse_decimal(value: &str, field: &str) -> anyhow::Result<Decimal> {
    Decimal::from_str(value)
        .map_err(|err| anyhow::anyhow!("Failed to parse {field}='{value}' as Decimal: {err}"))
}

pub(crate) fn parse_millis_timestamp(value: &str, field: &str) -> anyhow::Result<UnixNanos> {
    let millis: u64 = value
        .parse()
        .with_context(|| format!("Failed to parse {field}='{value}' as u64 millis"))?;
    let nanos = millis
        .checked_mul(NANOSECONDS_IN_MILLISECOND)
        .context("millisecond timestamp overflowed when converting to nanoseconds")?;
    Ok(UnixNanos::from(nanos))
}

fn resolve_settlement_currency(
    settle_coin: &str,
    base_currency: Currency,
    quote_currency: Currency,
) -> anyhow::Result<Currency> {
    if settle_coin.eq_ignore_ascii_case(base_currency.code.as_str()) {
        Ok(base_currency)
    } else if settle_coin.eq_ignore_ascii_case(quote_currency.code.as_str()) {
        Ok(quote_currency)
    } else {
        Err(anyhow::anyhow!(
            "unrecognised settlement currency '{settle_coin}'"
        ))
    }
}

fn get_currency(code: &str) -> Currency {
    Currency::try_from_str(code)
        .unwrap_or_else(|| Currency::new(code, 8, 0, code, CurrencyType::Crypto))
}

fn extract_strike_from_symbol(symbol: &str) -> anyhow::Result<Price> {
    let parts: Vec<&str> = symbol.split('-').collect();
    let strike = parts
        .get(2)
        .ok_or_else(|| anyhow::anyhow!("invalid option symbol '{symbol}'"))?;
    parse_price(strike, "option strike")
}

/// Parses a Bybit order into a Nautilus OrderStatusReport.
pub fn parse_order_status_report(
    order: &crate::http::models::BybitOrder,
    instrument: &InstrumentAny,
    account_id: nautilus_model::identifiers::AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<nautilus_model::reports::OrderStatusReport> {
    use crate::common::enums::{BybitOrderStatus, BybitOrderType, BybitTimeInForce};

    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(order.order_id);

    let order_side: OrderSide = order.side.into();

    let order_type: OrderType = match order.order_type {
        BybitOrderType::Market => OrderType::Market,
        BybitOrderType::Limit => OrderType::Limit,
        BybitOrderType::Unknown => OrderType::Limit,
    };

    let time_in_force: TimeInForce = match order.time_in_force {
        BybitTimeInForce::Gtc => TimeInForce::Gtc,
        BybitTimeInForce::Ioc => TimeInForce::Ioc,
        BybitTimeInForce::Fok => TimeInForce::Fok,
        BybitTimeInForce::PostOnly => TimeInForce::Gtc,
    };

    let quantity =
        parse_quantity_with_precision(&order.qty, instrument.size_precision(), "order.qty")?;

    let filled_qty = parse_quantity_with_precision(
        &order.cum_exec_qty,
        instrument.size_precision(),
        "order.cumExecQty",
    )?;

    // Map Bybit order status to Nautilus order status
    // Special case: if Bybit reports "Rejected" but the order has fills, treat it as Canceled.
    // This handles the case where the exchange partially fills an order then rejects the
    // remaining quantity (e.g., due to margin, risk limits, or liquidity constraints).
    // The state machine does not allow PARTIALLY_FILLED -> REJECTED transitions.
    let order_status: OrderStatus = match order.order_status {
        BybitOrderStatus::Created | BybitOrderStatus::New | BybitOrderStatus::Untriggered => {
            OrderStatus::Accepted
        }
        BybitOrderStatus::Rejected => {
            if filled_qty.is_positive() {
                OrderStatus::Canceled
            } else {
                OrderStatus::Rejected
            }
        }
        BybitOrderStatus::PartiallyFilled => OrderStatus::PartiallyFilled,
        BybitOrderStatus::Filled => OrderStatus::Filled,
        BybitOrderStatus::Canceled | BybitOrderStatus::PartiallyFilledCanceled => {
            OrderStatus::Canceled
        }
        BybitOrderStatus::Triggered => OrderStatus::Triggered,
        BybitOrderStatus::Deactivated => OrderStatus::Canceled,
    };

    let ts_accepted = parse_millis_timestamp(&order.created_time, "order.createdTime")?;
    let ts_last = parse_millis_timestamp(&order.updated_time, "order.updatedTime")?;

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
        Some(nautilus_core::uuid::UUID4::new()),
    );

    if !order.order_link_id.is_empty() {
        report = report.with_client_order_id(ClientOrderId::new(order.order_link_id.as_str()));
    }

    if !order.price.is_empty() && order.price != "0" {
        let price =
            parse_price_with_precision(&order.price, instrument.price_precision(), "order.price")?;
        report = report.with_price(price);
    }

    if let Some(avg_price) = &order.avg_price
        && !avg_price.is_empty()
        && avg_price != "0"
    {
        let avg_px = avg_price
            .parse::<f64>()
            .with_context(|| format!("Failed to parse avg_price='{avg_price}' as f64"))?;
        report = report.with_avg_px(avg_px);
    }

    if !order.trigger_price.is_empty() && order.trigger_price != "0" {
        let trigger_price = parse_price_with_precision(
            &order.trigger_price,
            instrument.price_precision(),
            "order.triggerPrice",
        )?;
        report = report.with_trigger_price(trigger_price);
    }

    Ok(report)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::BarSpecification,
        enums::{AggregationSource, BarAggregation, PositionSide, PriceType},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        common::testing::load_test_json,
        http::models::{
            BybitInstrumentInverseResponse, BybitInstrumentLinearResponse,
            BybitInstrumentOptionResponse, BybitInstrumentSpotResponse, BybitKlinesResponse,
            BybitTradesResponse,
        },
    };

    const TS: UnixNanos = UnixNanos::new(1_700_000_000_000_000_000);

    fn sample_fee_rate(
        symbol: &str,
        taker: &str,
        maker: &str,
        base_coin: Option<&str>,
    ) -> BybitFeeRate {
        BybitFeeRate {
            symbol: Ustr::from(symbol),
            taker_fee_rate: taker.to_string(),
            maker_fee_rate: maker.to_string(),
            base_coin: base_coin.map(Ustr::from),
        }
    }

    fn linear_instrument() -> InstrumentAny {
        let json = load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSDT", "0.00055", "0.0001", Some("BTC"));
        parse_linear_instrument(instrument, &fee_rate, TS, TS).unwrap()
    }

    #[rstest]
    fn parse_spot_instrument_builds_currency_pair() {
        let json = load_test_json("http_get_instruments_spot.json");
        let response: BybitInstrumentSpotResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSDT", "0.0006", "0.0001", Some("BTC"));

        let parsed = parse_spot_instrument(instrument, &fee_rate, TS, TS).unwrap();
        match parsed {
            InstrumentAny::CurrencyPair(pair) => {
                assert_eq!(pair.id.to_string(), "BTCUSDT-SPOT.BYBIT");
                assert_eq!(pair.price_increment, Price::from_str("0.1").unwrap());
                assert_eq!(pair.size_increment, Quantity::from_str("0.0001").unwrap());
                assert_eq!(pair.base_currency.code.as_str(), "BTC");
                assert_eq!(pair.quote_currency.code.as_str(), "USDT");
            }
            _ => panic!("expected CurrencyPair"),
        }
    }

    #[rstest]
    fn parse_linear_perpetual_instrument_builds_crypto_perpetual() {
        let json = load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSDT", "0.00055", "0.0001", Some("BTC"));

        let parsed = parse_linear_instrument(instrument, &fee_rate, TS, TS).unwrap();
        match parsed {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.to_string(), "BTCUSDT-LINEAR.BYBIT");
                assert!(!perp.is_inverse);
                assert_eq!(perp.price_increment, Price::from_str("0.5").unwrap());
                assert_eq!(perp.size_increment, Quantity::from_str("0.001").unwrap());
            }
            other => panic!("unexpected instrument variant: {other:?}"),
        }
    }

    #[rstest]
    fn parse_inverse_perpetual_instrument_builds_inverse_perpetual() {
        let json = load_test_json("http_get_instruments_inverse.json");
        let response: BybitInstrumentInverseResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];
        let fee_rate = sample_fee_rate("BTCUSD", "0.00075", "0.00025", Some("BTC"));

        let parsed = parse_inverse_instrument(instrument, &fee_rate, TS, TS).unwrap();
        match parsed {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.to_string(), "BTCUSD-INVERSE.BYBIT");
                assert!(perp.is_inverse);
                assert_eq!(perp.price_increment, Price::from_str("0.5").unwrap());
                assert_eq!(perp.size_increment, Quantity::from_str("1").unwrap());
            }
            other => panic!("unexpected instrument variant: {other:?}"),
        }
    }

    #[rstest]
    fn parse_option_instrument_builds_option_contract() {
        let json = load_test_json("http_get_instruments_option.json");
        let response: BybitInstrumentOptionResponse = serde_json::from_str(&json).unwrap();
        let instrument = &response.result.list[0];

        let parsed = parse_option_instrument(instrument, TS, TS).unwrap();
        match parsed {
            InstrumentAny::OptionContract(option) => {
                assert_eq!(option.id.to_string(), "ETH-26JUN26-16000-P-OPTION.BYBIT");
                assert_eq!(option.option_kind, OptionKind::Put);
                assert_eq!(option.price_increment, Price::from_str("0.1").unwrap());
                assert_eq!(option.lot_size, Quantity::from_str("1").unwrap());
            }
            other => panic!("unexpected instrument variant: {other:?}"),
        }
    }

    #[rstest]
    fn parse_http_trade_into_trade_tick() {
        let instrument = linear_instrument();
        let json = load_test_json("http_get_trades_recent.json");
        let response: BybitTradesResponse = serde_json::from_str(&json).unwrap();
        let trade = &response.result.list[0];

        let tick = parse_trade_tick(trade, &instrument, TS).unwrap();

        assert_eq!(tick.instrument_id, instrument.id());
        assert_eq!(tick.price, instrument.make_price(27450.50));
        assert_eq!(tick.size, instrument.make_qty(0.005, None));
        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        assert_eq!(
            tick.trade_id.to_string(),
            "a905d5c3-1ed0-4f37-83e4-9c73a2fe2f01"
        );
        assert_eq!(tick.ts_event, UnixNanos::new(1_709_891_679_000_000_000));
    }

    #[rstest]
    fn parse_kline_into_bar() {
        let instrument = linear_instrument();
        let json = load_test_json("http_get_klines_linear.json");
        let response: BybitKlinesResponse = serde_json::from_str(&json).unwrap();
        let kline = &response.result.list[0];

        let bar_type = BarType::new(
            instrument.id(),
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );

        let bar = parse_kline_bar(kline, &instrument, bar_type, false, TS).unwrap();

        assert_eq!(bar.bar_type.to_string(), bar_type.to_string());
        assert_eq!(bar.open, instrument.make_price(27450.0));
        assert_eq!(bar.high, instrument.make_price(27460.0));
        assert_eq!(bar.low, instrument.make_price(27440.0));
        assert_eq!(bar.close, instrument.make_price(27455.0));
        assert_eq!(bar.volume, instrument.make_qty(123.45, None));
        assert_eq!(bar.ts_event, UnixNanos::new(1_709_891_679_000_000_000));
    }

    #[rstest]
    fn parse_http_position_short_into_position_status_report() {
        use crate::http::models::BybitPositionListResponse;

        let json = load_test_json("http_get_positions.json");
        let response: BybitPositionListResponse = serde_json::from_str(&json).unwrap();

        // Get the short position (ETHUSDT, side="Sell", size="5.0")
        let short_position = &response.result.list[1];
        assert_eq!(short_position.symbol.as_str(), "ETHUSDT");
        assert_eq!(
            short_position.side,
            crate::common::enums::BybitPositionSide::Sell
        );

        // Create ETHUSDT instrument for parsing
        let eth_json = load_test_json("http_get_instruments_linear.json");
        let eth_response: BybitInstrumentLinearResponse = serde_json::from_str(&eth_json).unwrap();
        let eth_def = &eth_response.result.list[1]; // ETHUSDT is second in the list
        let fee_rate = sample_fee_rate("ETHUSDT", "0.00055", "0.0001", Some("ETH"));
        let eth_instrument = parse_linear_instrument(eth_def, &fee_rate, TS, TS).unwrap();

        let account_id = AccountId::new("BYBIT-001");
        let report =
            parse_position_status_report(short_position, account_id, &eth_instrument, TS).unwrap();

        // Verify short position is correctly parsed
        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id.symbol.as_str(), "ETHUSDT-LINEAR");
        assert_eq!(report.position_side.as_position_side(), PositionSide::Short);
        assert_eq!(report.quantity, eth_instrument.make_qty(5.0, None));
        assert_eq!(
            report.avg_px_open,
            Some(Decimal::try_from(3000.00).unwrap())
        );
        assert_eq!(report.ts_last, UnixNanos::new(1_697_673_700_112_000_000));
    }

    #[rstest]
    fn parse_http_order_partially_filled_rejected_maps_to_canceled() {
        use crate::http::models::BybitOrderHistoryResponse;

        let instrument = linear_instrument();
        let json = load_test_json("http_get_order_partially_filled_rejected.json");
        let response: BybitOrderHistoryResponse = serde_json::from_str(&json).unwrap();
        let order = &response.result.list[0];
        let account_id = AccountId::new("BYBIT-001");

        let report = parse_order_status_report(order, &instrument, account_id, TS).unwrap();

        // Verify that Bybit "Rejected" status with fills is mapped to Canceled, not Rejected
        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.filled_qty, instrument.make_qty(0.005, None));
        assert_eq!(
            report.client_order_id.as_ref().unwrap().to_string(),
            "O-20251001-164609-APEX-000-49"
        );
    }
}
