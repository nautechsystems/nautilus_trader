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
#[derive(Clone, Hash, PartialEq, Debug)]
pub struct Symbol {
    value: Box<String>,
}

impl Symbol {
    pub fn from_string(s: String) -> Symbol {
        Symbol {
            value: Box::from(s),
        }
    }

    pub fn from_str(s: &str) -> Symbol {
        Symbol {
            value: Box::from(s.to_owned()),
        }
    }
}

impl Display for Symbol {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}", self.value)
    }
}

#[cfg(test)]
mod tests {
    use crate::identifiers::symbol::Symbol;

    #[test]
    fn test_symbol_from_str() {
        let symbol1 = Symbol::from_str("XRD/USD");
        let symbol2 = Symbol::from_str("BTC/USD");

        assert_eq!(symbol1, symbol1);
        assert_ne!(symbol1, symbol2);
        assert_eq!(symbol1.to_string(), "XRD/USD");
    }

    #[test]
    fn test_symbol_as_str() {
        let symbol = Symbol::from_str("ETH-PERP");

        assert_eq!(symbol.to_string(), "ETH-PERP");
    }
}
