// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::enums::OrderSide;
use crate::identifiers::Symbol;
use rust_decimal::Decimal;
use std::os::raw::c_char;

pub trait Tick {
    fn get_symbol(&self) -> &Symbol;
    fn get_timestamp(&self) -> &u64;
}

type Price = Decimal;
type Quantity = Decimal;

#[repr(C)]
pub struct QuoteTickC {
    pub symbol: *mut c_char,
    pub bid: *mut c_char,
    pub ask: *mut c_char,
    pub bid_size: *mut c_char,
    pub ask_size: *mut c_char,
    pub timestamp: u64,
}

#[derive(Clone)]
/// Represents a single quote tick in a financial market.
pub struct QuoteTick {
    pub symbol: Symbol,
    pub bid: Decimal,
    pub ask: Decimal,
    pub bid_size: Decimal,
    pub ask_size: Decimal,
    pub timestamp: u64,
}

impl Tick for QuoteTick {
    fn get_symbol(&self) -> &Symbol {
        return &self.symbol;
    }

    fn get_timestamp(&self) -> &u64 {
        return &self.timestamp;
    }
}

#[derive(Clone)]
/// Represents a single trade tick in a financial market.
pub struct TradeTick {
    pub symbol: String,
    pub price: Price,
    pub size: Quantity,
    pub side: OrderSide,
    pub trade_match_id: String,
    pub timestamp: u64,
}
