// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

pub mod cash;
pub mod margin;
pub mod transformer;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{Py, PyAny, PyResult, Python, conversion::IntoPyObjectExt};

use crate::{
    accounts::{AccountAny, CashAccount, MarginAccount},
    enums::AccountType,
};

/// Converts a Python account object into a Rust `AccountAny` enum.
///
/// # Errors
///
/// Returns a `PyErr` if:
/// - retrieving the `account_type` attribute fails.
/// - extracting the object into `CashAccount` or `MarginAccount` fails.
/// - the `account_type` is unsupported.
pub fn pyobject_to_account_any(py: Python, account: Py<PyAny>) -> PyResult<AccountAny> {
    let account_type = account
        .getattr(py, "account_type")?
        .extract::<AccountType>(py)?;
    if account_type == AccountType::Cash {
        let cash = account.extract::<CashAccount>(py)?;
        Ok(AccountAny::Cash(cash))
    } else if account_type == AccountType::Margin {
        let margin = account.extract::<MarginAccount>(py)?;
        Ok(AccountAny::Margin(margin))
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
        AccountAny::Cash(account) => account.into_py_any(py),
        AccountAny::Margin(account) => account.into_py_any(py),
    }
}
