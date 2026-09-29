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

pub(crate) fn percentage_gain(gain: f64, loss: f64) -> f64 {
    let denominator = gain + loss;

    if denominator == 0.0 {
        return 50.0;
    }

    if denominator.is_finite() {
        return 100.0 * (gain / denominator);
    }

    let scale = gain.abs().max(loss.abs());
    let gain = gain / scale;
    let loss = loss / scale;
    100.0 * (gain / (gain + loss))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::percentage_gain;

    #[rstest]
    #[case(3.0, 1.0, 75.0)]
    #[case(0.0, 0.0, 50.0)]
    #[case(2.0, 0.0, 100.0)]
    #[case(0.0, 2.0, 0.0)]
    #[case(1e308, 1e308, 50.0)]
    #[case(f64::MAX, f64::MAX, 50.0)]
    fn test_percentage_gain(#[case] gain: f64, #[case] loss: f64, #[case] expected: f64) {
        assert_eq!(percentage_gain(gain, loss), expected);
    }
}
