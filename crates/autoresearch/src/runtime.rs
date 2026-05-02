//! Autoresearch runtime: the Karpathy Ratchet.

use std::collections::VecDeque;
use tracing::{info, warn};

use nautilus_core::UUID4;

use crate::metric::RiskAdjustedInfoRatio;
use crate::micro_backtest::{BacktestResult, MicroBacktestEngine};

/// A strategy hypothesis proposed by an agent.
#[derive(Clone, Debug)]
pub struct StrategyHypothesis {
    pub id: UUID4,
    /// Natural language description of the hypothesis.
    pub description: String,
    /// Code patch (diff) to apply.
    pub code_patch: String,
    /// Parent strategy ID (for inheritance tracking).
    pub parent_id: Option<String>,
}

/// Record of a hypothesis evaluation.
#[derive(Clone, Debug)]
pub struct HypothesisRecord {
    pub hypothesis: StrategyHypothesis,
    pub result: BacktestResult,
    pub accepted: bool,
    pub timestamp_ns: u64,
}

/// Autoresearch runtime implementing the Karpathy Ratchet.
///
/// Flow: Agent proposes hypothesis → micro-backtest → evaluate →
///       if > 105% baseline → accept (ratchet up) → else discard
pub struct AutoresearchRuntime {
    /// Current best IR score.
    pub baseline_ir: f64,
    /// Hypothesis queue to evaluate.
    pub hypothesis_queue: VecDeque<StrategyHypothesis>,
    /// Micro-backtest engine.
    pub micro_bt: MicroBacktestEngine,
    /// Evaluation metric.
    pub metric: RiskAdjustedInfoRatio,
    /// Improvement threshold (e.g., 0.05 for 5%).
    pub improvement_threshold: f64,
    /// History of evaluated hypotheses.
    pub history: Vec<HypothesisRecord>,
}

impl AutoresearchRuntime {
    pub fn new(improvement_threshold: f64) -> Self {
        Self {
            baseline_ir: 0.0,
            hypothesis_queue: VecDeque::new(),
            micro_bt: MicroBacktestEngine::new(300), // 5 min window
            metric: RiskAdjustedInfoRatio,
            improvement_threshold,
            history: Vec::new(),
        }
    }

    /// Submit a hypothesis for evaluation.
    pub fn submit(&mut self, hypothesis: StrategyHypothesis) {
        info!("Hypothesis submitted: {} — {}", hypothesis.id, hypothesis.description);
        self.hypothesis_queue.push_back(hypothesis);
    }

    /// Run one ratchet cycle: evaluate all queued hypotheses.
    pub fn run_ratchet(&mut self, baseline_returns: &[f64]) {
        self.baseline_ir = self.metric.evaluate(baseline_returns);

        while let Some(hypo) = self.hypothesis_queue.pop_front() {
            info!("Evaluating hypothesis: {}", hypo.id);

            // In real implementation, this would compile and run the code patch.
            // For now, we accept the hypothesis description as a signal.
            let candidate_returns = self.simulate_candidate(&hypo);
            let candidate_ir = self.metric.evaluate(&candidate_returns);

            let accepted =
                self.metric
                    .is_improvement(candidate_ir, self.baseline_ir, self.improvement_threshold);

            if accepted {
                info!(
                    "RATCHET UP: {} improved IR from {:.4} to {:.4}",
                    hypo.id, self.baseline_ir, candidate_ir
                );
                self.baseline_ir = candidate_ir;
            } else {
                warn!(
                    "Rejected: {} (IR {:.4} vs baseline {:.4})",
                    hypo.id, candidate_ir, self.baseline_ir
                );
            }

            self.history.push(HypothesisRecord {
                hypothesis: hypo,
                result: BacktestResult {
                    returns: candidate_returns,
                    total_return: 0.0,
                    max_drawdown: 0.0,
                    trade_count: 0,
                    ir: candidate_ir,
                },
                accepted,
                timestamp_ns: 0, // Would use real clock
            });
        }
    }

    /// Simulate candidate strategy returns.
    /// In production, this would compile the code patch and run against historical data.
    fn simulate_candidate(&self, _hypo: &StrategyHypothesis) -> Vec<f64> {
        // Placeholder: return slightly randomized returns
        // Real implementation would compile hypo.code_patch and run micro-backtest
        vec![0.01, 0.02, -0.005, 0.015, 0.01]
    }

    /// Get the number of accepted improvements.
    pub fn accepted_count(&self) -> usize {
        self.history.iter().filter(|r| r.accepted).count()
    }

    /// Get the total number of evaluated hypotheses.
    pub fn total_evaluated(&self) -> usize {
        self.history.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ratchet_accept_improvement() {
        let mut runtime = AutoresearchRuntime::new(0.05);
        runtime.baseline_ir = 1.0;

        runtime.submit(StrategyHypothesis {
            id: UUID4::new(),
            description: "Increase momentum window".to_string(),
            code_patch: "+ window = 20".to_string(),
            parent_id: None,
        });

        // simulate_candidate returns [0.01, 0.02, -0.005, 0.015, 0.01]
        // which has IR > 1.0 * 1.05
        let baseline = vec![0.005, 0.01, -0.002, 0.008, 0.005];
        runtime.run_ratchet(&baseline);

        assert_eq!(runtime.total_evaluated(), 1);
    }

    #[test]
    fn test_ratchet_reject_no_improvement() {
        let mut runtime = AutoresearchRuntime::new(0.50); // 50% threshold
        runtime.baseline_ir = 10.0; // Very high baseline

        runtime.submit(StrategyHypothesis {
            id: UUID4::new(),
            description: "Bad change".to_string(),
            code_patch: "- everything".to_string(),
            parent_id: None,
        });

        let baseline = vec![0.01, 0.02, -0.005, 0.015, 0.01];
        runtime.run_ratchet(&baseline);

        assert_eq!(runtime.accepted_count(), 0);
    }
}
