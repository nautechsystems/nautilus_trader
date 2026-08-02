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

//! Binance instrument fee fallbacks.

use rust_decimal::Decimal;

/// Default Spot maker and taker fee when account rates are unavailable.
pub const BINANCE_SPOT_FEE_DEFAULT: Decimal = Decimal::from_parts(1, 0, 0, false, 3);

/// Returns the documented USD-M VIP maker and taker rates used by legacy parity.
///
/// Tiers above 9 use tier 0 so an unknown venue value cannot silently grant a
/// lower commission estimate.
#[must_use]
pub fn futures_fee_tier_rates(tier: u8) -> (Decimal, Decimal) {
    match tier {
        1 => (Decimal::new(16, 5), Decimal::new(4, 4)),
        2 => (Decimal::new(14, 5), Decimal::new(35, 5)),
        3 => (Decimal::new(12, 5), Decimal::new(32, 5)),
        4 => (Decimal::new(1, 4), Decimal::new(3, 4)),
        5 => (Decimal::new(8, 5), Decimal::new(27, 5)),
        6 => (Decimal::new(6, 5), Decimal::new(25, 5)),
        7 => (Decimal::new(4, 5), Decimal::new(22, 5)),
        8 => (Decimal::new(2, 5), Decimal::new(2, 4)),
        9 => (Decimal::ZERO, Decimal::new(17, 5)),
        _ => (Decimal::new(2, 4), Decimal::new(5, 4)),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    #[case(0, dec!(0.0002), dec!(0.0005))]
    #[case(4, dec!(0.0001), dec!(0.0003))]
    #[case(9, dec!(0), dec!(0.00017))]
    #[case(10, dec!(0.0002), dec!(0.0005))]
    fn test_futures_fee_tier_rates(
        #[case] tier: u8,
        #[case] expected_maker: Decimal,
        #[case] expected_taker: Decimal,
    ) {
        assert_eq!(
            futures_fee_tier_rates(tier),
            (expected_maker, expected_taker)
        );
    }
}
