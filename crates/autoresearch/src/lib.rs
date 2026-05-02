//! Autoresearch runtime for KuaaMU Quant Engine.
//!
//! Implements the Karpathy Ratchet: Agent proposes strategy hypotheses,
//! micro-backtests validate them, and only improvements are retained.

pub mod metric;
pub mod micro_backtest;
pub mod runtime;

pub use metric::RiskAdjustedInfoRatio;
pub use runtime::{AutoresearchRuntime, StrategyHypothesis};
