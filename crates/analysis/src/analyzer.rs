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
use indexmap::{IndexMap, IndexSet};
use nautilus_core::{
    DurationNanos, UUID4, UnixNanos,
    correctness::{CorrectnessError, CorrectnessResult},
};
use nautilus_model::{
    accounts::{Account, AccountAny},
    events::PortfolioSnapshot,
    identifiers::{AccountId, PositionId},
    position::Position,
    types::{
        Currency, Money,
        fixed::{MAX_FLOAT_PRECISION, check_float_precision, fixed_i128_to_f64, raw_scales_match},
        money::MoneyRaw,
    },
};
use rust_decimal::prelude::ToPrimitive;

use crate::{
    Returns,
    snapshot::PortfolioStatistics,
    statistic::PortfolioStatistic,
    statistics::{
        expectancy::Expectancy, long_ratio::LongRatio, loser_avg::AvgLoser, loser_max::MaxLoser,
        loser_min::MinLoser, profit_factor::ProfitFactor, returns_avg::ReturnsAverage,
        returns_avg_loss::ReturnsAverageLoss, returns_avg_win::ReturnsAverageWin,
        returns_kurtosis::ReturnsKurtosis, returns_skewness::ReturnsSkewness,
        returns_volatility::ReturnsVolatility, risk_return_ratio::RiskReturnRatio,
        sharpe_ratio::SharpeRatio, sortino_ratio::SortinoRatio, tail_ratio::TailRatio,
        win_rate::WinRate, winner_avg::AvgWinner, winner_max::MaxWinner, winner_min::MinWinner,
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
#[cfg_attr(feature = "python", pyo3::pyclass(module = "nautilus_trader.analysis"))]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.analysis")
)]
pub struct PortfolioAnalyzer {
    pub statistics: AHashMap<String, Statistic>,
    pub account_balances_starting: IndexMap<Currency, Money>,
    pub account_balances: IndexMap<Currency, Money>,
    pub positions: Vec<Position>,
    pub realized_pnls: AHashMap<Currency, Vec<(PositionId, UnixNanos, f64)>>,
    pub recorded_realized_pnls: AHashMap<Currency, Vec<(PositionId, UnixNanos, f64)>>,
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
        analyzer.register_statistic(Arc::new(ReturnsSkewness::new()));
        analyzer.register_statistic(Arc::new(ReturnsKurtosis::new()));
        analyzer.register_statistic(Arc::new(ReturnsAverage {}));
        analyzer.register_statistic(Arc::new(ReturnsAverageLoss {}));
        analyzer.register_statistic(Arc::new(ReturnsAverageWin {}));
        analyzer.register_statistic(Arc::new(SharpeRatio::new(None)));
        analyzer.register_statistic(Arc::new(SortinoRatio::new(None)));
        analyzer.register_statistic(Arc::new(TailRatio {}));
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
            recorded_realized_pnls: AHashMap::new(),
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

    /// Replaces the registered statistics with `statistics`.
    ///
    /// Used to carry a caller's registered set onto an analyzer built for a single
    /// calculation, so custom statistics participate alongside the built-in defaults.
    pub fn replace_statistics(&mut self, statistics: AHashMap<String, Statistic>) {
        self.statistics = statistics;
    }

    /// Resets all analysis data to initial state.
    ///
    /// Registered statistics are retained; use [`Self::deregister_statistics`] to clear them.
    pub fn reset(&mut self) {
        self.account_balances_starting.clear();
        self.account_balances.clear();
        self.positions.clear();
        self.realized_pnls.clear();
        self.recorded_realized_pnls.clear();
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
    /// This clears calculated state before calculating, while preserving
    /// close-time PnLs recorded during portfolio processing.
    pub fn calculate_statistics(&mut self, account: &dyn Account, positions: &[Position]) {
        self.account_balances_starting = account.starting_balances().into_iter().collect();
        self.account_balances = account.balances_total().into_iter().collect();
        self.positions.clear();
        self.realized_pnls.clear();
        self.position_returns.clear();
        self.portfolio_returns.clear();
        self.returns.clear();

        self.add_positions(positions);

        match Self::calculate_account_returns(account) {
            Ok(Some(account_returns)) => {
                self.portfolio_returns = account_returns;
                self.sync_returns_alias();
            }
            Ok(None) => (),
            Err(e) => log::warn!("Cannot calculate account returns for {}: {e}", account.id()),
        }
    }

    /// Builds a populated analyzer from venue accounts and positions.
    ///
    /// Aggregates starting and total balances across all `accounts`, adds `positions` and
    /// `snapshots`, and seeds `recorded_realized_pnls` (close-time PnLs observed during the run).
    #[must_use]
    pub fn from_accounts(
        accounts: &[AccountAny],
        positions: &[Position],
        snapshots: &[Position],
        recorded_realized_pnls: AHashMap<Currency, Vec<(PositionId, UnixNanos, f64)>>,
    ) -> Self {
        Self::from_accounts_with_snapshots(
            accounts,
            positions,
            snapshots,
            &[],
            recorded_realized_pnls,
        )
    }

    /// Builds a populated analyzer from accounts, positions, and portfolio snapshots.
    ///
    /// Portfolio returns use daily mark-to-market equity when at least two UTC dates are
    /// available and every account resolves to one common currency. Otherwise the primary
    /// returns source falls back to position returns.
    /// Retains exact balances. Logs and omits balances that cannot be summed into representable
    /// totals at their stored scale and equity series that exceed the supported float precision.
    #[must_use]
    pub fn from_accounts_with_snapshots<'a>(
        accounts: &[AccountAny],
        positions: &[Position],
        position_snapshots: &[Position],
        portfolio_snapshots: impl IntoIterator<Item = &'a PortfolioSnapshot>,
        recorded_realized_pnls: AHashMap<Currency, Vec<(PositionId, UnixNanos, f64)>>,
    ) -> Self {
        let (mut analyzer, account_ids, _) = Self::build_from_accounts(
            accounts,
            positions,
            position_snapshots,
            recorded_realized_pnls,
        );
        analyzer.set_portfolio_returns_from_snapshots(&account_ids, portfolio_snapshots);
        analyzer
    }

