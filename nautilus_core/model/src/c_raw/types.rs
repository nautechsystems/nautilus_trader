// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
// ------------------------------------------------------------------------------------------------

use crate::enums::CurrencyType;
use crate::types::currency::Currency;
use crate::types::money::Money;
use crate::types::price::Price;
use crate::types::quantity::Quantity;
use nautilus_core::buffer::{Buffer16, Buffer32};
use std::ops::{AddAssign, SubAssign};

////////////////////////////////////////////////////////////////////////////////
// Price
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn price_new(value: f64, precision: u8) -> Price {
    Price::new(value, precision)
}

#[no_mangle]
pub extern "C" fn price_from_fixed(fixed: i64, precision: u8) -> Price {
    Price::from_fixed(fixed, precision)
}

#[no_mangle]
pub extern "C" fn price_free(price: Price) {
    drop(price); // Memory freed here
}

#[no_mangle]
pub extern "C" fn price_as_f64(price: &Price) -> f64 {
    price.as_f64()
}

#[no_mangle]
pub extern "C" fn price_add_assign(mut a: Price, b: Price) {
    a.add_assign(b);
}

#[no_mangle]
pub extern "C" fn price_sub_assign(mut a: Price, b: Price) {
    a.sub_assign(b);
}

////////////////////////////////////////////////////////////////////////////////
// Quantity
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn quantity_new(value: f64, precision: u8) -> Quantity {
    Quantity::new(value, precision)
}

#[no_mangle]
pub extern "C" fn quantity_from_fixed(fixed: u64, precision: u8) -> Quantity {
    Quantity::from_fixed(fixed, precision)
}

#[no_mangle]
pub extern "C" fn quantity_free(qty: Quantity) {
    drop(qty); // Memory freed here
}

#[no_mangle]
pub extern "C" fn quantity_as_f64(qty: &Quantity) -> f64 {
    qty.as_f64()
}

#[no_mangle]
pub extern "C" fn quantity_add_assign(mut a: Quantity, b: Quantity) {
    a.add_assign(b);
}

#[no_mangle]
pub extern "C" fn quantity_add_assign_u64(mut a: Quantity, b: u64) {
    a.add_assign(b);
}

#[no_mangle]
pub extern "C" fn quantity_sub_assign(mut a: Quantity, b: Quantity) {
    a.sub_assign(b);
}

#[no_mangle]
pub extern "C" fn quantity_sub_assign_u64(mut a: Quantity, b: u64) {
    a.sub_assign(b);
}

////////////////////////////////////////////////////////////////////////////////
// Currency
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn currency_new(
    code: Buffer16,
    precision: u8,
    iso4217: u16,
    name: Buffer32,
    currency_type: CurrencyType,
) -> Currency {
    Currency::new(code, precision, iso4217, name, currency_type)
}

#[no_mangle]
pub extern "C" fn currency_free(currency: Currency) {
    drop(currency); // Memory freed here
}

////////////////////////////////////////////////////////////////////////////////
// Money
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn money_new(amount: f64, currency: Currency) -> Money {
    Money::new(amount, currency)
}

#[no_mangle]
pub extern "C" fn money_from_fixed(fixed: i64, currency: Currency) -> Money {
    Money::from_fixed(fixed, currency)
}

#[no_mangle]
pub extern "C" fn money_free(money: Money) {
    drop(money); // Memory freed here
}

#[no_mangle]
pub extern "C" fn money_as_f64(money: &Money) -> f64 {
    money.as_f64()
}

#[no_mangle]
pub extern "C" fn money_add_assign(mut a: Money, b: Money) {
    a.add_assign(b);
}

#[no_mangle]
pub extern "C" fn money_sub_assign(mut a: Money, b: Money) {
    a.sub_assign(b);
}
