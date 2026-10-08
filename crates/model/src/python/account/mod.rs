// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

pub mod betting;
pub mod cash;
pub mod margin;
pub mod margin_model;
pub mod transformer;
pub mod wallet;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{Py, PyAny, PyResult, Python, conversion::IntoPyObjectExt};

use crate::{
    accounts::{AccountAny, BettingAccount, CashAccount, MarginAccount, WalletAccount},
    enums::AccountType,
    types::Currency,
};

/// Converts a Python account object into a Rust `AccountAny` enum.
///
/// # Errors
///
/// Returns a `PyErr` if:
/// - retrieving the `account_type` attribute fails.
/// - extracting the object into the account type's concrete class fails.
/// - the `account_type` is unsupported.
#[expect(clippy::needless_pass_by_value)]
pub fn pyobject_to_account_any(py: Python, account: Py<PyAny>) -> PyResult<AccountAny> {
    let account_type = account
        .getattr(py, "account_type")?
        .extract::<AccountType>(py)?;
    if account_type == AccountType::Margin {
        let margin = account.extract::<MarginAccount>(py)?;
        Ok(AccountAny::Margin(margin))
    } else if account_type == AccountType::Cash {
        let cash = account.extract::<CashAccount>(py)?;
        Ok(AccountAny::Cash(cash))
    } else if account_type == AccountType::Betting {
        let betting = account.extract::<BettingAccount>(py)?;
        Ok(AccountAny::Betting(betting))
    } else if account_type == AccountType::Wallet {
        let wallet = account.extract::<WalletAccount>(py)?;
        Ok(AccountAny::Wallet(wallet))
    } else {
        Err(to_pyvalue_err("Unsupported account type"))
    }
}

/// Converts a Rust `AccountAny` into a Python account object.
///
/// # Errors
///
/// Returns a `PyErr` if converting the underlying account into a Python object fails.
pub fn account_any_to_pyobject(py: Python, account: AccountAny) -> PyResult<Py<PyAny>> {
    match account {
        AccountAny::Margin(account) => account.into_py_any(py),
        AccountAny::Cash(account) => account.into_py_any(py),
        AccountAny::Betting(account) => account.into_py_any(py),
        AccountAny::Wallet(account) => account.into_py_any(py),
    }
}

fn resolve_balance_currency(
    currency: Option<Currency>,
    base_currency: Option<Currency>,
) -> PyResult<Currency> {
    currency.or(base_currency).ok_or_else(|| {
        to_pyvalue_err("`currency` must be specified for an account with no base currency")
    })
}

#[cfg(test)]
mod tests {
    use pyo3::Python;
    use rstest::rstest;

    use super::resolve_balance_currency;
    use crate::types::Currency;

    #[rstest]
    #[case(Some(Currency::USD()), None, Currency::USD())]
    #[case(None, Some(Currency::EUR()), Currency::EUR())]
    #[case(Some(Currency::USD()), Some(Currency::EUR()), Currency::USD())]
    fn test_resolve_balance_currency(
        #[case] currency: Option<Currency>,
        #[case] base_currency: Option<Currency>,
        #[case] expected: Currency,
    ) {
        let resolved = resolve_balance_currency(currency, base_currency).unwrap();

        assert_eq!(resolved, expected);
    }

    #[rstest]
    fn test_resolve_balance_currency_rejects_missing_currency() {
        Python::initialize();
        Python::attach(|_| {
            let error = resolve_balance_currency(None, None).unwrap_err();

            assert_eq!(
                error.to_string(),
                "ValueError: `currency` must be specified for an account with no base currency"
            );
        });
    }
}
