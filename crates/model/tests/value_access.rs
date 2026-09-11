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

use nautilus_model::types::{
    Currency, Money, Price, Quantity,
    money::{MONEY_RAW_MAX, MoneyRaw},
    price::{PRICE_ERROR, PRICE_RAW_MAX, PRICE_UNDEF, PriceRaw},
    quantity::{QUANTITY_RAW_MAX, QUANTITY_UNDEF, QuantityRaw},
};
use rstest::rstest;

#[rstest]
#[case(0)]
#[case(1)]
#[case(-1)]
#[case(PRICE_RAW_MAX)]
#[case(PRICE_UNDEF)]
#[case(PRICE_ERROR)]
fn test_price_raw_access_preserves_storage(#[case] raw: PriceRaw) {
    let price = Price::from_raw(raw, 0);
    assert_eq!(price.raw(), raw);
    assert_eq!(price.precision, 0);
}

#[rstest]
#[case(0)]
#[case(1)]
#[case(QUANTITY_RAW_MAX)]
#[case(QUANTITY_UNDEF)]
fn test_quantity_raw_access_preserves_storage(#[case] raw: QuantityRaw) {
    let quantity = Quantity::from_raw(raw, 0);
    assert_eq!(quantity.raw(), raw);
    assert_eq!(quantity.precision, 0);
}

#[rstest]
#[case(0)]
#[case(1)]
#[case(-1)]
#[case(MONEY_RAW_MAX)]
#[case(-MONEY_RAW_MAX)]
fn test_money_raw_access_preserves_storage(#[case] raw: MoneyRaw) {
    let currency = Currency::USD();
    let money = Money::from_raw(raw, currency);
    assert_eq!(money.raw(), raw);
    assert_eq!(money.currency, currency);
}

#[rstest]
#[case(-1, true)]
#[case(0, false)]
#[case(1, false)]
fn test_negative_predicates_preserve_subprecision_units(#[case] raw: i32, #[case] negative: bool) {
    let price = Price::from_raw(PriceRaw::from(raw), 0);
    let money = Money::from_raw(MoneyRaw::from(raw), Currency::USD());
    assert_eq!(price.is_negative(), negative);
    assert_eq!(money.is_negative(), negative);
    assert_eq!(money.abs().raw(), MoneyRaw::from(raw.abs()));
    assert_eq!(money.abs().currency, Currency::USD());
}

#[rstest]
#[case(1, 2, 3)]
#[case(QUANTITY_RAW_MAX - 1, 1, QUANTITY_RAW_MAX)]
#[case(QUANTITY_RAW_MAX, 1, QUANTITY_RAW_MAX)]
#[case(QUANTITY_RAW_MAX, QUANTITY_RAW_MAX, QUANTITY_RAW_MAX)]
fn test_quantity_saturating_add(
    #[case] lhs: QuantityRaw,
    #[case] rhs: QuantityRaw,
    #[case] expected: QuantityRaw,
) {
    let result = Quantity::from_raw(lhs, 2).saturating_add(Quantity::from_raw(rhs, 3));
    assert_eq!(result.raw(), expected);
    assert_eq!(result.precision, 3);
}
