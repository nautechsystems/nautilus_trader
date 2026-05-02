//! Autonomy level determination based on reputation score.

/// Agent autonomy levels based on on-chain reputation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AutonomyLevel {
    /// Frozen: must re-stake to continue.
    Frozen,
    /// Paper trading only.
    Low,
    /// Each trade needs human verification.
    Medium,
    /// Large trades (> $10k) need confirmation.
    High,
    /// Fully autonomous, no limits.
    Full,
}

impl AutonomyLevel {
    /// Determine autonomy level from a reputation score (0-100).
    pub fn from_score(score: f64) -> Self {
        match score {
            s if s > 90.0 => Self::Full,
            s if s > 70.0 => Self::High,
            s if s > 50.0 => Self::Medium,
            s if s > 30.0 => Self::Low,
            _ => Self::Frozen,
        }
    }

    /// Maximum trade size for this autonomy level (in quote currency).
    pub fn max_trade_size(&self) -> f64 {
        match self {
            Self::Frozen => 0.0,
            Self::Low => 0.0,        // Paper only
            Self::Medium => 1_000.0,
            Self::High => 10_000.0,
            Self::Full => f64::INFINITY,
        }
    }

    /// Whether human confirmation is required.
    pub fn requires_confirmation(&self) -> bool {
        matches!(self, Self::Low | Self::Medium)
    }
}

/// Autonomy slider with smooth transitions.
pub struct AutonomySlider {
    current_level: AutonomyLevel,
    score_history: Vec<f64>,
    /// Minimum observations before level change.
    min_observations: usize,
}

impl AutonomySlider {
    pub fn new(min_observations: usize) -> Self {
        Self {
            current_level: AutonomyLevel::Low,
            score_history: Vec::new(),
            min_observations,
        }
    }

    /// Update the slider with a new reputation score.
    pub fn update(&mut self, score: f64) -> AutonomyLevel {
        self.score_history.push(score);

        if self.score_history.len() < self.min_observations {
            return self.current_level;
        }

        // Use moving average for stability
        let avg = self.score_history.iter().sum::<f64>() / self.score_history.len() as f64;
        let new_level = AutonomyLevel::from_score(avg);

        // Only upgrade, never downgrade automatically (safety)
        if new_level > self.current_level {
            self.current_level = new_level;
        }

        self.current_level
    }

    /// Force a level change (for admin override).
    pub fn force_level(&mut self, level: AutonomyLevel) {
        self.current_level = level;
    }

    pub fn current(&self) -> AutonomyLevel {
        self.current_level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autonomy_levels() {
        assert_eq!(AutonomyLevel::from_score(95.0), AutonomyLevel::Full);
        assert_eq!(AutonomyLevel::from_score(80.0), AutonomyLevel::High);
        assert_eq!(AutonomyLevel::from_score(60.0), AutonomyLevel::Medium);
        assert_eq!(AutonomyLevel::from_score(40.0), AutonomyLevel::Low);
        assert_eq!(AutonomyLevel::from_score(20.0), AutonomyLevel::Frozen);
    }

    #[test]
    fn test_slider_upgrade_only() {
        let mut slider = AutonomySlider::new(3);

        // Not enough observations
        assert_eq!(slider.update(90.0), AutonomyLevel::Low);
        assert_eq!(slider.update(90.0), AutonomyLevel::Low);

        // Third observation triggers evaluation (avg = 90.0 → High)
        let level = slider.update(90.0);
        assert_eq!(level, AutonomyLevel::High);

        // Downgrade doesn't happen automatically
        let level = slider.update(20.0);
        assert_eq!(level, AutonomyLevel::High); // Still High
    }
}
