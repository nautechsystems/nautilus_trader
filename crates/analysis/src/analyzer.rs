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

use std::{collections::BTreeMap, fmt::Debug, sync::Arc};

use ahash::AHashMap;
use indexmap::IndexMap;
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_DAY};
use nautilus_model::{
    accounts::Account,
    identifiers::PositionId,
    position::Position,
    types::{Currency, Money},
};
use rust_decimal::Decimal;

use crate::{
    Returns,
    statistic::PortfolioStatistic,
    statistics::{
        expectancy::Expectancy, long_ratio::LongRatio, loser_avg::AvgLoser, loser_max::MaxLoser,
        loser_min::MinLoser, profit_factor::ProfitFactor, returns_avg::ReturnsAverage,
        returns_avg_loss::ReturnsAverageLoss, returns_avg_win::ReturnsAverageWin,
        returns_volatility::ReturnsVolatility, risk_return_ratio::RiskReturnRatio,
        sharpe_ratio::SharpeRatio, sortino_ratio::SortinoRatio, win_rate::WinRate,
        winner_avg::AvgWinner, winner_max::MaxWinner, winner_min::MinWinner,
    },
};

pub type Statistic = Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync>;

/// Analyzes portfolio performance and calculates various statistics.
///
/// The `PortfolioAnalyzer` tracks account balances, positions, and realized PnLs
/// to provide portfolio analysis including returns, PnL calculations,
/// and customizable statistics.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.analysis")
)]
pub struct PortfolioAnalyzer {
    pub statistics: AHashMap<String, Statistic>,
    pub account_balances_starting: IndexMap<Currency, Money>,
    pub account_balances: IndexMap<Currency, Money>,
    pub positions: Vec<Position>,
    pub realized_pnls: AHashMap<Currency, Vec<(PositionId, f64)>>,
    pub position_returns: Returns,
    pub portfolio_returns: Returns,
    /// Alias for the primary returns source.
    ///
    /// Contains portfolio returns when available, otherwise position returns.
    /// Kept as a public field for API stability; prefer the `returns()` accessor.
    pub returns: Returns,
}

impl Default for PortfolioAnalyzer {
    /// Creates a new default [`PortfolioAnalyzer`] instance.
    fn default() -> Self {
        let mut analyzer = Self::new();
        analyzer.register_statistic(Arc::new(MaxWinner {}));
        analyzer.register_statistic(Arc::new(AvgWinner {}));
        analyzer.register_statistic(Arc::new(MinWinner {}));
        analyzer.register_statistic(Arc::new(MinLoser {}));
        analyzer.register_statistic(Arc::new(AvgLoser {}));
        analyzer.register_statistic(Arc::new(MaxLoser {}));
        analyzer.register_statistic(Arc::new(Expectancy {}));
        analyzer.register_statistic(Arc::new(WinRate {}));
        analyzer.register_statistic(Arc::new(ReturnsVolatility::new(None)));
        analyzer.register_statistic(Arc::new(ReturnsAverage {}));
        analyzer.register_statistic(Arc::new(ReturnsAverageLoss {}));
        analyzer.register_statistic(Arc::new(ReturnsAverageWin {}));
        analyzer.register_statistic(Arc::new(SharpeRatio::new(None)));
        analyzer.register_statistic(Arc::new(SortinoRatio::new(None)));
        analyzer.register_statistic(Arc::new(ProfitFactor {}));
        analyzer.register_statistic(Arc::new(RiskReturnRatio {}));
        analyzer.register_statistic(Arc::new(LongRatio::new(None)));
        analyzer
    }
}

