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

use nautilus_core::buffer::{Buffer, Buffer128};
use std::fmt::{Debug, Display, Formatter, Result};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
pub struct PositionId {
    pub value: Buffer128,
}

impl From<&str> for PositionId {
    fn from(s: &str) -> PositionId {
        PositionId {
            value: Buffer128::from(s),
        }
    }
}

impl Display for PositionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_str())
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn position_id_free(position_id: PositionId) {
    drop(position_id); // Memory freed here
}

#[no_mangle]
pub extern "C" fn position_id_from_buffer(value: Buffer128) -> PositionId {
    PositionId { value }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::PositionId;

    #[test]
    fn test_position_id_from_str() {
        let position_id1 = PositionId::from("ETHUSDT.BINANCE-EMACross-001");
        let position_id2 = PositionId::from("BTCUSDT.BINANCE-EMACross-002");

        assert_eq!(position_id1, position_id1);
        assert_ne!(position_id1, position_id2);
        assert_eq!(position_id1.to_string(), "ETHUSDT.BINANCE-EMACross-001");
    }

    #[test]
    fn test_position_id_as_str() {
        let position_id = PositionId::from("ETHUSDT.BINANCE-EMACross-001");

        assert_eq!(position_id.to_string(), "ETHUSDT.BINANCE-EMACross-001");
    }
}
