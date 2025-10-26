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

//! Shared parsing helpers that transform BitMEX payloads into Nautilus types.

use chrono::{DateTime, Utc};
use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    data::bar::BarType,
    enums::{AccountType, AggressorSide, CurrencyType, LiquiditySide, PositionSide},
    events::AccountState,
    identifiers::{AccountId, InstrumentId, Symbol},
    instruments::{Instrument, InstrumentAny},
    types::{
        AccountBalance, Currency, Money, Price, Quantity,
        quantity::{QUANTITY_RAW_MAX, QuantityRaw},
    },
};
use rust_decimal::{Decimal, RoundingStrategy, prelude::ToPrimitive};
use ustr::Ustr;

use crate::{
    common::{
        consts::BITMEX_VENUE,
        enums::{BitmexLiquidityIndicator, BitmexSide},
    },
    websocket::messages::BitmexMarginMsg,
};

/// Strip NautilusTrader identifier from BitMEX rejection/cancellation reasons.
///
/// BitMEX appends our `text` field as `\nNautilusTrader` to their messages.
#[must_use]
pub fn clean_reason(reason: &str) -> String {
    reason.replace("\nNautilusTrader", "").trim().to_string()
}

/// Parses a Nautilus instrument ID from the given BitMEX `symbol` value.
#[must_use]
pub fn parse_instrument_id(symbol: Ustr) -> InstrumentId {
    InstrumentId::new(Symbol::from_ustr_unchecked(symbol), *BITMEX_VENUE)
}

/// Safely converts a `Quantity` into the integer units expected by the BitMEX REST API.
///
/// The API expects whole-number "contract" counts which vary per instrument. We always use the
/// instrument size increment (sourced from BitMEX `underlyingToPositionMultiplier`) to translate
/// Nautilus quantities back to venue units, so each instrument can have its own contract multiplier.
/// Values are rounded to the nearest whole contract (midpoint rounds away from zero) and clamped
/// to `u32::MAX` when necessary.
#[must_use]
pub fn quantity_to_u32(quantity: &Quantity, instrument: &InstrumentAny) -> u32 {
    let size_increment = instrument.size_increment();
    let step_decimal = size_increment.as_decimal();

    if step_decimal.is_zero() {
        let value = quantity.as_f64();
        if value > u32::MAX as f64 {
            tracing::warn!(
                "Quantity {value} exceeds u32::MAX without instrument increment, clamping",
            );
            return u32::MAX;
        }
        return value.max(0.0) as u32;
    }

    let units_decimal = quantity.as_decimal() / step_decimal;
    let rounded_units =
        units_decimal.round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero);

    match rounded_units.to_u128() {
        Some(units) if units <= u32::MAX as u128 => units as u32,
        Some(units) => {
            tracing::warn!(
                "Quantity {} converts to {units} contracts which exceeds u32::MAX, clamping",
                quantity.as_f64(),
            );
            u32::MAX
        }
        None => {
            tracing::warn!(
                "Failed to convert quantity {} to venue units, defaulting to 0",
                quantity.as_f64(),
            );
            0
        }
    }
}

/// Converts a BitMEX contracts value into a Nautilus quantity using instrument precision.
#[must_use]
pub fn parse_contracts_quantity(value: u64, instrument: &InstrumentAny) -> Quantity {
    let size_increment = instrument.size_increment();
    let precision = instrument.size_precision();

    let increment_raw: QuantityRaw = (&size_increment).into();
    let value_raw = QuantityRaw::from(value);

    let mut raw = increment_raw.saturating_mul(value_raw);
    if raw > QUANTITY_RAW_MAX {
        tracing::warn!(
            "Quantity value {value} exceeds QUANTITY_RAW_MAX {}, clamping",
            QUANTITY_RAW_MAX,
        );
        raw = QUANTITY_RAW_MAX;
    }

    Quantity::from_raw(raw, precision)
}