impl PortfolioAnalyzer {
    /// Creates a new [`PortfolioAnalyzer`] instance.
    ///
    /// Starts with empty state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            statistics: AHashMap::new(),
            account_balances_starting: IndexMap::new(),
            account_balances: IndexMap::new(),
            positions: Vec::new(),
            realized_pnls: AHashMap::new(),
            position_returns: BTreeMap::new(),
            portfolio_returns: BTreeMap::new(),
            returns: BTreeMap::new(),
        }
    }

    /// Registers a new portfolio statistic for calculation.
    pub fn register_statistic(&mut self, statistic: Statistic) {
        self.statistics.insert(statistic.name(), statistic);
    }

    /// Removes a specific statistic from calculation.
    pub fn deregister_statistic(&mut self, statistic: &Statistic) {
        self.statistics.remove(&statistic.name());
    }

    /// Removes all registered statistics.
    pub fn deregister_statistics(&mut self) {
        self.statistics.clear();
    }

    /// Resets all analysis data to initial state.
    pub fn reset(&mut self) {
        self.account_balances_starting.clear();
        self.account_balances.clear();
        self.positions.clear();
        self.realized_pnls.clear();
        self.position_returns.clear();
        self.portfolio_returns.clear();
        self.returns.clear();
    }

    /// Returns all tracked currencies.
    #[must_use]
    pub fn currencies(&self) -> Vec<&Currency> {
        self.account_balances.keys().collect()
    }

    /// Retrieves a specific statistic by name.
    #[must_use]
    pub fn statistic(&self, name: &str) -> Option<&Statistic> {
        self.statistics.get(name)
    }

    /// Returns the primary calculated returns.
    ///
    /// This returns portfolio returns when available, otherwise it falls back
    /// to position returns for backward compatibility.
    #[must_use]
    pub const fn returns(&self) -> &Returns {
        &self.returns
    }

    /// Returns the per-position calculated returns.
    #[must_use]
    pub const fn position_returns(&self) -> &Returns {
        &self.position_returns
    }

    /// Returns the portfolio calculated returns.
    #[must_use]
    pub const fn portfolio_returns(&self) -> &Returns {
        &self.portfolio_returns
    }

    /// Calculates statistics based on account and position data.
    ///
    /// This clears all previous state before calculating, so can be called
    /// multiple times without accumulating stale data.
    pub fn calculate_statistics(&mut self, account: &dyn Account, positions: &[Position]) {
        self.account_balances_starting = account.starting_balances().into_iter().collect();
        self.account_balances = account.balances_total().into_iter().collect();
        self.positions.clear();
        self.realized_pnls.clear();
        self.position_returns.clear();
        self.portfolio_returns.clear();
        self.returns.clear();

        self.add_positions(positions);

        if let Some(account_returns) = Self::calculate_account_returns(account) {
            self.portfolio_returns = account_returns;
            self.sync_returns_alias();
        }
    }

    /// Adds new positions for analysis.
    pub fn add_positions(&mut self, positions: &[Position]) {
        self.positions.extend_from_slice(positions);
        for position in positions {
            if let Some(ref pnl) = position.realized_pnl {
                self.add_trade(&position.id, pnl);
            }

            if let Some(ts_closed) = position.ts_closed
                && ts_closed.as_u64() > 0
                && position.realized_pnl.is_some()
            {
                self.add_position_return(ts_closed, position.realized_return);
            }
        }
    }

    /// Records a trade's PnL.
    pub fn add_trade(&mut self, position_id: &PositionId, pnl: &Money) {
        let currency = pnl.currency;
        let entry = self.realized_pnls.entry(currency).or_default();
        entry.push((*position_id, pnl.as_f64()));
    }

    /// Records a position return at a specific timestamp.
    pub fn add_position_return(&mut self, timestamp: UnixNanos, value: f64) {
        self.position_returns
            .entry(timestamp)
            .and_modify(|existing_value| *existing_value += value)
            .or_insert(value);

        // Mirror writes into the `returns` alias when no portfolio returns exist.
        // This avoids calling `sync_returns_alias` (which clones the full map)
        // on every insert.
        if self.portfolio_returns.is_empty() {
            self.returns
                .entry(timestamp)
                .and_modify(|existing_value| *existing_value += value)
                .or_insert(value);
        }
    }

    /// Records a return at a specific timestamp.
    ///
    /// This is a backward-compatible alias for [`Self::add_position_return`].
    pub fn add_return(&mut self, timestamp: UnixNanos, value: f64) {
        self.add_position_return(timestamp, value);
    }

    /// Computes daily portfolio returns from account balance snapshots.
    ///
    /// Returns `None` (falling back to per-position returns) when:
    /// - Fewer than two account state events exist.
    /// - Any event carries multiple balance currencies.
    /// - The balance currency changes between events.
    /// - Fewer than two distinct calendar days have balance data.
    ///
    /// Multi-currency accounts are not yet supported; the caller silently
    /// receives per-position returns in that case.
    fn calculate_account_returns(account: &dyn Account) -> Option<Returns> {
        let mut events = account.events();
        if events.len() < 2 {
            return None;
        }

        events.sort_by_key(|event| event.ts_event);

        let mut currency = None;
        let mut daily_balances = BTreeMap::new();

        for event in events {
            if event.balances.len() != 1 {
                return None;
            }

            let balance = event.balances[0];

            if let Some(existing_currency) = currency {
                if existing_currency != balance.currency {
                    return None;
                }
            } else {
                currency = Some(balance.currency);
            }

            let day_start = UnixNanos::from(
                event.ts_event.as_u64() - (event.ts_event.as_u64() % NANOSECONDS_IN_DAY),
            );
            daily_balances.insert(day_start, balance.total.as_f64());
        }

        if daily_balances.len() < 2 {
            return None;
        }

        let mut returns = Returns::new();
        let mut current_day = *daily_balances.keys().next()?;
        let last_day = *daily_balances.keys().next_back()?;
        let mut current_balance: Option<f64> = None;
        let mut previous_balance: Option<f64> = None;

        loop {
            if let Some(balance) = daily_balances.get(&current_day) {
                current_balance = Some(*balance);
            }

            let balance = current_balance?;

            if let Some(previous) = previous_balance
                && previous != 0.0
            {
                let value: f64 = (balance / previous) - 1.0;
                if value.is_finite() {
                    returns.insert(current_day, value);
                }
            }

            previous_balance = Some(balance);

            if current_day >= last_day {
                break;
            }

            current_day += UnixNanos::from(NANOSECONDS_IN_DAY);
        }

        (!returns.is_empty()).then_some(returns)
    }

    /// Retrieves realized PnLs for a specific currency.
    ///
    /// Returns `None` if no PnLs exist, or if multiple currencies exist
    /// without an explicit currency specified.
    #[must_use]
    pub fn realized_pnls(&self, currency: Option<&Currency>) -> Option<Vec<(PositionId, f64)>> {
        if self.realized_pnls.is_empty() {
            return None;
        }

        // Require explicit currency for multi-currency portfolios to avoid nondeterminism
        let currency = match currency {
            Some(c) => c,
            None if self.account_balances.len() == 1 => self.account_balances.keys().next()?,
            None => return None,
        };

        self.realized_pnls.get(currency).cloned()
    }

    /// Calculates total PnL including unrealized PnL if provided.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No currency is specified in a multi-currency portfolio.
    /// - The specified currency is not found in account balances.
    /// - The unrealized PnL currency does not match the specified currency.
    #[expect(clippy::missing_panics_doc)] // Guarded by length check
    pub fn total_pnl(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> Result<f64, &'static str> {
        if self.account_balances.is_empty() {
            return Ok(0.0);
        }

        // Require explicit currency for multi-currency portfolios to avoid nondeterminism
        let currency = match currency {
            Some(c) => c,
            None if self.account_balances.len() == 1 => {
                self.account_balances.keys().next().expect("len is 1")
            }
            None => return Err("Currency must be specified for multi-currency portfolio"),
        };

        if let Some(unrealized_pnl) = unrealized_pnl
            && unrealized_pnl.currency != *currency
        {
            return Err("Unrealized PnL currency does not match specified currency");
        }

        let account_balance = self
            .account_balances
            .get(currency)
            .ok_or("Specified currency not found in account balances")?;

        let default_money = &Money::new(0.0, *currency);
        let account_balance_starting = self
            .account_balances_starting
            .get(currency)
            .unwrap_or(default_money);

        let unrealized_pnl_f64 = unrealized_pnl.map_or(0.0, Money::as_f64);
        Ok((account_balance.as_f64() - account_balance_starting.as_f64()) + unrealized_pnl_f64)
    }

    /// Calculates total PnL as a percentage of starting balance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No currency is specified in a multi-currency portfolio.
    /// - The specified currency is not found in account balances.
    /// - The unrealized PnL currency does not match the specified currency.
    #[expect(clippy::missing_panics_doc)] // Guarded by length check
    pub fn total_pnl_percentage(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> Result<f64, &'static str> {
        if self.account_balances.is_empty() {
            return Ok(0.0);
        }

        // Require explicit currency for multi-currency portfolios to avoid nondeterminism
        let currency = match currency {
            Some(c) => c,
            None if self.account_balances.len() == 1 => {
                self.account_balances.keys().next().expect("len is 1")
            }
            None => return Err("Currency must be specified for multi-currency portfolio"),
        };

        if let Some(unrealized_pnl) = unrealized_pnl
            && unrealized_pnl.currency != *currency
        {
            return Err("Unrealized PnL currency does not match specified currency");
        }

        let account_balance = self
            .account_balances
            .get(currency)
            .ok_or("Specified currency not found in account balances")?;

        let default_money = &Money::new(0.0, *currency);
        let account_balance_starting = self
            .account_balances_starting
            .get(currency)
            .unwrap_or(default_money);

        if account_balance_starting.as_decimal() == Decimal::ZERO {
            return Ok(0.0);
        }

        let unrealized_pnl_f64 = unrealized_pnl.map_or(0.0, Money::as_f64);
        let current = account_balance.as_f64() + unrealized_pnl_f64;
        let starting = account_balance_starting.as_f64();
        let difference = current - starting;

        Ok((difference / starting) * 100.0)
    }

    /// Gets all PnL-related performance statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if PnL calculations fail, for example due to:
    ///
    /// - No currency specified for a multi-currency portfolio.
    /// - Unrealized PnL currency not matching the specified currency.
    /// - Specified currency not found in account balances.
    pub fn get_performance_stats_pnls(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> Result<AHashMap<String, f64>, &'static str> {
        let mut output = AHashMap::new();

        output.insert(
            "PnL (total)".to_string(),
            self.total_pnl(currency, unrealized_pnl)?,
        );
        output.insert(
            "PnL% (total)".to_string(),
            self.total_pnl_percentage(currency, unrealized_pnl)?,
        );

        if let Some(realized_pnls) = self.realized_pnls(currency) {
            for (name, stat) in &self.statistics {
                if let Some(value) = stat.calculate_from_realized_pnls(
                    &realized_pnls
                        .iter()
                        .map(|(_, pnl)| *pnl)
                        .collect::<Vec<f64>>(),
                ) {
                    output.insert(name.clone(), value);
                }
            }
        }

        Ok(output)
    }

    /// Gets all return-based performance statistics.
    #[must_use]
    pub fn get_performance_stats_returns(&self) -> AHashMap<String, f64> {
        self.calculate_returns_stats(self.returns())
    }

    /// Gets all position-return-based performance statistics.
    #[must_use]
    pub fn get_performance_stats_position_returns(&self) -> AHashMap<String, f64> {
        self.calculate_returns_stats(self.position_returns())
    }

    /// Gets all portfolio-return-based performance statistics.
    #[must_use]
    pub fn get_performance_stats_portfolio_returns(&self) -> AHashMap<String, f64> {
        self.calculate_returns_stats(self.portfolio_returns())
    }

    /// Gets general portfolio statistics.
    #[must_use]
    pub fn get_performance_stats_general(&self) -> AHashMap<String, f64> {
        let mut output = AHashMap::new();

        for (name, stat) in &self.statistics {
            if let Some(value) = stat.calculate_from_positions(&self.positions) {
                output.insert(name.clone(), value);
            }
        }

        output
    }

    /// Calculates the maximum length of statistic names for formatting.
    fn get_max_length_name(&self) -> usize {
        self.statistics.keys().map(String::len).max().unwrap_or(0)
    }

    fn calculate_returns_stats(&self, returns: &Returns) -> AHashMap<String, f64> {
        let mut output = AHashMap::new();

        for (name, stat) in &self.statistics {
            if let Some(value) = stat.calculate_from_returns(returns) {
                output.insert(name.clone(), value);
            }
        }

        output
    }

    fn format_returns_stats(&self, stats: AHashMap<String, f64>) -> Vec<String> {
        let max_length = self.get_max_length_name();
        let mut entries: Vec<_> = stats.into_iter().collect();
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));

        let mut output = Vec::new();

        for (k, v) in entries {
            let padding = max_length.saturating_sub(k.len()) + 1;
            output.push(format!("{}: {}{:.2}", k, " ".repeat(padding), v));
        }

        output
    }

    fn sync_returns_alias(&mut self) {
        if self.portfolio_returns.is_empty() {
            self.returns = self.position_returns.clone();
            return;
        }

        self.returns = self.portfolio_returns.clone();
    }

    /// Gets formatted PnL statistics as strings.
    ///
    /// # Errors
    ///
    /// Returns an error if PnL statistics calculation fails.
    pub fn get_stats_pnls_formatted(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> Result<Vec<String>, String> {
        let max_length = self.get_max_length_name();
        let stats = self.get_performance_stats_pnls(currency, unrealized_pnl)?;

        let mut entries: Vec<_> = stats.into_iter().collect();
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));

        let mut output = Vec::new();

        for (k, v) in entries {
            let padding = if max_length > k.len() {
                max_length - k.len() + 1
            } else {
                1
            };
            output.push(format!("{}: {}{:.2}", k, " ".repeat(padding), v));
        }

        Ok(output)
    }

    /// Gets formatted return statistics as strings.
    #[must_use]
    pub fn get_stats_returns_formatted(&self) -> Vec<String> {
        self.format_returns_stats(self.get_performance_stats_returns())
    }

    /// Gets formatted position-return statistics as strings.
    #[must_use]
    pub fn get_stats_position_returns_formatted(&self) -> Vec<String> {
        self.format_returns_stats(self.get_performance_stats_position_returns())
    }

    /// Gets formatted portfolio-return statistics as strings.
    #[must_use]
    pub fn get_stats_portfolio_returns_formatted(&self) -> Vec<String> {
        self.format_returns_stats(self.get_performance_stats_portfolio_returns())
    }

    /// Gets formatted general statistics as strings.
    #[must_use]
    pub fn get_stats_general_formatted(&self) -> Vec<String> {
        let max_length = self.get_max_length_name();
        let stats = self.get_performance_stats_general();

        let mut entries: Vec<_> = stats.into_iter().collect();
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));

        let mut output = Vec::new();

        for (k, v) in entries {
            let padding = max_length - k.len() + 1;
            output.push(format!("{}: {}{}", k, " ".repeat(padding), v));
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ahash::{AHashMap, AHashSet};
    use indexmap::IndexMap;
    use nautilus_core::{UUID4, approx_eq};
    use nautilus_model::{
        enums::{AccountType, InstrumentClass, LiquiditySide, OrderSide, PositionSide},
        events::{AccountState, OrderFilled},
        identifiers::{
            AccountId, ClientOrderId,
            stubs::{instrument_id_aud_usd_sim, strategy_id_ema_cross, trader_id},
        },
        instruments::InstrumentAny,
        stubs::TestDefault,
        types::{AccountBalance, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    /// Mock implementation of `PortfolioStatistic` for testing.
    #[derive(Debug)]
    struct MockStatistic {
        name: String,
    }

    impl MockStatistic {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
            }
        }
    }

    impl PortfolioStatistic for MockStatistic {
        type Item = f64;

        fn name(&self) -> String {
            self.name.clone()
        }

        fn calculate_from_realized_pnls(&self, pnls: &[f64]) -> Option<f64> {
            Some(pnls.iter().sum())
        }

        fn calculate_from_returns(&self, returns: &Returns) -> Option<f64> {
            Some(returns.values().sum())
        }

        fn calculate_from_positions(&self, positions: &[Position]) -> Option<f64> {
            Some(positions.len() as f64)
        }
    }

    fn create_mock_position(
        id: &str,
        realized_pnl: f64,
        realized_return: f64,
        currency: Currency,
    ) -> Position {
        Position {
            events: Vec::new(),
            adjustments: Vec::new(),
            trader_id: trader_id(),
            strategy_id: strategy_id_ema_cross(),
            instrument_id: instrument_id_aud_usd_sim(),
            id: PositionId::new(id),
            account_id: AccountId::new("test-account"),
            opening_order_id: ClientOrderId::test_default(),
            closing_order_id: None,
            entry: OrderSide::NoOrderSide,
            side: PositionSide::NoPositionSide,
            signed_qty: 0.0,
            quantity: Quantity::default(),
            peak_qty: Quantity::default(),
            price_precision: 2,
            size_precision: 2,
            multiplier: Quantity::default(),
            is_inverse: false,
            is_currency_pair: true,
            instrument_class: InstrumentClass::Spot,
            base_currency: None,
            quote_currency: Currency::USD(),
            settlement_currency: Currency::USD(),
            ts_init: UnixNanos::default(),
            ts_opened: UnixNanos::default(),
            ts_last: UnixNanos::default(),
            ts_closed: Some(UnixNanos::from(1_706_659_200_000_000_000)),
            duration_ns: 2,
            avg_px_open: 0.0,
            avg_px_close: None,
            realized_return,
            realized_pnl: Some(Money::new(realized_pnl, currency)),
            trade_ids: AHashSet::new(),
            buy_qty: Quantity::default(),
            sell_qty: Quantity::default(),
            commissions: IndexMap::new(),
        }
    }

    struct MockAccount {
        starting_balances: AHashMap<Currency, Money>,
        current_balances: AHashMap<Currency, Money>,
        events: Vec<AccountState>,
    }

    impl Account for MockAccount {
        fn starting_balances(&self) -> IndexMap<Currency, Money> {
            self.starting_balances.clone().into_iter().collect()
        }
        fn balances_total(&self) -> IndexMap<Currency, Money> {
            self.current_balances.clone().into_iter().collect()
        }
        fn id(&self) -> AccountId {
            todo!()
        }
        fn account_type(&self) -> AccountType {
            todo!()
        }
        fn base_currency(&self) -> Option<Currency> {
            todo!()
        }
        fn is_cash_account(&self) -> bool {
            todo!()
        }
        fn is_margin_account(&self) -> bool {
            todo!()
        }
        fn calculated_account_state(&self) -> bool {
            todo!()
        }
        fn balance_total(&self, _: Option<Currency>) -> Option<Money> {
            todo!()
        }
        fn balance_free(&self, _: Option<Currency>) -> Option<Money> {
            todo!()
        }
        fn balances_free(&self) -> IndexMap<Currency, Money> {
            todo!()
        }
        fn balance_locked(&self, _: Option<Currency>) -> Option<Money> {
            todo!()
        }
        fn balances_locked(&self) -> IndexMap<Currency, Money> {
            todo!()
        }
        fn last_event(&self) -> Option<AccountState> {
            self.events.last().cloned()
        }
        fn events(&self) -> Vec<AccountState> {
            self.events.clone()
        }
        fn event_count(&self) -> usize {
            self.events.len()
        }
        fn currencies(&self) -> Vec<Currency> {
            self.current_balances.keys().copied().collect()
        }
        fn balances(&self) -> IndexMap<Currency, AccountBalance> {
            todo!()
        }
        fn apply(&mut self, _: AccountState) -> anyhow::Result<()> {
            todo!()
        }
        fn calculate_balance_locked(
            &mut self,
            _: &InstrumentAny,
            _: OrderSide,
            _: Quantity,
            _: Price,
            _: Option<bool>,
        ) -> Result<Money, anyhow::Error> {
            todo!()
        }
        fn calculate_pnls(
            &self,
            _: &InstrumentAny,
            _: &OrderFilled,
            _: Option<Position>,
        ) -> Result<Vec<Money>, anyhow::Error> {
            todo!()
        }
        fn calculate_commission(
            &self,
            _: &InstrumentAny,
            _: Quantity,
            _: Price,
            _: LiquiditySide,
            _: Option<bool>,
        ) -> Result<Money, anyhow::Error> {
            todo!()
        }

        fn balance(&self, _: Option<Currency>) -> Option<&AccountBalance> {
            todo!()
        }

        fn purge_account_events(&mut self, _: UnixNanos, _: u64) {
            // MockAccount doesn't need purging
        }
    }

    fn create_account_state(total: f64, currency: Currency, ts_event: u64) -> AccountState {
        AccountState::new(
            AccountId::new("test-account"),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::new(total, currency),
                Money::new(0.0, currency),
                Money::new(total, currency),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_event),
            Some(currency),
        )
    }

    #[rstest]
    fn test_register_and_deregister_statistics() {
        let mut analyzer = PortfolioAnalyzer::new();
        let stat: Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync> =
            Arc::new(MockStatistic::new("test_stat"));

        // Test registration
        analyzer.register_statistic(Arc::clone(&stat));
        assert!(analyzer.statistic("test_stat").is_some());

        // Test deregistration
        analyzer.deregister_statistic(&stat);
        assert!(analyzer.statistic("test_stat").is_none());

        // Test deregister all
        let stat1: Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync> =
            Arc::new(MockStatistic::new("stat1"));
        let stat2: Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync> =
            Arc::new(MockStatistic::new("stat2"));
        analyzer.register_statistic(Arc::clone(&stat1));
        analyzer.register_statistic(Arc::clone(&stat2));
        analyzer.deregister_statistics();
        assert!(analyzer.statistics.is_empty());
    }

    #[rstest]
    fn test_calculate_total_pnl() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();

        // Set up mock account data
        let mut starting_balances = AHashMap::new();
        starting_balances.insert(currency, Money::new(1000.0, currency));

        let mut current_balances = AHashMap::new();
        current_balances.insert(currency, Money::new(1500.0, currency));

        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![],
        };

        analyzer.calculate_statistics(&account, &[]);

        // Test total PnL calculation
        let result = analyzer.total_pnl(Some(&currency), None).unwrap();
        assert!(approx_eq!(f64, result, 500.0, epsilon = 1e-9));

        // Test with unrealized PnL
        let unrealized_pnl = Money::new(100.0, currency);
        let result = analyzer
            .total_pnl(Some(&currency), Some(&unrealized_pnl))
            .unwrap();
        assert!(approx_eq!(f64, result, 600.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_calculate_total_pnl_percentage() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();

        // Set up mock account data
        let mut starting_balances = AHashMap::new();
        starting_balances.insert(currency, Money::new(1000.0, currency));

        let mut current_balances = AHashMap::new();
        current_balances.insert(currency, Money::new(1500.0, currency));

        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![],
        };

        analyzer.calculate_statistics(&account, &[]);

        // Test percentage calculation
        let result = analyzer
            .total_pnl_percentage(Some(&currency), None)
            .unwrap();
        assert!(approx_eq!(f64, result, 50.0, epsilon = 1e-9)); // (1500 - 1000) / 1000 * 100

        // Test with unrealized PnL
        let unrealized_pnl = Money::new(500.0, currency);
        let result = analyzer
            .total_pnl_percentage(Some(&currency), Some(&unrealized_pnl))
            .unwrap();
        assert!(approx_eq!(f64, result, 100.0, epsilon = 1e-9)); // (2000 - 1000) / 1000 * 100
    }

    #[rstest]
    fn test_add_positions_and_returns() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();

        let positions = vec![
            create_mock_position("AUD/USD", 100.0, 0.1, currency),
            create_mock_position("AUD/USD", 200.0, 0.2, currency),
        ];

        analyzer.add_positions(&positions);

        // Verify realized PnLs were recorded
        let pnls = analyzer.realized_pnls(Some(&currency)).unwrap();
        assert_eq!(pnls.len(), 2);
        assert!(approx_eq!(f64, pnls[0].1, 100.0, epsilon = 1e-9));
        assert!(approx_eq!(f64, pnls[1].1, 200.0, epsilon = 1e-9));

        // Verify returns were recorded
        let returns = analyzer.returns();
        let position_returns = analyzer.position_returns();
        assert_eq!(returns.len(), 1);
        assert_eq!(position_returns.len(), 1);
        assert!(analyzer.portfolio_returns().is_empty());
        assert!(approx_eq!(
            f64,
            *returns.values().next().unwrap(),
            0.30000000000000004,
            epsilon = 1e-9
        ));
        assert!(approx_eq!(
            f64,
            *position_returns.values().next().unwrap(),
            0.30000000000000004,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_add_positions_skips_position_returns_without_real_close_timestamp() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();
        let mut position = create_mock_position("AUD/USD", 100.0, 0.1, currency);
        position.ts_closed = Some(UnixNanos::default());

        analyzer.add_positions(&[position]);

        assert!(analyzer.position_returns().is_empty());
        assert!(analyzer.returns().is_empty());
    }

    #[rstest]
    fn test_performance_stats_calculation() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();
        let stat: Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync> =
            Arc::new(MockStatistic::new("test_stat"));
        analyzer.register_statistic(Arc::clone(&stat));

        // Add some positions
        let positions = vec![
            create_mock_position("AUD/USD", 100.0, 0.1, currency),
            create_mock_position("AUD/USD", 200.0, 0.2, currency),
        ];

        let mut starting_balances = AHashMap::new();
        starting_balances.insert(currency, Money::new(1000.0, currency));

        let mut current_balances = AHashMap::new();
        current_balances.insert(currency, Money::new(1500.0, currency));

        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![],
        };

        analyzer.calculate_statistics(&account, &positions);

        // Test PnL stats
        let pnl_stats = analyzer
            .get_performance_stats_pnls(Some(&currency), None)
            .unwrap();
        assert!(pnl_stats.contains_key("PnL (total)"));
        assert!(pnl_stats.contains_key("PnL% (total)"));
        assert!(pnl_stats.contains_key("test_stat"));

        // Test returns stats
        let return_stats = analyzer.get_performance_stats_returns();
        assert!(return_stats.contains_key("test_stat"));

        // Test general stats
        let general_stats = analyzer.get_performance_stats_general();
        assert!(general_stats.contains_key("test_stat"));
    }

    #[rstest]
    fn test_formatted_output() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();
        let stat: Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync> =
            Arc::new(MockStatistic::new("test_stat"));
        analyzer.register_statistic(Arc::clone(&stat));

        let positions = vec![
            create_mock_position("AUD/USD", 100.0, 0.1, currency),
            create_mock_position("AUD/USD", 200.0, 0.2, currency),
        ];

        let mut starting_balances = AHashMap::new();
        starting_balances.insert(currency, Money::new(1000.0, currency));

        let mut current_balances = AHashMap::new();
        current_balances.insert(currency, Money::new(1500.0, currency));

        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![],
        };

        analyzer.calculate_statistics(&account, &positions);

        // Test formatted outputs
        let pnl_formatted = analyzer
            .get_stats_pnls_formatted(Some(&currency), None)
            .unwrap();
        assert!(!pnl_formatted.is_empty());
        assert!(pnl_formatted.iter().all(|s| s.contains(':')));

        let returns_formatted = analyzer.get_stats_returns_formatted();
        assert!(!returns_formatted.is_empty());
        assert!(returns_formatted.iter().all(|s| s.contains(':')));

        let general_formatted = analyzer.get_stats_general_formatted();
        assert!(!general_formatted.is_empty());
        assert!(general_formatted.iter().all(|s| s.contains(':')));
    }

    #[rstest]
    fn test_reset() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();

        let positions = vec![create_mock_position("AUD/USD", 100.0, 0.1, currency)];
        let mut starting_balances = AHashMap::new();
        starting_balances.insert(currency, Money::new(1000.0, currency));
        let mut current_balances = AHashMap::new();
        current_balances.insert(currency, Money::new(1500.0, currency));

        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![],
        };

        analyzer.calculate_statistics(&account, &positions);

        analyzer.reset();

        assert!(analyzer.account_balances_starting.is_empty());
        assert!(analyzer.account_balances.is_empty());
        assert!(analyzer.positions.is_empty());
        assert!(analyzer.realized_pnls.is_empty());
        assert!(analyzer.position_returns.is_empty());
        assert!(analyzer.portfolio_returns.is_empty());
        assert!(analyzer.returns.is_empty());
    }

    #[rstest]
    fn test_currencies_preserve_account_balance_order() {
        // Pin IndexMap iteration on PortfolioAnalyzer::account_balances:
        // currencies() drives the per-currency stat computation in
        // BacktestEngine::run, so the returned Vec must reflect the
        // upstream account balance order across runs.
        let mut analyzer = PortfolioAnalyzer::new();
        let inserts = [
            (Currency::BTC(), Money::new(1.0, Currency::BTC())),
            (Currency::USD(), Money::new(2.0, Currency::USD())),
            (Currency::ETH(), Money::new(3.0, Currency::ETH())),
        ];

        for (currency, money) in inserts {
            analyzer.account_balances.insert(currency, money);
        }

        let returned: Vec<Currency> = analyzer.currencies().into_iter().copied().collect();
        assert_eq!(
            returned,
            vec![Currency::BTC(), Currency::USD(), Currency::ETH()],
        );
    }

    #[rstest]
    fn test_calculate_statistics_clears_previous_positions() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();

        let positions1 = vec![create_mock_position("pos1", 100.0, 0.1, currency)];
        let positions2 = vec![create_mock_position("pos2", 200.0, 0.2, currency)];

        let mut starting_balances = AHashMap::new();
        starting_balances.insert(currency, Money::new(1000.0, currency));
        let mut current_balances = AHashMap::new();
        current_balances.insert(currency, Money::new(1500.0, currency));

        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![],
        };

        // First calculation
        analyzer.calculate_statistics(&account, &positions1);
        assert_eq!(analyzer.positions.len(), 1);

        // Second calculation should NOT accumulate
        analyzer.calculate_statistics(&account, &positions2);
        assert_eq!(analyzer.positions.len(), 1);
    }

    #[rstest]
    fn test_calculate_statistics_uses_account_state_returns_when_available() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();
        let positions = vec![
            create_mock_position("AUD/USD", 100.0, 0.1, currency),
            create_mock_position("EUR/USD", 200.0, 0.2, currency),
        ];

        let mut starting_balances = AHashMap::new();
        starting_balances.insert(currency, Money::new(1000.0, currency));

        let mut current_balances = AHashMap::new();
        current_balances.insert(currency, Money::new(1100.0, currency));

        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![
                create_account_state(1000.0, currency, 1_704_067_200_000_000_000),
                create_account_state(1050.0, currency, 1_704_844_800_000_000_000),
                create_account_state(1100.0, currency, 1_706_659_200_000_000_000),
            ],
        };

        analyzer.calculate_statistics(&account, &positions);

        let position_returns = analyzer.position_returns();
        let portfolio_returns = analyzer.portfolio_returns();
        let returns = analyzer.returns();
        assert_eq!(position_returns.len(), 1);
        assert_eq!(portfolio_returns.len(), 30);
        assert_eq!(returns, portfolio_returns);
        assert!(approx_eq!(
            f64,
            *portfolio_returns
                .get(&UnixNanos::from(1_704_153_600_000_000_000))
                .unwrap(),
            0.0,
            epsilon = 1e-9
        ));
        assert!(approx_eq!(
            f64,
            *portfolio_returns
                .get(&UnixNanos::from(1_704_844_800_000_000_000))
                .unwrap(),
            0.05,
            epsilon = 1e-9
        ));
        assert!(approx_eq!(
            f64,
            *portfolio_returns
                .get(&UnixNanos::from(1_706_659_200_000_000_000))
                .unwrap(),
            (1100.0 / 1050.0) - 1.0,
            epsilon = 1e-9
        ));
        assert!(approx_eq!(
            f64,
            *position_returns.values().next().unwrap(),
            0.30000000000000004,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_calculate_statistics_skips_non_finite_account_returns() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();

        let mut starting_balances = AHashMap::new();
        starting_balances.insert(currency, Money::new(0.0, currency));

        let mut current_balances = AHashMap::new();
        current_balances.insert(currency, Money::new(1050.0, currency));

        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![
                create_account_state(0.0, currency, 1_704_067_200_000_000_000),
                create_account_state(1000.0, currency, 1_704_844_800_000_000_000),
                create_account_state(1050.0, currency, 1_706_659_200_000_000_000),
            ],
        };

        analyzer.calculate_statistics(&account, &[]);

        let returns = analyzer.returns();
        assert!(returns.values().all(|value| value.is_finite()));
        assert!(approx_eq!(
            f64,
            *returns
                .get(&UnixNanos::from(1_706_659_200_000_000_000))
                .unwrap(),
            0.05,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_calculate_statistics_falls_back_to_position_returns_without_account_events() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();
        let positions = vec![
            create_mock_position("AUD/USD", 100.0, 0.1, currency),
            create_mock_position("EUR/USD", 200.0, 0.2, currency),
        ];

        let mut starting_balances = AHashMap::new();
        starting_balances.insert(currency, Money::new(1000.0, currency));

        let mut current_balances = AHashMap::new();
        current_balances.insert(currency, Money::new(1100.0, currency));

        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![],
        };

        analyzer.calculate_statistics(&account, &positions);

        let returns = analyzer.returns();
        assert!(analyzer.portfolio_returns().is_empty());
        assert_eq!(returns, analyzer.position_returns());
        assert_eq!(returns.len(), 1);
        assert!(approx_eq!(
            f64,
            *returns.values().next().unwrap(),
            0.30000000000000004,
            epsilon = 1e-9
        ));
    }

    #[rstest]
    fn test_get_performance_stats_returns_prefers_portfolio_returns() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();
        let stat: Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync> =
            Arc::new(MockStatistic::new("test_stat"));
        analyzer.register_statistic(Arc::clone(&stat));

        let positions = vec![
            create_mock_position("AUD/USD", 100.0, 0.1, currency),
            create_mock_position("EUR/USD", 200.0, 0.2, currency),
        ];

        let mut starting_balances = AHashMap::new();
        starting_balances.insert(currency, Money::new(1000.0, currency));

        let mut current_balances = AHashMap::new();
        current_balances.insert(currency, Money::new(1100.0, currency));

        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![
                create_account_state(1000.0, currency, 1_704_067_200_000_000_000),
                create_account_state(1050.0, currency, 1_704_844_800_000_000_000),
                create_account_state(1100.0, currency, 1_706_659_200_000_000_000),
            ],
        };

        analyzer.calculate_statistics(&account, &positions);

        let position_stats = analyzer.get_performance_stats_position_returns();
        let portfolio_stats = analyzer.get_performance_stats_portfolio_returns();
        let returns_stats = analyzer.get_performance_stats_returns();

        assert!(approx_eq!(
            f64,
            *position_stats.get("test_stat").unwrap(),
            0.30000000000000004,
            epsilon = 1e-9
        ));
        assert_eq!(returns_stats, portfolio_stats);
    }
}
