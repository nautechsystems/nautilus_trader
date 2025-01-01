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

use nautilus_core::python::to_pyvalue_err;
use pyo3::{prelude::*, types::PyDict};

use crate::{
    accounts::{Account, CashAccount, MarginAccount},
    events::AccountState,
};

#[pyfunction]
pub fn cash_account_from_account_events(
    events: Vec<Bound<'_, PyDict>>,
    calculate_account_state: bool,
) -> PyResult<CashAccount> {
    let account_events = events
        .into_iter()
        .map(|obj| AccountState::py_from_dict(&obj))
        .collect::<PyResult<Vec<AccountState>>>()
        .unwrap();
    if account_events.is_empty() {
        return Err(to_pyvalue_err("No account events"));
    }
    let init_event = account_events[0].clone();
    let mut cash_account = CashAccount::new(init_event, calculate_account_state);
    for event in account_events.iter().skip(1) {
        cash_account.apply(event.clone());
    }
    Ok(cash_account)
}

#[pyfunction]
pub fn margin_account_from_account_events(
    events: Vec<Bound<'_, PyDict>>,
    calculate_account_state: bool,
) -> PyResult<MarginAccount> {
    let account_events = events
        .into_iter()
        .map(|obj| AccountState::py_from_dict(&obj))
        .collect::<PyResult<Vec<AccountState>>>()
        .unwrap();
    if account_events.is_empty() {
        return Err(to_pyvalue_err("No account events"));
    }
    let init_event = account_events[0].clone();
    let mut margin_account = MarginAccount::new(init_event, calculate_account_state);
    for event in account_events.iter().skip(1) {
        margin_account.apply(event.clone());
    }
    Ok(margin_account)
}
