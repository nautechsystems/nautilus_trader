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

#![cfg(feature = "defi")]

use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashSet, hash_map::DefaultHasher},
    fmt::Debug,
    hash::{Hash, Hasher},
};

use alloy_primitives::U256;
use nautilus_model::{
    enums::CurrencyType,
    types::{
        Currency, Money, Price, Quantity,
        money::MONEY_RAW_MAX,
        price::{PRICE_ERROR, PRICE_RAW_MAX},
        quantity::{QUANTITY_RAW_MAX, QUANTITY_UNDEF},
    },
};
use proptest::{prelude::*, test_runner::Config as ProptestConfig};
use rstest::rstest;

#[rstest]
#[case(0, 16, 18, 0)]
#[case(11_000_000_000_000_000, 16, 18, 1_100_000_000_000_000_000)]
#[case(1_100_000_000_000_000_000, 18, 16, 11_000_000_000_000_000)]
#[case(1_100_000_000_000_000_001, 18, 18, 1_100_000_000_000_000_001)]
#[case(
    100_000_000_000_000_000_000_000_000_001,
    18,
    18,
    100_000_000_000_000_000_000_000_000_001
)]
fn test_scale_money_from_quantity_exact(
    #[case] raw: u128,
    #[case] source_precision: u8,
    #[case] target_precision: u8,
    #[case] expected: i128,
) {
    let currency = Currency::new("TOKEN", target_precision, 0, "Token", CurrencyType::Crypto);
    let quantity = Quantity::from_raw(raw, source_precision);
    let money = Money::from_quantity(quantity, currency).unwrap();

    assert_eq!((money.raw, money.currency), (expected, currency));
    assert_eq!(money.currency.precision, target_precision);
}

#[rstest]
#[case(1, 18, 16, "quantity for TOKEN loses precision when decreasing raw scale".to_string())]
#[case(QUANTITY_UNDEF, 0, 18, "quantity was undefined".to_string())]
#[case((MONEY_RAW_MAX / 100 + 1) as u128, 16, 18, format!(
    "`raw` value {} exceeded bounds [{}, {MONEY_RAW_MAX}] for Money",
    (MONEY_RAW_MAX / 100 + 1) * 100, -MONEY_RAW_MAX
))]
fn test_scale_money_from_quantity_rejects_invalid_conversion(
    #[case] raw: u128,
    #[case] source_precision: u8,
    #[case] target_precision: u8,
    #[case] expected: String,
) {
    let currency = Currency::new("TOKEN", target_precision, 0, "Token", CurrencyType::Crypto);
    let quantity = Quantity::from_raw(raw, source_precision);
    let error = Money::from_quantity(quantity, currency).unwrap_err();

    assert_eq!(error.to_string(), expected);
}

#[rstest]
fn test_scale_price_error_preserves_identity(
    #[values(0, 16, 17, 18)] lhs_precision: u8,
    #[values(0, 16, 17, 18)] rhs_precision: u8,
) {
    let lhs = Price::from_raw(PRICE_ERROR, lhs_precision);
    let rhs = Price::from_raw(PRICE_ERROR, rhs_precision);
    assert_ordering(lhs, rhs, Ordering::Equal);
    assert_ordering(
        lhs,
        Price::from_raw(-PRICE_RAW_MAX, rhs_precision),
        Ordering::Less,
    );
    assert_ordering(
        Price::from_raw(-PRICE_RAW_MAX, lhs_precision),
        rhs,
        Ordering::Greater,
    );
}

