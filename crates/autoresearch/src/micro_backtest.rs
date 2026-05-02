//! Micro-backtest engine for rapid strategy validation.

/// Result of a micro-backtest run.
#[derive(Clone, Debug)]
pub struct BacktestResult {
    /// Per-period returns.
    pub returns: Vec<f64>,
    /// Total return.
    pub total_return: f64,
    /// Maximum drawdown.
    pub max_drawdown: f64,
    /// Number of trades.
    pub trade_count: u32,
    /// Information ratio.
    pub ir: f64,
}

/// Delta between candidate and baseline backtest results.
#[derive(Clone, Debug)]
pub struct BacktestDelta {
    /// Events where candidate and baseline diverged.
    pub divergences: Vec<FillDiff>,
    /// Candidate's IR vs baseline.
    pub ir_delta: f64,
}

#[derive(Clone, Debug)]
pub struct FillDiff {
    pub timestamp_ns: u64,
    pub baseline_return: f64,
    pub candidate_return: f64,
}

/// Simplified micro-backtest engine.
///
/// Runs a strategy over a short window (5 min default) to validate hypotheses.
pub struct MicroBacktestEngine {
    /// Window size in nanoseconds.
    pub window_ns: u64,
}

impl MicroBacktestEngine {
    pub fn new(window_secs: u64) -> Self {
        Self {
            window_ns: window_secs * 1_000_000_000,
        }
    }

    /// Run a backtest on historical returns and compute result.
    pub fn run(&self, returns: &[f64]) -> BacktestResult {
        let total_return: f64 = returns.iter().sum();
        let max_drawdown = Self::compute_max_drawdown(returns);

        let metric = crate::metric::RiskAdjustedInfoRatio;
        let ir = metric.evaluate(returns);

        BacktestResult {
            returns: returns.to_vec(),
            total_return,
            max_drawdown,
            trade_count: returns.len() as u32,
            ir,
        }
    }

    /// Compute incremental delta between two strategies.
    pub fn run_delta(&self, baseline_returns: &[f64], candidate_returns: &[f64]) -> BacktestDelta {
        let min_len = baseline_returns.len().min(candidate_returns.len());
        let mut divergences = Vec::new();

        for i in 0..min_len {
            if (baseline_returns[i] - candidate_returns[i]).abs() > 1e-8 {
                divergences.push(FillDiff {
                    timestamp_ns: i as u64,
                    baseline_return: baseline_returns[i],
                    candidate_return: candidate_returns[i],
                });
            }
        }

        let metric = crate::metric::RiskAdjustedInfoRatio;
        let baseline_ir = metric.evaluate(baseline_returns);
        let candidate_ir = metric.evaluate(candidate_returns);

        BacktestDelta {
            divergences,
            ir_delta: candidate_ir - baseline_ir,
        }
    }

    fn compute_max_drawdown(returns: &[f64]) -> f64 {
        let mut cumulative = 0.0;
        let mut peak = 0.0;
        let mut max_dd = 0.0;

        for r in returns {
            cumulative += r;
            if cumulative > peak {
                peak = cumulative;
            }
            let dd = peak - cumulative;
            if dd > max_dd {
                max_dd = dd;
            }
        }

        max_dd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_micro_backtest_basic() {
        let engine = MicroBacktestEngine::new(300);
        let returns = vec![0.01, -0.005, 0.02, -0.01, 0.015];
        let result = engine.run(&returns);

        assert!((result.total_return - 0.03).abs() < 1e-10);
        assert!(result.max_drawdown > 0.0);
        assert_eq!(result.trade_count, 5);
    }

    #[test]
    fn test_delta_backtest() {
        let engine = MicroBacktestEngine::new(300);
        let baseline = vec![0.01, 0.02, 0.01];
        let candidate = vec![0.01, 0.03, 0.01]; // diverges at index 1

        let delta = engine.run_delta(&baseline, &candidate);
        assert_eq!(delta.divergences.len(), 1);
        assert_eq!(delta.divergences[0].timestamp_ns, 1);
    }
}
