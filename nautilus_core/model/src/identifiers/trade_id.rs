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

use nautilus_core::buffer::{Buffer, Buffer32};
use std::fmt::{Debug, Display, Formatter, Result};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
pub struct TradeId {
    value: Buffer32,
}

impl From<&str> for TradeId {
    fn from(s: &str) -> TradeId {
        TradeId {
            value: Buffer32::from(s),
        }
    }
}

impl Display for TradeId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_str())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::identifiers::trade_id::TradeId;

    #[test]
    fn test_instrument_id_from_str() {
        let trade_id1 = TradeId::from("123456789");
        let trade_id2 = TradeId::from("234567890");

        assert_eq!(trade_id1, trade_id1);
        assert_ne!(trade_id1, trade_id2);
    }

    #[test]
    fn test_trade_id_as_str() {
        let trade_id = TradeId::from("1234567890");

        assert_eq!(trade_id.to_string(), "1234567890");
    }
}
