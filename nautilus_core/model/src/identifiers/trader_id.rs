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
pub struct TraderId {
    pub value: Buffer32,
}

impl From<&str> for TraderId {
    fn from(s: &str) -> TraderId {
        TraderId {
            value: Buffer32::from(s),
        }
    }
}

impl Display for TraderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_str())
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn trader_id_free(trader_id: TraderId) {
    drop(trader_id); // Memory freed here
}

#[no_mangle]
pub extern "C" fn trader_id_from_buffer(value: Buffer32) -> TraderId {
    TraderId { value }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::TraderId;

    #[test]
    fn test_trader_id_from_str() {
        let trader_id1 = TraderId::from("EMACross-001");
        let trader_id2 = TraderId::from("EMACross-002");

        assert_eq!(trader_id1, trader_id1);
        assert_ne!(trader_id1, trader_id2);
        assert_eq!(trader_id1.to_string(), "EMACross-001");
    }

    #[test]
    fn test_trader_id_as_str() {
        let trader_id = TraderId::from("EMACross-001");

        assert_eq!(trader_id.to_string(), "EMACross-001");
    }
}
