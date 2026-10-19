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

//! A user custom data type.

use bytes::Bytes;
use nautilus_core::UnixNanos;
use nautilus_model::data::DataType;
use serde::{Deserialize, Serialize};

/// Represents a custom data.
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
pub struct CustomData {
    pub data_type: DataType,
    pub value: Bytes,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl CustomData {
    /// Creates a new [`CustomData`] instance.
    pub const fn new(
        data_type: DataType,
        value: Bytes,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            data_type,
            value,
            ts_event,
            ts_init,
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::DataType;
    use rstest::rstest;

    use super::*;

    fn custom_data() -> CustomData {
        CustomData::new(
            DataType::new("MyData", None, None),
            Bytes::from_static(b"payload"),
            UnixNanos::from(3),
            UnixNanos::from(5),
        )
    }

    #[rstest]
    fn test_new_assigns_every_field() {
        let data = custom_data();

        assert_eq!(data.data_type, DataType::new("MyData", None, None));
        assert_eq!(data.value, Bytes::from_static(b"payload"));
        assert_eq!(data.ts_event, UnixNanos::from(3));
        assert_eq!(data.ts_init, UnixNanos::from(5));
    }

    #[rstest]
    #[case::data_type(CustomData { data_type: DataType::new("OtherData", None, None), ..custom_data() })]
    #[case::value(CustomData { value: Bytes::from_static(b"other"), ..custom_data() })]
    #[case::ts_event(CustomData { ts_event: UnixNanos::from(4), ..custom_data() })]
    #[case::ts_init(CustomData { ts_init: UnixNanos::from(6), ..custom_data() })]
    fn test_equality_discriminates_each_field(#[case] other: CustomData) {
        let data = custom_data();

        assert_eq!(data, custom_data());
        assert_ne!(data, other);
    }

    #[rstest]
    fn test_serde_round_trips() {
        let data = custom_data();

        let json = serde_json::to_string(&data).unwrap();

        assert_eq!(serde_json::from_str::<CustomData>(&json).unwrap(), data);
    }
}