    /// Builds an analyzer from accounts, positions, and portfolio snapshots.
    ///
    /// # Errors
    ///
    /// Returns an error if balances cannot be summed into representable totals at their stored
    /// scale or if a balance, daily equity value, or realized PnL exceeds the supported float
    /// precision.
    pub fn try_from_accounts_with_snapshots<'a>(
        accounts: &[AccountAny],
        positions: &[Position],
        position_snapshots: &[Position],
        portfolio_snapshots: impl IntoIterator<Item = &'a PortfolioSnapshot>,
        recorded_realized_pnls: AHashMap<Currency, Vec<(PositionId, UnixNanos, f64)>>,
    ) -> CorrectnessResult<Self> {
        check_position_pnl_precision(positions.iter().chain(position_snapshots))?;

        for currency in recorded_realized_pnls.keys() {
            check_float_precision(currency.precision).map_err(|e| {
                CorrectnessError::PredicateViolation {
                    message: format!("Cannot analyze recorded realized PnL in {currency}: {e}"),
                }
            })?;
        }

        let (mut analyzer, account_ids, balance_result) = Self::build_from_accounts(
            accounts,
            positions,
            position_snapshots,
            recorded_realized_pnls,
        );

        balance_result?;
        analyzer.try_set_portfolio_returns_from_snapshots(&account_ids, portfolio_snapshots)?;
        Ok(analyzer)
    }

    fn build_from_accounts(
        accounts: &[AccountAny],
        positions: &[Position],
        position_snapshots: &[Position],
        recorded_realized_pnls: AHashMap<Currency, Vec<(PositionId, UnixNanos, f64)>>,
    ) -> (Self, Vec<AccountId>, CorrectnessResult<()>) {
        let mut analyzer = Self::default();
        let mut account_ids = Vec::with_capacity(accounts.len());
        let mut starting_totals = IndexMap::new();
        let mut current_totals = IndexMap::new();
        let mut balance_error = None;
        let mut invalid_totals = IndexSet::new();

        for account in accounts {
            let account_ref: &dyn Account = match account {
                AccountAny::Margin(margin) => margin,
                AccountAny::Cash(cash) => cash,
                AccountAny::Betting(betting) => betting,
                AccountAny::Wallet(wallet) => wallet,
            };

            let account_id = account_ref.id();
            account_ids.push(account_id);

            for (balances, totals) in [
                (account_ref.starting_balances(), &mut starting_totals),
                (account_ref.balances_total(), &mut current_totals),
            ] {
                Self::sum_account_balances(
                    account_id,
                    balances,
                    totals,
                    &mut invalid_totals,
                    &mut balance_error,
                );
            }
        }

        for (totals, balances) in [
            (starting_totals, &mut analyzer.account_balances_starting),
            (current_totals, &mut analyzer.account_balances),
        ] {
            for (currency, (raw, amount_currency)) in totals {
                if let Ok(total) = Money::from_raw_checked(raw, amount_currency) {
                    balances.insert(currency, total);
                } else {
                    invalid_totals.insert(currency);
                }
            }
        }

        for currency in invalid_totals {
            log::warn!("Cannot sum portfolio balances for {currency}");
            analyzer.account_balances_starting.shift_remove(&currency);
            analyzer.account_balances.shift_remove(&currency);
            balance_error.get_or_insert_with(|| CorrectnessError::PredicateViolation {
                message: format!("Cannot sum portfolio balances for {currency}"),
            });
        }

        analyzer.add_positions(positions);
        analyzer.add_positions(position_snapshots);
        analyzer.recorded_realized_pnls = recorded_realized_pnls;
        (analyzer, account_ids, balance_error.map_or(Ok(()), Err))
    }

    fn sum_account_balances(
        account_id: AccountId,
        balances: impl IntoIterator<Item = (Currency, Money)>,
        totals: &mut IndexMap<Currency, (MoneyRaw, Currency)>,
        invalid_totals: &mut IndexSet<Currency>,
        balance_error: &mut Option<CorrectnessError>,
    ) {
        for (currency, money) in balances {
            if let Err(e) = check_float_precision(currency.precision.max(money.currency.precision))
            {
                balance_error.get_or_insert_with(|| CorrectnessError::PredicateViolation {
                    message: format!("Cannot analyze {currency} balance for {account_id}: {e}"),
                });
            }

            totals
                .entry(currency)
                .and_modify(|(raw, amount_currency)| {
                    if *amount_currency == money.currency
                        && raw_scales_match(amount_currency.precision, money.currency.precision)
                        && let Some(sum) = raw.checked_add(money.raw())
                    {
                        *raw = sum;
                    } else {
                        invalid_totals.insert(currency);
                    }
                })
                .or_insert((money.raw(), money.currency));
        }
    }

    /// Replaces the primary returns source with snapshot-backed portfolio returns when resolvable.
    pub fn set_portfolio_returns_from_snapshots<'a>(
        &mut self,
        account_ids: &[AccountId],
        snapshots: impl IntoIterator<Item = &'a PortfolioSnapshot>,
    ) {
        if let Err(e) = self.try_set_portfolio_returns_from_snapshots(account_ids, snapshots) {
            log::warn!("Cannot calculate portfolio returns from snapshots: {e}");
        }
    }

    fn try_set_portfolio_returns_from_snapshots<'a>(
        &mut self,
        account_ids: &[AccountId],
        snapshots: impl IntoIterator<Item = &'a PortfolioSnapshot>,
    ) -> CorrectnessResult<()> {
        if let Some(returns) = Self::calculate_snapshot_returns(account_ids, snapshots)? {
            self.portfolio_returns = returns;
            self.sync_returns_alias();
        }

        Ok(())
    }

    /// Collects an owned [`PortfolioStatistics`] snapshot from the current analyzer state.
    #[must_use]
    pub fn statistics(&self) -> PortfolioStatistics {
        let mut pnls = AHashMap::new();

        for currency in self.currencies() {
            match self.get_performance_stats_pnls(Some(currency), None) {
                Ok(stats) => {
                    pnls.insert(currency.code.to_string(), stats);
                }
                Err(e) => log::warn!("Cannot calculate PnL statistics for {currency}: {e}"),
            }
        }
        PortfolioStatistics {
            pnls,
            returns: self.get_performance_stats_returns(),
            general: self.get_performance_stats_general(),
            returns_series: self.returns.clone(),
        }
    }

    /// Adds new positions for analysis.
    pub fn add_positions(&mut self, positions: &[Position]) {
        self.positions.extend_from_slice(positions);
        for position in positions {
            if let Some(ref pnl) = position.realized_pnl {
                self.add_trade(&position.id, position.ts_last, pnl);
            }

            if let Some(ts_closed) = position.ts_closed
                && ts_closed.as_u64() > 0
                && position.realized_pnl.is_some()
            {
                self.add_position_return(ts_closed, position.realized_return);
            }
        }
    }

    /// Records a trade's PnL realized at `ts_event`.
    pub fn add_trade(&mut self, position_id: &PositionId, ts_event: UnixNanos, pnl: &Money) {
        let currency = pnl.currency;
        let entry = self.realized_pnls.entry(currency).or_default();
        entry.push((*position_id, ts_event, trade_pnl_value(pnl)));
    }

    /// Records a trade's PnL realized at `ts_event`, observed during portfolio processing.
    pub fn record_trade(&mut self, position_id: &PositionId, ts_event: UnixNanos, pnl: &Money) {
        let currency = pnl.currency;
        // Currency keys compare by code. Keep native precision visible to checked reporting.
        if currency.precision > MAX_FLOAT_PRECISION
            && self
                .recorded_realized_pnls
                .get_key_value(&currency)
                .is_some_and(|(key, _)| key.precision < currency.precision)
            && let Some(records) = self.recorded_realized_pnls.remove(&currency)
        {
            self.recorded_realized_pnls.insert(currency, records);
        }

        let entry = self.recorded_realized_pnls.entry(currency).or_default();
        entry.push((*position_id, ts_event, trade_pnl_value(pnl)));
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
    fn calculate_account_returns(account: &dyn Account) -> CorrectnessResult<Option<Returns>> {
        let mut events = account.events();
        if events.len() < 2 {
            return Ok(None);
        }

        events.sort_by_key(|event| event.ts_event);

        let mut currency = None;
        let mut daily_balances = BTreeMap::new();

        for event in events {
            if event.balances.is_empty() {
                continue;
            }

            if event.balances.len() != 1 {
                return Ok(None);
            }

            let balance = event.balances[0];

            if let Some(existing_currency) = currency {
                if existing_currency != balance.currency {
                    return Ok(None);
                }
            } else {
                currency = Some(balance.currency);
            }

            let day_start = event.ts_event.floor(DurationNanos::from_days(1));
            check_float_precision(balance.total.currency.precision).map_err(|e| {
                CorrectnessError::PredicateViolation {
                    message: format!(
                        "Cannot analyze {} balance for {}: {e}",
                        balance.currency,
                        account.id()
                    ),
                }
            })?;

            daily_balances.insert(day_start, balance.total.as_f64());
        }

        Ok(Self::calculate_daily_returns(&daily_balances))
    }

    fn calculate_snapshot_returns<'a>(
        account_ids: &[AccountId],
        snapshots: impl IntoIterator<Item = &'a PortfolioSnapshot>,
    ) -> CorrectnessResult<Option<Returns>> {
        let expected_accounts: IndexSet<AccountId> = account_ids.iter().copied().collect();
        if expected_accounts.is_empty() {
            return Ok(None);
        }

        let mut currency = None;
        let mut equity_by_account: AHashMap<AccountId, BTreeMap<UnixNanos, f64>> = AHashMap::new();

        for snapshot in snapshots {
            if !expected_accounts.contains(&snapshot.account_id) {
                continue;
            }

            if !snapshot.unpriced_instruments.is_empty() {
                continue;
            }

            if snapshot.total_equity.len() != 1 {
                return Ok(None);
            }

            let equity = snapshot
                .base_currency_equity
                .unwrap_or(snapshot.total_equity[0]);

            if let Some(existing_currency) = currency {
                if existing_currency != equity.currency {
                    return Ok(None);
                }
            } else {
                currency = Some(equity.currency);
            }

            let is_registration = !equity_by_account.contains_key(&snapshot.account_id);
            let day_start = Self::snapshot_day_start(snapshot.ts_event, is_registration);
            check_float_precision(equity.currency.precision).map_err(|e| {
                CorrectnessError::PredicateViolation {
                    message: format!(
                        "Cannot analyze {} snapshot equity for {}: {e}",
                        equity.currency, snapshot.account_id
                    ),
                }
            })?;

            equity_by_account
                .entry(snapshot.account_id)
                .or_default()
                .insert(day_start, equity.as_f64());
        }

        if equity_by_account.len() != expected_accounts.len() {
            return Ok(None);
        }

        let daily_equity = Self::aggregate_daily_equity(&expected_accounts, &equity_by_account);
        Ok(daily_equity.and_then(|equity| Self::calculate_daily_returns(&equity)))
    }

    fn aggregate_daily_equity(
        expected_accounts: &IndexSet<AccountId>,
        equity_by_account: &AHashMap<AccountId, BTreeMap<UnixNanos, f64>>,
    ) -> Option<BTreeMap<UnixNanos, f64>> {
        let first_day = equity_by_account
            .values()
            .filter_map(|equity| equity.keys().next().copied())
            .min()?;
        let last_day = equity_by_account
            .values()
            .filter_map(|equity| equity.keys().next_back().copied())
            .max()?;

        let mut daily_equity = BTreeMap::new();
        let mut current_equity = AHashMap::new();
        let mut current_day = first_day;

        loop {
            for account_id in expected_accounts {
                if let Some(equity) = equity_by_account
                    .get(account_id)
                    .and_then(|values| values.get(&current_day))
                {
                    current_equity.insert(*account_id, *equity);
                }
            }

            if current_equity.len() == expected_accounts.len() {
                let total = expected_accounts
                    .iter()
                    .map(|account_id| current_equity[account_id])
                    .sum();
                daily_equity.insert(current_day, total);
            }

            if current_day >= last_day {
                break;
            }

            current_day += DurationNanos::from_days(1);
        }

        Some(daily_equity)
    }

    fn snapshot_day_start(ts_event: UnixNanos, is_registration: bool) -> UnixNanos {
        let day = DurationNanos::from_days(1);
        let day_start = ts_event.floor(day);
        if is_registration || (ts_event == day_start && !ts_event.is_zero()) {
            day_start.saturating_sub(day)
        } else {
            day_start
        }
    }

    fn calculate_daily_returns(daily_equity: &BTreeMap<UnixNanos, f64>) -> Option<Returns> {
        if daily_equity.len() < 2 {
            return None;
        }

        let mut returns = Returns::new();
        let mut current_day = *daily_equity.keys().next()?;
        let last_day = *daily_equity.keys().next_back()?;
        let mut current_balance: Option<f64> = None;
        let mut previous_balance: Option<f64> = None;

        loop {
            if let Some(balance) = daily_equity.get(&current_day) {
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

            current_day += DurationNanos::from_days(1);
        }

        (!returns.is_empty()).then_some(returns)
    }

    /// Retrieves trade PnL records for a specific currency.
    ///
    /// Each record is `(position_id, ts_event, realized_pnl)`, where `ts_event` is the
    /// position's last event time (the close time for closed cycles). Duplicate position
    /// IDs are preserved for NETTING position cycles. Records are returned in ascending
    /// `ts_event` order, with ties keeping their source order.
    ///
    /// Native PnLs (derived from analyzed positions) and PnLs recorded live during
    /// portfolio processing are merged per cycle: a native record is excluded only when a
    /// recorded record shares its `(position_id, ts_event)`. Recorded values therefore take
    /// precedence for the cycles they cover, while native cycles that were never recorded
    /// are retained rather than dropped by position ID.
    ///
    /// Returns `None` if no PnLs exist, or if multiple currencies exist
    /// without an explicit currency specified.
    #[must_use]
    pub fn trade_pnl_records(
        &self,
        currency: Option<&Currency>,
    ) -> Option<Vec<(PositionId, UnixNanos, f64)>> {
        if self.realized_pnls.is_empty() && self.recorded_realized_pnls.is_empty() {
            return None;
        }

        // Require explicit currency for multi-currency portfolios to avoid nondeterminism
        let currency = self.resolve_pnl_currency(currency).ok()?;

        let realized_pnls = self.realized_pnls.get(&currency);
        let recorded_realized_pnls = self.recorded_realized_pnls.get(&currency);

        let mut output = match (realized_pnls, recorded_realized_pnls) {
            (None, None) => return None,
            (Some(realized_pnls), None) => realized_pnls.clone(),
            (None, Some(recorded_realized_pnls)) => recorded_realized_pnls.clone(),
            (Some(realized_pnls), Some(recorded_realized_pnls)) => {
                let recorded_keys: IndexSet<(PositionId, UnixNanos)> = recorded_realized_pnls
                    .iter()
                    .map(|(position_id, ts_event, _)| {
                        (canonical_position_id(*position_id), *ts_event)
                    })
                    .collect();
                let mut merged: Vec<(PositionId, UnixNanos, f64)> = realized_pnls
                    .iter()
                    .copied()
                    .filter(|(position_id, ts_event, _)| {
                        let key = (canonical_position_id(*position_id), *ts_event);
                        !recorded_keys.contains(&key)
                    })
                    .collect();
                merged.extend(recorded_realized_pnls.iter().copied());

                merged
            }
        };

        // Stable sort, so records sharing a timestamp keep their source order and the
        // sequence stays deterministic across runs.
        output.sort_by_key(|(_, ts_event, _)| *ts_event);

        Some(output)
    }

    /// Retrieves realized PnLs for a specific currency.
    ///
    /// Each record is `(position_id, ts_event, realized_pnl)`, in ascending `ts_event` order.
    /// Returns `None` if no PnLs exist, or if multiple currencies exist without an explicit
    /// currency specified.
    #[must_use]
    pub fn realized_pnls(
        &self,
        currency: Option<&Currency>,
    ) -> Option<Vec<(PositionId, UnixNanos, f64)>> {
        self.trade_pnl_records(currency)
    }

    /// Calculates total PnL including unrealized PnL if provided.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No currency is specified in a multi-currency portfolio.
    /// - The specified currency is not found in account balances.
    /// - The unrealized PnL currency does not match the specified currency.
    /// - A money value exceeds the supported float precision.
    /// - The fixed-point PnL calculation overflows.
    pub fn total_pnl(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> Result<f64, &'static str> {
        let Some((current, starting)) = self.pnl_balances(currency, unrealized_pnl)? else {
            return Ok(0.0);
        };

        total_pnl_raw(&current, &starting, unrealized_pnl).map(fixed_i128_to_f64)
    }

    /// Calculates total PnL as a percentage of starting balance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No currency is specified in a multi-currency portfolio.
    /// - The specified currency is not found in account balances.
    /// - The unrealized PnL currency does not match the specified currency.
    /// - A money value exceeds the supported float precision.
    /// - The fixed-point PnL calculation overflows.
    pub fn total_pnl_percentage(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> Result<f64, &'static str> {
        let Some((current, starting)) = self.pnl_balances(currency, unrealized_pnl)? else {
            return Ok(0.0);
        };

        if starting.raw() == 0 {
            return Ok(0.0);
        }

        let pnl = total_pnl_raw(&current, &starting, unrealized_pnl)?;
        Ok(fixed_i128_to_f64(pnl) / starting.as_f64() * 100.0)
    }

    fn pnl_balances(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> Result<Option<(Money, Money)>, &'static str> {
        if self.account_balances.is_empty() {
            return Ok(None);
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

        let account_balance_starting = self
            .account_balances_starting
            .get(currency)
            .copied()
            .unwrap_or_else(|| Money::zero(*currency));

        Ok(Some((*account_balance, account_balance_starting)))
    }

    /// Gets all PnL-related performance statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    ///
    /// - No currency specified for a multi-currency portfolio.
    /// - Unrealized PnL currency not matching the specified currency.
    /// - Specified currency not found in account balances.
    /// - A money value exceeds the supported float precision.
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

        let records = self.trade_pnl_records(currency);
        let has_records = !self.realized_pnls.is_empty() || !self.recorded_realized_pnls.is_empty();

        // `trade_pnl_records` returns `None` both when the resolved currency has no records
        // and when an unspecified currency cannot be resolved. Only the first may dispatch on
        // an empty slice; the second would report values that ignore real PnLs.
        if records.is_none() && has_records {
            self.resolve_pnl_currency(currency)?;
        }

        let realized_pnls: Vec<f64> = records
            .unwrap_or_default()
            .iter()
            .map(|(_, _, pnl)| *pnl)
            .collect();

        for (name, stat) in &self.statistics {
            if let Some(value) = stat.calculate_from_realized_pnls(&realized_pnls) {
                output.insert(name.clone(), value);
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

    /// Gets all benchmark-relative return statistics for the primary returns.
    ///
    /// This is stateless: the `benchmark` series is supplied by the caller rather
    /// than stored on the analyzer. Only statistics that override
    /// [`PortfolioStatistic::calculate_from_returns_with_benchmark`] (the benchmark-relative
    /// statistics) contribute values; all others return `None` and are skipped.
    #[must_use]
    pub fn get_performance_stats_returns_vs_benchmark(
        &self,
        benchmark: &Returns,
    ) -> AHashMap<String, f64> {
        let mut output = AHashMap::new();

        for (name, stat) in &self.statistics {
            if let Some(value) =
                stat.calculate_from_returns_with_benchmark(self.returns(), benchmark)
            {
                output.insert(name.clone(), value);
            }
        }

        output
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

    /// Resolves the currency for PnL record queries: the explicit currency when given,
    /// otherwise the single account-balance currency, otherwise the single currency across
    /// realized PnL records.
    ///
    /// # Errors
    ///
    /// Returns an error if the currency is unspecified and cannot be resolved to exactly
    /// one currency.
    fn resolve_pnl_currency(&self, currency: Option<&Currency>) -> Result<Currency, &'static str> {
        match currency {
            Some(c) => Ok(*c),
            None if self.account_balances.len() == 1 => {
                Ok(*self.account_balances.keys().next().expect("len is 1"))
            }
            None => {
                let mut currencies: IndexSet<Currency> =
                    self.realized_pnls.keys().copied().collect();
                currencies.extend(self.recorded_realized_pnls.keys().copied());
                if currencies.len() != 1 {
                    return Err("Currency must be specified for multi-currency portfolio");
                }

                Ok(*currencies.first().expect("len is 1"))
            }
        }
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

pub(crate) fn check_position_pnl_precision<'a>(
    positions: impl IntoIterator<Item = &'a Position>,
) -> CorrectnessResult<()> {
    for position in positions {
        if let Some(pnl) = position.realized_pnl {
            check_realized_pnl_precision(position.id, &pnl)?;
        }
    }

    Ok(())
}

pub(crate) fn check_realized_pnl_precision(
    position_id: PositionId,
    pnl: &Money,
) -> CorrectnessResult<()> {
    check_float_precision(pnl.currency.precision).map_err(|e| {
        CorrectnessError::PredicateViolation {
            message: format!(
                "Cannot analyze {} realized PnL for {position_id}: {e}",
                pnl.currency
            ),
        }
    })
}

fn total_pnl_raw(
    current: &Money,
    starting: &Money,
    unrealized_pnl: Option<&Money>,
) -> Result<i128, &'static str> {
    let unrealized = unrealized_pnl.map(pnl_money_raw).transpose()?.unwrap_or(0);
    let current = pnl_money_raw(current)?;
    let starting = pnl_money_raw(starting)?;

    current
        .checked_sub(starting)
        .and_then(|pnl| pnl.checked_add(unrealized))
        .ok_or("Total PnL exceeds the fixed-point calculation range")
}

#[allow(
    clippy::useless_conversion,
    reason = "i128::from widens MoneyRaw in standard-precision builds"
)]
fn pnl_money_raw(money: &Money) -> Result<i128, &'static str> {
    // The precision check ensures every operand uses FIXED_SCALAR.
    check_float_precision(money.currency.precision)
        .map_err(|_| "Money precision exceeds maximum float precision")?;
    Ok(i128::from(money.raw()))
}