#[rstest]
#[case(1, 1, Ordering::Equal)]
#[case(100, 1, Ordering::Greater)]
#[case(1, 100, Ordering::Less)]
#[case(-1, -1, Ordering::Equal)]
#[case(-100, -1, Ordering::Less)]
#[case(-1, -100, Ordering::Greater)]
#[case(-1, 1, Ordering::Less)]
#[case(0, 0, Ordering::Equal)]
fn test_scale_ordering(
    #[case] lhs: i128,
    #[case] rhs: i128,
    #[case] expected: Ordering,
    #[values(16, 17, 18)] lhs_precision: u8,
    #[values(16, 17, 18)] rhs_precision: u8,
) {
    let lhs_raw = lhs * 10_i128.pow(u32::from(lhs_precision));
    let rhs_raw = rhs * 10_i128.pow(u32::from(rhs_precision));
    assert_ordering(
        Price::from_raw(lhs_raw, lhs_precision),
        Price::from_raw(rhs_raw, rhs_precision),
        expected,
    );
    let lhs_currency = Currency::new("SCALE", lhs_precision, 0, "Scale", CurrencyType::Crypto);
    let rhs_currency = Currency::new("SCALE", rhs_precision, 0, "Scale", CurrencyType::Crypto);
    assert_ordering(
        Money::from_raw(lhs_raw, lhs_currency),
        Money::from_raw(rhs_raw, rhs_currency),
        expected,
    );

    if lhs >= 0 && rhs >= 0 {
        assert_ordering(
            Quantity::from_raw(lhs_raw as u128, lhs_precision),
            Quantity::from_raw(rhs_raw as u128, rhs_precision),
            expected,
        );
    }
}

#[rstest]
fn test_scale_ordering_preserves_low_digits() {
    let raw = 10_i128.pow(28);
    assert_ordering(
        Price::from_raw(raw, 17),
        Price::from_raw(raw * 10 + 1, 18),
        Ordering::Less,
    );
    assert_ordering(
        Price::from_raw(-raw, 17),
        Price::from_raw(-raw * 10 - 1, 18),
        Ordering::Greater,
    );
    assert_ordering(
        Quantity::from_raw(raw as u128, 17),
        Quantity::from_raw((raw * 10 + 1) as u128, 18),
        Ordering::Less,
    );
    let currency17 = Currency::new("SCALE", 17, 0, "Scale", CurrencyType::Crypto);
    let currency18 = Currency::new("SCALE", 18, 0, "Scale", CurrencyType::Crypto);
    assert_ordering(
        Money::from_raw(raw, currency17),
        Money::from_raw(raw * 10 + 1, currency18),
        Ordering::Less,
    );
    assert_ordering(
        Money::from_raw(-raw, currency17),
        Money::from_raw(-raw * 10 - 1, currency18),
        Ordering::Greater,
    );
}

#[rstest]
#[case(16, 17)]
#[case(16, 18)]
#[case(17, 16)]
#[case(17, 18)]
#[case(18, 16)]
#[case(18, 17)]
#[should_panic(expected = "mismatched decimal scales")]
fn test_scale_price_arithmetic_rejects_mismatch(
    #[case] lhs_precision: u8,
    #[case] rhs_precision: u8,
    #[values(false, true)] subtract: bool,
) {
    let lhs_raw = 10_i128.pow(u32::from(lhs_precision));
    let rhs_raw = 10_i128.pow(u32::from(rhs_precision));
    let lhs = Price::from_raw(lhs_raw, lhs_precision);
    let rhs = Price::from_raw(rhs_raw, rhs_precision);
    let _ = if subtract { lhs - rhs } else { lhs + rhs };
}

#[rstest]
#[case(16, 17)]
#[case(16, 18)]
#[case(17, 16)]
#[case(17, 18)]
#[case(18, 16)]
#[case(18, 17)]
#[should_panic(expected = "mismatched decimal scales")]
fn test_scale_quantity_arithmetic_rejects_mismatch(
    #[case] lhs_precision: u8,
    #[case] rhs_precision: u8,
    #[values(false, true)] subtract: bool,
) {
    let lhs_raw = 10_u128.pow(u32::from(lhs_precision));
    let rhs_raw = 10_u128.pow(u32::from(rhs_precision));
    let lhs = Quantity::from_raw(lhs_raw, lhs_precision);
    let rhs = Quantity::from_raw(rhs_raw, rhs_precision);
    let _ = if subtract { lhs - rhs } else { lhs + rhs };
}

