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

use std::collections::VecDeque;

use super::moments::{centered_offset, offset_is_stale};

// Rolling regression sums held relative to a reference point inside the window,
// with a periodic reseed from the live window.
// Accumulating `y - offset` keeps the residual cancellation on the order of the
// spread inside the window rather than of the price level.
#[derive(Debug)]
struct ShiftedTrend {
    offset: f64,
    seeded: bool,
    sum_y: f64,
    sum_xy: f64,
    sum_y_sq: f64,
    count: usize,
    pushes_since_reseed: usize,
}

impl ShiftedTrend {
    const fn new() -> Self {
        Self {
            offset: 0.0,
            seeded: false,
            sum_y: 0.0,
            sum_xy: 0.0,
            sum_y_sq: 0.0,
            count: 0,
            pushes_since_reseed: 0,
        }
    }

    fn push(&mut self, value: f64, index: usize) {
        if !self.seeded {
            self.offset = value;
            self.seeded = true;
        }
        let d = value - self.offset;
        self.sum_y += d;
        self.sum_xy += index as f64 * d;
        self.sum_y_sq += d * d;
        self.count += 1;
        self.pushes_since_reseed += 1;
    }

    // Drop the value at position 0 and shift every remaining index down by
    // one, using `sum((i-1)*y_i) = sum(i*y_i) - sum(y_i) + y_0`.
    fn slide(&mut self, front: f64) {
        let d = front - self.offset;
        self.sum_xy = self.sum_xy - self.sum_y + d;
        self.sum_y -= d;
        self.sum_y_sq -= d * d;
        self.count -= 1;
    }

    fn needs_reseed(&self, period: usize) -> bool {
        self.pushes_since_reseed >= period || offset_is_stale(self.sum_y, self.sum_y_sq, self.count)
    }

    // Rebuild every sum from the live window, re-centering `offset` on the value
    // closest to its mean.
    fn reseed<'a, I>(&mut self, values: I)
    where
        I: Iterator<Item = &'a f64> + Clone,
    {
        let mut total = 0.0;
        let mut count = 0_usize;

        for v in values.clone() {
            total += v;
            count += 1;
        }

        if count == 0 {
            self.reset();
            return;
        }
        self.offset = centered_offset(values.clone(), total / count as f64);
        self.seeded = true;
        self.count = count;
        self.sum_y = 0.0;
        self.sum_xy = 0.0;
        self.sum_y_sq = 0.0;

        for (index, v) in values.enumerate() {
            let d = v - self.offset;
            self.sum_y += d;
            self.sum_xy += index as f64 * d;
            self.sum_y_sq += d * d;
        }
        self.pushes_since_reseed = 0;
    }

    fn reset(&mut self) {
        self.offset = 0.0;
        self.seeded = false;
        self.sum_y = 0.0;
        self.sum_xy = 0.0;
        self.sum_y_sq = 0.0;
        self.count = 0;
        self.pushes_since_reseed = 0;
    }
}

/// Rolling ordinary-least-squares fit of the last `period` inputs against their
/// own position index `x = 0, 1, ..., period - 1`.
///
/// Shared by the linear-regression family (endpoint, slope, intercept, forecast
/// and coefficient of determination), each of which differs only in the quantity
/// it reads back from the same O(1) sliding-window sums.
#[derive(Debug)]
pub(crate) struct RollingOls {
    period: usize,
    window: VecDeque<f64>,
    // Closed form of `sum(x)` over `x = 0, 1, ..., period - 1`.
    sum_x: f64,
    // Closed form of `n * sum(xx) - sum(x)^2`, the OLS denominator.
    denom: f64,
    trend: ShiftedTrend,
}

impl RollingOls {
    pub(crate) fn new(period: usize) -> Self {
        let n = period as f64;
        // Closed forms for x = 0, 1, ..., period - 1.
        let sum_x = n * (n - 1.0) / 2.0;
        let sum_xx = (n - 1.0) * n * (2.0 * n - 1.0) / 6.0;
        Self {
            period,
            window: VecDeque::with_capacity(period),
            sum_x,
            denom: n * sum_xx - sum_x * sum_x,
            trend: ShiftedTrend::new(),
        }
    }

    /// Pushes `value` and reports whether the window is now full.
    pub(crate) fn push(&mut self, value: f64) -> bool {
        if self.window.len() == self.period {
            let front = self.window.pop_front().expect("window is non-empty");
            self.trend.slide(front);
        }
        let index = self.window.len();
        self.window.push_back(value);
        self.trend.push(value, index);
        if self.trend.needs_reseed(self.period) {
            self.trend.reseed(self.window.iter());
        }
        self.window.len() == self.period
    }

    /// Slope `b` of the fit `y = a + b*x`, invariant under the offset shift.
    pub(crate) fn slope(&self) -> f64 {
        let n = self.period as f64;
        (n * self.trend.sum_xy - self.sum_x * self.trend.sum_y) / self.denom
    }

    /// Intercept `a` of the fit, an absolute level, so the reference point the
    /// sums are held relative to comes back here.
    pub(crate) fn intercept(&self, slope: f64) -> f64 {
        let n = self.period as f64;
        (self.trend.sum_y - slope * self.sum_x) / n + self.trend.offset
    }

    pub(crate) const fn sum_y(&self) -> f64 {
        self.trend.sum_y
    }

    pub(crate) const fn sum_y_sq(&self) -> f64 {
        self.trend.sum_y_sq
    }

    pub(crate) const fn denom(&self) -> f64 {
        self.denom
    }

    pub(crate) fn reset(&mut self) {
        self.window.clear();
        self.trend.reset();
    }
}
