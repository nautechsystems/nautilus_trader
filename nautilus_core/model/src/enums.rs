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
// -------------------------------------------------------------------------------------------------

use std::fmt::{Debug, Display, Formatter, Result};

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum CurrencyType {
    Crypto,
    Fiat,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum OrderSide {
    Buy = 1,
    Sell = 2,
}

impl OrderSide {
    pub fn as_str(&self) -> &'static str {
        match self {
            OrderSide::Buy => "BUY",
            OrderSide::Sell => "SELL",
        }
    }
}

impl From<&str> for OrderSide {
    fn from(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "BUY" => OrderSide::Buy,
            "SELL" => OrderSide::Sell,
            _ => panic!("Invalid `OrderSide` value, was {s}"),
        }
    }
}

// impl Into<&str> for OrderSide {
//     fn into(self) -> &'static str {
//         match self {
//             OrderSide::Buy => "BUY",
//             OrderSide::Sell => "SELL",
//         }
//     }
// }

// impl ToString for OrderSide {
//     fn to_string(&self) -> String {
//         match self {
//             OrderSide::Buy => "BUY",
//             OrderSide::Sell => "SELL",
//         }.parse().unwrap()
//     }
// }

impl Display for OrderSide {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.as_str())
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PriceType {
    Bid = 1,
    Ask = 2,
    Mid = 3,
    Last = 4,
}
impl PriceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PriceType::Bid => "BID",
            PriceType::Ask => "ASK",
            PriceType::Mid => "MID",
            PriceType::Last => "LAST",
        }
    }
}
impl Display for PriceType {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.as_str())
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum BookLevel {
    L1_TBBO = 1,
    L2_MBP = 2,
    L3_MBO = 3,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum BookAction {
    Add = 1,
    Update = 2,
    Delete = 3,
    Clear = 4,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum DepthType {
    Volume = 1,
    Exposure = 2,
}
