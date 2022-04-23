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
use std::str::FromStr;

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
pub struct TradeId {
    value: u64,
}

impl TradeId {
    pub fn new(value: u64) -> TradeId {
        TradeId { value }
    }

    pub fn from_str(s: &str) -> TradeId {
        TradeId {
            value: u64::from_str(s).expect("u64::from_str failed"),
        }
    }
}

impl Display for TradeId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

#[cfg(test)]
mod tests {
    use crate::identifiers::trade_id::TradeId;

    #[test]
    fn test_instrument_id_from_str() {
        let trade_id1 = TradeId::from_str("123456789");
        let trade_id2 = TradeId::from_str("234567890");

        assert_eq!(trade_id1, trade_id1);
        assert_ne!(trade_id1, trade_id2);
        assert_eq!(trade_id1.value, 123456789)
    }
}
