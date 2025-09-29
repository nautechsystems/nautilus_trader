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

use alloy_primitives::U256;

/// Returns the position of the most significant bit (highest set bit) in a U256 number.
pub fn most_significant_bit(x: U256) -> i32 {
    if x.is_zero() {
        return 0;
    }

    255 - x.leading_zeros() as i32
}

/// Returns the position of the least significant bit (lowest set bit) in a U256 number.
pub fn least_significant_bit(x: U256) -> i32 {
    if x.is_zero() {
        return 0;
    }
    x.trailing_zeros() as i32
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_most_significant_bit() {
        for i in 0..=255 {
            let x = U256::ONE << i;
            assert_eq!(most_significant_bit(x), i);
        }
        for i in 1..=255 {
            let x = (U256::ONE << i) - U256::ONE;
            assert_eq!(most_significant_bit(x), i - 1);
        }
        assert_eq!(most_significant_bit(U256::MAX), 255);
    }

    #[rstest]
    fn test_least_significant_bit() {
        for i in 0..=255 {
            let x = U256::ONE << i;
            assert_eq!(least_significant_bit(x), i);
        }
        for i in 1..=255 {
            let x = (U256::ONE << i) - U256::ONE;
            assert_eq!(least_significant_bit(x), 0);
        }
        assert_eq!(least_significant_bit(U256::MAX), 0);
    }
}
