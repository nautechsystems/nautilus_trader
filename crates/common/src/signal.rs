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

//! A user signal type.

use std::{any::Any, sync::Arc};

use nautilus_core::UnixNanos;
use nautilus_model::data::{HasTsInit, custom::CustomDataTrait};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// Represents a generic signal.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.common", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")
)]
pub struct Signal {
    pub name: Ustr,
    pub value: String,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl Signal {
    /// Creates a new [`Signal`] instance.
    #[must_use]
    pub const fn new(name: Ustr, value: String, ts_event: UnixNanos, ts_init: UnixNanos) -> Self {
        Self {
            name,
            value,
            ts_event,
            ts_init,
        }
    }
}

impl HasTsInit for Signal {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for Signal {
    fn type_name(&self) -> &'static str {
        "Signal"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|o| self == o)
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        use pyo3::IntoPyObjectExt;
        self.clone().into_py_any(py)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::stubs::StubCustomData;
    use rstest::rstest;

    use super::*;

    fn test_signal() -> Signal {
        Signal::new(
            Ustr::from("price_alert"),
            "TRIGGERED".to_string(),
            UnixNanos::from(3),
            UnixNanos::from(5),
        )
    }

    #[rstest]
    fn test_new_assigns_every_field() {
        let signal = test_signal();

        assert_eq!(signal.name, Ustr::from("price_alert"));
        assert_eq!(signal.value, "TRIGGERED");
        assert_eq!(signal.ts_event, UnixNanos::from(3));
        assert_eq!(signal.ts_init, UnixNanos::from(5));
    }

    #[rstest]
    fn test_custom_data_trait_accessors() {
        let signal = test_signal();

        assert_eq!(signal.type_name(), "Signal");
        assert_eq!(CustomDataTrait::ts_event(&signal), UnixNanos::from(3));
        assert_eq!(HasTsInit::ts_init(&signal), UnixNanos::from(5));
        assert_eq!(
            signal.as_any().downcast_ref::<Signal>(),
            Some(&test_signal())
        );
    }

    #[rstest]
    fn test_to_json_round_trips() {
        let signal = test_signal();

        let json = signal.to_json().unwrap();

        assert_eq!(serde_json::from_str::<Signal>(&json).unwrap(), signal);
    }

    #[rstest]
    fn test_clone_arc_preserves_value() {
        let signal = test_signal();

        let cloned = signal.clone_arc();

        assert_eq!(cloned.type_name(), "Signal");
        assert_eq!(cloned.as_any().downcast_ref::<Signal>(), Some(&signal));
    }

    #[rstest]
    #[case::name(Signal { name: Ustr::from("other_name"), ..test_signal() })]
    #[case::value(Signal { value: "CLEARED".to_string(), ..test_signal() })]
    #[case::ts_event(Signal { ts_event: UnixNanos::from(4), ..test_signal() })]
    #[case::ts_init(Signal { ts_init: UnixNanos::from(6), ..test_signal() })]
    fn test_eq_arc_rejects_any_differing_field(#[case] other: Signal) {
        let signal = test_signal();

        assert!(signal.eq_arc(&test_signal()));
        assert!(!signal.eq_arc(&other));
    }

    #[rstest]
    fn test_eq_arc_rejects_a_different_custom_data_type() {
        let signal = test_signal();

        let other = StubCustomData {
            ts_init: UnixNanos::from(5),
            value: 1,
        };

        assert!(!signal.eq_arc(&other));
    }
}
