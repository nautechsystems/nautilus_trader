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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    mem::{align_of, offset_of, size_of},
};

use nautilus_model::{
    enums::CurrencyType,
    types::{Currency, Money, money::MoneyRaw},
};
use rstest::rstest;
use ustr::Ustr;

#[repr(C)]
struct LegacyCurrency {
    code: Ustr,
    precision: u8,
    iso4217: u16,
    name: Ustr,
    currency_type: CurrencyType,
}

#[repr(C)]
struct LegacyMoney {
    raw: MoneyRaw,
    currency: LegacyCurrency,
}

#[rstest]
fn native_layout_matches_legacy() {
    assert_eq!(size_of::<Currency>(), size_of::<LegacyCurrency>());
    assert_eq!(align_of::<Currency>(), align_of::<LegacyCurrency>());
    assert_eq!(offset_of!(Currency, code), offset_of!(LegacyCurrency, code));
    assert_eq!(
        offset_of!(Currency, precision),
        offset_of!(LegacyCurrency, precision)
    );
    assert_eq!(
        offset_of!(Currency, iso4217),
        offset_of!(LegacyCurrency, iso4217)
    );
    assert_eq!(offset_of!(Currency, name), offset_of!(LegacyCurrency, name));
    assert_eq!(
        offset_of!(Currency, currency_type),
        offset_of!(LegacyCurrency, currency_type)
    );
    assert_eq!(size_of::<Money>(), size_of::<LegacyMoney>());
    assert_eq!(align_of::<Money>(), align_of::<LegacyMoney>());
    assert_eq!(
        offset_of!(Money, currency),
        offset_of!(LegacyMoney, currency)
    );
}

#[rstest]
fn construction_equality_and_hash_remain_code_based() {
    let original = Currency::USD();

    let constructed = Currency {
        code: original.code,
        precision: 7,
        iso4217: 123,
        name: Ustr::from("Other metadata"),
        currency_type: CurrencyType::Crypto,
    };

    let mut first = DefaultHasher::new();
    let mut second = DefaultHasher::new();
    original.hash(&mut first);
    constructed.hash(&mut second);

    assert_eq!(original, constructed);
    assert_eq!(first.finish(), second.finish());
    assert_eq!(serde_json::to_string(&constructed).unwrap(), "\"USD\"");
}

#[rstest]
fn ordinary_money_keeps_exact_amounts_and_serialization() {
    let first = Money::from_decimal_exact("1.2345".parse().unwrap(), Currency::USD()).unwrap();
    let second = Money::from_decimal_exact("2.3456".parse().unwrap(), Currency::USD()).unwrap();
    let sum = first + second;
    let encoded = serde_json::to_string(&sum).unwrap();
    let restored: Money = serde_json::from_str(&encoded).unwrap();

    assert_eq!(sum.as_decimal(), "3.5801".parse().unwrap());
    assert_eq!(encoded, "\"3.5801 USD\"");
    assert_eq!(restored.as_decimal(), sum.as_decimal());
    assert_eq!(restored.currency.precision, 2);
    assert_eq!(restored.currency.name, "United States dollar");
}
