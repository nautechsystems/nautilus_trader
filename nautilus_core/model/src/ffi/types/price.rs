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

use crate::types::price::Price;

// TODO: Document panic
#[no_mangle]
pub extern "C" fn price_new(value: f64, precision: u8) -> Price {
    // SAFETY: Assumes `value` and `precision` are properly validated
    Price::new(value, precision).unwrap()
}

#[no_mangle]
pub extern "C" fn price_from_raw(raw: i64, precision: u8) -> Price {
    Price::from_raw(raw, precision).unwrap()
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