/// Converts the BitMEX `underlyingToPositionMultiplier` into a normalized contract size and
/// size increment for Nautilus instruments.
///
/// The returned decimal retains BitMEX precision (clamped to `max_scale`) so downstream
/// quantity conversions stay lossless.
///
/// # Errors
///
/// Returns an error when the multiplier cannot be represented with the configured precision.
pub fn derive_contract_decimal_and_increment(
    multiplier: Option<f64>,
    max_scale: u32,
) -> anyhow::Result<(Decimal, Quantity)> {
    let raw_multiplier = multiplier.unwrap_or(1.0);
    let contract_size = if raw_multiplier > 0.0 {
        1.0 / raw_multiplier
    } else {
        1.0
    };

    let mut contract_decimal = Decimal::from_f64_retain(contract_size)
        .ok_or_else(|| anyhow::anyhow!("Invalid contract size {contract_size}"))?;
    if contract_decimal.scale() > max_scale {
        contract_decimal = contract_decimal
            .round_dp_with_strategy(max_scale, RoundingStrategy::MidpointAwayFromZero);
    }
    contract_decimal = contract_decimal.normalize();
    let contract_precision = contract_decimal.scale() as u8;
    let size_increment = Quantity::from_decimal(contract_decimal, contract_precision)?;

    Ok((contract_decimal, size_increment))
}

/// Converts an optional contract-count field (e.g. `lotSize`, `maxOrderQty`) into a Nautilus
/// quantity using the previously derived contract size.
///
/// # Errors
///
/// Returns an error when the raw value cannot be represented with the available precision.
pub fn convert_contract_quantity(
    value: Option<f64>,
    contract_decimal: Decimal,
    max_scale: u32,
    field_name: &str,
) -> anyhow::Result<Option<Quantity>> {
    value
        .map(|raw| {
            let mut decimal = Decimal::from_f64_retain(raw)
                .ok_or_else(|| anyhow::anyhow!("Invalid {field_name} value"))?
                * contract_decimal;
            let scale = decimal.scale();
            if scale > max_scale {
                decimal = decimal
                    .round_dp_with_strategy(max_scale, RoundingStrategy::MidpointAwayFromZero);
            }
            let decimal = decimal.normalize();
            let precision = decimal.scale() as u8;
            Quantity::from_decimal(decimal, precision)
        })
        .transpose()
}

/// Converts a signed BitMEX contracts value into a Nautilus quantity using instrument precision.
#[must_use]
pub fn parse_signed_contracts_quantity(value: i64, instrument: &InstrumentAny) -> Quantity {
    let abs_value = value.checked_abs().unwrap_or_else(|| {
        tracing::warn!("Quantity value {value} overflowed when taking absolute value");
        i64::MAX
    }) as u64;
    parse_contracts_quantity(abs_value, instrument)
}

/// Converts a fractional size into a quantity honoring the instrument precision.
#[must_use]
pub fn parse_fractional_quantity(value: f64, instrument: &InstrumentAny) -> Quantity {
    if value < 0.0 {
        tracing::warn!("Received negative fractional quantity {value}, defaulting to 0.0");
        return instrument.make_qty(0.0, None);
    }

    instrument.try_make_qty(value, None).unwrap_or_else(|err| {
        tracing::warn!(
            "Failed to convert fractional quantity {value} with precision {}: {err}",
            instrument.size_precision(),
        );
        instrument.make_qty(0.0, None)
    })
}

/// Normalizes the OHLC values reported by BitMEX trade bins to ensure `high >= max(open, close)`
/// and `low <= min(open, close)`.
///
/// # Panics
///
/// Panics if the price array is empty. This should never occur because the caller always supplies
/// four price values (open/high/low/close).
#[must_use]
pub fn normalize_trade_bin_prices(
    open: Price,
    mut high: Price,
    mut low: Price,
    close: Price,
    symbol: &Ustr,
    bar_type: Option<&BarType>,
) -> (Price, Price, Price, Price) {
    let price_extremes = [open, high, low, close];
    let max_price = *price_extremes
        .iter()
        .max()
        .expect("Price array contains values");
    let min_price = *price_extremes
        .iter()
        .min()
        .expect("Price array contains values");

    if high < max_price || low > min_price {
        match bar_type {
            Some(bt) => {
                tracing::warn!(symbol = %symbol, ?bt, "Adjusting BitMEX trade bin extremes");
            }
            None => tracing::warn!(symbol = %symbol, "Adjusting BitMEX trade bin extremes"),
        }
        high = max_price;
        low = min_price;
    }

    (open, high, low, close)
}

