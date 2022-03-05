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

use crate::primitives::currency::Currency;
use crate::primitives::fixed::{f64_to_fixed_i64, fixed_i64_to_f64};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter, Result};
use std::hash::{Hash, Hasher};
use std::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

#[repr(C)]
#[derive(Eq, Clone)]
pub struct Money {
    fixed: i64,
    pub currency: Currency,
}

impl Money {
    pub fn new(amount: f64, currency: Currency) -> Money {
        Money {
            fixed: f64_to_fixed_i64(amount, currency.precision as i8),
            currency,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.fixed == 0
    }
    pub fn as_f64(&self) -> f64 {
        fixed_i64_to_f64(self.fixed)
    }
}

impl Hash for Money {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fixed.hash(state);
        self.currency.hash(state);
    }
}

impl PartialEq for Money {
    fn eq(&self, other: &Self) -> bool {
        self.fixed == other.fixed && self.currency == other.currency
    }
}

impl PartialOrd for Money {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.fixed.partial_cmp(&other.fixed)
    }

    fn lt(&self, other: &Self) -> bool {
        assert_eq!(self.currency, other.currency);
        self.fixed.lt(&other.fixed)
    }

    fn le(&self, other: &Self) -> bool {
        assert_eq!(self.currency, other.currency);
        self.fixed.le(&other.fixed)
    }

    fn gt(&self, other: &Self) -> bool {
        assert_eq!(self.currency, other.currency);
        self.fixed.gt(&other.fixed)
    }

    fn ge(&self, other: &Self) -> bool {
        assert_eq!(self.currency, other.currency);
        self.fixed.ge(&other.fixed)
    }
}

impl Ord for Money {
    fn cmp(&self, other: &Self) -> Ordering {
        assert_eq!(self.currency, other.currency);
        self.fixed.cmp(&other.fixed)
    }
}

impl Neg for Money {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Money {
            fixed: -self.fixed,
            currency: self.currency,
        }
    }
}

impl Add for Money {
    type Output = Self;
    fn add(self, rhs: Money) -> Self::Output {
        assert_eq!(self.currency, rhs.currency);
        Money {
            fixed: self.fixed + rhs.fixed,
            currency: self.currency,
        }
    }
}

impl Sub for Money {
    type Output = Self;
    fn sub(self, rhs: Money) -> Self::Output {
        assert_eq!(self.currency, rhs.currency);
        Money {
            fixed: self.fixed - rhs.fixed,
            currency: self.currency,
        }
    }
}

impl Mul for Money {
    type Output = Self;
    fn mul(self, rhs: Money) -> Self {
        assert_eq!(self.currency, rhs.currency);
        Money {
            fixed: self.fixed * rhs.fixed,
            currency: self.currency,
        }
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, other: Self) {
        assert_eq!(self.currency, other.currency);
        self.fixed += other.fixed;
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, other: Self) {
        assert_eq!(self.currency, other.currency);
        self.fixed -= other.fixed;
    }
}

impl MulAssign for Money {
    fn mul_assign(&mut self, multiplier: Self) {
        self.fixed *= multiplier.fixed;
    }
}

impl Add<f64> for Money {
    type Output = f64;
    fn add(self, rhs: f64) -> Self::Output {
        self.as_f64() + rhs
    }
}

impl Sub<f64> for Money {
    type Output = f64;
    fn sub(self, rhs: f64) -> Self::Output {
        self.as_f64() - rhs
    }
}

impl Mul<f64> for Money {
    type Output = f64;
    fn mul(self, rhs: f64) -> Self::Output {
        self.as_f64() * rhs
    }
}

impl Display for Money {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(
            f,
            "{:.*} {}",
            self.currency.precision as usize,
            self.as_f64(),
            self.currency.code
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::CurrencyType;

    #[test]
    fn test_money_new_usd() {
        let usd = Currency::new("USD", 2, 840, "United States dollar", CurrencyType::FIAT);
        let money = Money::new(1000.0, usd);

        assert_eq!("1000.00 USD", money.to_string());
    }

    #[test]
    fn test_money_new_btc() {
        let btc = Currency::new("BTC", 8, 0, "Bitcoin", CurrencyType::FIAT);

        let money = Money::new(10.3, btc);

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
