//! Risk-adjusted performance metrics for strategy evaluation.

/// Risk-adjusted information ratio.
///
/// IR = mean(excess_returns) / std(excess_returns)
/// Normalized to allow comparison across strategies and capital sizes.
#[derive(Clone, Copy, Debug)]
pub struct RiskAdjustedInfoRatio;

impl RiskAdjustedInfoRatio {
    /// Evaluate the information ratio for a return series.
    pub fn evaluate(&self, returns: &[f64]) -> f64 {
        if returns.is_empty() {
            return 0.0;
        }

        let n = returns.len() as f64;
        let mean = returns.iter().sum::<f64>() / n;

        if n < 2.0 {
            return 0.0;
        }

        let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
        let std = variance.sqrt();

        mean / (std + 1e-8)
    }

    /// Evaluate with risk normalization (returns / risk).
    pub fn evaluate_risk_adjusted(&self, returns: &[f64], risks: &[f64]) -> f64 {
        let adjusted: Vec<f64> = returns
            .iter()
            .zip(risks.iter())
            .map(|(r, risk)| r / (risk + 1e-8))
            .collect();

        self.evaluate(&adjusted)
    }

    /// Compare candidate vs baseline. Returns true if candidate is significantly better.
    pub fn is_improvement(&self, candidate_ir: f64, baseline_ir: f64, threshold: f64) -> bool {
        candidate_ir > baseline_ir * (1.0 + threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ir_positive_returns() {
        let metric = RiskAdjustedInfoRatio;
        let returns = vec![0.01, 0.02, 0.015, 0.01, 0.02];
        let ir = metric.evaluate(&returns);
        assert!(ir > 0.0);
    }

    #[test]
    fn test_ir_negative_returns() {
        let metric = RiskAdjustedInfoRatio;
        let returns = vec![-0.01, -0.02, -0.015];
        let ir = metric.evaluate(&returns);
        assert!(ir < 0.0);
    }

    #[test]
    fn test_ir_empty() {
        let metric = RiskAdjustedInfoRatio;
        assert_eq!(metric.evaluate(&[]), 0.0);
    }

    #[test]
    fn test_improvement_check() {
        let metric = RiskAdjustedInfoRatio;
        assert!(metric.is_improvement(1.1, 1.0, 0.05));
        assert!(!metric.is_improvement(1.04, 1.0, 0.05));
    }
}
