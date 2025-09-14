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

use chrono::{DateTime, Utc};
use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    enums::{AccountType, AggressorSide, CurrencyType, LiquiditySide, PositionSide},
    events::AccountState,
    identifiers::{AccountId, InstrumentId, Symbol},
    types::{AccountBalance, Currency, Money, QUANTITY_MAX, Quantity},
};
use ustr::Ustr;

use crate::{
    common::{
        consts::BITMEX_VENUE,
        enums::{BitmexLiquidityIndicator, BitmexSide},
    },
    websocket::messages::BitmexMarginMsg,
};

/// Parses a Nautilus instrument ID from the given BitMEX `symbol` value.
#[must_use]
pub fn parse_instrument_id(symbol: Ustr) -> InstrumentId {
    InstrumentId::new(Symbol::from_ustr_unchecked(symbol), *BITMEX_VENUE)
}

/// Safely converts a Quantity to u32 for BitMEX API.
///
/// Logs a warning if truncation occurs.
#[must_use]
pub fn quantity_to_u32(quantity: &Quantity) -> u32 {
    let value = quantity.as_f64();
    if value > u32::MAX as f64 {
        tracing::warn!(
            "Quantity {value} exceeds u32::MAX, clamping to {}",
            u32::MAX
        );
        u32::MAX
    } else if value < 0.0 {
        tracing::warn!("Quantity {value} is negative, using 0");
        0
    } else {
        value as u32
    }
}

#[must_use]
pub fn parse_contracts_quantity(value: u64) -> Quantity {
    let size_workaround = std::cmp::min(QUANTITY_MAX as u64, value);
    // TODO: Log with more visibility for now
    if value > QUANTITY_MAX as u64 {
        tracing::warn!(
            "Quantity value {value} exceeds QUANTITY_MAX {QUANTITY_MAX}, clamping to maximum",
        );
    }
    Quantity::new(size_workaround as f64, 0)
}

#[must_use]
pub fn parse_frac_quantity(value: f64, size_precision: u8) -> Quantity {
    let value_u64 = value as u64;
    let size_workaround = std::cmp::min(QUANTITY_MAX as u64, value as u64);
    // TODO: Log with more visibility for now
    if value_u64 > QUANTITY_MAX as u64 {
        tracing::warn!(
            "Quantity value {value} exceeds QUANTITY_MAX {QUANTITY_MAX}, clamping to maximum",
        );
    }
    Quantity::new(size_workaround as f64, size_precision)
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

#[must_use]
pub const fn parse_aggressor_side(side: &Option<BitmexSide>) -> AggressorSide {
    match side {
        Some(BitmexSide::Buy) => AggressorSide::Buyer,
        Some(BitmexSide::Sell) => AggressorSide::Seller,
        None => AggressorSide::NoAggressor,
    }
}

#[must_use]
pub fn parse_liquidity_side(liquidity: &Option<BitmexLiquidityIndicator>) -> LiquiditySide {
    liquidity
        .map(std::convert::Into::into)
        .unwrap_or(LiquiditySide::NoLiquiditySide)
}

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
    use nautilus_model::enums::AccountType;
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

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
