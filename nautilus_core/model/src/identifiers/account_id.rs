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

//! Represents a valid account ID.

use std::{
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use nautilus_core::correctness::{check_string_contains, check_valid_string};
use ustr::Ustr;

use super::venue::Venue;

/// Represents a valid account ID.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct AccountId(Ustr);

impl AccountId {
    /// Creates a new [`AccountId`] instance.
    ///
    /// Must be correctly formatted with two valid strings either side of a hyphen '-'.
    /// It is expected an account ID is the name of the issuer with an account number
    /// separated by a hyphen.
    ///
    /// Example: "IB-D02851908".
    /// # Panics
    ///
    /// Panics if `value` is not a valid string, or does not contain a hyphen '-' separator.
    pub fn new(value: &str) -> anyhow::Result<Self> {
        check_valid_string(value, stringify!(value))?;
        check_string_contains(value, "-", stringify!(value))?;

        Ok(Self(Ustr::from(value)))
    }

    /// Sets the inner identifier value.
    pub(crate) fn set_inner(&mut self, value: &str) {
        self.0 = Ustr::from(value);
    }

    /// Returns the inner identifier value.
    #[must_use]
    pub fn inner(&self) -> Ustr {
        self.0
    }

    /// Returns the inner identifier value as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns the account issuer for this identifier.
    #[must_use]
    pub fn get_issuer(&self) -> Venue {
        // SAFETY: Account ID is guaranteed to have chars either side of a hyphen
        Venue::from_str_unchecked(self.0.split('-').collect::<Vec<&str>>().first().unwrap())
    }

    /// Returns the account ID assigned by the issuer.
    #[must_use]
    pub fn get_issuers_id(&self) -> &str {
        // SAFETY: Account ID is guaranteed to have chars either side of a hyphen
        self.0.split('-').collect::<Vec<&str>>().last().unwrap()
    }
}

impl Debug for AccountId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Display for AccountId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for AccountId {
    fn from(input: &str) -> Self {
        Self::new(input).unwrap()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::identifiers::stubs::*;

    #[rstest]
    fn test_account_id_new_invalid_string() {
        let s = "";
        let result = AccountId::new(s);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_account_id_new_missing_hyphen() {
        let s = "123456789";
        let result = AccountId::new(s);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_account_id_fmt() {
        let s = "IB-U123456789";
        let account_id = AccountId::new(s).unwrap();
        let formatted = format!("{account_id}");
        assert_eq!(formatted, s);
    }

    #[rstest]
    fn test_string_reprs(account_ib: AccountId) {
        assert_eq!(account_ib.as_str(), "IB-1234567890");
    }

    #[rstest]
    fn test_get_issuer(account_ib: AccountId) {
        assert_eq!(account_ib.get_issuer(), Venue::new("IB").unwrap());
    }

    #[rstest]
    fn test_get_issuers_id(account_ib: AccountId) {
        assert_eq!(account_ib.get_issuers_id(), "1234567890");
    }
}
