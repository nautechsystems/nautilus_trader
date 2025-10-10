use std::fmt::Display;

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_model::data::bar::Bar;

use crate::indicator::Indicator;

const MAX_PERIOD: usize = 1_024;

/// Money Flow Index (MFI)
///
/// Uses typical price and volume to measure buying/selling pressure across a rolling window.
/// Value is in the range [0.0, 1.0], consistent with RSI-style normalization used in this crate.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct MoneyFlowIndex {
    pub period: usize,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    // Rolling buffers
    pos_flow: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    neg_flow: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    // State to compute price change sign
    last_typical: f64,
}

impl Display for MoneyFlowIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for MoneyFlowIndex {
    fn name(&self) -> String {
        stringify!(MoneyFlowIndex).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        let typical_price = (bar.high.as_f64() + bar.low.as_f64() + bar.close.as_f64()) / 3.0;
        let volume: f64 = (&bar.volume).into();
        self.update_raw(typical_price, volume);
    }

    fn reset(&mut self) {
        self.value = 0.5;  // Reset to neutral
        self.count = 0;
        self.has_inputs = false;
        self.initialized = false;
        self.last_typical = 0.0;
        self.pos_flow.clear();
        self.neg_flow.clear();
    }
}

impl MoneyFlowIndex {
    /// Creates a new [`MoneyFlowIndex`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is not positive (> 0) or exceeds `MAX_PERIOD`.
    #[must_use]
    pub fn new(period: usize) -> Self {
        assert!(period > 0, "MoneyFlowIndex: period must be > 0");
        assert!(
            period <= MAX_PERIOD,
            "MoneyFlowIndex: period {period} exceeds MAX_PERIOD ({MAX_PERIOD})"
        );

        Self {
            period,
            value: 0.5,  // Start neutral
            count: 0,
            initialized: false,
            has_inputs: false,
            pos_flow: ArrayDeque::new(),
            neg_flow: ArrayDeque::new(),
            last_typical: 0.0,
        }
    }

    /// Update with a bar's typical price and volume.
    pub fn update_raw(&mut self, typical_price: f64, volume: f64) -> f64 {
        // Seed on first input
        if !self.has_inputs {
            self.has_inputs = true;
            self.last_typical = typical_price;
            // First sample has no prior change; MFI conventionally starts from neutral (0.5)
            self.push_flows(0.0, 0.0);
            self.recompute();
            return self.value;
        }

        // Money flow = typical_price * volume; sign determined by price change
        let raw_flow = typical_price * volume;
        
        // Handle non-finite values
        if !raw_flow.is_finite() {
            self.push_flows(0.0, 0.0);
            self.last_typical = typical_price;
            self.recompute();
            return self.value;
        }
        
        if typical_price > self.last_typical {
            self.push_flows(raw_flow, 0.0);
        } else if typical_price < self.last_typical {
            self.push_flows(0.0, raw_flow);
        } else {
            // Flat price, both positive and negative flows are zero for this step
            self.push_flows(0.0, 0.0);
        }

        self.last_typical = typical_price;
        self.recompute();
        self.value
    }

    fn push_flows(&mut self, pos: f64, neg: f64) {
        if self.pos_flow.len() == self.period {
            self.pos_flow.pop_front();
        } else {
            self.count += 1;
        }
        if self.neg_flow.len() == self.period {
            self.neg_flow.pop_front();
        }
        let _ = self.pos_flow.push_back(pos);
        let _ = self.neg_flow.push_back(neg);

        if !self.initialized && self.pos_flow.len() >= self.period {
            self.initialized = true;
        }
    }

    fn recompute(&mut self) {
        // Keep neutral until fully warmed up
        if self.pos_flow.len() < self.period {
            self.value = 0.5;
            return;
        }
        
        let pos_sum: f64 = self.pos_flow.iter().sum();
        let neg_sum: f64 = self.neg_flow.iter().sum();

        // Avoid division by zero and enforce neutral start when no flows exist yet.
        if pos_sum == 0.0 && neg_sum == 0.0 {
            // Neutral baseline when no directional money flow has been observed.
            self.value = 0.5;
            return;
        }

        // If there is positive flow but no negative flow, saturate to 1.0 (max buying pressure).
        if neg_sum == 0.0 {
            self.value = 1.0;
            return;
        }

        let mf_ratio = pos_sum / neg_sum;
        // RSI-style normalization: 1 - 1/(1+R)
        self.value = 1.0 - (1.0 / (1.0 + mf_ratio));
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::bar::Bar;
    use rstest::{fixture, rstest};

    use super::MoneyFlowIndex;
    use crate::indicator::Indicator;
    use crate::stubs::bar_ethusdt_binance_minute_bid;

    #[rstest]
    fn test_mfi_initialized(mfi_10: MoneyFlowIndex) {
        let display_str = format!("{mfi_10}");
        assert_eq!(display_str, "MoneyFlowIndex(10)");
        assert_eq!(mfi_10.period, 10);
        assert!(!mfi_10.initialized());
        assert!(!mfi_10.has_inputs());
    }

    #[rstest]
    fn test_handle_bar_single(mut mfi_10: MoneyFlowIndex, bar_ethusdt_binance_minute_bid: Bar) {
        mfi_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert!(mfi_10.has_inputs());
        // First sample seeded neutral; value well-defined
        assert!(mfi_10.value >= 0.0 && mfi_10.value <= 1.0);
    }

    #[rstest]
    fn test_update_increasing_prices_sets_high_mfi(mut mfi_10: MoneyFlowIndex) {
        // Increasing typical prices with constant volume → positive flows dominate
        for i in 1..=mfi_10.period {
            let tp = 100.0 + i as f64;
            mfi_10.update_raw(tp, 10.0);
        }
        assert!(mfi_10.initialized());
        assert!(mfi_10.value > 0.5);
    }

    #[rstest]
    fn test_update_decreasing_prices_sets_low_mfi(mut mfi_10: MoneyFlowIndex) {
        // Decreasing typical prices with constant volume → negative flows dominate
        // Start with a seed
        mfi_10.update_raw(100.0, 10.0);
        for i in (1..=mfi_10.period).rev() {
            let tp = 100.0 + i as f64 - 1.0;
            mfi_10.update_raw(tp, 10.0);
        }
        assert!(mfi_10.initialized());
        assert!(mfi_10.value < 0.5);
    }

    #[rstest]
    fn test_reset(mut mfi_10: MoneyFlowIndex) {
        mfi_10.update_raw(100.0, 10.0);
        mfi_10.reset();
        assert_eq!(mfi_10.value, 0.5);  // Should reset to neutral
        assert_eq!(mfi_10.count, 0);
        assert!(!mfi_10.has_inputs());
        assert!(!mfi_10.initialized());
        assert_eq!(mfi_10.pos_flow.len(), 0);
        assert_eq!(mfi_10.neg_flow.len(), 0);
    }

    #[rstest]
    fn test_neutral_value_when_no_flows(mut mfi_10: MoneyFlowIndex) {
        // First update seeds with no prior change; both pos/neg sums remain zero.
        mfi_10.update_raw(100.0, 10.0);
        assert!((mfi_10.value - 0.5).abs() < 1e-12);
    }

    // Fixture
    #[fixture]
    fn mfi_10() -> MoneyFlowIndex {
        MoneyFlowIndex::new(10)
    }
}
