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
use nautilus_core::buffer::{Buffer16, Buffer32};

#[repr(C)]
#[derive(Eq, PartialEq, Clone, Hash, Debug)]
pub struct Currency {
    pub code: Buffer16,
    pub precision: u8,
    pub iso4217: u16,
    pub name: Buffer32,
    pub currency_type: CurrencyType,
}

impl Currency {
    pub fn new(
        code: Buffer16,
        precision: u8,
        iso4217: u16,
        name: Buffer32,
        currency_type: CurrencyType,
    ) -> Currency {
        Currency {
            code,
            precision,
            iso4217,
            name,
            currency_type,
        }
    }
}

#[allow(unused_imports)] // warning: unused import: `std::fmt::Write as FmtWrite`
#[cfg(test)]
mod tests {
    use crate::enums::CurrencyType;
    use crate::types::currency::Currency;
    use nautilus_core::buffer::{Buffer16, Buffer32};

    #[test]
    fn test_price_new() {
        let currency = Currency::new(
            Buffer16::from_str("AUD"),
            8,
            036,
            Buffer32::from_str("Australian dollar"),
            CurrencyType::FIAT,
        );

        assert_eq!(currency, currency);
        assert_eq!(currency.code.to_str(), "AUD");
        assert_eq!(currency.precision, 8);
        assert_eq!(currency.iso4217, 036);
        assert_eq!(currency.name.to_str(), "Australian dollar");
        assert_eq!(currency.currency_type, CurrencyType::FIAT);
    }
}
