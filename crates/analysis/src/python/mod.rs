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

//! Python bindings from [PyO3](https://pyo3.rs).

pub mod analyzer;
pub mod statistics;

use pyo3::{prelude::*, pymodule};

/// Initializes the Python `analysis` module.
///
/// Adds the `PortfolioAnalyzer` class and all portfolio statistics.
///
/// # Errors
///
/// Returns a Python exception if adding any class fails.
#[pymodule]
pub fn analysis(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::analyzer::PortfolioAnalyzer>()?;

    // Statistics - Returns-based
    m.add_class::<crate::statistics::cagr::CAGR>()?;
    m.add_class::<crate::statistics::calmar_ratio::CalmarRatio>()?;
    m.add_class::<crate::statistics::max_drawdown::MaxDrawdown>()?;
    m.add_class::<crate::statistics::profit_factor::ProfitFactor>()?;
    m.add_class::<crate::statistics::returns_avg::ReturnsAverage>()?;
    m.add_class::<crate::statistics::returns_avg_loss::ReturnsAverageLoss>()?;
    m.add_class::<crate::statistics::returns_avg_win::ReturnsAverageWin>()?;
    m.add_class::<crate::statistics::returns_volatility::ReturnsVolatility>()?;
    m.add_class::<crate::statistics::risk_return_ratio::RiskReturnRatio>()?;
    m.add_class::<crate::statistics::sharpe_ratio::SharpeRatio>()?;
    m.add_class::<crate::statistics::sortino_ratio::SortinoRatio>()?;

    // Statistics - PnL-based
    m.add_class::<crate::statistics::expectancy::Expectancy>()?;
    m.add_class::<crate::statistics::loser_avg::AvgLoser>()?;
    m.add_class::<crate::statistics::loser_max::MaxLoser>()?;
    m.add_class::<crate::statistics::loser_min::MinLoser>()?;
    m.add_class::<crate::statistics::win_rate::WinRate>()?;
    m.add_class::<crate::statistics::winner_avg::AvgWinner>()?;
    m.add_class::<crate::statistics::winner_max::MaxWinner>()?;
    m.add_class::<crate::statistics::winner_min::MinWinner>()?;

    // Statistics - Position-based
    m.add_class::<crate::statistics::long_ratio::LongRatio>()?;

    Ok(())
}
