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
