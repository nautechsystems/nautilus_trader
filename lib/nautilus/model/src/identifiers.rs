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

use nautilus_core::text::CStringRaw;

pub trait Identifier {
    fn get_value(&self) -> &str;
}

#[repr(C)]
#[derive(Clone)]
pub struct Symbol {
    pub value: CStringRaw,
}

// impl Symbol
// {
//     pub fn c_symbol_new(value: CStringRaw) -> Symbol {
//         return Symbol { value };
//     }
// }
//
// impl Identifier for Symbol {
//     fn get_value(&self) -> &str {
//         return &c_str_raw_to_string(self.value);
//     }
// }

// #[cfg(test)]
// mod tests {
//     use crate::identifiers::{Identifier, Symbol};
//
//     #[test]
//     fn instantiate() {
//         let symbol = Symbol {
//             value: "AUD/USD".to_string(),
//         };
//
//         assert_eq!("AUD/USD", symbol.value);
//         assert_eq!("AUD/USD", symbol.get_value());
//     }
// }
