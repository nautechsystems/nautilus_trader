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
use nautilus_model::identifiers::{AccountId, Venue};
use pyo3::{
    PyRef, Python,
    types::{PyAnyMethods, PyDict, PyDictMethods},
};
use rstest::rstest;

#[rstest]
#[case("http://fixture:proxy-secret@proxy.example:8080", true)]
#[case("https://proxy.example:8443", true)]
#[case("127.0.0.1:8080", false)]
#[case("socks5://127.0.0.1:1080", false)]
#[case("http://fixture:proxy-secret@[::1", false)]
fn test_python_proxy_url_validation(
    #[values(false, true)] execution: bool,
    #[case] value: &str,
    #[case] valid: bool,
) {
    Python::initialize();
    Python::attach(|py| {
        let kwargs = PyDict::new(py);
        kwargs.set_item("proxy_url", value).unwrap();
        let class = if execution {
            kwargs
                .set_item("account_id", AccountId::from("LIGHTER-123"))
                .unwrap();
            py.get_type::<LighterExecutionClientConfig>().into_any()
        } else {
            py.get_type::<LighterDataClientConfig>().into_any()
        };

        let result = class.call((), Some(&kwargs));
        if valid {
            let config = result.unwrap();
            let proxy = if execution {
                config
                    .extract::<LighterExecutionClientConfig>()
                    .unwrap()
                    .proxy_url
            } else {
                config
                    .extract::<LighterDataClientConfig>()
                    .unwrap()
                    .proxy_url
            };

            assert_eq!(proxy.unwrap().expose_secret(), value);
        } else {
            let error = result.unwrap_err();
            assert!(error.is_instance_of::<pyo3::exceptions::PyValueError>(py));
            assert_eq!(
                error.value(py).to_string(),
                "Lighter proxy_url must be an absolute http:// or https:// URL with a host"
            );
        }
    });
}

#[rstest]
#[case("base_url_ws", "wss://example.com/stream?readonly=true", false)]
#[case("base_url_ws", "wss://example.com/stream?readonly=false", true)]
#[case(
    "base_url_ws",
    "wss://example.com/stream?readonly=false&readonly=true",
    false
)]
#[case("venue", "LIGHTER-ALT", false)]
#[case("venue", "LIGHTER_ALT", true)]
fn test_python_execution_config_footguns(
    #[case] field: &str,
    #[case] value: &str,
    #[case] valid: bool,
) {
    Python::initialize();
    Python::attach(|py| {
        let kwargs = PyDict::new(py);
        kwargs
            .set_item("account_id", AccountId::from("LIGHTER_ALT-123"))
            .unwrap();

        if field == "venue" {
            kwargs.set_item(field, Venue::from(value)).unwrap();
        } else {
            kwargs.set_item(field, value).unwrap();
        }

        let result = py
            .get_type::<LighterExecutionClientConfig>()
            .call((), Some(&kwargs));

        if valid {
            let config = result
                .unwrap()
                .extract::<LighterExecutionClientConfig>()
                .unwrap();

            let actual = if field == "venue" {
                config.venue.unwrap().to_string()
            } else {
                config.base_url_ws.unwrap()
            };

            assert_eq!(actual, value);
        } else {
            let error = result.unwrap_err();
            assert!(error.is_instance_of::<pyo3::exceptions::PyValueError>(py));
            assert!(error.value(py).to_string().contains(if field == "venue" {
                "account ID issuer can match"
            } else {
                "does not disable trading"
            }));
        }
    });
}

#[rstest]
#[case(None, Some(50))]
#[case(Some(0), Some(0))]
#[case(Some(9_999), Some(9_999))]
#[case(Some(10_000), None)]
#[case(Some(u32::MAX), None)]
fn test_python_market_order_slippage_bps(
    #[case] value: Option<u32>,
    #[case] expected: Option<u32>,
) {
    Python::initialize();
    Python::attach(|py| {
        let kwargs = PyDict::new(py);
        kwargs
            .set_item("account_id", AccountId::from("LIGHTER-123"))
            .unwrap();

        if let Some(value) = value {
            kwargs.set_item("market_order_slippage_bps", value).unwrap();
        }

        let result = py
            .get_type::<LighterExecutionClientConfig>()
            .call((), Some(&kwargs));

        if let Some(expected) = expected {
            assert_eq!(
                result
                    .unwrap()
                    .extract::<LighterExecutionClientConfig>()
                    .unwrap()
                    .market_order_slippage_bps,
                expected
            );
        } else {
            let error = result.unwrap_err();
            assert!(error.is_instance_of::<pyo3::exceptions::PyValueError>(py));
            assert_eq!(
                error.value(py).to_string(),
                "Lighter market_order_slippage_bps must be in [0, 10000)"
            );
        }
    });
}

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
