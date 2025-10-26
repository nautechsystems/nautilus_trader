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

use alloy_primitives::{Address, U256};
use serde::{Deserialize, Serialize};

use crate::defi::tick_map::full_math::{FullMath, Q128};

/// Represents a concentrated liquidity position in a DEX pool.
///
/// This struct tracks a specific liquidity provider's position within a price range,
/// including the liquidity amount, fee accumulation, and token deposits/withdrawals.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolPosition {
    /// The owner of the position
    pub owner: Address,
    /// The lower tick boundary of the position
    pub tick_lower: i32,
    /// The upper tick boundary of the position
    pub tick_upper: i32,
    /// The amount of liquidity in the position
    pub liquidity: u128,
    /// Fee growth per unit of liquidity for token0 as of the last action on the position
    pub fee_growth_inside_0_last: U256,
    /// Fee growth per unit of liquidity for token1 as of the last action on the position
    pub fee_growth_inside_1_last: U256,
    /// The fees owed to the position for token0
    pub tokens_owed_0: u128,
    /// The fees owed to the position for token1
    pub tokens_owed_1: u128,
    /// Total amount of token0 deposited into this position
    pub total_amount0_deposited: U256,
    /// Total amount of token1 deposited into this position
    pub total_amount1_deposited: U256,
    /// Total amount of token0 collected from this position
    pub total_amount0_collected: u128,
    /// Total amount of token1 collected from this position
    pub total_amount1_collected: u128,
}

impl PoolPosition {
    /// Creates a [`PoolPosition`] with the specified parameters.
    #[must_use]
    pub fn new(owner: Address, tick_lower: i32, tick_upper: i32, liquidity: i128) -> Self {
        Self {
            owner,
            tick_lower,
            tick_upper,
            liquidity: liquidity.unsigned_abs(),
            fee_growth_inside_0_last: U256::ZERO,
            fee_growth_inside_1_last: U256::ZERO,
            tokens_owed_0: 0,
            tokens_owed_1: 0,
            total_amount0_deposited: U256::ZERO,
            total_amount1_deposited: U256::ZERO,
            total_amount0_collected: 0,
            total_amount1_collected: 0,
        }
    }

    /// Generates a unique string key for a position based on owner and tick range.
    #[must_use]
    pub fn get_position_key(owner: &Address, tick_lower: i32, tick_upper: i32) -> String {
        format!("{}:{}:{}", owner, tick_lower, tick_upper)
    }

    /// Updates the liquidity amount by the given delta.
    ///
    /// Positive values increase liquidity, negative values decrease it.
    /// Uses saturating arithmetic to prevent underflow.
    pub fn update_liquidity(&mut self, liquidity_delta: i128) {
        if liquidity_delta < 0 {
            self.liquidity = self.liquidity.saturating_sub((-liquidity_delta) as u128);
        } else {
            self.liquidity = self.liquidity.saturating_add(liquidity_delta as u128);
        }
    }

    /// Updates the position's fee tracking based on current fee growth inside the position's range.
    ///
    /// Calculates the fees earned since the last update and adds them to tokens_owed.
    /// Updates the last known fee growth values for future calculations.
    pub fn update_fees(&mut self, fee_growth_inside_0: U256, fee_growth_inside_1: U256) {
        if self.liquidity > 0 {
            // Calculate fee deltas
            let fee_delta_0 = fee_growth_inside_0.saturating_sub(self.fee_growth_inside_0_last);
            let fee_delta_1 = fee_growth_inside_1.saturating_sub(self.fee_growth_inside_1_last);

            let tokens_owed_0_full =
                FullMath::mul_div(fee_delta_0, U256::from(self.liquidity), Q128)
                    .unwrap_or(U256::ZERO);

            let tokens_owed_1_full =
                FullMath::mul_div(fee_delta_1, U256::from(self.liquidity), Q128)
                    .unwrap_or(U256::ZERO);

            self.tokens_owed_0 = self
                .tokens_owed_0
                .wrapping_add(FullMath::truncate_to_u128(tokens_owed_0_full));
            self.tokens_owed_1 = self
                .tokens_owed_1
                .wrapping_add(FullMath::truncate_to_u128(tokens_owed_1_full));
        }

        self.fee_growth_inside_0_last = fee_growth_inside_0;
        self.fee_growth_inside_1_last = fee_growth_inside_1;
    }

    /// Collects fees owed to the position, up to the requested amounts.
    ///
    /// Reduces tokens_owed by the collected amounts and tracks total collections.
    /// Cannot collect more than what is currently owed.
    pub fn collect_fees(&mut self, amount0: u128, amount1: u128) {
        let collect_amount_0 = amount0.min(self.tokens_owed_0);
        let collect_amount_1 = amount1.min(self.tokens_owed_1);

        self.tokens_owed_0 -= collect_amount_0;
        self.tokens_owed_1 -= collect_amount_1;

        self.total_amount0_collected += collect_amount_0;
        self.total_amount1_collected += collect_amount_1;
    }

