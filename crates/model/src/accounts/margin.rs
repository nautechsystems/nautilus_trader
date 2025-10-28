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

//! Implementation of a *margin* account capable of holding leveraged positions and tracking
//! instrument-specific leverage ratios.

#![allow(dead_code)]

use std::{
    collections::HashMap,
    fmt::Display,
    hash::{Hash, Hasher},
    ops::{Deref, DerefMut},
};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    accounts::{Account, base::BaseAccount},
    enums::{AccountType, LiquiditySide, OrderSide},
    events::{AccountState, OrderFilled},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    position::Position,
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity, money::MoneyRaw},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct MarginAccount {
    pub base: BaseAccount,
    pub leverages: HashMap<InstrumentId, Decimal>,
    pub margins: HashMap<InstrumentId, MarginBalance>,
    pub default_leverage: Decimal,
}

impl MarginAccount {
    /// Creates a new [`MarginAccount`] instance.
    pub fn new(event: AccountState, calculate_account_state: bool) -> Self {
        Self {
            base: BaseAccount::new(event, calculate_account_state),
            leverages: HashMap::new(),
            margins: HashMap::new(),
            default_leverage: Decimal::ONE,
        }
    }

    pub fn set_default_leverage(&mut self, leverage: Decimal) {
        self.default_leverage = leverage;
    }

    pub fn set_leverage(&mut self, instrument_id: InstrumentId, leverage: Decimal) {
        self.leverages.insert(instrument_id, leverage);
    }

    #[must_use]
    pub fn get_leverage(&self, instrument_id: &InstrumentId) -> Decimal {
        *self
            .leverages
            .get(instrument_id)
            .unwrap_or(&self.default_leverage)
    }

    #[must_use]
    pub fn is_unleveraged(&self, instrument_id: InstrumentId) -> bool {
        self.get_leverage(&instrument_id) == Decimal::ONE
    }

    #[must_use]
    pub fn is_cash_account(&self) -> bool {
        self.account_type == AccountType::Cash
    }
    #[must_use]
    pub fn is_margin_account(&self) -> bool {
        self.account_type == AccountType::Margin
    }

    #[must_use]
    pub fn initial_margins(&self) -> HashMap<InstrumentId, Money> {
        let mut initial_margins: HashMap<InstrumentId, Money> = HashMap::new();
        self.margins.values().for_each(|margin_balance| {
            initial_margins.insert(margin_balance.instrument_id, margin_balance.initial);
        });
        initial_margins
    }

    #[must_use]
    pub fn maintenance_margins(&self) -> HashMap<InstrumentId, Money> {
        let mut maintenance_margins: HashMap<InstrumentId, Money> = HashMap::new();
        self.margins.values().for_each(|margin_balance| {
            maintenance_margins.insert(margin_balance.instrument_id, margin_balance.maintenance);
        });
        maintenance_margins
    }

    /// Updates the initial margin for the specified instrument.
    ///
    /// # Panics
    ///
    /// Panics if an existing margin balance is found but cannot be unwrapped.
    pub fn update_initial_margin(&mut self, instrument_id: InstrumentId, margin_init: Money) {
        let margin_balance = self.margins.get(&instrument_id);
        if let Some(balance) = margin_balance {
            // update the margin_balance initial property with margin_init
            let mut new_margin_balance = *balance;
            new_margin_balance.initial = margin_init;
            self.margins.insert(instrument_id, new_margin_balance);
        } else {
            self.margins.insert(
                instrument_id,
                MarginBalance::new(
                    margin_init,
                    Money::new(0.0, margin_init.currency),
                    instrument_id,
                ),
            );
        }
        self.recalculate_balance(margin_init.currency);
    }

    /// Returns the initial margin amount for the specified instrument.
    ///
    /// # Panics
    ///
    /// Panics if no margin balance exists for the given `instrument_id`.
    #[must_use]
    pub fn initial_margin(&self, instrument_id: InstrumentId) -> Money {
        let margin_balance = self.margins.get(&instrument_id);
        assert!(
            margin_balance.is_some(),
            "Cannot get margin_init when no margin_balance"
        );
        margin_balance.unwrap().initial
    }

