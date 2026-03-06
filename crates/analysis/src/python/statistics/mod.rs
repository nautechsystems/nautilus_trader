//! Python bindings for trading performance statistics.

pub mod cagr;
pub mod calmar_ratio;
pub mod expectancy;
pub mod long_ratio;
pub mod loser_avg;
pub mod loser_max;
pub mod loser_min;
pub mod max_drawdown;
pub mod profit_factor;
pub mod returns_avg;
pub mod returns_avg_loss;
pub mod returns_avg_win;
pub mod returns_volatility;
pub mod risk_return_ratio;
pub mod sharpe_ratio;
pub mod sortino_ratio;
pub mod win_rate;
pub mod winner_avg;
pub mod winner_max;
pub mod winner_min;

use std::collections::BTreeMap;

use nautilus_core::UnixNanos;

fn transform_returns(raw_returns: BTreeMap<u64, f64>) -> BTreeMap<UnixNanos, f64> {
    raw_returns
        .keys()
        .map(|&k| (UnixNanos::from(k), raw_returns[&k]))
        .collect()
}