    /// Updates position token amounts based on liquidity delta.
    ///
    /// For positive liquidity delta (mint), tracks deposited amounts.
    /// For negative liquidity delta (burn), adds amounts to tokens owed.
    pub fn update_amounts(&mut self, liquidity_delta: i128, amount0: U256, amount1: U256) {
        if liquidity_delta > 0 {
            // Mint: track deposited amounts
            self.total_amount0_deposited += amount0;
            self.total_amount1_deposited += amount1;
        } else {
            self.tokens_owed_0 = self
                .tokens_owed_0
                .wrapping_add(FullMath::truncate_to_u128(amount0));
            self.tokens_owed_1 = self
                .tokens_owed_1
                .wrapping_add(FullMath::truncate_to_u128(amount1));
        }
    }

    /// Checks if the position is completely empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.liquidity == 0 && self.tokens_owed_0 == 0 && self.tokens_owed_1 == 0
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use alloy_primitives::address;

    use super::*;

    #[test]
    fn test_new_position() {
        let owner = address!("1234567890123456789012345678901234567890");
        let tick_lower = -100;
        let tick_upper = 100;
        let liquidity = 1000i128;

        let position = PoolPosition::new(owner, tick_lower, tick_upper, liquidity);

        assert_eq!(position.owner, owner);
        assert_eq!(position.tick_lower, tick_lower);
        assert_eq!(position.tick_upper, tick_upper);
        assert_eq!(position.liquidity, liquidity as u128);
        assert_eq!(position.fee_growth_inside_0_last, U256::ZERO);
        assert_eq!(position.fee_growth_inside_1_last, U256::ZERO);
        assert_eq!(position.tokens_owed_0, 0);
        assert_eq!(position.tokens_owed_1, 0);
    }

    #[test]
    fn test_get_position_key() {
        let owner = address!("1234567890123456789012345678901234567890");
        let tick_lower = -100;
        let tick_upper = 100;

        let key = PoolPosition::get_position_key(&owner, tick_lower, tick_upper);
        let expected = format!("{:?}:{}:{}", owner, tick_lower, tick_upper);
        assert_eq!(key, expected);
    }

    #[test]
    fn test_update_liquidity_positive() {
        let owner = address!("1234567890123456789012345678901234567890");
        let mut position = PoolPosition::new(owner, -100, 100, 1000);

        position.update_liquidity(500);
        assert_eq!(position.liquidity, 1500);
    }

    #[test]
    fn test_update_liquidity_negative() {
        let owner = address!("1234567890123456789012345678901234567890");
        let mut position = PoolPosition::new(owner, -100, 100, 1000);

        position.update_liquidity(-300);
        assert_eq!(position.liquidity, 700);
    }

    #[test]
    fn test_update_liquidity_negative_saturating() {
        let owner = address!("1234567890123456789012345678901234567890");
        let mut position = PoolPosition::new(owner, -100, 100, 1000);

        position.update_liquidity(-2000); // More than current liquidity
        assert_eq!(position.liquidity, 0);
    }

    #[test]
    fn test_update_fees() {
        let owner = address!("1234567890123456789012345678901234567890");
        let mut position = PoolPosition::new(owner, -100, 100, 1000);

        let fee_growth_inside_0 = U256::from(100);
        let fee_growth_inside_1 = U256::from(200);

        position.update_fees(fee_growth_inside_0, fee_growth_inside_1);

        assert_eq!(position.fee_growth_inside_0_last, fee_growth_inside_0);
        assert_eq!(position.fee_growth_inside_1_last, fee_growth_inside_1);
        // With liquidity 1000 and fee growth 100, should earn 100*1000/2^128 ≈ 0 (due to division)
        // In practice this would be larger numbers
    }

    #[test]
    fn test_collect_fees() {
        let owner = address!("1234567890123456789012345678901234567890");
        let mut position = PoolPosition::new(owner, -100, 100, 1000);

        // Set some owed tokens
        position.tokens_owed_0 = 100;
        position.tokens_owed_1 = 200;

        // Collect partial fees
        position.collect_fees(50, 150);

        assert_eq!(position.total_amount0_collected, 50);
        assert_eq!(position.total_amount1_collected, 150);
        assert_eq!(position.tokens_owed_0, 50);
        assert_eq!(position.tokens_owed_1, 50);
    }

    #[test]
    fn test_collect_fees_more_than_owed() {
        let owner = address!("1234567890123456789012345678901234567890");
        let mut position = PoolPosition::new(owner, -100, 100, 1000);

        position.tokens_owed_0 = 100;
        position.tokens_owed_1 = 200;

        // Try to collect more than owed
        position.collect_fees(150, 300);

        assert_eq!(position.total_amount0_collected, 100); // Can only collect what's owed
        assert_eq!(position.total_amount1_collected, 200);
        assert_eq!(position.tokens_owed_0, 0);
        assert_eq!(position.tokens_owed_1, 0);
    }

    #[test]
    fn test_is_empty() {
        let owner = address!("1234567890123456789012345678901234567890");
        let mut position = PoolPosition::new(owner, -100, 100, 0);

        assert!(position.is_empty());

        position.liquidity = 100;
        assert!(!position.is_empty());

        position.liquidity = 0;
        position.tokens_owed_0 = 50;
        assert!(!position.is_empty());

        position.tokens_owed_0 = 0;
        position.tokens_owed_1 = 25;
        assert!(!position.is_empty());

        position.tokens_owed_1 = 0;
        assert!(position.is_empty());
    }
}
