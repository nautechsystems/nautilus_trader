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
pub struct Symbol {
    pub value: Buffer32,
}

impl From<&str> for Symbol {
    fn from(s: &str) -> Symbol {
        Symbol {
            value: Buffer32::from(s),
        }
    }
}

impl Display for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value.to_str())
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn symbol_free(symbol: Symbol) {
    drop(symbol); // Memory freed here
}

#[no_mangle]
pub extern "C" fn symbol_from_buffer(value: Buffer32) -> Symbol {
    Symbol { value }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::Symbol;

    #[test]
    fn test_symbol_from_str() {
        let symbol1 = Symbol::from("XRD/USD");
        let symbol2 = Symbol::from("BTC/USD");

        assert_eq!(symbol1, symbol1);
        assert_ne!(symbol1, symbol2);
        assert_eq!(symbol1.to_string(), "XRD/USD");
    }

    #[test]
    fn test_symbol_as_str() {
        let symbol = Symbol::from("ETH-PERP");

        assert_eq!(symbol.to_string(), "ETH-PERP");
    }
}
