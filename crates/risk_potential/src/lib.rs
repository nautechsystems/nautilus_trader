//! Differentiable risk potential field for KuaaMU Quant Engine.
//!
//! Risk is not a hard wall but a potential field. Agents sense the gradient
//! and self-regulate. The engine retains non-bypassable hard limits.

/// Risk potential field with analytical gradient computation.
///
/// Uses logarithmic barrier functions: as position approaches limit,
/// potential → +∞. Gradient tells the agent which direction to move.
#[derive(Clone, Copy, Debug)]
pub struct RiskPotentialField {
    /// Maximum absolute position size.
    pub position_limit: f64,
    /// Maximum drawdown (negative, e.g., -0.05 for -5%).
    pub drawdown_limit: f64,
    /// Maximum concentration in a single instrument (0.0-1.0).
    pub concentration_limit: f64,
}

impl RiskPotentialField {
    pub fn new(position_limit: f64, drawdown_limit: f64, concentration_limit: f64) -> Self {
        Self {
            position_limit,
            drawdown_limit,
            concentration_limit,
        }
    }

    /// Position potential: U(p) = -ln(1 - |p|/p_max)
    /// Approaches +∞ as |p| → p_max.
    #[inline(always)]
    pub fn position_potential(&self, position: f64) -> f64 {
        let ratio = position.abs() / self.position_limit;
        if ratio >= 1.0 {
            f64::INFINITY
        } else {
            -((1.0 - ratio).ln())
        }
    }

    /// Position gradient: dU/dp = sign(p) / (1 - |p|/p_max) / p_max
    /// Positive gradient = direction to reduce position.
    #[inline(always)]
    pub fn position_gradient(&self, position: f64) -> f64 {
        let ratio = position.abs() / self.position_limit;
        if ratio >= 1.0 {
            f64::INFINITY * position.signum()
        } else {
            position.signum() / ((1.0 - ratio) * self.position_limit)
        }
    }

    /// Drawdown potential: U(dd) = -ln(1 - dd/dd_limit)
    /// dd should be negative (drawdown), dd_limit is negative.
    #[inline(always)]
    pub fn drawdown_potential(&self, drawdown: f64) -> f64 {
        if self.drawdown_limit >= 0.0 {
            return 0.0;
        }
        let ratio = drawdown / self.drawdown_limit; // both negative → positive ratio
        if ratio >= 1.0 {
            f64::INFINITY
        } else if ratio <= 0.0 {
            0.0
        } else {
            -((1.0 - ratio).ln())
        }
    }

    /// Concentration potential: U(c) = -ln(1 - c/c_max)
    #[inline(always)]
    pub fn concentration_potential(&self, weight: f64) -> f64 {
        let ratio = weight.abs() / self.concentration_limit;
        if ratio >= 1.0 {
            f64::INFINITY
        } else if ratio <= 0.0 {
            0.0
        } else {
            -((1.0 - ratio).ln())
        }
    }

    /// Total potential field.
    #[inline(always)]
    pub fn total(&self, position: f64, drawdown: f64, max_weight: f64) -> f64 {
        self.position_potential(position)
            + self.drawdown_potential(drawdown)
            + self.concentration_potential(max_weight)
    }

    /// Full gradient vector (for agent consumption).
    #[inline(always)]
    pub fn gradient(&self, position: f64, drawdown: f64, max_weight: f64) -> RiskGradient {
        RiskGradient {
            position: self.position_gradient(position),
            drawdown: self.drawdown_gradient(drawdown),
            concentration: self.concentration_gradient(max_weight),
        }
    }

    fn drawdown_gradient(&self, drawdown: f64) -> f64 {
        if self.drawdown_limit >= 0.0 {
            return 0.0;
        }
        let ratio = drawdown / self.drawdown_limit;
        if ratio >= 1.0 {
            f64::INFINITY
        } else if ratio <= 0.0 {
            0.0
        } else {
            1.0 / ((1.0 - ratio) * self.drawdown_limit)
        }
    }

    fn concentration_gradient(&self, weight: f64) -> f64 {
        let ratio = weight.abs() / self.concentration_limit;
        if ratio >= 1.0 {
            f64::INFINITY
        } else if ratio <= 0.0 {
            0.0
        } else {
            weight.signum() / ((1.0 - ratio) * self.concentration_limit)
        }
    }

    /// Check hard limits (non-bypassable). Returns true if within limits.
    #[inline(always)]
    pub fn hard_check(&self, position: f64, drawdown: f64, max_weight: f64) -> bool {
        position.abs() < self.position_limit
            && drawdown > self.drawdown_limit
            && max_weight < self.concentration_limit
    }
}

/// Gradient vector from the risk potential field.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RiskGradient {
    /// Gradient for position (positive = reduce).
    pub position: f64,
    /// Gradient for drawdown.
    pub drawdown: f64,
    /// Gradient for concentration.
    pub concentration: f64,
}

impl RiskGradient {
    /// Total pressure magnitude (L2 norm).
    #[inline(always)]
    pub fn magnitude(&self) -> f64 {
        (self.position.powi(2) + self.drawdown.powi(2) + self.concentration.powi(2)).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_field() -> RiskPotentialField {
        RiskPotentialField::new(1000.0, -0.05, 0.3)
    }

    #[test]
    fn test_position_potential_zero() {
        let field = default_field();
        let p = field.position_potential(0.0);
        assert!((p - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_position_potential_increases() {
        let field = default_field();
        let p1 = field.position_potential(100.0);
        let p2 = field.position_potential(500.0);
        let p3 = field.position_potential(900.0);
        assert!(p1 < p2);
        assert!(p2 < p3);
    }

    #[test]
    fn test_position_potential_at_limit() {
        let field = default_field();
        let p = field.position_potential(1000.0);
        assert!(p.is_infinite());
    }

    #[test]
    fn test_position_gradient_direction() {
        let field = default_field();
        // Positive position → positive gradient (reduce)
        let g = field.position_gradient(500.0);
        assert!(g > 0.0);

        // Negative position → negative gradient (increase)
        let g = field.position_gradient(-500.0);
        assert!(g < 0.0);
    }

    #[test]
    fn test_hard_check_within_limits() {
        let field = default_field();
        assert!(field.hard_check(500.0, -0.02, 0.2));
    }

    #[test]
    fn test_hard_check_exceeds_position() {
        let field = default_field();
        assert!(!field.hard_check(1001.0, -0.02, 0.2));
    }

    #[test]
    fn test_hard_check_exceeds_drawdown() {
        let field = default_field();
        assert!(!field.hard_check(500.0, -0.06, 0.2));
    }

    #[test]
    fn test_total_potential() {
        let field = default_field();
        let total = field.total(500.0, -0.02, 0.2);
        assert!(total > 0.0);
    }

    #[test]
    fn test_gradient_magnitude() {
        let grad = RiskGradient {
            position: 3.0,
            drawdown: 4.0,
            concentration: 0.0,
        };
        assert!((grad.magnitude() - 5.0).abs() < 1e-10);
    }
}
