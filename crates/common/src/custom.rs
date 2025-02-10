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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
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
