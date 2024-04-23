// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::correctness::check_valid_string;
use serde::{Deserialize, Serialize, Serializer};
use ustr::Ustr;

use super::fixed::check_fixed_precision;
use crate::{currencies::CURRENCY_MAP, enums::CurrencyType};

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Currency {
    pub code: Ustr,
    pub precision: u8,
    pub iso4217: u16,
    pub name: Ustr,
    pub currency_type: CurrencyType,
}

impl Currency {
    pub fn new(
        code: &str,
        precision: u8,
        iso4217: u16,
        name: &str,
        currency_type: CurrencyType,
    ) -> anyhow::Result<Self> {
        check_valid_string(code, "code")?;
        check_valid_string(name, "name")?;
        check_fixed_precision(precision)?;

        Ok(Self {
            code: Ustr::from(code),
            precision,
            iso4217,
            name: Ustr::from(name),
            currency_type,
        })
    }

    pub fn register(currency: Self, overwrite: bool) -> anyhow::Result<()> {
        let mut map = CURRENCY_MAP
            .lock()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        if !overwrite && map.contains_key(currency.code.as_str()) {
            // If overwrite is false and the currency already exists, simply return
            return Ok(());
        }

        // Insert or overwrite the currency in the map
        map.insert(currency.code.to_string(), currency);
        Ok(())
    }

    pub fn is_fiat(code: &str) -> anyhow::Result<bool> {
        let currency = Self::from_str(code)?;
        Ok(currency.currency_type == CurrencyType::Fiat)
    }

    pub fn is_crypto(code: &str) -> anyhow::Result<bool> {
        let currency = Self::from_str(code)?;
        Ok(currency.currency_type == CurrencyType::Crypto)
    }

    pub fn is_commodity_backed(code: &str) -> anyhow::Result<bool> {
        let currency = Self::from_str(code)?;
        Ok(currency.currency_type == CurrencyType::CommodityBacked)
    }
}

impl PartialEq for Currency {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
    }
}

impl Hash for Currency {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.code.hash(state);
    }
}

impl FromStr for Currency {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let map_guard = CURRENCY_MAP
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire lock on `CURRENCY_MAP`: {e}"))?;
        map_guard
            .get(s)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Unknown currency: {s}"))
    }
}

impl From<&str> for Currency {
    fn from(input: &str) -> Self {
        input.parse().unwrap()
    }
}

impl Serialize for Currency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.code.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Currency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let currency_str: &str = Deserialize::deserialize(deserializer)?;
        Self::from_str(currency_str).map_err(serde::de::Error::custom)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{enums::CurrencyType, types::currency::Currency};

    #[rstest]
    #[should_panic(expected = "code")]
    fn test_invalid_currency_code() {
        let _ = Currency::new("", 2, 840, "United States dollar", CurrencyType::Fiat).unwrap();
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision() {
        // Precision out of range for fixed
        let _ = Currency::new("USD", 10, 840, "United States dollar", CurrencyType::Fiat).unwrap();
    }

    #[rstest]
    fn test_new_for_fiat() {
        let currency =
            Currency::new("AUD", 2, 36, "Australian dollar", CurrencyType::Fiat).unwrap();
        assert_eq!(currency, currency);
        assert_eq!(currency.code.as_str(), "AUD");
        assert_eq!(currency.precision, 2);
        assert_eq!(currency.iso4217, 36);
        assert_eq!(currency.name.as_str(), "Australian dollar");
        assert_eq!(currency.currency_type, CurrencyType::Fiat);
    }

    #[rstest]
    fn test_new_for_crypto() {
        let currency = Currency::new("ETH", 8, 0, "Ether", CurrencyType::Crypto).unwrap();
        assert_eq!(currency, currency);
        assert_eq!(currency.code.as_str(), "ETH");
        assert_eq!(currency.precision, 8);
        assert_eq!(currency.iso4217, 0);
        assert_eq!(currency.name.as_str(), "Ether");
        assert_eq!(currency.currency_type, CurrencyType::Crypto);
    }

    #[rstest]
    fn test_equality() {
        let currency1 =
            Currency::new("USD", 2, 840, "United States dollar", CurrencyType::Fiat).unwrap();
        let currency2 =
            Currency::new("USD", 2, 840, "United States dollar", CurrencyType::Fiat).unwrap();
        assert_eq!(currency1, currency2);
    }

    #[rstest]
    fn test_serialization_deserialization() {
        let currency = Currency::USD();
        let serialized = serde_json::to_string(&currency).unwrap();
        let deserialized: Currency = serde_json::from_str(&serialized).unwrap();
        assert_eq!(currency, deserialized);
    }
}
