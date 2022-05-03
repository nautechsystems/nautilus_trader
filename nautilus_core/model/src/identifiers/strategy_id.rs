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

use nautilus_core::buffer::{Buffer, Buffer36};
use std::fmt::{Debug, Display, Formatter, Result};

#[repr(C)]
#[derive(Clone, Hash, PartialEq, Debug)]
pub struct StrategyId {
    pub value: Buffer36,
}

impl From<&str> for StrategyId {
    fn from(s: &str) -> StrategyId {
        StrategyId {
            value: Buffer36::from(s),
        }
    }
}

impl Display for StrategyId {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_str())
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn strategy_id_free(strategy_id: StrategyId) {
    drop(strategy_id); // Memory freed here
}

#[no_mangle]
pub extern "C" fn strategy_id_from_buffer(value: Buffer36) -> StrategyId {
    StrategyId { value }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::StrategyId;

    #[test]
    fn test_strategy_id_from_str() {
        let strategy_id1 = StrategyId::from("EMACross-001");
        let strategy_id2 = StrategyId::from("EMACross-002");

        assert_eq!(strategy_id1, strategy_id1);
        assert_ne!(strategy_id1, strategy_id2);
        assert_eq!(strategy_id1.to_string(), "EMACross-001");
    }

    #[test]
    fn test_strategy_id_as_str() {
        let strategy_id = StrategyId::from("EMACross-001");

        assert_eq!(strategy_id.to_string(), "EMACross-001");
    }
}
