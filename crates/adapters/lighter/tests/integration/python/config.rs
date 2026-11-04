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

use nautilus_lighter::config::{LighterDataClientConfig, LighterExecutionClientConfig};
use nautilus_model::identifiers::AccountId;
use pyo3::{
    PyRef, Python,
    types::{PyAnyMethods, PyDict, PyDictMethods},
};
use rstest::rstest;

#[rstest]
#[case(None, true)]
#[case(Some(420), true)]
#[case(Some(1_295_940), true)]
#[case(Some(419), false)]
#[case(Some(1_295_941), false)]
fn test_python_scheduled_cancel(#[case] timeout: Option<u64>, #[case] valid: bool) {
    Python::initialize();
    Python::attach(|py| {
        let kwargs = PyDict::new(py);
        kwargs
            .set_item("account_id", AccountId::from("LIGHTER-123"))
            .unwrap();

        if let Some(timeout) = timeout {
            kwargs
                .set_item("scheduled_cancel_timeout_secs", timeout)
                .unwrap();
        }

        let result = py
            .get_type::<LighterExecutionClientConfig>()
            .call((), Some(&kwargs));
        assert_eq!(result.is_ok(), valid);

        if valid {
            let config = result.unwrap();
            assert_eq!(
                config
                    .getattr("scheduled_cancel_timeout_secs")
                    .unwrap()
                    .extract::<Option<u64>>()
                    .unwrap(),
                timeout
            );
            assert_eq!(
                config
                    .extract::<PyRef<LighterExecutionClientConfig>>()
                    .unwrap()
                    .scheduled_cancel_timeout_secs,
                timeout
            );
        } else {
            assert!(
                result
                    .unwrap_err()
                    .is_instance_of::<pyo3::exceptions::PyValueError>(py)
            );
        }
    });
}

#[rstest]
fn test_data_config_py_new_book_timeout() {
    Python::initialize();
    Python::attach(|py| {
        let kwargs = PyDict::new(py);
        kwargs.set_item("rest_quota_per_min", 7).unwrap();
        kwargs.set_item("book_snapshot_timeout_secs", 42).unwrap();
        let config = py
            .get_type::<LighterDataClientConfig>()
            .call((), Some(&kwargs))
            .unwrap()
            .extract::<LighterDataClientConfig>()
            .unwrap();

        assert_eq!(config.rest_quota_per_min, Some(7));
        assert_eq!(config.book_snapshot_timeout_secs, 42);
    });
}

#[rstest]
fn test_data_config_py_new_book_timeout_default() {
    Python::initialize();
    Python::attach(|py| {
        let config = py
            .get_type::<LighterDataClientConfig>()
            .call0()
            .unwrap()
            .extract::<LighterDataClientConfig>()
            .unwrap();

        assert_eq!(config.book_snapshot_timeout_secs, 10);
    });
}