#[rstest]
#[case(16, 17)]
#[case(16, 18)]
#[case(17, 16)]
#[case(17, 18)]
#[case(18, 16)]
#[case(18, 17)]
#[should_panic(expected = "mismatched decimal scales")]
fn test_scale_money_arithmetic_rejects_mismatch(
    #[case] lhs_precision: u8,
    #[case] rhs_precision: u8,
    #[values(false, true)] subtract: bool,
) {
    let lhs_raw = 10_i128.pow(u32::from(lhs_precision));
    let rhs_raw = 10_i128.pow(u32::from(rhs_precision));
    let lhs_currency = Currency::new("SCALE", lhs_precision, 0, "Scale", CurrencyType::Crypto);
    let rhs_currency = Currency::new("SCALE", rhs_precision, 0, "Scale", CurrencyType::Crypto);
    let lhs = Money::from_raw(lhs_raw, lhs_currency);
    let rhs = Money::from_raw(rhs_raw, rhs_currency);
    let _ = if subtract { lhs - rhs } else { lhs + rhs };
}

#[rstest]
#[should_panic(expected = "mismatched decimal scales")]
fn test_scale_quantity_saturating_sub_rejects_mismatch() {
    let _ = Quantity::from_raw(10_u128.pow(18), 18)
        .saturating_sub(Quantity::from_raw(10_u128.pow(16), 16));
}

#[rstest]
#[should_panic(expected = "mismatched decimal scales")]
fn test_scale_quantity_sum_rejects_mismatch() {
    let _: Quantity = [
        Quantity::from_raw(10_u128.pow(18), 18),
        Quantity::from_raw(10_u128.pow(16), 16),
    ]
    .into_iter()
    .sum();
}

#[rstest]
#[should_panic(expected = "Overflow occurred when multiplying `Quantity`")]
fn test_scale_quantity_mul_rejects_domain_overflow(#[values(17, 18)] precision: u8) {
    let _ = Quantity::from_raw(QUANTITY_RAW_MAX, precision)
        * Quantity::from_raw(2 * 10_u128.pow(u32::from(precision)), precision);
}

#[rstest]
fn test_scale_quantity_sum(#[values(16, 17, 18)] precision: u8) {
    let scale = 10_u128.pow(u32::from(precision));
    let quantities = [
        Quantity::from_raw(2 * scale, precision),
        Quantity::from_raw(3 * scale, precision),
    ];
    let owned: Quantity = quantities.into_iter().sum();
    let borrowed: Quantity = quantities.iter().sum();
    assert_eq!((owned.raw, owned.precision), (5 * scale, precision));
    assert_eq!((borrowed.raw, borrowed.precision), (5 * scale, precision));
}

#[rstest]
#[case(2, 3)]
#[case(100, 200)]
fn test_scale_quantity_mul(
    #[case] lhs: u128,
    #[case] rhs: u128,
    #[values(16, 17, 18)] lhs_precision: u8,
    #[values(16, 17, 18)] rhs_precision: u8,
) {
    let lhs_scale = 10_u128.pow(u32::from(lhs_precision));
    let rhs_scale = 10_u128.pow(u32::from(rhs_precision));
    let product = Quantity::from_raw(lhs * lhs_scale, lhs_precision)
        * Quantity::from_raw(rhs * rhs_scale, rhs_precision);
    assert_eq!(
        (product.raw, product.precision),
        (
            lhs * rhs * lhs_scale.max(rhs_scale),
            lhs_precision.max(rhs_precision)
        )
    );
}

