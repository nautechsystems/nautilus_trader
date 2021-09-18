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

use crate::enums::CurrencyType;
use rust_decimal::Decimal;
use std::fmt;
use std::str::FromStr;

pub struct Currency {
    pub code: String,
    pub precision: usize,
    pub currency_type: CurrencyType,
}

pub struct Money {
    pub amount: Decimal,
    pub currency: Currency,
}

impl Money {
    pub fn new(amount: Decimal, currency: Currency) -> Money {
        Money {
            amount: Decimal::from_str(&format!("{:.*}", currency.precision, amount)).unwrap(),
            currency,
        }
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:.*} {}",
            self.currency.precision, self.amount, self.currency.code
        )
    }
}

pub struct AccountBalance {
    pub currency: Currency,
    pub total: Money,
    pub locked: Money,
    pub free: Money,
}

// impl AccountBalance {
//     pub fn new(currency:)
// }
impl fmt::Display for AccountBalance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.currency.code, self.total, self.locked, self.free,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_money_new_usd() {
        let usd = Currency {
            code: String::from("USD"),
            precision: 2,
            currency_type: CurrencyType::Fiat,
        };
        let money = Money {
            amount: Decimal::new(1000, 0),
            currency: usd,
        };

        assert_eq!("1000.00 USD", money.to_string());
    }

    #[test]
    fn test_money_new_btc() {
        let btc = Currency {
            code: String::from("BTC"),
            precision: 8,
            currency_type: CurrencyType::Fiat,
        };
        let money = Money {
            amount: Decimal::new(103, 1),
            currency: btc,
        };

        assert_eq!("10.30000000 BTC", money.to_string());
    }

    // #[test]
    // fn test_account_balance() {
    //     let usd = Currency {
    //         code: String::from("USD"),
    //         precision: 2,
    //         currency_type: CurrencyType::Fiat,
    //     };
    //     let balance = AccountBalance {
    //         currency: usd,
    //         total: Money {
    //             amount: Decimal::new(103, 1),
    //             currency: usd,
    //         },
    //         locked: Money {
    //             amount: Decimal::new(0, 0),
    //             currency: usd,
    //         },
    //         free: Money {
    //             amount: Decimal::new(103, 1),
    //             currency: usd,
    //         },
    //     };
    //
    //     assert_eq!(balance.to_string(), "10.30000000 BTC");
    // }
}