    /// Updates the maintenance margin for the specified instrument.
    ///
    /// # Panics
    ///
    /// Panics if an existing margin balance is found but cannot be unwrapped.
    pub fn update_maintenance_margin(
        &mut self,
        instrument_id: InstrumentId,
        margin_maintenance: Money,
    ) {
        let margin_balance = self.margins.get(&instrument_id);
        if let Some(balance) = margin_balance {
            // update the margin_balance maintenance property with margin_maintenance
            let mut new_margin_balance = *balance;
            new_margin_balance.maintenance = margin_maintenance;
            self.margins.insert(instrument_id, new_margin_balance);
        } else {
            self.margins.insert(
                instrument_id,
                MarginBalance::new(
                    Money::new(0.0, margin_maintenance.currency),
                    margin_maintenance,
                    instrument_id,
                ),
            );
        }
        self.recalculate_balance(margin_maintenance.currency);
    }

    /// Returns the maintenance margin amount for the specified instrument.
    ///
    /// # Panics
    ///
    /// Panics if no margin balance exists for the given `instrument_id`.
    #[must_use]
    pub fn maintenance_margin(&self, instrument_id: InstrumentId) -> Money {
        let margin_balance = self.margins.get(&instrument_id);
        assert!(
            margin_balance.is_some(),
            "Cannot get maintenance_margin when no margin_balance"
        );
        margin_balance.unwrap().maintenance
    }

    /// Calculates the initial margin amount for the specified instrument and quantity.
    ///
    /// # Errors
    ///
    /// Returns an error if the margin calculation produces a value that cannot be represented as `Money`.
    ///
    /// # Panics
    ///
    /// Panics if `instrument.base_currency()` is `None` for inverse instruments.
    pub fn calculate_initial_margin<T: Instrument>(
        &mut self,
        instrument: T,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        let notional = instrument.calculate_notional_value(quantity, price, use_quote_for_inverse);
        let mut leverage = self.get_leverage(&instrument.id());
        if leverage == Decimal::ZERO {
            self.leverages
                .insert(instrument.id(), self.default_leverage);
            leverage = self.default_leverage;
        }
        let notional_decimal = notional.as_decimal();
        let adjusted_notional = notional_decimal / leverage;
        let margin_decimal = adjusted_notional * instrument.margin_init();

        let use_quote_for_inverse = use_quote_for_inverse.unwrap_or(false);
        let currency = if instrument.is_inverse() && !use_quote_for_inverse {
            instrument.base_currency().unwrap()
        } else {
            instrument.quote_currency()
        };

        Money::from_decimal(margin_decimal, currency)
    }

    /// Calculates the maintenance margin amount for the specified instrument and quantity.
    ///
    /// # Errors
    ///
    /// Returns an error if the margin calculation produces a value that cannot be represented as `Money`.
    ///
    /// # Panics
    ///
    /// Panics if `instrument.base_currency()` is `None` for inverse instruments.
    pub fn calculate_maintenance_margin<T: Instrument>(
        &mut self,
        instrument: T,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        let notional = instrument.calculate_notional_value(quantity, price, use_quote_for_inverse);
        let mut leverage = self.get_leverage(&instrument.id());
        if leverage == Decimal::ZERO {
            self.leverages
                .insert(instrument.id(), self.default_leverage);
            leverage = self.default_leverage;
        }
        let notional_decimal = notional.as_decimal();
        let adjusted_notional = notional_decimal / leverage;
        let margin_decimal = adjusted_notional * instrument.margin_maint();

        let use_quote_for_inverse = use_quote_for_inverse.unwrap_or(false);
        let currency = if instrument.is_inverse() && !use_quote_for_inverse {
            instrument.base_currency().unwrap()
        } else {
            instrument.quote_currency()
        };

        Money::from_decimal(margin_decimal, currency)
    }