fn trade_pnl_value(pnl: &Money) -> f64 {
    if pnl.currency.precision > MAX_FLOAT_PRECISION {
        pnl.as_decimal()
            .to_f64()
            .expect("Decimal money values fit in f64")
    } else {
        pnl.as_f64()
    }
}

fn canonical_position_id(position_id: PositionId) -> PositionId {
    const UUID4_STRING_LEN: usize = 36;

    let value = position_id.as_str();
    let Some(separator_index) = value.len().checked_sub(UUID4_STRING_LEN + 1) else {
        return position_id;
    };

    if separator_index == 0 || value.as_bytes()[separator_index] != b'-' {
        return position_id;
    }

    let suffix = &value[separator_index + 1..];
    if suffix.parse::<UUID4>().is_ok() {
        PositionId::new(&value[..separator_index])
    } else {
        position_id
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ahash::{AHashMap, AHashSet};
    use indexmap::IndexMap;
    use nautilus_core::{DurationNanos, UUID4, approx_eq, datetime::NANOSECONDS_IN_DAY};
    use nautilus_model::{
        accounts::{AccountAny, CashAccount, MarginAccount},
        enums::{AccountType, InstrumentClass, LiquiditySide, OrderSide, PositionSide},
        events::{AccountState, OrderFilled, PortfolioSnapshot},
        fees::MakerTakerFeeRates,
        identifiers::{
            AccountId, ClientOrderId,
            stubs::{instrument_id_aud_usd_sim, strategy_id_ema_cross, trader_id},
        },
        instruments::InstrumentAny,
        position::PositionReplayEvents,
        stubs::TestDefault,
        types::{
            AccountBalance, Money, Price, Quantity,
            fixed::FIXED_PRECISION,
            money::{MONEY_RAW_MAX, MONEY_RAW_MIN},
        },
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::statistics::beta_ratio::BetaRatio;

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

    /// Mock implementation returning a fixed value for every input category.
    ///
    /// Two instances can share a name while differing in value, which makes
    /// duplicate-name replacement observable.
    #[derive(Debug)]
    struct ConstantStatistic {
        name: String,
        value: f64,
    }

    impl ConstantStatistic {
        fn new(name: &str, value: f64) -> Self {
            Self {
                name: name.to_string(),
                value,
            }
        }
    }

    impl PortfolioStatistic for ConstantStatistic {
        type Item = f64;

        fn name(&self) -> String {
            self.name.clone()
        }

        fn calculate_from_realized_pnls(&self, _pnls: &[f64]) -> Option<f64> {
            Some(self.value)
        }

        fn calculate_from_returns(&self, _returns: &Returns) -> Option<f64> {
            Some(self.value)
        }

        fn calculate_from_positions(&self, _positions: &[Position]) -> Option<f64> {
            Some(self.value)
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
            replay_events: PositionReplayEvents::default(),
            fill_voids: Vec::new(),
            trader_id: trader_id(),
            strategy_id: strategy_id_ema_cross(),
            instrument_id: instrument_id_aud_usd_sim(),
            id: PositionId::new(id),
            account_id: AccountId::new("test-account"),
            opening_order_id: ClientOrderId::test_default(),
            closing_order_id: None,
            entry: OrderSide::Buy,
            side: PositionSide::Flat,
            signed_qty: 0.into(),
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
            duration_ns: DurationNanos::new(2),
            avg_px_open: 0.into(),
            avg_px_close: None,
            realized_return,
            realized_pnl: Some(Money::new(realized_pnl, currency)),
            trade_ids: AHashSet::new(),
            buy_qty: Quantity::default(),
            sell_qty: Quantity::default(),
            commissions: IndexMap::new(),
            cost_open: 0.into(),
            cost_close: 0.into(),
            qty_closed: 0.into(),
            realized_pnl_remainder: 0.into(),
            allows_negative_price: Some(false),
            inverse_gross_pnl_closed: 0.into(),
            inverse_entry_notional_closed: 0.into(),
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
            &self,
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
            _: MakerTakerFeeRates,
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

    fn create_portfolio_snapshot(
        account_id: AccountId,
        equity: Decimal,
        currency: Currency,
        ts_event: u64,
    ) -> PortfolioSnapshot {
        let equity = Money::from_decimal(equity, currency).unwrap();

        PortfolioSnapshot::new(
            account_id,
            AccountType::Cash,
            Some(currency),
            vec![],
            vec![],
            vec![],
            vec![],
            vec![equity],
            Some(equity),
            false,
            vec![],
            vec![],
            vec![],
            UUID4::new(),
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_event),
        )
    }

    #[rstest]
    fn test_calculate_snapshot_returns_tracks_daily_mark_to_market_equity() {
        let account_id = AccountId::new("SIM-001");
        let currency = Currency::USD();
        let snapshots = [
            create_portfolio_snapshot(
                account_id,
                Decimal::from(10_000),
                currency,
                NANOSECONDS_IN_DAY + NANOSECONDS_IN_DAY / 2,
            ),
            create_portfolio_snapshot(
                account_id,
                Decimal::from(10_500),
                currency,
                NANOSECONDS_IN_DAY + 3 * NANOSECONDS_IN_DAY / 4,
            ),
            create_portfolio_snapshot(
                account_id,
                Decimal::from(11_000),
                currency,
                2 * NANOSECONDS_IN_DAY,
            ),
            create_portfolio_snapshot(
                account_id,
                Decimal::from(12_100),
                currency,
                3 * NANOSECONDS_IN_DAY,
            ),
        ];

        let returns =
            PortfolioAnalyzer::calculate_snapshot_returns(&[account_id], snapshots.iter())
                .unwrap()
                .unwrap();
        let values: Vec<f64> = returns.values().copied().collect();
        let dates: Vec<UnixNanos> = returns.keys().copied().collect();

        assert_eq!(
            dates,
            vec![
                UnixNanos::from(NANOSECONDS_IN_DAY),
                UnixNanos::from(2 * NANOSECONDS_IN_DAY),
            ]
        );
        assert_eq!(values.len(), 2);
        assert!(approx_eq!(f64, values[0], 0.1, epsilon = 1e-12));
        assert!(approx_eq!(f64, values[1], 0.1, epsilon = 1e-12));
    }

    #[rstest]
    fn test_calculate_snapshot_returns_aggregates_accounts_in_one_currency() {
        let account_a = AccountId::new("SIM-001");
        let account_b = AccountId::new("SIM-002");
        let currency = Currency::USD();
        let snapshots = [
            create_portfolio_snapshot(account_a, Decimal::from(100), currency, NANOSECONDS_IN_DAY),
            create_portfolio_snapshot(account_b, Decimal::from(50), currency, NANOSECONDS_IN_DAY),
            create_portfolio_snapshot(
                account_a,
                Decimal::from(110),
                currency,
                2 * NANOSECONDS_IN_DAY,
            ),
        ];

        let returns = PortfolioAnalyzer::calculate_snapshot_returns(
            &[account_a, account_b],
            snapshots.iter(),
        )
        .unwrap()
        .unwrap();

        assert!(approx_eq!(
            f64,
            returns[&UnixNanos::from(NANOSECONDS_IN_DAY)],
            160.0 / 150.0 - 1.0,
            epsilon = 1e-12
        ));
    }

    #[rstest]
    fn test_calculate_snapshot_returns_uses_single_total_without_base_currency() {
        let account_id = AccountId::new("SIM-001");
        let currency = Currency::USD();
        let mut first =
            create_portfolio_snapshot(account_id, Decimal::from(100), currency, NANOSECONDS_IN_DAY);
        let mut second = create_portfolio_snapshot(
            account_id,
            Decimal::from(110),
            currency,
            2 * NANOSECONDS_IN_DAY,
        );
        first.base_currency_equity = None;
        second.base_currency_equity = None;
        let snapshots = [first, second];

        let returns =
            PortfolioAnalyzer::calculate_snapshot_returns(&[account_id], snapshots.iter())
                .unwrap()
                .unwrap();

        assert!(approx_eq!(
            f64,
            returns[&UnixNanos::from(NANOSECONDS_IN_DAY)],
            0.1,
            epsilon = 1e-12
        ));
    }

    #[rstest]
    fn test_calculate_snapshot_returns_rejects_multi_currency_total_with_base_equity() {
        let account_id = AccountId::new("SIM-001");
        let mut first = create_portfolio_snapshot(
            account_id,
            Decimal::from(100),
            Currency::USD(),
            NANOSECONDS_IN_DAY,
        );
        let mut second = create_portfolio_snapshot(
            account_id,
            Decimal::from(110),
            Currency::USD(),
            2 * NANOSECONDS_IN_DAY,
        );
        first.total_equity.push(Money::new(50.0, Currency::AUD()));
        second.total_equity.push(Money::new(55.0, Currency::AUD()));
        let snapshots = [first, second];

        let returns =
            PortfolioAnalyzer::calculate_snapshot_returns(&[account_id], snapshots.iter()).unwrap();

        assert!(returns.is_none());
    }

    #[rstest]
    fn test_calculate_snapshot_returns_forward_fills_unpriced_dates() {
        let account_id = AccountId::new("SIM-001");
        let currency = Currency::USD();
        let first =
            create_portfolio_snapshot(account_id, Decimal::from(100), currency, NANOSECONDS_IN_DAY);
        let mut unpriced =
            create_portfolio_snapshot(account_id, Decimal::ZERO, currency, 2 * NANOSECONDS_IN_DAY);
        unpriced.unpriced_instruments = vec![instrument_id_aud_usd_sim()];
        let last = create_portfolio_snapshot(
            account_id,
            Decimal::from(110),
            currency,
            3 * NANOSECONDS_IN_DAY,
        );
        let snapshots = [first, unpriced, last];

        let returns =
            PortfolioAnalyzer::calculate_snapshot_returns(&[account_id], snapshots.iter())
                .unwrap()
                .unwrap();

        assert!(approx_eq!(
            f64,
            returns[&UnixNanos::from(NANOSECONDS_IN_DAY)],
            0.0,
            epsilon = 1e-12
        ));
        assert!(approx_eq!(
            f64,
            returns[&UnixNanos::from(2 * NANOSECONDS_IN_DAY)],
            0.1,
            epsilon = 1e-12
        ));
    }

    #[rstest]
    fn test_calculate_snapshot_returns_rejects_mixed_account_currencies() {
        let account_a = AccountId::new("SIM-001");
        let account_b = AccountId::new("SIM-002");
        let snapshots = [
            create_portfolio_snapshot(
                account_a,
                Decimal::from(100),
                Currency::USD(),
                NANOSECONDS_IN_DAY,
            ),
            create_portfolio_snapshot(
                account_b,
                Decimal::from(100),
                Currency::AUD(),
                NANOSECONDS_IN_DAY,
            ),
        ];

        let returns = PortfolioAnalyzer::calculate_snapshot_returns(
            &[account_a, account_b],
            snapshots.iter(),
        )
        .unwrap();

        assert!(returns.is_none());
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
    fn test_register_statistic_replaces_matching_name() {
        let mut analyzer = PortfolioAnalyzer::new();
        analyzer.register_statistic(Arc::new(ConstantStatistic::new("Custom", 1.0)));
        analyzer.register_statistic(Arc::new(ConstantStatistic::new("Custom", 2.0)));

        let stats = analyzer.get_performance_stats_general();

        assert_eq!(analyzer.statistics.len(), 1);
        assert_eq!(stats["Custom"], 2.0);
    }

    #[rstest]
    fn test_pnl_statistics_reject_unresolved_currency() {
        // Two currencies and no account balances: the currency cannot be resolved, so
        // dispatching on an empty slice would report a result that ignores both trades.
        let mut analyzer = PortfolioAnalyzer::new();
        analyzer.register_statistic(Arc::new(ConstantStatistic::new("Custom", 1.0)));
        analyzer.add_trade(
            &PositionId::new("P-USD"),
            UnixNanos::from(1),
            &Money::new(10.0, Currency::USD()),
        );
        analyzer.add_trade(
            &PositionId::new("P-EUR"),
            UnixNanos::from(2),
            &Money::new(5.0, Currency::EUR()),
        );

        let result = analyzer.get_performance_stats_pnls(None, None);

        assert_eq!(
            result,
            Err("Currency must be specified for multi-currency portfolio")
        );
    }

    #[rstest]
    fn test_pnl_statistics_resolve_single_pnl_currency() {
        // One realized-PnL currency and no explicit currency: resolution falls back to that
        // currency, so statistics dispatch on its records.
        let mut analyzer = PortfolioAnalyzer::new();
        analyzer.register_statistic(Arc::new(MockStatistic::new("test_stat")));
        let currency = Currency::USD();
        let position_id = PositionId::new("P-USD");
        analyzer.add_trade(
            &position_id,
            UnixNanos::from(1),
            &Money::new(10.0, currency),
        );

        let records = analyzer.trade_pnl_records(None).unwrap();
        let stats = analyzer.get_performance_stats_pnls(None, None).unwrap();

        assert_eq!(records, vec![(position_id, UnixNanos::from(1), 10.0)]);
        assert_eq!(stats["test_stat"], 10.0);
    }

    #[rstest]
    fn test_pnl_statistics_prefer_account_balance_currency() {
        // A single account-balance currency resolves the query even when the realized-PnL
        // records are in another currency: the balance currency wins, has no records, and
        // statistics dispatch on an empty slice instead of erroring.
        let mut analyzer = PortfolioAnalyzer::new();
        analyzer.register_statistic(Arc::new(MockStatistic::new("test_stat")));
        let currency = Currency::USD();
        analyzer
            .account_balances
            .insert(currency, Money::new(1000.0, currency));
        analyzer.add_trade(
            &PositionId::new("P-EUR"),
            UnixNanos::from(1),
            &Money::new(5.0, Currency::EUR()),
        );

        let stats = analyzer.get_performance_stats_pnls(None, None).unwrap();

        assert!(analyzer.trade_pnl_records(None).is_none());
        assert_eq!(stats["test_stat"], 0.0);
    }

    #[rstest]
    fn test_trade_pnl_records_sorted_by_event_time() {
        // A recorded PnL at t=1 and an unmatched position-derived PnL at t=2 are merged from
        // two sources; without sorting the derived record leads and the sequence reads [t2, t1].
        let currency = Currency::USD();
        let mut analyzer = PortfolioAnalyzer::new();
        analyzer.add_trade(
            &PositionId::new("P-LATE"),
            UnixNanos::from(2),
            &Money::new(5.0, currency),
        );
        analyzer.record_trade(
            &PositionId::new("P-EARLY"),
            UnixNanos::from(1),
            &Money::new(7.0, currency),
        );

        let records = analyzer.trade_pnl_records(Some(&currency)).unwrap();

        assert_eq!(
            records,
            vec![
                (PositionId::new("P-EARLY"), UnixNanos::from(1), 7.0),
                (PositionId::new("P-LATE"), UnixNanos::from(2), 5.0),
            ]
        );
    }

    #[rstest]
    fn test_pnl_statistics_run_without_any_trades() {
        // No realized PnLs at all: registered statistics still receive an empty slice, so a
        // statistic defined for empty input reports its value instead of vanishing.
        let mut analyzer = PortfolioAnalyzer::new();
        analyzer.register_statistic(Arc::new(ConstantStatistic::new("Custom", 0.0)));

        let stats = analyzer
            .get_performance_stats_pnls(Some(&Currency::USD()), None)
            .unwrap();

        assert_eq!(stats["Custom"], 0.0);
        assert_eq!(stats["PnL (total)"], 0.0);
    }

    #[rstest]
    fn test_pnl_statistics_run_without_any_trades_or_currency() {
        // No realized PnLs and no explicit currency: with nothing to resolve, statistics
        // still receive an empty slice instead of erroring.
        let mut analyzer = PortfolioAnalyzer::new();
        analyzer.register_statistic(Arc::new(ConstantStatistic::new("Custom", 0.0)));

        let stats = analyzer.get_performance_stats_pnls(None, None).unwrap();

        assert_eq!(stats["Custom"], 0.0);
        assert_eq!(stats["PnL (total)"], 0.0);
    }

    #[rstest]
    fn test_reset_retains_registered_statistics() {
        let currency = Currency::USD();
        let mut analyzer = PortfolioAnalyzer::new();
        analyzer.register_statistic(Arc::new(ConstantStatistic::new("Custom", 7.5)));
        analyzer.add_positions(&[create_mock_position("AUD/USD", 100.0, 0.1, currency)]);

        analyzer.reset();

        assert!(analyzer.positions.is_empty());
        assert_eq!(analyzer.statistics.len(), 1);
        assert_eq!(analyzer.get_performance_stats_general()["Custom"], 7.5);
    }

    #[rstest]
    fn test_formatted_general_stats_include_custom_statistic() {
        // The post-run analysis log formats from this same method, so a registered
        // custom statistic must appear alongside the built-in defaults.
        let mut analyzer = PortfolioAnalyzer::new();
        analyzer.register_statistic(Arc::new(ConstantStatistic::new("Custom", 12.5)));

        let lines = analyzer.get_stats_general_formatted();

        assert_eq!(lines, vec!["Custom:  12.5".to_string()]);
    }

    #[rstest]
    fn test_replace_statistics_adopts_given_set() {
        let mut analyzer = PortfolioAnalyzer::default();
        let default_count = analyzer.statistics.len();

        let mut replacement: AHashMap<String, Statistic> = AHashMap::new();
        replacement.insert(
            "Custom".to_string(),
            Arc::new(ConstantStatistic::new("Custom", 3.25)),
        );
        analyzer.replace_statistics(replacement);

        let stats = analyzer.get_performance_stats_general();

        assert!(default_count > 1);
        assert_eq!(analyzer.statistics.len(), 1);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats["Custom"], 3.25);
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
    fn test_calculate_total_pnl_preserves_small_balance_change() {
        let mut analyzer = PortfolioAnalyzer::new();
        let mut currency = Currency::USD();
        currency.precision = FIXED_PRECISION;
        let starting = Money::from_mantissa_exponent(1_000_000_000, 0, currency);
        let current = Money::from_raw(starting.raw() + 3, currency);
        let unrealized = Money::from_raw(-1, currency);
        analyzer
            .account_balances_starting
            .insert(currency, starting);
        analyzer.account_balances.insert(currency, current);

        let pnl = analyzer.total_pnl(None, Some(&unrealized)).unwrap();
        let percentage = analyzer
            .total_pnl_percentage(None, Some(&unrealized))
            .unwrap();

        let expected = Money::from_raw(2, currency).as_f64();
        let expected_percentage = expected / 10_000_000.0;
        assert_eq!(pnl, expected);
        assert!(approx_eq!(
            f64,
            percentage,
            expected_percentage,
            epsilon = expected_percentage * 2.0 * f64::EPSILON
        ));
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
        assert!(approx_eq!(f64, pnls[0].2, 100.0, epsilon = 1e-9));
        assert!(approx_eq!(f64, pnls[1].2, 200.0, epsilon = 1e-9));

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
    fn test_add_positions_records_open_position_realized_pnl() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();
        let mut position = create_mock_position("AUD/USD", 100.0, 0.1, currency);
        position.ts_closed = None;
        // Distinct from ts_opened (default 0) so the record is keyed by the last event time.
        position.ts_last = UnixNanos::from(7);
        let position_id = position.id;

        analyzer.add_positions(&[position]);

        let records = analyzer.trade_pnl_records(Some(&currency)).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0], (position_id, UnixNanos::from(7), 100.0));
        assert!(analyzer.position_returns().is_empty());
    }

    #[rstest]
    fn test_trade_pnl_records_keeps_unrecorded_native_cycle() {
        // A NETTING id with two native cycles where only the later cycle was recorded:
        // the earlier native cycle must survive rather than be dropped by position ID.
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();
        let position_id = PositionId::new("pos1");

        analyzer.add_trade(
            &position_id,
            UnixNanos::from(1),
            &Money::new(10.0, currency),
        );
        analyzer.add_trade(
            &position_id,
            UnixNanos::from(2),
            &Money::new(20.0, currency),
        );
        analyzer.record_trade(
            &position_id,
            UnixNanos::from(2),
            &Money::new(25.0, currency),
        );

        let records = analyzer.trade_pnl_records(Some(&currency)).unwrap();

        assert_eq!(
            records,
            vec![
                (position_id, UnixNanos::from(1), 10.0),
                (position_id, UnixNanos::from(2), 25.0),
            ]
        );
    }

    #[rstest]
    fn test_trade_pnl_records_drops_recorded_snapshot_alias() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();
        let position_id = PositionId::new("pos1");
        let snapshot_id = PositionId::new(format!("{}-{}", position_id.as_str(), UUID4::new()));
        let ts_event = UnixNanos::from(1);

        analyzer.add_trade(&snapshot_id, ts_event, &Money::new(10.0, currency));
        analyzer.record_trade(&position_id, ts_event, &Money::new(10.0, currency));

        let records = analyzer.trade_pnl_records(Some(&currency)).unwrap();

        assert_eq!(records, vec![(position_id, ts_event, 10.0)]);
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
    fn test_calculate_statistics_preserves_recorded_realized_pnls() {
        let mut analyzer = PortfolioAnalyzer::new();
        let account_currency = Currency::EUR();
        let native_currency = Currency::USD();
        let stat: Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync> =
            Arc::new(MockStatistic::new("test_stat"));
        analyzer.register_statistic(Arc::clone(&stat));
        analyzer.record_trade(
            &PositionId::new("pos1"),
            UnixNanos::from(1),
            &Money::new(90.0, account_currency),
        );

        let positions = vec![create_mock_position("pos1", 100.0, 0.1, native_currency)];

        let mut starting_balances = AHashMap::new();
        starting_balances.insert(account_currency, Money::new(1000.0, account_currency));

        let mut current_balances = AHashMap::new();
        current_balances.insert(account_currency, Money::new(1100.0, account_currency));

        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![],
        };

        analyzer.calculate_statistics(&account, &positions);

        let native_pnls = analyzer.realized_pnls(Some(&native_currency)).unwrap();
        let recorded_pnls = analyzer.realized_pnls(Some(&account_currency)).unwrap();
        let pnl_stats = analyzer
            .get_performance_stats_pnls(Some(&account_currency), None)
            .unwrap();

        assert_eq!(native_pnls[0].2, 100.0);
        assert_eq!(recorded_pnls[0].2, 90.0);
        assert_eq!(*pnl_stats.get("test_stat").unwrap(), 90.0);
    }

    #[rstest]
    fn test_record_trade_preserves_duplicate_position_ids() {
        let mut analyzer = PortfolioAnalyzer::new();
        let account_currency = Currency::EUR();
        let stat: Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync> =
            Arc::new(MockStatistic::new("test_stat"));
        let position_id = PositionId::new("pos1");

        analyzer.register_statistic(Arc::clone(&stat));
        analyzer.record_trade(
            &position_id,
            UnixNanos::from(1),
            &Money::new(90.0, account_currency),
        );
        analyzer.record_trade(
            &position_id,
            UnixNanos::from(2),
            &Money::new(-45.0, account_currency),
        );

        let records = analyzer.trade_pnl_records(Some(&account_currency)).unwrap();
        let recorded_pnls = analyzer.realized_pnls(Some(&account_currency)).unwrap();
        let pnl_stats = analyzer
            .get_performance_stats_pnls(Some(&account_currency), None)
            .unwrap();

        assert_eq!(records[0], (position_id, UnixNanos::from(1), 90.0));
        assert_eq!(records[1], (position_id, UnixNanos::from(2), -45.0));
        assert_eq!(
            recorded_pnls,
            vec![
                (position_id, UnixNanos::from(1), 90.0),
                (position_id, UnixNanos::from(2), -45.0),
            ]
        );
        assert_eq!(*pnl_stats.get("test_stat").unwrap(), 45.0);
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
        assert!(analyzer.recorded_realized_pnls.is_empty());
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
    fn test_calculate_statistics_skips_empty_balance_events() {
        let mut analyzer = PortfolioAnalyzer::new();
        let currency = Currency::USD();
        let mut starting_balances = AHashMap::new();
        starting_balances.insert(currency, Money::new(1000.0, currency));
        let mut current_balances = AHashMap::new();
        current_balances.insert(currency, Money::new(1050.0, currency));
        let empty_event = AccountState::new(
            AccountId::new("test-account"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1_705_276_800_000_000_000),
            UnixNanos::from(1_705_276_800_000_000_000),
            Some(currency),
        );
        let account = MockAccount {
            starting_balances,
            current_balances,
            events: vec![
                create_account_state(1000.0, currency, 1_704_067_200_000_000_000),
                empty_event,
                create_account_state(1050.0, currency, 1_706_659_200_000_000_000),
            ],
        };

        analyzer.calculate_statistics(&account, &[]);

        let portfolio_returns = analyzer.portfolio_returns();
        assert_eq!(portfolio_returns.len(), 30);
        assert_eq!(analyzer.returns(), portfolio_returns);
        assert!(approx_eq!(
            f64,
            *portfolio_returns
                .get(&UnixNanos::from(1_706_659_200_000_000_000))
                .unwrap(),
            0.05,
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

    #[rstest]
    fn test_from_accounts_aggregates_balances_and_positions() {
        let currency = Currency::USD();
        let positions = vec![
            create_mock_position("pos1", 100.0, 0.1, currency),
            create_mock_position("pos2", 200.0, 0.2, currency),
        ];

        let analyzer = PortfolioAnalyzer::from_accounts(
            &[AccountAny::Cash(CashAccount::default())],
            &positions,
            &[],
            AHashMap::new(),
        );

        assert_eq!(analyzer.positions.len(), positions.len());
        assert!(!analyzer.account_balances.is_empty());
    }

    #[rstest]
    fn test_from_accounts_sums_balances_across_accounts() {
        let usd = Currency::USD();
        let one = PortfolioAnalyzer::from_accounts(
            &[AccountAny::Cash(CashAccount::default())],
            &[],
            &[],
            AHashMap::new(),
        );
        let two = PortfolioAnalyzer::from_accounts(
            &[
                AccountAny::Cash(CashAccount::default()),
                AccountAny::Cash(CashAccount::default()),
            ],
            &[],
            &[],
            AHashMap::new(),
        );

        let single = one.account_balances.get(&usd).unwrap().as_decimal();
        let summed = two.account_balances.get(&usd).unwrap().as_decimal();
        let single_start = one
            .account_balances_starting
            .get(&usd)
            .unwrap()
            .as_decimal();
        let summed_start = two
            .account_balances_starting
            .get(&usd)
            .unwrap()
            .as_decimal();

        assert_eq!(summed, single + single);
        assert_eq!(summed_start, single_start + single_start);
        assert_ne!(summed, single);
    }

    #[rstest]
    #[case::in_range(true)]
    #[case::out_of_range(false)]
    fn test_from_accounts_checks_final_balance_bounds(#[case] include_offset: bool) {
        let mut currency = Currency::USD();
        currency.precision = FIXED_PRECISION;
        let expected = Money::from_raw(MONEY_RAW_MAX / 2 + 1, currency);
        let ts = UnixNanos::from(1);

        let accounts = [
            ("SIM-001", expected),
            ("SIM-002", expected),
            ("SIM-003", -expected),
        ]
        .map(|(account_id, amount)| {
            let event = AccountState::new(
                AccountId::new(account_id),
                AccountType::Margin,
                vec![AccountBalance::new(amount, Money::zero(currency), amount)],
                vec![],
                true,
                UUID4::new(),
                ts,
                ts,
                Some(currency),
            );

            AccountAny::Margin(MarginAccount::new(event, true))
        });

        let accounts = if include_offset {
            &accounts[..]
        } else {
            &accounts[..2]
        };

        let exact = PortfolioAnalyzer::from_accounts(accounts, &[], &[], AHashMap::new());
        let checked = PortfolioAnalyzer::try_from_accounts_with_snapshots(
            accounts,
            &[],
            &[],
            &[],
            AHashMap::new(),
        );

        if include_offset {
            for analyzer in [exact, checked.unwrap()] {
                assert_eq!(analyzer.account_balances[&currency], expected);
                assert_eq!(analyzer.account_balances_starting[&currency], expected);
            }
        } else {
            assert!(exact.account_balances.is_empty());
            assert!(exact.account_balances_starting.is_empty());
            assert_eq!(
                checked.unwrap_err().to_string(),
                "Cannot sum portfolio balances for USD"
            );
        }
    }

    #[rstest]
    fn test_from_accounts_omits_currency_with_out_of_range_current_total() {
        let usd = Currency::USD();
        let eur = Currency::EUR();
        let min = Money::from_raw(MONEY_RAW_MIN, usd);
        let minus_unit = Money::from_raw(-1, usd);
        let zero = Money::zero(usd);

        let mut initial = create_account_state(10.0, eur, 1);
        initial.balances.push(AccountBalance::new(min, zero, min));
        let first = CashAccount::new(initial, true, true);

        let second_id = AccountId::new("test-account-2");
        let mut initial = create_account_state(0.0, usd, 1);
        initial.account_id = second_id;
        let mut second = CashAccount::new(initial, true, true);
        let mut update = create_account_state(0.0, usd, 2);
        update.account_id = second_id;
        update.balances = vec![AccountBalance::new(minus_unit, zero, minus_unit)];
        second.apply(update).unwrap();

        let accounts = [AccountAny::Cash(first), AccountAny::Cash(second)];
        let exact = PortfolioAnalyzer::from_accounts(&accounts, &[], &[], AHashMap::new());
        let checked = PortfolioAnalyzer::try_from_accounts_with_snapshots(
            &accounts,
            &[],
            &[],
            &[],
            AHashMap::new(),
        );

        let valid_totals = IndexMap::from([(eur, Money::from("10 EUR"))]);
        assert_eq!(exact.account_balances, valid_totals);
        assert_eq!(exact.account_balances_starting, valid_totals);
        assert_eq!(
            checked.unwrap_err().to_string(),
            "Cannot sum portfolio balances for USD"
        );
    }

    #[rstest]
    #[case(16, true)]
    #[case(17, false)]
    #[case(18, false)]
    fn test_checked_analyzer_rejects_native_realized_pnl(
        #[case] precision: u8,
        #[case] supported: bool,
    ) {
        use nautilus_model::types::fixed::check_fixed_precision;

        if check_fixed_precision(precision).is_err() {
            return;
        }

        let usd = Currency::USD();
        let mut normal = create_mock_position("P-SUPPORTED", 2.0, 0.1, usd);
        let mut native = create_mock_position("P-NATIVE", 1.0, 0.2, usd);
        normal.ts_last = 1.into();
        native.ts_last = 2.into();
        let mut native_currency = usd;
        native_currency.precision = precision;
        let exact = Decimal::ONE + Decimal::new(1, u32::from(precision));
        let pnl = Money::from_decimal(exact, native_currency).unwrap();
        native.realized_pnl = Some(pnl);
        let positions = &[normal.clone(), native.clone()];
        let precision_error =
            format!("Fixed-point precision {precision} exceeds maximum float precision 16");
        let position_error =
            format!("Cannot analyze USD realized PnL for P-NATIVE: {precision_error}");
        let recorded_error =
            format!("Cannot analyze recorded realized PnL in USD: {precision_error}");

        let mut records = PortfolioAnalyzer::new();
        records.record_trade(&normal.id, 1.into(), &Money::from("2 USD"));
        records.record_trade(&native.id, 2.into(), &pnl);

        for (positions, snapshots, recorded, expected_error) in [
            (
                positions.as_slice(),
                &[][..],
                AHashMap::new(),
                &position_error,
            ),
            (
                &[][..],
                positions.as_slice(),
                AHashMap::new(),
                &position_error,
            ),
            (
                &[][..],
                &[][..],
                records.recorded_realized_pnls,
                &recorded_error,
            ),
        ] {
            let result = PortfolioAnalyzer::try_from_accounts_with_snapshots(
                &[],
                positions,
                snapshots,
                &[],
                recorded,
            );

            if supported {
                assert_eq!(
                    result.unwrap().realized_pnls(Some(&usd)).unwrap(),
                    [(normal.id, 1.into(), 2.0), (native.id, 2.into(), 1.0)]
                );
            } else {
                assert_eq!(result.unwrap_err().to_string(), *expected_error);
            }
        }

        #[cfg(feature = "python")]
        {
            use pyo3::{Py, Python, prelude::PyAnyMethods};

            Python::initialize();
            Python::attach(|py| {
                let analyzer = Py::new(py, PortfolioAnalyzer::new()).unwrap();
                let positions = vec![Py::new(py, normal).unwrap(), Py::new(py, native).unwrap()];
                let result = analyzer
                    .bind(py)
                    .call_method1("add_positions", (positions,));
                let state = analyzer.borrow(py);

                if supported {
                    result.unwrap();
                    assert_eq!(state.positions.len(), 2);
                } else {
                    let e = result.unwrap_err();
                    assert!(e.is_instance_of::<pyo3::exceptions::PyValueError>(py));
                    assert_eq!(e.value(py).to_string(), position_error);
                    assert!(state.positions.is_empty());
                    assert!(state.realized_pnls.is_empty());
                }
            });
        }
    }

    #[rstest]
    fn test_statistics_snapshot_matches_getters() {
        let currency = Currency::USD();
        let positions = vec![
            create_mock_position("pos1", 100.0, 0.1, currency),
            create_mock_position("pos2", 200.0, 0.2, currency),
        ];

        let analyzer = PortfolioAnalyzer::from_accounts(
            &[AccountAny::Cash(CashAccount::default())],
            &positions,
            &[],
            AHashMap::new(),
        );

        let snapshot = analyzer.statistics();
        assert!(maps_equal_nan_aware(
            &snapshot.returns,
            &analyzer.get_performance_stats_returns()
        ));
        assert!(maps_equal_nan_aware(
            &snapshot.general,
            &analyzer.get_performance_stats_general()
        ));
        assert_eq!(&snapshot.returns_series, analyzer.returns());

        for currency in analyzer.currencies() {
            let expected = analyzer
                .get_performance_stats_pnls(Some(currency), None)
                .unwrap();
            let actual = snapshot.pnls.get(&currency.code.to_string()).unwrap();
            assert!(maps_equal_nan_aware(actual, &expected));
        }
    }

    fn maps_equal_nan_aware(a: &AHashMap<String, f64>, b: &AHashMap<String, f64>) -> bool {
        if a.len() != b.len() {
            return false;
        }
        a.iter().all(|(k, v)| {
            b.get(k)
                .is_some_and(|bv| (v.is_nan() && bv.is_nan()) || (v == bv))
        })
    }

    #[rstest]
    fn test_get_performance_stats_returns_vs_benchmark() {
        let mut analyzer = PortfolioAnalyzer::new();
        analyzer.register_statistic(Arc::new(BetaRatio::new()));
        analyzer.register_statistic(Arc::new(SharpeRatio::new(None)));

        let one_day = 86_400_000_000_000_u64;
        let start = 1_600_000_000_000_000_000_u64;
        for (i, value) in [0.03, -0.01, 0.02, 0.04].iter().enumerate() {
            analyzer.add_return(UnixNanos::from(start + i as u64 * one_day), *value);
        }

        let mut benchmark: Returns = BTreeMap::new();
        for (i, value) in [0.01, 0.005, 0.005, 0.01].iter().enumerate() {
            benchmark.insert(UnixNanos::from(start + i as u64 * one_day), *value);
        }

        let stats = analyzer.get_performance_stats_returns_vs_benchmark(&benchmark);

        // r = [0.03, -0.01, 0.02, 0.04], b = [0.01, 0.005, 0.005, 0.01]:
        //   mean_r = 0.02, mean_b = 0.0075
        //   Cov = 1.5e-4 / 3 = 5e-5, Var(b) = 2.5e-5 / 3 -> beta = 6.0
        // Only the benchmark-relative statistic contributes; SharpeRatio
        // returns None from the default and is skipped.
        assert_eq!(stats.len(), 1);
        assert!(approx_eq!(
            f64,
            *stats.get("Beta").unwrap(),
            6.0,
            epsilon = 1e-9
        ));
        assert!(!stats.contains_key("Sharpe Ratio (252 days)"));
    }
}
