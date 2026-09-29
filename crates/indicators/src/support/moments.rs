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

use super::sum::ScaledSum;

#[derive(Debug, Clone)]
pub(crate) struct ShiftedMoments {
    offset: f64,
    seeded: bool,
    sum: f64,
    sum_sq: f64,
    pushes_since_reseed: usize,
}

impl ShiftedMoments {
    pub(crate) const fn new() -> Self {
        Self {
            offset: 0.0,
            seeded: false,
            sum: 0.0,
            sum_sq: 0.0,
            pushes_since_reseed: 0,
        }
    }

    pub(crate) fn push(&mut self, value: f64) {
        if !self.seeded {
            self.offset = value;
            self.seeded = true;
        }
        let shifted = value - self.offset;
        self.sum += shifted;
        self.sum_sq += shifted * shifted;
        self.pushes_since_reseed += 1;
    }

    pub(crate) fn evict(&mut self, value: f64) {
        let shifted = value - self.offset;
        self.sum -= shifted;
        self.sum_sq -= shifted * shifted;
    }

    pub(crate) fn mean(&self, n: usize) -> f64 {
        self.offset + self.sum / n as f64
    }

    pub(crate) fn std_dev<'a>(
        &self,
        n: usize,
        values: impl Iterator<Item = &'a f64> + Clone,
    ) -> f64 {
        let mean = self.sum / n as f64;
        let variance = self.sum_sq / n as f64 - mean * mean;
        if variance.is_finite() {
            return variance.max(0.0).sqrt();
        }
        let scale = values.clone().copied().map(f64::abs).fold(0.0, f64::max);
        if scale == 0.0 {
            return 0.0;
        }
        let normalized_mean = values.clone().map(|value| value / scale).sum::<f64>() / n as f64;
        let squares = values
            .map(|value| (value / scale - normalized_mean).powi(2))
            .sum::<f64>();
        (squares / n as f64).sqrt() * scale
    }

    pub(crate) const fn needs_reseed(&self, period: usize) -> bool {
        self.pushes_since_reseed >= period
    }

    pub(crate) fn reseed<'a, I>(&mut self, values: I)
    where
        I: IntoIterator<Item = &'a f64>,
        I::IntoIter: Clone,
    {
        let values = values.into_iter();
        let count = values.clone().count();
        if count == 0 {
            self.reset();
            return;
        }
        self.offset = values.clone().copied().sum::<ScaledSum>().mean(count);
        self.seeded = true;
        self.sum = 0.0;
        self.sum_sq = 0.0;

        for &value in values {
            let shifted = value - self.offset;
            self.sum += shifted;
            self.sum_sq += shifted * shifted;
        }
        self.pushes_since_reseed = 0;
    }

    pub(crate) fn reset(&mut self) {
        self.offset = 0.0;
        self.seeded = false;
        self.sum = 0.0;
        self.sum_sq = 0.0;
        self.pushes_since_reseed = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn shifted_moments_eviction_and_reset() {
        let mut moments = ShiftedMoments::new();
        let mut window = VecDeque::from([1.0e12 + 1.0, 1.0e12 + 3.0, 1.0e12 + 5.0]);
        for &value in &window {
            moments.push(value);
        }
        assert!(moments.needs_reseed(3));
        moments.reseed(&window);
        assert_eq!(moments.mean(3), 1.0e12 + 3.0);
        assert_eq!(moments.std_dev(3, window.iter()), (8.0_f64 / 3.0).sqrt());

        moments.evict(window.pop_front().unwrap());
        window.push_back(1.0e12 + 7.0);
        moments.push(1.0e12 + 7.0);
        moments.reseed(&window);
        assert_eq!(moments.mean(3), 1.0e12 + 5.0);
        assert_eq!(moments.std_dev(3, window.iter()), (8.0_f64 / 3.0).sqrt());
        assert!(!moments.needs_reseed(1));

        moments.reset();
        moments.push(-7.0);
        assert_eq!(moments.mean(1), -7.0);
        assert_eq!(moments.std_dev(1, [-7.0].iter()), 0.0);
        moments.reseed(&VecDeque::new());
        assert!(!moments.needs_reseed(1));
        moments.push(11.0);
        assert_eq!(moments.mean(1), 11.0);
    }

    #[rstest]
    #[case(2)]
    #[case(7)]
    #[case(31)]
    fn shifted_moments_match_centered_reference_over_reseeds(#[case] period: usize) {
        let mut moments = ShiftedMoments::new();
        let mut window = VecDeque::new();

        for i in 0..10_000 {
            let value = 1.0e12 + f64::from((i * 17) % 101) / 8.0;

            if window.len() == period {
                moments.evict(window.pop_front().unwrap());
            }
            window.push_back(value);
            moments.push(value);
            if moments.needs_reseed(period) {
                moments.reseed(&window);
            }

            if window.len() < period {
                continue;
            }
            let anchor = window[0];
            let mean = window.iter().map(|x| x - anchor).sum::<f64>() / period as f64;
            let squared = window
                .iter()
                .map(|x| (x - anchor - mean).powi(2))
                .sum::<f64>();
            let population = (squared / period as f64).sqrt();
            assert!((moments.std_dev(period, window.iter()) - population).abs() <= 1.0e-6);
        }
    }
}