/// Normalizes the volume reported by BitMEX trade bins, defaulting to zero when the exchange
/// returns negative or missing values.
#[must_use]
pub fn normalize_trade_bin_volume(volume: Option<i64>, symbol: &Ustr) -> u64 {
    match volume {
        Some(v) if v >= 0 => v as u64,
        Some(v) => {
            tracing::warn!(symbol = %symbol, volume = v, "Received negative volume in BitMEX trade bin");
            0
        }
        None => {
            tracing::warn!(symbol = %symbol, "Trade bin missing volume, defaulting to 0");
            0
        }
    }
}

/// Parses the given datetime (UTC) into a `UnixNanos` timestamp.
/// If `value` is `None`, then defaults to the UNIX epoch (0 nanoseconds).
///
/// Returns epoch (0) for invalid timestamps that cannot be converted to nanoseconds.
#[must_use]
pub fn parse_optional_datetime_to_unix_nanos(
    value: &Option<DateTime<Utc>>,
    field: &str,
) -> UnixNanos {
    value
        .map(|dt| {
            UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or_else(|| {
                tracing::error!(field = field, timestamp = ?dt, "Invalid timestamp - out of range");
                0
            }) as u64)
        })
        .unwrap_or_default()
}

/// Maps an optional BitMEX side to the corresponding Nautilus aggressor side.
#[must_use]
pub const fn parse_aggressor_side(side: &Option<BitmexSide>) -> AggressorSide {
    match side {
        Some(BitmexSide::Buy) => AggressorSide::Buyer,
        Some(BitmexSide::Sell) => AggressorSide::Seller,
        None => AggressorSide::NoAggressor,
    }
}

/// Maps BitMEX liquidity indicators onto Nautilus liquidity sides.
#[must_use]
pub fn parse_liquidity_side(liquidity: &Option<BitmexLiquidityIndicator>) -> LiquiditySide {
    liquidity
        .map(std::convert::Into::into)
        .unwrap_or(LiquiditySide::NoLiquiditySide)
}

/// Derives a Nautilus position side from the BitMEX `currentQty` value.
#[must_use]
pub const fn parse_position_side(current_qty: Option<i64>) -> PositionSide {
    match current_qty {
        Some(qty) if qty > 0 => PositionSide::Long,
        Some(qty) if qty < 0 => PositionSide::Short,
        _ => PositionSide::Flat,
    }
}

/// Maps BitMEX currency codes to standard Nautilus currency codes.
///
/// BitMEX uses some non-standard currency codes:
/// - "XBt" -> "XBT" (Bitcoin)
/// - "USDt" -> "USDT" (Tether)
/// - "LAMp" -> "USDT" (Test currency, mapped to USDT)
/// - "RLUSd" -> "RLUSD" (Ripple USD stablecoin)
/// - "MAMUSd" -> "MAMUSD" (Unknown stablecoin)
///
/// For other currencies, converts to uppercase.
#[must_use]
pub fn map_bitmex_currency(bitmex_currency: &str) -> String {
    match bitmex_currency {
        "XBt" => "XBT".to_string(),
        "USDt" => "USDT".to_string(),
        "LAMp" => "USDT".to_string(), // Map test currency to USDT
        "RLUSd" => "RLUSD".to_string(),
        "MAMUSd" => "MAMUSD".to_string(),
        other => other.to_uppercase(),
    }
}

