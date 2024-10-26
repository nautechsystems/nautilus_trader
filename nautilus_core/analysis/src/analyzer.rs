// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    sync::Arc,
};

use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    accounts::base::Account,
    identifiers::PositionId,
    position::Position,
    types::{currency::Currency, money::Money},
};
use rust_decimal::Decimal;

use crate::{statistic::PortfolioStatistic, Returns};

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.analysis")
)]
pub struct PortfolioAnalyzer {
    statistics: HashMap<String, Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync>>,
    account_balances_starting: HashMap<Currency, Money>,
    account_balances: HashMap<Currency, Money>,
    positions: Vec<Position>,
    realized_pnls: HashMap<Currency, Vec<(PositionId, f64)>>,
    returns: Returns,
}

impl Default for PortfolioAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl PortfolioAnalyzer {
    pub fn new() -> Self {
        Self {
            statistics: HashMap::new(),
            account_balances_starting: HashMap::new(),
            account_balances: HashMap::new(),
            positions: Vec::new(),
            realized_pnls: HashMap::new(),
            returns: BTreeMap::new(),
        }
    }

    pub fn register_statistic(
        &mut self,
        statistic: Arc<(dyn PortfolioStatistic<Item = f64> + Send + Sync)>,
    ) {
        self.statistics.insert(statistic.name(), statistic);
    }

    pub fn deregister_statistic(&mut self, statistic: Box<dyn PortfolioStatistic<Item = f64>>) {
        self.statistics.remove(&statistic.name());
    }

    pub fn deregister_statistics(&mut self) {
        self.statistics.clear();
    }

    pub fn reset(&mut self) {
        self.account_balances_starting.clear();
        self.account_balances.clear();
        self.realized_pnls.clear();
        self.returns.clear();
    }

    fn get_max_length_name(&self) -> usize {
        self.statistics
            .keys()
            .map(std::string::String::len)
            .max()
            .unwrap_or(0)
    }

    pub fn currencies(&self) -> Vec<&Currency> {
        self.account_balances.keys().collect()
    }

    pub fn statistic(
        &self,
        name: &str,
    ) -> Option<&Arc<dyn PortfolioStatistic<Item = f64> + Send + Sync>> {
        self.statistics.get(name)
    }

    pub const fn returns(&self) -> &Returns {
        &self.returns
    }

    pub fn calculate_statistics(&mut self, account: &dyn Account, positions: &[Position]) {
        self.account_balances_starting = account.starting_balances();
        self.account_balances = account.balances_total();
        self.realized_pnls.clear();
        self.returns.clear();

        self.add_positions(positions);
    }

    pub fn add_positions(&mut self, positions: &[Position]) {
        self.positions.extend_from_slice(positions);
        for position in positions {
            self.add_trade(&position.id, &position.realized_pnl.unwrap());
            if let Some(ref pnl) = position.realized_pnl {
                self.add_trade(&position.id, pnl);
            }
            self.add_return(
                position.ts_closed.unwrap_or(UnixNanos::default()),
                position.realized_return,
            );
        }
    }

    pub fn add_trade(&mut self, position_id: &PositionId, pnl: &Money) {
        let currency = pnl.currency;
        let entry = self.realized_pnls.entry(currency).or_default();
        entry.push((*position_id, pnl.as_f64()));
    }

    pub fn add_return(&mut self, timestamp: UnixNanos, value: f64) {
        self.returns
            .entry(timestamp)
            .and_modify(|existing_value| *existing_value += value)
            .or_insert(value);
    }

    pub fn realized_pnls(&self, currency: Option<&Currency>) -> Option<Vec<(PositionId, f64)>> {
        if self.realized_pnls.is_empty() {
            return None;
        }
        let currency = currency.or_else(|| self.account_balances.keys().next())?;
        self.realized_pnls.get(currency).cloned()
    }

    pub fn total_pnl(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> Result<f64, &'static str> {
        if self.account_balances.is_empty() {
            return Ok(0.0);
        }

        let currency = currency
            .or_else(|| self.account_balances.keys().next())
            .ok_or("Currency not specified for multi-currency portfolio")?;

        if let Some(unrealized_pnl) = unrealized_pnl {
            if unrealized_pnl.currency != *currency {
                return Err("Unrealized PnL currency does not match specified currency");
            }
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

        let unrealized_pnl_f64 =
            unrealized_pnl.map_or(0.0, nautilus_model::types::money::Money::as_f64);
        Ok((account_balance.as_f64() - account_balance_starting.as_f64()) + unrealized_pnl_f64)
    }

    pub fn total_pnl_percentage(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> Result<f64, &'static str> {
        if self.account_balances.is_empty() {
            return Ok(0.0);
        }

        let currency = currency
            .or_else(|| self.account_balances.keys().next())
            .ok_or("Currency not specified for multi-currency portfolio")?;

        if let Some(unrealized_pnl) = unrealized_pnl {
            if unrealized_pnl.currency != *currency {
                return Err("Unrealized PnL currency does not match specified currency");
            }
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

        let unrealized_pnl_f64 =
            unrealized_pnl.map_or(0.0, nautilus_model::types::money::Money::as_f64);
        let current = account_balance.as_f64() + unrealized_pnl_f64;
        let starting = account_balance_starting.as_f64();
        let difference = current - starting;

        Ok((difference / starting) * 100.0)
    }

    pub fn get_performance_stats_pnls(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> Result<HashMap<String, f64>, &'static str> {
        let mut output = HashMap::new();

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

    pub fn get_performance_stats_returns(&self) -> HashMap<String, f64> {
        let mut output = HashMap::new();

        for (name, stat) in &self.statistics {
            if let Some(value) = stat.calculate_from_returns(&self.returns) {
                output.insert(name.clone(), value);
            }
        }

        output
    }

    pub fn get_performance_stats_general(&self) -> HashMap<String, f64> {
        let mut output = HashMap::new();

        for (name, stat) in &self.statistics {
            if let Some(value) = stat.calculate_from_positions(&self.positions) {
                output.insert(name.clone(), value);
            }
        }

        output
    }

    pub fn get_stats_pnls_formatted(
        &self,
        currency: Option<&Currency>,
        unrealized_pnl: Option<&Money>,
    ) -> Result<Vec<String>, &'static str> {
        let max_length = self.get_max_length_name();
        let stats = self.get_performance_stats_pnls(currency, unrealized_pnl)?;

        let mut output = Vec::new();
        for (k, v) in stats {
            let padding = max_length - k.len() + 1;
            output.push(format!("{}: {}{:.2}", k, " ".repeat(padding), v));
        }

        Ok(output)
    }

    pub fn get_stats_returns_formatted(&self) -> Vec<String> {
        let max_length = self.get_max_length_name();
        let stats = self.get_performance_stats_returns();

        let mut output = Vec::new();
        for (k, v) in stats {
            let padding = max_length - k.len() + 1;
            output.push(format!("{}: {}{:.2}", k, " ".repeat(padding), v));
        }

        output
    }

    pub fn get_stats_general_formatted(&self) -> Vec<String> {
        let max_length = self.get_max_length_name();
        let stats = self.get_performance_stats_general();

        let mut output = Vec::new();
        for (k, v) in stats {
            let padding = max_length - k.len() + 1;
            output.push(format!("{}: {}{}", k, " ".repeat(padding), v));
        }

        output
    }
}