    /// Recalculates the account balance for the specified currency based on current margins.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - No starting balance exists for the given `currency`.
    /// - Total free margin would be negative.
    /// - Margin calculation overflows.
    pub fn recalculate_balance(&mut self, currency: Currency) {
        let current_balance = match self.balances.get(&currency) {
            Some(balance) => balance,
            None => panic!("Cannot recalculate balance when no starting balance"),
        };

        let mut total_margin: MoneyRaw = 0;
        for margin in self.margins.values() {
            if margin.currency == currency {
                total_margin = total_margin
                    .checked_add(margin.initial.raw)
                    .and_then(|sum| sum.checked_add(margin.maintenance.raw))
                    .unwrap_or_else(|| {
                        panic!(
                            "Margin calculation overflow for currency {}: total would exceed maximum",
                            currency.code
                        )
                    });
            }
        }

        let total_free = current_balance.total.raw - total_margin;
        // TODO error handle this with AccountMarginExceeded
        assert!(
            total_free >= 0,
            "Cannot recalculate balance when total_free is less than 0.0"
        );
        let new_balance = AccountBalance::new(
            current_balance.total,
            Money::from_raw(total_margin, currency),
            Money::from_raw(total_free, currency),
        );
        self.balances.insert(currency, new_balance);
    }
}

impl Deref for MarginAccount {
    type Target = BaseAccount;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for MarginAccount {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl Account for MarginAccount {
    fn id(&self) -> AccountId {
        self.id
    }

    fn account_type(&self) -> AccountType {
        self.account_type
    }

    fn base_currency(&self) -> Option<Currency> {
        self.base_currency
    }

    fn is_cash_account(&self) -> bool {
        self.account_type == AccountType::Cash
    }

    fn is_margin_account(&self) -> bool {
        self.account_type == AccountType::Margin
    }

    fn calculated_account_state(&self) -> bool {
        false // TODO (implement this logic)
    }

    fn balance_total(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance_total(currency)
    }

    fn balances_total(&self) -> HashMap<Currency, Money> {
        self.base_balances_total()
    }

    fn balance_free(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance_free(currency)
    }

    fn balances_free(&self) -> HashMap<Currency, Money> {
        self.base_balances_free()
    }

    fn balance_locked(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance_locked(currency)
    }

    fn balances_locked(&self) -> HashMap<Currency, Money> {
        self.base_balances_locked()
    }

    fn balance(&self, currency: Option<Currency>) -> Option<&AccountBalance> {
        self.base_balance(currency)
    }

    fn last_event(&self) -> Option<AccountState> {
        self.base_last_event()
    }

    fn events(&self) -> Vec<AccountState> {
        self.events.clone()
    }

    fn event_count(&self) -> usize {
        self.events.len()
    }

    fn currencies(&self) -> Vec<Currency> {
        self.balances.keys().copied().collect()
    }

    fn starting_balances(&self) -> HashMap<Currency, Money> {
        self.balances_starting.clone()
    }

    fn balances(&self) -> HashMap<Currency, AccountBalance> {
        self.balances.clone()
    }

    fn apply(&mut self, event: AccountState) {
        self.base_apply(event);
    }

    fn purge_account_events(&mut self, ts_now: nautilus_core::UnixNanos, lookback_secs: u64) {
        self.base.base_purge_account_events(ts_now, lookback_secs);
    }

    fn calculate_balance_locked(
        &mut self,
        instrument: InstrumentAny,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        self.base_calculate_balance_locked(instrument, side, quantity, price, use_quote_for_inverse)
    }

    fn calculate_pnls(
        &self,
        _instrument: InstrumentAny, // TBD if this should be removed
        fill: OrderFilled,
        position: Option<Position>,
    ) -> anyhow::Result<Vec<Money>> {
        let mut pnls: Vec<Money> = Vec::new();

        if let Some(ref pos) = position
            && pos.quantity.is_positive()
            && pos.entry != fill.order_side
        {
            // Calculate and add PnL using the minimum of fill quantity and position quantity
            // to avoid double-limiting that occurs in position.calculate_pnl()
            let pnl_quantity = Quantity::from_raw(
                fill.last_qty.raw.min(pos.quantity.raw),
                fill.last_qty.precision,
            );
            let pnl = pos.calculate_pnl(pos.avg_px_open, fill.last_px.as_f64(), pnl_quantity);
            pnls.push(pnl);
        }

        Ok(pnls)
    }

    fn calculate_commission(
        &self,
        instrument: InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        self.base_calculate_commission(
            instrument,
            last_qty,
            last_px,
            liquidity_side,
            use_quote_for_inverse,
        )
    }
}

impl PartialEq for MarginAccount {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for MarginAccount {}

impl Display for MarginAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MarginAccount(id={}, type={}, base={})",
            self.id,
            self.account_type,
            self.base_currency.map_or_else(
                || "None".to_string(),
                |base_currency| format!("{}", base_currency.code)
            ),
        )
    }
}

