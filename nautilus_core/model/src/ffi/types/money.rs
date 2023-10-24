// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::ops::{AddAssign, SubAssign};

use crate::types::{currency::Currency, money::Money};

// TODO: Document panic
#[no_mangle]
pub extern "C" fn money_new(amount: f64, currency: Currency) -> Money {
    // SAFETY: Assumes `amount` is properly validated
    Money::new(amount, currency).unwrap()
}

#[no_mangle]
pub extern "C" fn money_from_raw(raw: i64, currency: Currency) -> Money {
    Money::from_raw(raw, currency)
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