/// Parses a BitMEX margin message into a Nautilus account state.
///
/// # Errors
///
/// Returns an error if the margin data cannot be parsed into valid balance values.
pub fn parse_account_state(
    margin: &BitmexMarginMsg,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    tracing::debug!(
        "Parsing margin: currency={}, wallet_balance={:?}, available_margin={:?}, init_margin={:?}, maint_margin={:?}, foreign_margin_balance={:?}, foreign_requirement={:?}",
        margin.currency,
        margin.wallet_balance,
        margin.available_margin,
        margin.init_margin,
        margin.maint_margin,
        margin.foreign_margin_balance,
        margin.foreign_requirement
    );

    let currency_str = map_bitmex_currency(&margin.currency);

    let currency = match Currency::try_from_str(&currency_str) {
        Some(c) => c,
        None => {
            // Create a default crypto currency for unknown codes to avoid disrupting flows
            tracing::warn!(
                "Unknown currency '{currency_str}' in margin message, creating default crypto currency"
            );
            Currency::new(&currency_str, 8, 0, &currency_str, CurrencyType::Crypto)
        }
    };

    // BitMEX returns values in satoshis for BTC (XBt) or microunits for USDT/LAMp
    let divisor = if margin.currency == "XBt" {
        100_000_000.0 // Satoshis to BTC
    } else if margin.currency == "USDt" || margin.currency == "LAMp" {
        1_000_000.0 // Microunits to units
    } else {
        1.0
    };

    // Wallet balance is the actual asset amount
    let total = if let Some(wallet_balance) = margin.wallet_balance {
        Money::new(wallet_balance as f64 / divisor, currency)
    } else if let Some(margin_balance) = margin.margin_balance {
        Money::new(margin_balance as f64 / divisor, currency)
    } else if let Some(available) = margin.available_margin {
        // Fallback when only available_margin is provided
        Money::new(available as f64 / divisor, currency)
    } else {
        Money::new(0.0, currency)
    };

    // Calculate how much is locked for margin requirements
    let margin_used = if let Some(init_margin) = margin.init_margin {
        Money::new(init_margin as f64 / divisor, currency)
    } else {
        Money::new(0.0, currency)
    };

    // Free balance: prefer withdrawable_margin, then available_margin, then calculate
    let free = if let Some(withdrawable) = margin.withdrawable_margin {
        Money::new(withdrawable as f64 / divisor, currency)
    } else if let Some(available) = margin.available_margin {
        // Available margin already accounts for orders and positions
        let available_money = Money::new(available as f64 / divisor, currency);
        // Ensure it doesn't exceed total (can happen with unrealized PnL)
        if available_money > total {
            total
        } else {
            available_money
        }
    } else {
        // Fallback: free = total - init_margin
        let calculated_free = total - margin_used;
        if calculated_free < Money::new(0.0, currency) {
            Money::new(0.0, currency)
        } else {
            calculated_free
        }
    };

    // Locked is what's being used for margin
    let locked = total - free;

    let balance = AccountBalance::new(total, locked, free);
    let balances = vec![balance];

    // Skip margin details - BitMEX uses account-level cross-margin which doesn't map
    // well to Nautilus's per-instrument margin model, we track balances only.
    let margins = Vec::new();

    let account_type = AccountType::Margin;
    let is_reported = true;
    let event_id = UUID4::new();
    let ts_event =
        UnixNanos::from(margin.timestamp.timestamp_nanos_opt().unwrap_or_default() as u64);

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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use nautilus_model::{instruments::CurrencyPair, types::fixed::FIXED_PRECISION};
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_clean_reason_strips_nautilus_trader() {
        assert_eq!(
            clean_reason(
                "Canceled: Order had execInst of ParticipateDoNotInitiate\nNautilusTrader"
            ),
            "Canceled: Order had execInst of ParticipateDoNotInitiate"
        );

        assert_eq!(clean_reason("Some error\nNautilusTrader"), "Some error");
        assert_eq!(
            clean_reason("Multiple lines\nSome content\nNautilusTrader"),
            "Multiple lines\nSome content"
        );
        assert_eq!(clean_reason("No identifier here"), "No identifier here");
        assert_eq!(clean_reason("  \nNautilusTrader  "), "");
    }

    fn make_test_spot_instrument(size_increment: f64, size_precision: u8) -> InstrumentAny {
        let instrument_id = InstrumentId::from("SOLUSDT.BITMEX");
        let raw_symbol = Symbol::from("SOLUSDT");
        let base_currency = Currency::from("SOL");
        let quote_currency = Currency::from("USDT");
        let price_precision = 2;
        let price_increment = Price::new(0.01, price_precision);
        let size_increment = Quantity::new(size_increment, size_precision);
        let instrument = CurrencyPair::new(
            instrument_id,
            raw_symbol,
            base_currency,
            quote_currency,
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            None, // multiplier
            None, // lot_size
            None, // max_quantity
            None, // min_quantity
            None, // max_notional
            None, // min_notional
            None, // max_price
            None, // min_price
            None, // margin_init
            None, // margin_maint
            None, // maker_fee
            None, // taker_fee
            UnixNanos::from(0),
            UnixNanos::from(0),
        );
        InstrumentAny::CurrencyPair(instrument)
    }

    #[rstest]
    fn test_quantity_to_u32_scaled() {
        let instrument = make_test_spot_instrument(0.0001, 4);
        let qty = Quantity::new(0.1, 4);
        assert_eq!(quantity_to_u32(&qty, &instrument), 1_000);
    }

    #[rstest]
    fn test_parse_contracts_quantity_scaled() {
        let instrument = make_test_spot_instrument(0.0001, 4);
        let qty = parse_contracts_quantity(1_000, &instrument);
        assert!((qty.as_f64() - 0.1).abs() < 1e-9);
        assert_eq!(qty.precision, 4);
    }

    #[rstest]
    fn test_convert_contract_quantity_scaling() {
        let max_scale = FIXED_PRECISION as u32;
        let (contract_decimal, size_increment) =
            derive_contract_decimal_and_increment(Some(10_000.0), max_scale).unwrap();
        assert!((size_increment.as_f64() - 0.0001).abs() < 1e-12);

        let lot_qty =
            convert_contract_quantity(Some(1_000.0), contract_decimal, max_scale, "lot size")
                .unwrap()
                .unwrap();
        assert!((lot_qty.as_f64() - 0.1).abs() < 1e-9);
        assert_eq!(lot_qty.precision, 1);
    }

    #[rstest]
    fn test_derive_contract_decimal_defaults_to_one() {
        let max_scale = FIXED_PRECISION as u32;
        let (contract_decimal, size_increment) =
            derive_contract_decimal_and_increment(Some(0.0), max_scale).unwrap();
        assert_eq!(contract_decimal, Decimal::ONE);
        assert_eq!(size_increment.as_f64(), 1.0);
    }

    #[rstest]
    fn test_parse_account_state() {
        let margin_msg = BitmexMarginMsg {
            account: 123456,
            currency: Ustr::from("XBt"),
            risk_limit: Some(1000000000),
            amount: Some(5000000),
            prev_realised_pnl: Some(100000),
            gross_comm: Some(1000),
            gross_open_cost: Some(200000),
            gross_open_premium: None,
            gross_exec_cost: None,
            gross_mark_value: Some(210000),
            risk_value: Some(50000),
            init_margin: Some(20000),
            maint_margin: Some(10000),
            target_excess_margin: Some(5000),
            realised_pnl: Some(100000),
            unrealised_pnl: Some(10000),
            wallet_balance: Some(5000000),
            margin_balance: Some(5010000),
            margin_leverage: Some(2.5),
            margin_used_pcnt: Some(0.25),
            excess_margin: Some(4990000),
            available_margin: Some(4980000),
            withdrawable_margin: Some(4900000),
            maker_fee_discount: Some(0.1),
            taker_fee_discount: Some(0.05),
            timestamp: chrono::Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap(),
            foreign_margin_balance: None,
            foreign_requirement: None,
        };

        let account_id = AccountId::new("BITMEX-001");
        let ts_init = UnixNanos::from(1_000_000_000);

        let account_state = parse_account_state(&margin_msg, account_id, ts_init).unwrap();

        assert_eq!(account_state.account_id, account_id);
        assert_eq!(account_state.account_type, AccountType::Margin);
        assert_eq!(account_state.balances.len(), 1);
        assert_eq!(account_state.margins.len(), 0); // No margins tracked
        assert!(account_state.is_reported);

        let xbt_balance = &account_state.balances[0];
        assert_eq!(xbt_balance.currency, Currency::from("XBT"));
        assert_eq!(xbt_balance.total.as_f64(), 0.05); // 5000000 satoshis = 0.05 XBT wallet balance
        assert_eq!(xbt_balance.free.as_f64(), 0.049); // 4900000 satoshis = 0.049 XBT withdrawable
        assert_eq!(xbt_balance.locked.as_f64(), 0.001); // 100000 satoshis locked
    }

    #[rstest]
    fn test_parse_account_state_usdt() {
        let margin_msg = BitmexMarginMsg {
            account: 123456,
            currency: Ustr::from("USDt"),
            risk_limit: Some(1000000000),
            amount: Some(10000000000), // 10000 USDT in microunits
            prev_realised_pnl: None,
            gross_comm: None,
            gross_open_cost: None,
            gross_open_premium: None,
            gross_exec_cost: None,
            gross_mark_value: None,
            risk_value: None,
            init_margin: Some(500000),  // 0.5 USDT in microunits
            maint_margin: Some(250000), // 0.25 USDT in microunits
            target_excess_margin: None,
            realised_pnl: None,
            unrealised_pnl: None,
            wallet_balance: Some(10000000000),
            margin_balance: Some(10000000000),
            margin_leverage: None,
            margin_used_pcnt: None,
            excess_margin: None,
            available_margin: Some(9500000000), // 9500 USDT available
            withdrawable_margin: None,
            maker_fee_discount: None,
            taker_fee_discount: None,
            timestamp: chrono::Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap(),
            foreign_margin_balance: None,
            foreign_requirement: None,
        };

        let account_id = AccountId::new("BITMEX-001");
        let ts_init = UnixNanos::from(1_000_000_000);

        let account_state = parse_account_state(&margin_msg, account_id, ts_init).unwrap();

        let usdt_balance = &account_state.balances[0];
        assert_eq!(usdt_balance.currency, Currency::USDT());
        assert_eq!(usdt_balance.total.as_f64(), 10000.0);
        assert_eq!(usdt_balance.free.as_f64(), 9500.0);
        assert_eq!(usdt_balance.locked.as_f64(), 500.0);

        assert_eq!(account_state.margins.len(), 0); // No margins tracked
    }

    #[rstest]
    fn test_parse_margin_message_with_missing_fields() {
        // Create a margin message with missing optional fields
        let margin_msg = BitmexMarginMsg {
            account: 123456,
            currency: Ustr::from("XBt"),
            risk_limit: None,
            amount: None,
            prev_realised_pnl: None,
            gross_comm: None,
            gross_open_cost: None,
            gross_open_premium: None,
            gross_exec_cost: None,
            gross_mark_value: None,
            risk_value: None,
            init_margin: None,  // Missing
            maint_margin: None, // Missing
            target_excess_margin: None,
            realised_pnl: None,
            unrealised_pnl: None,
            wallet_balance: Some(100000),
            margin_balance: None,
            margin_leverage: None,
            margin_used_pcnt: None,
            excess_margin: None,
            available_margin: Some(95000),
            withdrawable_margin: None,
            maker_fee_discount: None,
            taker_fee_discount: None,
            timestamp: chrono::Utc::now(),
            foreign_margin_balance: None,
            foreign_requirement: None,
        };

        let account_id = AccountId::new("BITMEX-123456");
        let ts_init = UnixNanos::from(1_000_000_000);

        let account_state = parse_account_state(&margin_msg, account_id, ts_init)
            .expect("Should parse even with missing margin fields");

        // Should have balance but no margins
        assert_eq!(account_state.balances.len(), 1);
        assert_eq!(account_state.margins.len(), 0); // No margins tracked
    }

    #[rstest]
    fn test_parse_margin_message_with_only_available_margin() {
        // This is the case we saw in the logs - only available_margin is populated
        let margin_msg = BitmexMarginMsg {
            account: 1667725,
            currency: Ustr::from("USDt"),
            risk_limit: None,
            amount: None,
            prev_realised_pnl: None,
            gross_comm: None,
            gross_open_cost: None,
            gross_open_premium: None,
            gross_exec_cost: None,
            gross_mark_value: None,
            risk_value: None,
            init_margin: None,
            maint_margin: None,
            target_excess_margin: None,
            realised_pnl: None,
            unrealised_pnl: None,
            wallet_balance: None, // None
            margin_balance: None, // None
            margin_leverage: None,
            margin_used_pcnt: None,
            excess_margin: None,
            available_margin: Some(107859036), // Only this is populated
            withdrawable_margin: None,
            maker_fee_discount: None,
            taker_fee_discount: None,
            timestamp: chrono::Utc::now(),
            foreign_margin_balance: None,
            foreign_requirement: None,
        };

        let account_id = AccountId::new("BITMEX-1667725");
        let ts_init = UnixNanos::from(1_000_000_000);

        let account_state = parse_account_state(&margin_msg, account_id, ts_init)
            .expect("Should handle case with only available_margin");

        // Check the balance accounting equation holds
        let balance = &account_state.balances[0];
        assert_eq!(balance.currency, Currency::USDT());
        assert_eq!(balance.total.as_f64(), 107.859036); // Total should equal free when only available_margin is present
        assert_eq!(balance.free.as_f64(), 107.859036);
        assert_eq!(balance.locked.as_f64(), 0.0);

        // Verify the accounting equation: total = locked + free
        assert_eq!(balance.total, balance.locked + balance.free);
    }

    #[rstest]
    fn test_parse_margin_available_exceeds_wallet() {
        // Test case where available margin exceeds wallet balance (bonus margin scenario)
        let margin_msg = BitmexMarginMsg {
            account: 123456,
            currency: Ustr::from("XBt"),
            risk_limit: None,
            amount: Some(70772),
            prev_realised_pnl: None,
            gross_comm: None,
            gross_open_cost: None,
            gross_open_premium: None,
            gross_exec_cost: None,
            gross_mark_value: None,
            risk_value: None,
            init_margin: Some(0),
            maint_margin: Some(0),
            target_excess_margin: None,
            realised_pnl: None,
            unrealised_pnl: None,
            wallet_balance: Some(70772), // 0.00070772 BTC
            margin_balance: None,
            margin_leverage: None,
            margin_used_pcnt: None,
            excess_margin: None,
            available_margin: Some(94381), // 0.00094381 BTC - exceeds wallet!
            withdrawable_margin: None,
            maker_fee_discount: None,
            taker_fee_discount: None,
            timestamp: chrono::Utc::now(),
            foreign_margin_balance: None,
            foreign_requirement: None,
        };

        let account_id = AccountId::new("BITMEX-123456");
        let ts_init = UnixNanos::from(1_000_000_000);

        let account_state = parse_account_state(&margin_msg, account_id, ts_init)
            .expect("Should handle available > wallet case");

        // Wallet balance is the actual asset amount, not available margin
        let balance = &account_state.balances[0];
        assert_eq!(balance.currency, Currency::from("XBT"));
        assert_eq!(balance.total.as_f64(), 0.00070772); // Wallet balance (actual assets)
        assert_eq!(balance.free.as_f64(), 0.00070772); // All free since no margin locked
        assert_eq!(balance.locked.as_f64(), 0.0);

        // Verify the accounting equation: total = locked + free
        assert_eq!(balance.total, balance.locked + balance.free);
    }

    #[rstest]
    fn test_parse_margin_message_with_foreign_requirements() {
        // Test case where trading USDT-settled contracts with XBT margin
        let margin_msg = BitmexMarginMsg {
            account: 123456,
            currency: Ustr::from("XBt"),
            risk_limit: Some(1000000000),
            amount: Some(100000000), // 1 BTC
            prev_realised_pnl: None,
            gross_comm: None,
            gross_open_cost: None,
            gross_open_premium: None,
            gross_exec_cost: None,
            gross_mark_value: None,
            risk_value: None,
            init_margin: None,  // No direct margin in XBT
            maint_margin: None, // No direct margin in XBT
            target_excess_margin: None,
            realised_pnl: None,
            unrealised_pnl: None,
            wallet_balance: Some(100000000),
            margin_balance: Some(100000000),
            margin_leverage: None,
            margin_used_pcnt: None,
            excess_margin: None,
            available_margin: Some(95000000), // 0.95 BTC available
            withdrawable_margin: None,
            maker_fee_discount: None,
            taker_fee_discount: None,
            timestamp: chrono::Utc::now(),
            foreign_margin_balance: Some(100000000), // Foreign margin balance in satoshis
            foreign_requirement: Some(5000000),      // 0.05 BTC required for USDT positions
        };

        let account_id = AccountId::new("BITMEX-123456");
        let ts_init = UnixNanos::from(1_000_000_000);

        let account_state = parse_account_state(&margin_msg, account_id, ts_init)
            .expect("Failed to parse account state with foreign requirements");

        // Check balance
        let balance = &account_state.balances[0];
        assert_eq!(balance.currency, Currency::from("XBT"));
        assert_eq!(balance.total.as_f64(), 1.0);
        assert_eq!(balance.free.as_f64(), 0.95);
        assert_eq!(balance.locked.as_f64(), 0.05);

        // No margins tracked
        assert_eq!(account_state.margins.len(), 0);
    }

    #[rstest]
    fn test_parse_margin_message_with_both_standard_and_foreign() {
        // Test case with both standard and foreign margin requirements
        let margin_msg = BitmexMarginMsg {
            account: 123456,
            currency: Ustr::from("XBt"),
            risk_limit: Some(1000000000),
            amount: Some(100000000), // 1 BTC
            prev_realised_pnl: None,
            gross_comm: None,
            gross_open_cost: None,
            gross_open_premium: None,
            gross_exec_cost: None,
            gross_mark_value: None,
            risk_value: None,
            init_margin: Some(2000000),  // 0.02 BTC for XBT positions
            maint_margin: Some(1000000), // 0.01 BTC for XBT positions
            target_excess_margin: None,
            realised_pnl: None,
            unrealised_pnl: None,
            wallet_balance: Some(100000000),
            margin_balance: Some(100000000),
            margin_leverage: None,
            margin_used_pcnt: None,
            excess_margin: None,
            available_margin: Some(93000000), // 0.93 BTC available
            withdrawable_margin: None,
            maker_fee_discount: None,
            taker_fee_discount: None,
            timestamp: chrono::Utc::now(),
            foreign_margin_balance: Some(100000000),
            foreign_requirement: Some(5000000), // 0.05 BTC for USDT positions
        };

        let account_id = AccountId::new("BITMEX-123456");
        let ts_init = UnixNanos::from(1_000_000_000);

        let account_state = parse_account_state(&margin_msg, account_id, ts_init)
            .expect("Failed to parse account state with both margins");

        // Check balance
        let balance = &account_state.balances[0];
        assert_eq!(balance.currency, Currency::from("XBT"));
        assert_eq!(balance.total.as_f64(), 1.0);
        assert_eq!(balance.free.as_f64(), 0.93);
        assert_eq!(balance.locked.as_f64(), 0.07); // 0.02 + 0.05 = 0.07 total margin

        // No margins tracked
        assert_eq!(account_state.margins.len(), 0);
    }
}