#[rstest]
fn test_scale_quantity_mul_preserves_fractional_remainders(
    #[values(16, 17, 18)] lhs_precision: u8,
    #[values(16, 17, 18)] rhs_precision: u8,
) {
    let lhs_scale = 10_u128.pow(u32::from(lhs_precision));
    let rhs_scale = 10_u128.pow(u32::from(rhs_precision));
    let lhs = Quantity::from_raw(2 * lhs_scale - 1, lhs_precision);
    let rhs = Quantity::from_raw(3 * rhs_scale + 1, rhs_precision);
    let expected = U256::from(lhs.raw) * U256::from(rhs.raw) / U256::from(lhs_scale.min(rhs_scale));
    let expected = u128::try_from(expected).unwrap();
    let product = lhs * rhs;
    let reverse = rhs * lhs;

    assert_eq!(
        (product.raw, product.precision),
        (expected, lhs_precision.max(rhs_precision))
    );
    assert_eq!(
        (reverse.raw, reverse.precision),
        (product.raw, product.precision)
    );
}

#[rstest]
fn test_scale_quantity_mul_preserves_remainders_after_overflow(#[values(17, 18)] precision: u8) {
    let scale = 10_u128.pow(u32::from(precision));
    let lhs = Quantity::from_raw(2 * scale - 1, precision);
    let rhs = Quantity::from_raw(QUANTITY_RAW_MAX / 3, precision);
    let product = lhs * rhs;
    let expected = 2 * rhs.raw - rhs.raw.div_ceil(scale);

    assert_eq!(lhs.raw.checked_mul(rhs.raw), None);
    assert_eq!((product.raw, product.precision), (expected, precision));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4_096))]

    #[rstest]
    fn prop_scale_ordering_matches_wide_integer_products(
        lhs_raw in 0_i128..=PRICE_RAW_MAX,
        rhs_raw in 0_i128..=PRICE_RAW_MAX,
        lhs_precision in 16_u8..=18,
        rhs_precision in 16_u8..=18,
    ) {
        let lhs_scaled = U256::from(lhs_raw as u128) * U256::from(10_u128.pow(u32::from(rhs_precision)));
        let rhs_scaled = U256::from(rhs_raw as u128) * U256::from(10_u128.pow(u32::from(lhs_precision)));
        let expected = lhs_scaled.cmp(&rhs_scaled);
        assert_ordering(Price::from_raw(lhs_raw, lhs_precision), Price::from_raw(rhs_raw, rhs_precision), expected);
        assert_ordering(Price::from_raw(-lhs_raw, lhs_precision), Price::from_raw(-rhs_raw, rhs_precision), expected.reverse());
        assert_ordering(Quantity::from_raw(lhs_raw as u128, lhs_precision), Quantity::from_raw(rhs_raw as u128, rhs_precision), expected);
    }
}

fn assert_ordering<T: Copy + Debug + Ord + Hash>(lhs: T, rhs: T, expected: Ordering) {
    assert_eq!(lhs.cmp(&rhs), expected);
    assert_eq!(lhs.partial_cmp(&rhs), Some(expected));
    assert_eq!(lhs == rhs, expected == Ordering::Equal);
    assert_eq!(lhs < rhs, expected == Ordering::Less);
    assert_eq!(lhs <= rhs, expected != Ordering::Greater);
    assert_eq!(lhs > rhs, expected == Ordering::Greater);
    assert_eq!(lhs >= rhs, expected != Ordering::Less);
    if expected == Ordering::Equal {
        let mut lhs_hash = DefaultHasher::new();
        let mut rhs_hash = DefaultHasher::new();
        lhs.hash(&mut lhs_hash);
        rhs.hash(&mut rhs_hash);
        assert_eq!(lhs_hash.finish(), rhs_hash.finish());
    }
    let count = if expected == Ordering::Equal { 1 } else { 2 };
    assert_eq!(HashSet::from([lhs, rhs]).len(), count);
    assert_eq!(BTreeSet::from([lhs, rhs]).len(), count);
}
