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

// Past this ratio of squared offset drift to variance, `sum_sq / n - mean^2`
// cancels more than about ten of its mantissa bits.
const STALE_DRIFT_RATIO: f64 = 1024.0;

/// Reports whether the window mean has drifted so far from the reference point
/// that the shifted variance `sum_sq / n - mean^2` loses its precision.
///
/// `sum` and `sum_sq` are the window's sums of `value - offset` and its square.
pub(crate) fn offset_is_stale(sum: f64, sum_sq: f64, count: usize) -> bool {
    if count == 0 {
        return false;
    }
    let n = count as f64;
    let mean = sum / n;
    let drift = mean * mean;
    drift > STALE_DRIFT_RATIO * (sum_sq / n - drift).max(0.0)
}

/// Returns the window value closest to `mean`, the reference point a reseed
/// shifts by. A flat window then shifts to exact zeros, and the drift starts
/// within one standard deviation, so a fresh reseed is never stale.
pub(crate) fn centered_offset<'a, I>(values: I, mean: f64) -> f64
where
    I: Iterator<Item = &'a f64>,
{
    values
        .copied()
        .min_by(|a, b| (a - mean).abs().total_cmp(&(b - mean).abs()))
        .unwrap_or(mean)
}

#[derive(Debug, Clone)]
pub(crate) struct ShiftedMoments {
    offset: f64,
    seeded: bool,
    sum: f64,
    sum_sq: f64,
    count: usize,
    pushes_since_reseed: usize,
}

impl ShiftedMoments {
    pub(crate) const fn new() -> Self {
        Self {
            offset: 0.0,
            seeded: false,
            sum: 0.0,
            sum_sq: 0.0,
            count: 0,
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
        self.count += 1;
        self.pushes_since_reseed += 1;
    }

    pub(crate) fn evict(&mut self, value: f64) {
        let shifted = value - self.offset;
        self.sum -= shifted;
        self.sum_sq -= shifted * shifted;
        self.count -= 1;
    }

    pub(crate) fn mean(&self, n: usize) -> f64 {
        self.offset + self.sum / n as f64
    }

    pub(crate) fn std_dev(&self, n: usize) -> f64 {
        let mean = self.sum / n as f64;
        (self.sum_sq / n as f64 - mean * mean).max(0.0).sqrt()
    }

    pub(crate) fn needs_reseed(&self, period: usize) -> bool {
        self.pushes_since_reseed >= period || offset_is_stale(self.sum, self.sum_sq, self.count)
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
        let mean = values.clone().sum::<f64>() / count as f64;
        self.offset = centered_offset(values.clone(), mean);
        self.seeded = true;
        self.sum = 0.0;
        self.sum_sq = 0.0;
        self.count = count;

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
        self.count = 0;
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
        assert_eq!(moments.std_dev(3), (8.0_f64 / 3.0).sqrt());

        moments.evict(window.pop_front().unwrap());
        window.push_back(1.0e12 + 7.0);
        moments.push(1.0e12 + 7.0);
        moments.reseed(&window);
        assert_eq!(moments.mean(3), 1.0e12 + 5.0);
        assert_eq!(moments.std_dev(3), (8.0_f64 / 3.0).sqrt());
        assert!(!moments.needs_reseed(1));

        moments.reset();
        moments.push(-7.0);
        assert_eq!(moments.mean(1), -7.0);
        assert_eq!(moments.std_dev(1), 0.0);
        moments.reseed(&VecDeque::new());
        assert!(!moments.needs_reseed(1));
        moments.push(11.0);
        assert_eq!(moments.mean(1), 11.0);
    }

    #[rstest]
    fn stale_offset_reseeds_before_the_periodic_reseed() {
        let tiny = 2.0_f64.powi(-24);
        let mut moments = ShiftedMoments::new();
        moments.push(110.0);
        moments.push(100.0);
        assert!(!moments.needs_reseed(10));
        moments.evict(110.0);
        moments.push(100.0 + tiny);
        // Three pushes are short of the period, but the offset is stale.
        assert!(moments.needs_reseed(10));
        moments.reseed(&VecDeque::from([100.0, 100.0 + tiny]));
        assert!(!moments.needs_reseed(10));
        assert_eq!(moments.mean(2), 100.0 + tiny / 2.0);
        assert_eq!(moments.std_dev(2), tiny / 2.0);
    }

    #[rstest]
    fn flat_window_reseed_never_reports_stale() {
        let mut moments = ShiftedMoments::new();
        let window = VecDeque::from([0.1, 0.1, 0.1]);
        moments.push(7.0);
        moments.reseed(&window);
        assert!(!moments.needs_reseed(4));
        assert_eq!(moments.mean(3), 0.1);
        assert_eq!(moments.std_dev(3), 0.0);
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
            assert!((moments.std_dev(period) - population).abs() <= 1.0e-6);
        }
    }
}
