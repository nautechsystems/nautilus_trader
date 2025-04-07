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

#[inline]
#[must_use]
pub fn linear_weight(x1: f64, x2: f64, x: f64) -> f64 {
    (x - x1) / (x2 - x1)
}

#[inline]
#[must_use]
pub fn linear_weighting(y1: f64, y2: f64, x1_diff: f64) -> f64 {
    x1_diff.mul_add(y2 - y1, y1)
}

#[inline]
#[must_use]
pub fn pos_search(x: f64, xs: &[f64]) -> usize {
    let n_elem = xs.len();
    let pos = xs.partition_point(|&val| val < x);
    std::cmp::min(std::cmp::max(pos.saturating_sub(1), 0), n_elem - 1)
}

#[inline]
#[must_use]
pub fn quad_polynomial(x: f64, x0: f64, x1: f64, x2: f64, y0: f64, y1: f64, y2: f64) -> f64 {
    y0 * (x - x1) * (x - x2) / ((x0 - x1) * (x0 - x2))
        + y1 * (x - x0) * (x - x2) / ((x1 - x0) * (x1 - x2))
        + y2 * (x - x0) * (x - x1) / ((x2 - x0) * (x2 - x1))
}

#[must_use]
pub fn quadratic_interpolation(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
    let n_elem = xs.len();
    let epsilon = 1e-8;

    assert!(
        (n_elem >= 3),
        "Need at least 3 points for quadratic interpolation"
    );

    if x <= xs[0] {
        return ys[0];
    }

    if x >= xs[n_elem - 1] {
        return ys[n_elem - 1];
    }

    let pos = pos_search(x, xs);

    if (xs[pos] - x).abs() < epsilon {
        return ys[pos];
    }

    if pos == 0 {
        return quad_polynomial(x, xs[0], xs[1], xs[2], ys[0], ys[1], ys[2]);
    }

    if pos == n_elem - 2 {
        return quad_polynomial(
            x,
            xs[n_elem - 3],
            xs[n_elem - 2],
            xs[n_elem - 1],
            ys[n_elem - 3],
            ys[n_elem - 2],
            ys[n_elem - 1],
        );
    }

    let w = linear_weight(xs[pos], xs[pos + 1], x);

    linear_weighting(
        quad_polynomial(
            x,
            xs[pos - 1],
            xs[pos],
            xs[pos + 1],
            ys[pos - 1],
            ys[pos],
            ys[pos + 1],
        ),
        quad_polynomial(
            x,
            xs[pos],
            xs[pos + 1],
            xs[pos + 2],
            ys[pos],
            ys[pos + 1],
            ys[pos + 2],
        ),
        w,
    )
}
