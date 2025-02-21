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

use std::fmt::Display;

use nautilus_core::correctness::{FAILED, check_in_range_inclusive_f64};
use rand::{Rng, SeedableRng, rngs::StdRng};

#[derive(Debug, Clone)]
pub struct FillModel {
    /// The probability of limit order filling if the market rests on its price.
    prob_fill_on_limit: f64,
    /// The probability of stop orders filling if the market rests on its price.
    prob_fill_on_stop: f64,
    /// The probability of order fill prices slipping by one tick.
    prob_slippage: f64,
    /// Random number generator
    rng: StdRng,
}

impl FillModel {
    /// Creates a new [`FillModel`] instance.
    pub fn new(
        prob_fill_on_limit: f64,
        prob_fill_on_stop: f64,
        prob_slippage: f64,
        random_seed: Option<u64>,
    ) -> anyhow::Result<Self> {
        check_in_range_inclusive_f64(prob_fill_on_limit, 0.0, 1.0, "prob_fill_on_limit")
            .expect(FAILED);
        check_in_range_inclusive_f64(prob_fill_on_stop, 0.0, 1.0, "prob_fill_on_stop")
            .expect(FAILED);
        check_in_range_inclusive_f64(prob_slippage, 0.0, 1.0, "prob_slippage").expect(FAILED);
        let rng = match random_seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_os_rng(),
        };
        Ok(Self {
            prob_fill_on_limit,
            prob_fill_on_stop,
            prob_slippage,
            rng,
        })
    }

    pub fn is_limit_filled(&mut self) -> bool {
        self.event_success(self.prob_fill_on_limit)
    }

    pub fn is_stop_filled(&mut self) -> bool {
        self.event_success(self.prob_fill_on_stop)
    }

    pub fn is_slipped(&mut self) -> bool {
        self.event_success(self.prob_slippage)
    }

    fn event_success(&mut self, probability: f64) -> bool {
        match probability {
            0.0 => false,
            1.0 => true,
            _ => self.rng.random_bool(probability),
        }
    }
}

impl Display for FillModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FillModel(prob_fill_on_limit: {}, prob_fill_on_stop: {}, prob_slippage: {})",
            self.prob_fill_on_limit, self.prob_fill_on_stop, self.prob_slippage
        )
    }
}

impl Default for FillModel {
    /// Creates a new default [`FillModel`] instance.
    fn default() -> Self {
        Self::new(0.5, 0.5, 0.1, None).unwrap()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn fill_model() -> FillModel {
        let seed = 42;
        FillModel::new(0.5, 0.5, 0.1, Some(seed)).unwrap()
    }

    #[rstest]
    #[should_panic(
        expected = "Condition failed: invalid f64 for 'prob_fill_on_limit' not in range [0, 1], was 1.1"
    )]
    fn test_fill_model_param_prob_fill_on_limit_error() {
        let _ = super::FillModel::new(1.1, 0.5, 0.1, None).unwrap();
    }

    #[rstest]
    #[should_panic(
        expected = "Condition failed: invalid f64 for 'prob_fill_on_stop' not in range [0, 1], was 1.1"
    )]
    fn test_fill_model_param_prob_fill_on_stop_error() {
        let _ = super::FillModel::new(0.5, 1.1, 0.1, None).unwrap();
    }

    #[rstest]
    #[should_panic(
        expected = "Condition failed: invalid f64 for 'prob_slippage' not in range [0, 1], was 1.1"
    )]
    fn test_fill_model_param_prob_slippage_error() {
        let _ = super::FillModel::new(0.5, 0.5, 1.1, None).unwrap();
    }

    #[rstest]
    fn test_fill_model_is_limit_filled(mut fill_model: FillModel) {
        // because of fixed seed this is deterministic
        let result = fill_model.is_limit_filled();
        assert!(!result);
    }

    #[rstest]
    fn test_fill_model_is_stop_filled(mut fill_model: FillModel) {
        // because of fixed seed this is deterministic
        let result = fill_model.is_stop_filled();
        assert!(!result);
    }

    #[rstest]
    fn test_fill_model_is_slipped(mut fill_model: FillModel) {
        // because of fixed seed this is deterministic
        let result = fill_model.is_slipped();
        assert!(!result);
    }
}
