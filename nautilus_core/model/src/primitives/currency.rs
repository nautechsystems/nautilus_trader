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

use crate::enums::CurrencyType;

#[repr(C)]
#[derive(Eq, PartialEq, Clone, Hash, Debug)]
pub struct Currency {
    pub code: Box<String>,
    pub precision: u8,
    pub iso4217: u16,
    pub name: Box<String>,
    pub currency_type: CurrencyType,
}

impl Currency {
    pub fn new(
        code: &str,
        precision: u8,
        iso4217: u16,
        name: &str,
        currency_type: CurrencyType,
    ) -> Currency {
        Currency {
            code: Box::from(code.to_string()),
            precision,
            iso4217,
            name: Box::from(name.to_string()),
            currency_type,
        }
    }
}
