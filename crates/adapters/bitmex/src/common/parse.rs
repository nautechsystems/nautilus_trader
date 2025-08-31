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
    enums::{AccountType, PositionSide},
    events::AccountState,
    identifiers::{AccountId, InstrumentId, Symbol},
    types::{AccountBalance, Currency, Money},
};

use crate::{
    common::{
        consts::BITMEX_VENUE,
        enums::{
            BitmexContingencyType, BitmexLiquidityIndicator, BitmexOrderStatus, BitmexOrderType,
            BitmexSide, BitmexTimeInForce,
        },
    },
    websocket::messages::BitmexMarginMsg,
};

/// Parses a Nautilus instrument ID from the given BitMEX `symbol` value.
#[must_use]
pub fn parse_instrument_id(symbol: &str) -> InstrumentId {
    InstrumentId::new(Symbol::from_str_unchecked(symbol), *BITMEX_VENUE)
}

/// Parses the given datetime (UTC) into a `UnixNanos` timestamp.
/// If `value` is `None`, then defaults to the UNIX epoch (0 nanoseconds).
///
/// # Panics
///
/// Panics if the timestamp cannot be converted to nanoseconds (should never happen with valid timestamps).
#[must_use]
pub fn parse_optional_datetime_to_unix_nanos(
    value: &Option<DateTime<Utc>>,
    field: &str,
) -> UnixNanos {
    value
        .map(|dt| {
            UnixNanos::from(
                dt.timestamp_nanos_opt()
                    .unwrap_or_else(|| panic!("Invalid timestamp for `{field}`"))
                    as u64,
            )
        })
        .unwrap_or_default()
}

#[must_use]
pub const fn parse_aggressor_side(
    side: &Option<BitmexSide>,
) -> nautilus_model::enums::AggressorSide {
    match side {
        Some(BitmexSide::Buy) => nautilus_model::enums::AggressorSide::Buyer,
        Some(BitmexSide::Sell) => nautilus_model::enums::AggressorSide::Seller,
        None => nautilus_model::enums::AggressorSide::NoAggressor,
    }
}

#[must_use]
pub fn parse_liquidity_side(
    liquidity: &Option<BitmexLiquidityIndicator>,
) -> nautilus_model::enums::LiquiditySide {
    liquidity
        .map(std::convert::Into::into)
        .unwrap_or(nautilus_model::enums::LiquiditySide::NoLiquiditySide)
}

#[must_use]
pub const fn parse_position_side(current_qty: Option<i64>) -> PositionSide {
    match current_qty {
        Some(qty) if qty > 0 => PositionSide::Long,
        Some(qty) if qty < 0 => PositionSide::Short,
        _ => PositionSide::Flat,
    }
}

/// Parse a BitMEX time in force into a Nautilus time in force.
///
/// # Panics
///
/// Panics if an unsupported `TimeInForce` variant is encountered.
#[must_use]
pub fn parse_time_in_force(tif: &BitmexTimeInForce) -> nautilus_model::enums::TimeInForce {
    (*tif).into()
}

#[must_use]
pub fn parse_order_type(order_type: &BitmexOrderType) -> nautilus_model::enums::OrderType {
    (*order_type).into()
}

#[must_use]
pub fn parse_order_status(order_status: &BitmexOrderStatus) -> nautilus_model::enums::OrderStatus {
    (*order_status).into()
}

#[must_use]
pub fn parse_contingency_type(
    contingency_type: &BitmexContingencyType,
) -> nautilus_model::enums::ContingencyType {
    (*contingency_type).into()
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
    // BitMEX uses "XBt" but we need "XBT" for Currency
    // BitMEX uses "USDt" but we need "USDT" for Currency
    let currency_str = if margin.currency == "XBt" {
        "XBT"
    } else if margin.currency == "USDt" {
        "USDT"
    } else {
        &margin.currency
    };
    let currency = Currency::from(currency_str);

    // BitMEX returns values in satoshis for BTC (XBt) or cents for USD
    // We need to convert to the actual value
    let divisor = if margin.currency == "XBt" {
        100_000_000.0 // Satoshis to BTC
    } else if margin.currency == "USDt" {
        1_000_000.0 // Microunits to units
    } else {
        1.0
    };

    // Calculate total balance from wallet balance
    let total = if let Some(wallet_balance) = margin.wallet_balance {
        Money::new(wallet_balance as f64 / divisor, currency)
    } else {
        Money::new(0.0, currency)
    };

    // Calculate free balance from available margin
    let free = if let Some(available_margin) = margin.available_margin {
        Money::new(available_margin as f64 / divisor, currency)
    } else {
        Money::new(0.0, currency)
    };

    // Calculate locked balance as the difference
    let locked = total - free;

    let balance = AccountBalance::new(total, locked, free);
    let balances = vec![balance];
    let margins = vec![]; // BitMEX margin info is already in the balances

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

    use super::*;

    #[rstest]
    fn test_parse_account_state() {
        let margin_msg = BitmexMarginMsg {
            account: 123456,
            currency: "XBt".to_string(),
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
        assert_eq!(account_state.margins.len(), 0);
        assert!(account_state.is_reported);

        // Check XBT balance (converted from satoshis)
        let xbt_balance = &account_state.balances[0];
        assert_eq!(xbt_balance.currency, Currency::from("XBT"));
        assert_eq!(xbt_balance.total.as_f64(), 0.05); // 5000000 satoshis = 0.05 XBT
        assert_eq!(xbt_balance.free.as_f64(), 0.0498); // 4980000 satoshis = 0.0498 XBT
        assert_eq!(xbt_balance.locked.as_f64(), 0.0002); // difference
    }

    #[rstest]
    fn test_parse_account_state_usdt() {
        let margin_msg = BitmexMarginMsg {
            account: 123456,
            currency: "USDt".to_string(),
            risk_limit: Some(1000000000),
            amount: Some(10000000000), // 10000 USDT in microunits
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

        // Check USDT balance (converted from microunits)
        let usdt_balance = &account_state.balances[0];
        assert_eq!(usdt_balance.currency, Currency::USDT());
        assert_eq!(usdt_balance.total.as_f64(), 10000.0);
        assert_eq!(usdt_balance.free.as_f64(), 9500.0);
        assert_eq!(usdt_balance.locked.as_f64(), 500.0);
    }
}