impl Hash for MarginAccount {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use nautilus_core::UnixNanos;
    use rstest::rstest;
    use rust_decimal::Decimal;

    use crate::{
        accounts::{Account, MarginAccount, stubs::*},
        enums::{LiquiditySide, OrderSide, OrderType},
        events::{AccountState, OrderFilled, account::stubs::*},
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
            VenueOrderId,
            stubs::{uuid4, *},
        },
        instruments::{CryptoPerpetual, CurrencyPair, InstrumentAny, stubs::*},
        position::Position,
        types::{Currency, Money, Price, Quantity},
    };

    #[rstest]
    fn test_display(margin_account: MarginAccount) {
        assert_eq!(
            margin_account.to_string(),
            "MarginAccount(id=SIM-001, type=MARGIN, base=USD)"
        );
    }

    #[rstest]
    fn test_base_account_properties(
        margin_account: MarginAccount,
        margin_account_state: AccountState,
    ) {
        assert_eq!(margin_account.base_currency, Some(Currency::from("USD")));
        assert_eq!(
            margin_account.last_event(),
            Some(margin_account_state.clone())
        );
        assert_eq!(margin_account.events(), vec![margin_account_state]);
        assert_eq!(margin_account.event_count(), 1);
        assert_eq!(
            margin_account.balance_total(None),
            Some(Money::from("1525000 USD"))
        );
        assert_eq!(
            margin_account.balance_free(None),
            Some(Money::from("1500000 USD"))
        );
        assert_eq!(
            margin_account.balance_locked(None),
            Some(Money::from("25000 USD"))
        );
        let mut balances_total_expected = HashMap::new();
        balances_total_expected.insert(Currency::from("USD"), Money::from("1525000 USD"));
        assert_eq!(margin_account.balances_total(), balances_total_expected);
        let mut balances_free_expected = HashMap::new();
        balances_free_expected.insert(Currency::from("USD"), Money::from("1500000 USD"));
        assert_eq!(margin_account.balances_free(), balances_free_expected);
        let mut balances_locked_expected = HashMap::new();
        balances_locked_expected.insert(Currency::from("USD"), Money::from("25000 USD"));
        assert_eq!(margin_account.balances_locked(), balances_locked_expected);
    }

    #[rstest]
    fn test_set_default_leverage(mut margin_account: MarginAccount) {
        assert_eq!(margin_account.default_leverage, Decimal::ONE);
        margin_account.set_default_leverage(Decimal::from(10));
        assert_eq!(margin_account.default_leverage, Decimal::from(10));
    }

    #[rstest]
    fn test_get_leverage_default_leverage(
        margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        assert_eq!(
            margin_account.get_leverage(&instrument_id_aud_usd_sim),
            Decimal::ONE
        );
    }

    #[rstest]
    fn test_set_leverage(
        mut margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        assert_eq!(margin_account.leverages.len(), 0);
        margin_account.set_leverage(instrument_id_aud_usd_sim, Decimal::from(10));
        assert_eq!(margin_account.leverages.len(), 1);
        assert_eq!(
            margin_account.get_leverage(&instrument_id_aud_usd_sim),
            Decimal::from(10)
        );
    }

    #[rstest]
    fn test_is_unleveraged_with_leverage_returns_false(
        mut margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        margin_account.set_leverage(instrument_id_aud_usd_sim, Decimal::from(10));
        assert!(!margin_account.is_unleveraged(instrument_id_aud_usd_sim));
    }

    #[rstest]
    fn test_is_unleveraged_with_no_leverage_returns_true(
        mut margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        margin_account.set_leverage(instrument_id_aud_usd_sim, Decimal::ONE);
        assert!(margin_account.is_unleveraged(instrument_id_aud_usd_sim));
    }

    #[rstest]
    fn test_is_unleveraged_with_default_leverage_of_1_returns_true(
        margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        assert!(margin_account.is_unleveraged(instrument_id_aud_usd_sim));
    }

    #[rstest]
    fn test_update_margin_init(
        mut margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        assert_eq!(margin_account.margins.len(), 0);
        let margin = Money::from("10000 USD");
        margin_account.update_initial_margin(instrument_id_aud_usd_sim, margin);
        assert_eq!(
            margin_account.initial_margin(instrument_id_aud_usd_sim),
            margin
        );
        let margins: Vec<Money> = margin_account
            .margins
            .values()
            .map(|margin_balance| margin_balance.initial)
            .collect();
        assert_eq!(margins, vec![margin]);
    }

    #[rstest]
    fn test_update_margin_maintenance(
        mut margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        let margin = Money::from("10000 USD");
        margin_account.update_maintenance_margin(instrument_id_aud_usd_sim, margin);
        assert_eq!(
            margin_account.maintenance_margin(instrument_id_aud_usd_sim),
            margin
        );
        let margins: Vec<Money> = margin_account
            .margins
            .values()
            .map(|margin_balance| margin_balance.maintenance)
            .collect();
        assert_eq!(margins, vec![margin]);
    }

    #[rstest]
    fn test_calculate_margin_init_with_leverage(
        mut margin_account: MarginAccount,
        audusd_sim: CurrencyPair,
    ) {
        margin_account.set_leverage(audusd_sim.id, Decimal::from(50));
        let result = margin_account
            .calculate_initial_margin(
                audusd_sim,
                Quantity::from(100_000),
                Price::from("0.8000"),
                None,
            )
            .unwrap();
        assert_eq!(result, Money::from("48.00 USD"));
    }

    #[rstest]
    fn test_calculate_margin_init_with_default_leverage(
        mut margin_account: MarginAccount,
        audusd_sim: CurrencyPair,
    ) {
        margin_account.set_default_leverage(Decimal::from(10));
        let result = margin_account
            .calculate_initial_margin(
                audusd_sim,
                Quantity::from(100_000),
                Price::from("0.8"),
                None,
            )
            .unwrap();
        assert_eq!(result, Money::from("240.00 USD"));
    }

    #[rstest]
    fn test_calculate_margin_init_with_no_leverage_for_inverse(
        mut margin_account: MarginAccount,
        xbtusd_bitmex: CryptoPerpetual,
    ) {
        let result_use_quote_inverse_true = margin_account
            .calculate_initial_margin(
                xbtusd_bitmex,
                Quantity::from(100_000),
                Price::from("11493.60"),
                Some(false),
            )
            .unwrap();
        assert_eq!(result_use_quote_inverse_true, Money::from("0.08700494 BTC"));
        let result_use_quote_inverse_false = margin_account
            .calculate_initial_margin(
                xbtusd_bitmex,
                Quantity::from(100_000),
                Price::from("11493.60"),
                Some(true),
            )
            .unwrap();
        assert_eq!(result_use_quote_inverse_false, Money::from("1000 USD"));
    }

    #[rstest]
    fn test_calculate_margin_maintenance_with_no_leverage(
        mut margin_account: MarginAccount,
        xbtusd_bitmex: CryptoPerpetual,
    ) {
        let result = margin_account
            .calculate_maintenance_margin(
                xbtusd_bitmex,
                Quantity::from(100_000),
                Price::from("11493.60"),
                None,
            )
            .unwrap();
        assert_eq!(result, Money::from("0.03045173 BTC"));
    }

    #[rstest]
    fn test_calculate_margin_maintenance_with_leverage_fx_instrument(
        mut margin_account: MarginAccount,
        audusd_sim: CurrencyPair,
    ) {
        margin_account.set_default_leverage(Decimal::from(50));
        let result = margin_account
            .calculate_maintenance_margin(
                audusd_sim,
                Quantity::from(1_000_000),
                Price::from("1"),
                None,
            )
            .unwrap();
        assert_eq!(result, Money::from("600.00 USD"));
    }

    #[rstest]
    fn test_calculate_margin_maintenance_with_leverage_inverse_instrument(
        mut margin_account: MarginAccount,
        xbtusd_bitmex: CryptoPerpetual,
    ) {
        margin_account.set_default_leverage(Decimal::from(10));
        let result = margin_account
            .calculate_maintenance_margin(
                xbtusd_bitmex,
                Quantity::from(100_000),
                Price::from("100000.00"),
                None,
            )
            .unwrap();
        assert_eq!(result, Money::from("0.00035000 BTC"));
    }

    #[rstest]
    fn test_calculate_pnls_github_issue_2657() {
        // Create a margin account
        let account_state = margin_account_state();
        let account = MarginAccount::new(account_state, false);

        // Create BTCUSDT instrument
        let btcusdt = currency_pair_btcusdt();
        let btcusdt_any = InstrumentAny::CurrencyPair(btcusdt);

        // Create initial position with BUY 0.001 BTC at 50000.00
        let fill1 = OrderFilled::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            btcusdt.id,
            ClientOrderId::from("O-1"),
            VenueOrderId::from("V-1"),
            AccountId::from("SIM-001"),
            TradeId::from("T-1"),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("0.001"),
            Price::from("50000.00"),
            btcusdt.quote_currency,
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::default(),
            false,
            Some(PositionId::from("P-GITHUB-2657")),
            None,
        );

        let position = Position::new(&btcusdt_any, fill1);

        // Create second fill that sells MORE than position size (0.002 > 0.001)
        let fill2 = OrderFilled::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            btcusdt.id,
            ClientOrderId::from("O-2"),
            VenueOrderId::from("V-2"),
            AccountId::from("SIM-001"),
            TradeId::from("T-2"),
            OrderSide::Sell,
            OrderType::Market,
            Quantity::from("0.002"), // This is larger than position quantity!
            Price::from("50075.00"),
            btcusdt.quote_currency,
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::from(2_000_000_000),
            UnixNanos::default(),
            false,
            Some(PositionId::from("P-GITHUB-2657")),
            None,
        );

        // Test the fix - should only calculate PnL for position quantity (0.001), not fill quantity (0.002)
        let pnls = account
            .calculate_pnls(btcusdt_any, fill2, Some(position))
            .unwrap();

        // Should have exactly one PnL entry
        assert_eq!(pnls.len(), 1);

        // Expected PnL should be for 0.001 BTC, not 0.002 BTC
        // PnL = (50075.00 - 50000.00) * 0.001 = 75.0 * 0.001 = 0.075 USDT
        let expected_pnl = Money::from("0.075 USDT");
        assert_eq!(pnls[0], expected_pnl);
    }

    #[rstest]
    fn test_calculate_initial_margin_with_zero_leverage_falls_back_to_default(
        mut margin_account: MarginAccount,
        audusd_sim: CurrencyPair,
    ) {
        // Set default leverage
        margin_account.set_default_leverage(Decimal::from(10));

        // Set instrument-specific leverage to 0.0 (invalid)
        margin_account.set_leverage(audusd_sim.id, Decimal::ZERO);

        // Should not panic, should use default leverage instead
        let result = margin_account
            .calculate_initial_margin(
                audusd_sim,
                Quantity::from(100_000),
                Price::from("0.8"),
                None,
            )
            .unwrap();

        // With default leverage of 10.0, notional of 80,000 / 10 = 8,000
        // Initial margin rate is 0.03, so 8,000 * 0.03 = 240.00
        assert_eq!(result, Money::from("240.00 USD"));

        // Verify that the hashmap was updated with default leverage
        assert_eq!(
            margin_account.get_leverage(&audusd_sim.id),
            Decimal::from(10)
        );
    }

    #[rstest]
    fn test_calculate_maintenance_margin_with_zero_leverage_falls_back_to_default(
        mut margin_account: MarginAccount,
        audusd_sim: CurrencyPair,
    ) {
        // Set default leverage
        margin_account.set_default_leverage(Decimal::from(50));

        // Set instrument-specific leverage to 0.0 (invalid)
        margin_account.set_leverage(audusd_sim.id, Decimal::ZERO);

        // Should not panic, should use default leverage instead
        let result = margin_account
            .calculate_maintenance_margin(
                audusd_sim,
                Quantity::from(1_000_000),
                Price::from("1"),
                None,
            )
            .unwrap();

        // With default leverage of 50.0, notional of 1,000,000 / 50 = 20,000
        // Maintenance margin rate is 0.03, so 20,000 * 0.03 = 600.00
        assert_eq!(result, Money::from("600.00 USD"));

        // Verify that the hashmap was updated with default leverage
        assert_eq!(
            margin_account.get_leverage(&audusd_sim.id),
            Decimal::from(50)
        );
    }

    #[rstest]
    fn test_calculate_pnls_with_same_side_fill_returns_empty() {
        use nautilus_core::UnixNanos;

        use crate::{
            enums::{LiquiditySide, OrderSide, OrderType},
            events::OrderFilled,
            identifiers::{
                AccountId, ClientOrderId, PositionId, StrategyId, TradeId, TraderId, VenueOrderId,
                stubs::uuid4,
            },
            instruments::InstrumentAny,
            position::Position,
            types::{Price, Quantity},
        };

        // Create a margin account
        let account_state = margin_account_state();
        let account = MarginAccount::new(account_state, false);

        // Create BTCUSDT instrument
        let btcusdt = currency_pair_btcusdt();
        let btcusdt_any = InstrumentAny::CurrencyPair(btcusdt);

        // Create initial position with BUY 1.0 BTC at 50000.00
        let fill1 = OrderFilled::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            btcusdt.id,
            ClientOrderId::from("O-1"),
            VenueOrderId::from("V-1"),
            AccountId::from("SIM-001"),
            TradeId::from("T-1"),
            OrderSide::Buy,
            OrderType::Market,
            Quantity::from("1.0"),
            Price::from("50000.00"),
            btcusdt.quote_currency,
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::from(1_000_000_000),
            UnixNanos::default(),
            false,
            Some(PositionId::from("P-123456")),
            None,
        );

        let position = Position::new(&btcusdt_any, fill1);

        // Create second fill that also BUYS (same side as position entry)
        let fill2 = OrderFilled::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            btcusdt.id,
            ClientOrderId::from("O-2"),
            VenueOrderId::from("V-2"),
            AccountId::from("SIM-001"),
            TradeId::from("T-2"),
            OrderSide::Buy, // Same side as position entry
            OrderType::Market,
            Quantity::from("0.5"),
            Price::from("51000.00"),
            btcusdt.quote_currency,
            LiquiditySide::Taker,
            uuid4(),
            UnixNanos::from(2_000_000_000),
            UnixNanos::default(),
            false,
            Some(PositionId::from("P-123456")),
            None,
        );

        // Test that no PnL is calculated for same-side fills
        let pnls = account
            .calculate_pnls(btcusdt_any, fill2, Some(position))
            .unwrap();

        // Should return empty PnL list
        assert_eq!(pnls.len(), 0);
    }
}
