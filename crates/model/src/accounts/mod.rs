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

//! Account types such as `CashAccount` and `MarginAccount`.

#[macro_use]
mod macros;

pub mod any;
pub mod base;
pub mod betting;
pub mod cash;
pub mod margin;
pub mod margin_model;
pub mod wallet;

#[cfg(any(test, feature = "test-support"))]
pub mod stubs;

use enum_dispatch::enum_dispatch;
use indexmap::IndexMap;
use nautilus_core::UnixNanos;

// Re-exports
pub use crate::accounts::{
    any::AccountAny, base::BaseAccount, betting::BettingAccount, cash::CashAccount,
    margin::MarginAccount, wallet::WalletAccount,
};
use crate::{
    enums::{AccountType, LiquiditySide, OrderSide},
    events::{AccountState, OrderFilled},
    fees::MakerTakerFeeRates,
    identifiers::AccountId,
    instruments::InstrumentAny,
    position::{Position, mul_rounded, price, qty},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

#[enum_dispatch]
pub trait Account: 'static + Send {
    fn id(&self) -> AccountId;
    fn account_type(&self) -> AccountType;
    fn base_currency(&self) -> Option<Currency>;
    fn is_cash_account(&self) -> bool;
    fn is_margin_account(&self) -> bool;
    fn calculated_account_state(&self) -> bool;
    fn balance_total(&self, currency: Option<Currency>) -> Option<Money>;
    fn balances_total(&self) -> IndexMap<Currency, Money>;
    fn balance_free(&self, currency: Option<Currency>) -> Option<Money>;
    fn balances_free(&self) -> IndexMap<Currency, Money>;
    fn balance_locked(&self, currency: Option<Currency>) -> Option<Money>;
    fn balances_locked(&self) -> IndexMap<Currency, Money>;
    fn balance(&self, currency: Option<Currency>) -> Option<&AccountBalance>;
    fn last_event(&self) -> Option<AccountState>;
    fn events(&self) -> Vec<AccountState>;
    fn event_count(&self) -> usize;
    fn currencies(&self) -> Vec<Currency>;
    fn starting_balances(&self) -> IndexMap<Currency, Money>;
    fn balances(&self) -> IndexMap<Currency, AccountBalance>;
    /// Applies an account state event to update the account.
    ///
    /// Implementations reject the event before mutating any state, so a rejected event leaves
    /// the account unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error if `event.account_id` does not match this account's ID, or if the
    /// account state cannot be applied (e.g., negative balance when borrowing is not allowed
    /// for a cash account).
    fn apply(&mut self, event: AccountState) -> anyhow::Result<()>;
    fn purge_account_events(&mut self, ts_now: UnixNanos, lookback_secs: u64);

    /// Calculates locked balance for the order parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if calculating locked balance fails.
    fn calculate_balance_locked(
        &self,
        instrument: &InstrumentAny,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money>;

    /// Calculates PnLs for the fill and position.
    ///
    /// A supplied position represents its state before the fill.
    ///
    /// # Errors
    ///
    /// Returns an error if calculating PnLs fails.
    fn calculate_pnls(
        &self,
        instrument: &InstrumentAny,
        fill: &OrderFilled,
        position: Option<Position>,
    ) -> anyhow::Result<Vec<Money>>;

    /// Calculates commission for the order fill parameters from explicitly resolved fee rates.
    ///
    /// # Errors
    ///
    /// Returns an error if calculating commission fails.
    fn calculate_commission(
        &self,
        instrument: &InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        fee_rates: MakerTakerFeeRates,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money>;
}

fn fill_position_pnl(
    instrument: &InstrumentAny,
    fill: &OrderFilled,
    position: Option<&Position>,
) -> anyhow::Result<Option<Money>> {
    let Some(position) = position else {
        if fill.position_id.is_some() {
            let position = Position::new_checked(instrument, fill.clone())?;
            return Ok(position.realized_pnl);
        }

        mul_rounded(price(fill.last_px)?, qty(fill.last_qty)?)?;
        return Ok(None);
    };

    let mut position = position.clone_without_events();
    let prior = if position.is_closed() {
        Money::zero(position.settlement_currency)
    } else {
        position
            .realized_pnl
            .unwrap_or(Money::zero(position.settlement_currency))
    };

    let mut fill = fill.clone();
    fill.position_id.get_or_insert(position.id);

    let opening_pnl = if !position.is_closed()
        && position.is_opposite_side(fill.order_side)
        && fill.last_qty > position.quantity
    {
        let (closing, opening) =
            fill.split_for_position_flip(position.quantity, fill.position_id, fill.event_id)?;
        let opening = Position::new_checked(instrument, opening)?;
        position.try_apply(&closing)?;
        opening
            .realized_pnl
            .unwrap_or(Money::zero(position.settlement_currency))
    } else {
        position.try_apply(&fill)?;
        Money::zero(position.settlement_currency)
    };

    let pnl = position
        .realized_pnl
        .unwrap_or(Money::zero(position.settlement_currency));
    let increment = pnl
        .checked_sub(prior)
        .and_then(|pnl| pnl.checked_add(opening_pnl))
        .ok_or_else(|| anyhow::anyhow!("position net PnL increment overflow"))?;
    Ok(Some(increment))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{Account, AccountAny, CashAccount, MarginAccount, WalletAccount};
    use crate::{
        enums::OrderSide,
        events::{
            OrderFilled,
            account::stubs::{cash_account_state, margin_account_state, wallet_account_state},
            order::spec::OrderFilledSpec,
        },
        identifiers::{ClientOrderId, PositionId, TradeId},
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        position::Position,
        types::{Currency, Money, Price, Quantity},
    };

    #[cfg(feature = "high-precision")]
    #[rstest]
    #[case::cash(CashAccount::new(cash_account_state(), true, false).into(), vec!["-0.0152415787532388 USD"])]
    #[case::margin(MarginAccount::new(margin_account_state(), true).into(), vec![])]
    #[case::wallet(WalletAccount::new(wallet_account_state(), true).into(), vec!["0.1234567890123456 AUD", "-0.0152415787532388 USD"])]
    fn test_calculate_pnls_accepts_bounded_opening_product(
        #[case] account: AccountAny,
        #[case] expected: Vec<&str>,
        #[values(false, true)] position_id: bool,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let mut fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .last_qty(Quantity::from("0.1234567890123456"))
            .last_px(Price::from("0.1234567890123456"))
            .commission(Money::from("0 USD"))
            .build();
        fill.position_id = position_id.then(|| PositionId::from("P-BOUNDED-PRODUCT"));

        let result = account.calculate_pnls(&instrument, &fill, None).unwrap();
        let expected: Vec<_> = expected
            .into_iter()
            .map(|value| Money::from_str_exact(value).unwrap())
            .collect();

        assert_eq!(result, expected);
    }

    #[rstest]
    #[case::cash(CashAccount::new(cash_account_state(), true, false).into(), vec!["18 USD"], vec!["7.20 USD"])]
    #[case::margin(MarginAccount::new(margin_account_state(), true).into(), vec!["11.40 USD"], vec!["0 USD"])]
    #[case::wallet(WalletAccount::new(wallet_account_state(), true).into(), vec!["-9 AUD", "18 USD"], vec!["-10 AUD", "7.20 USD"])]
    fn test_calculate_pnls_accepts_break_even_residual(
        #[case] account: AccountAny,
        #[case] expected: Vec<&str>,
        #[case] expected_at_average: Vec<&str>,
        #[values(false, true)] close_at_average: bool,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-BREAK-EVEN"))
            .last_qty(Quantity::from(5))
            .last_px(Price::from("1.00000"))
            .commission(Money::zero(Currency::USD()))
            .build();
        let mut position = Position::new(&instrument, opening);

        let mut fills = vec![
            ("T-ADD", OrderSide::Buy, 10, "0.60000"),
            ("T-REDUCE-1", OrderSide::Sell, 1, "0.70000"),
            ("T-REDUCE-2", OrderSide::Sell, 5, "0.74000"),
        ];

        if close_at_average {
            fills[1].3 = "10.00000";
            fills[2].3 = "1.00000";
            fills.push(("T-ADD-2", OrderSide::Buy, 1, "0.60000"));
        }

        for (trade_id, side, qty, px) in fills {
            let fill = OrderFilledSpec::builder()
                .instrument_id(instrument.id())
                .position_id(position.id)
                .trade_id(TradeId::from(trade_id))
                .order_side(side)
                .last_qty(Quantity::from(qty))
                .last_px(Price::from(px))
                .commission(Money::zero(Currency::USD()))
                .build();
            position.try_apply(&fill).unwrap();
        }

        let (closing_qty, closing_px, expected) = if close_at_average {
            (10, "0.72000", expected_at_average)
        } else {
            (9, "2.00000", expected)
        };

        let closing = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position.id)
            .trade_id(TradeId::from("T-CLOSE"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(closing_qty))
            .last_px(Price::from(closing_px))
            .commission(Money::zero(Currency::USD()))
            .build();
        let result = account
            .calculate_pnls(&instrument, &closing, Some(position))
            .unwrap();
        let expected: Vec<_> = expected.into_iter().map(Money::from).collect();

        assert_eq!(result, expected);
    }

    #[rstest]
    #[case::cash(CashAccount::new(cash_account_state(), true, false).into())]
    #[case::margin(MarginAccount::new(margin_account_state(), true).into())]
    #[case::wallet(WalletAccount::new(wallet_account_state(), true).into())]
    fn test_calculate_pnls_reopens_closed_position(
        #[case] account: AccountAny,
        #[values(OrderSide::Buy, OrderSide::Sell)] entry: OrderSide,
    ) {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());

        let opposite = if entry == OrderSide::Buy {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        };

        let opening = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-REOPEN"))
            .client_order_id(ClientOrderId::from("O-OPEN"))
            .trade_id(TradeId::from("T-OPEN"))
            .order_side(entry)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("1"))
            .commission(Money::from("1 USD"))
            .build();
        let mut position = Position::new(&instrument, opening.clone());

        let closing = OrderFilled {
            client_order_id: ClientOrderId::from("O-CLOSE"),
            trade_id: TradeId::from("T-CLOSE"),
            order_side: opposite,
            last_px: Price::from("2"),
            ..opening.clone()
        };

        position.try_apply(&closing).unwrap();
        assert!(position.is_closed());

        let reopening = OrderFilled {
            client_order_id: ClientOrderId::from("O-REOPEN"),
            trade_id: TradeId::from("T-REOPEN"),
            order_side: opposite,
            last_qty: Quantity::from(3),
            last_px: Price::from("4"),
            commission: Some(Money::from("3 USD")),
            ..opening
        };

        let mut expected = if account.is_margin_account() {
            Vec::new()
        } else if opposite == OrderSide::Buy {
            vec![Money::from("-12 USD")]
        } else {
            vec![Money::from("12 USD")]
        };

        if account.base_currency().is_none() {
            expected.insert(
                0,
                if opposite == OrderSide::Buy {
                    Money::from("3 AUD")
                } else {
                    Money::from("-3 AUD")
                },
            );
        }

        let pnls = account
            .calculate_pnls(&instrument, &reopening, Some(position))
            .unwrap();

        assert_eq!(pnls, expected);
    }
}
