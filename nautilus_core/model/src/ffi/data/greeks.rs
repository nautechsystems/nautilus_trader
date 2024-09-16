// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::data::greeks::{
    black_scholes_greeks, imply_vol, imply_vol_and_greeks, BlackScholesGreeksResult,
    ImplyVolAndGreeksResult,
};

#[no_mangle]
pub extern "C" fn greeks_black_scholes_greeks(
    s: f64,
    r: f64,
    b: f64,
    sigma: f64,
    is_call: u8,
    k: f64,
    t: f64,
    multiplier: f64,
) -> BlackScholesGreeksResult {
    black_scholes_greeks(s, r, b, sigma, is_call != 0, k, t, multiplier)
}

#[no_mangle]
pub extern "C" fn greeks_imply_vol(
    s: f64,
    r: f64,
    b: f64,
    is_call: u8,
    k: f64,
    t: f64,
    price: f64,
) -> f64 {
    imply_vol(s, r, b, is_call != 0, k, t, price)
}

#[no_mangle]
pub extern "C" fn greeks_imply_vol_and_greeks(
    s: f64,
    r: f64,
    b: f64,
    is_call: u8,
    k: f64,
    t: f64,
    price: f64,
    multiplier: f64,
) -> ImplyVolAndGreeksResult {
    imply_vol_and_greeks(s, r, b, is_call != 0, k, t, price, multiplier)
}
