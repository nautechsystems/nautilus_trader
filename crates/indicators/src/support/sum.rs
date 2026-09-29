// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::iter::Sum;

pub(crate) const SMA_RESEED_WINDOWS: usize = 16;
pub(crate) const MAX_PERIOD: usize = 1 << 24;

#[derive(Debug, Clone)]
pub(crate) struct ScaledSum {
    total: f64,
    scale: Option<f64>,
    recovery: usize,
}

impl ScaledSum {
    pub(crate) const fn new() -> Self {
        Self {
            total: 0.0,
            scale: None,
            recovery: 0,
        }
    }

    pub(crate) fn add(&mut self, value: f64) {
        if let Some(scale) = self.scale {
            if self.total == 0.0 {
                self.total += value;
                self.scale = None;
                return;
            }

            let next_scale = scale.max(value.abs());
            self.total = self.total * (scale / next_scale) + value / next_scale;
            self.scale = Some(next_scale);
        } else {
            let total = self.total + value;

            if total.is_finite() || !value.is_finite() {
                self.total = total;
            } else {
                let scale = self.total.abs().max(value.abs());
                self.total = self.total / scale + value / scale;
                self.scale = Some(scale);
            }
        }
    }

    pub(crate) const fn value(&self) -> f64 {
        match self.scale {
            Some(scale) => self.total * scale,
            None => self.total,
        }
    }

    pub(crate) const fn needs_rebuild(&self) -> bool {
        self.scale.is_some() || self.recovery > 0
    }

    pub(crate) fn mean(&self, count: usize) -> f64 {
        let value = self.value();

        if value.is_finite() {
            return value / count as f64;
        }

        match self.scale {
            Some(scale) => (self.total / count as f64).clamp(-1.0, 1.0) * scale,
            None => self.total / count as f64,
        }
    }

    pub(crate) fn ratio(&self, denominator: &Self) -> f64 {
        let numerator_value = self.value();
        let denominator_value = denominator.value();

        if numerator_value.is_finite() && denominator_value.is_finite() {
            return numerator_value / denominator_value;
        }

        if numerator_value.is_finite() {
            return (numerator_value / denominator.total) / denominator.scale.unwrap_or(1.0);
        }

        if denominator_value.is_finite() {
            return self.total * (self.scale.unwrap_or(1.0) / denominator_value);
        }

        (self.total / denominator.total)
            * (self.scale.unwrap_or(1.0) / denominator.scale.unwrap_or(1.0))
    }

    pub(crate) fn rebuild<I: Iterator<Item = f64>>(&mut self, values: I, count: usize) {
        // Rebuild for a complete eviction window after overflow: scaling can lose
        // small contributions which matter once the large samples leave.
        let recovery = if self.scale.is_some() {
            count
        } else {
            self.recovery.saturating_sub(1)
        };
        *self = values.sum();
        self.recovery = recovery;
    }

    pub(crate) const fn reset(&mut self) {
        self.total = 0.0;
        self.scale = None;
        self.recovery = 0;
    }
}

impl Sum<f64> for ScaledSum {
    fn sum<I: Iterator<Item = f64>>(values: I) -> Self {
        let mut sum = Self::new();

        for value in values {
            sum.add(value);
        }

        sum
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(1e308)]
    #[case(f64::MAX)]
    #[case(-f64::MAX)]
    fn scaled_sum_preserves_finite_constant_means(#[case] value: f64) {
        let mut sum = ScaledSum::new();

        for count in 1..=31 {
            sum.add(value);
            assert_eq!(sum.mean(count), value);
        }

        sum.reset();
        assert_eq!(sum.total, 0.0);
        assert_eq!(sum.scale, None);
    }

    #[rstest]
    fn scaled_sums_compare_overflowing_totals_without_infinity_division() {
        let numerator: ScaledSum = [1e308, 1e308, 1e308].into_iter().sum();
        let denominator: ScaledSum = [1e308, 1e308].into_iter().sum();

        assert_eq!(numerator.ratio(&denominator), 1.5);
        assert_eq!(denominator.ratio(&numerator), 2.0 / 3.0);
    }

    #[rstest]
    fn representable_ratios_and_means_do_not_underflow_their_scaled_components() {
        let numerator: ScaledSum = [1e308, 1e308, -1e308, -1e308, 4.940_656_458_412_465_5e-16]
            .into_iter()
            .sum();
        let denominator: ScaledSum = [1e-100].into_iter().sum();

        assert_eq!(numerator.ratio(&denominator), 4.940_656_458_412_466e84);
        assert_eq!(numerator.mean(5), 9.881_312_916_824_931e-17);
    }

    #[rstest]
    fn cancellation_restores_small_representable_contributions() {
        let sum: ScaledSum = [1e308, 1e308, -1e308, -1e308, 1e-100].into_iter().sum();

        assert_eq!(sum.value(), 1e-100);
        assert_eq!(sum.mean(5), 2e-101);
    }

    #[rstest]
    fn overflowing_denominator_preserves_a_representable_subnormal_ratio() {
        let numerator: ScaledSum = [1.230e-15].into_iter().sum();
        let denominator: ScaledSum = [1e308, 1e308, 1e308, 1e308, 9e307].into_iter().sum();

        assert_eq!(numerator.ratio(&denominator), f64::from_bits(1));
    }
}
