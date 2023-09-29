// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    fmt::{Display, Formatter},
    hash::Hash,
};

use nautilus_core::{serialization::Serializable, time::UnixNanos};
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};

use crate::identifiers::instrument_id::InstrumentId;

/// Represents a single quote tick in a financial market.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")]
pub struct Ticker {
    /// The quotes instrument ID.
    pub instrument_id: InstrumentId,
    /// The UNIX timestamp (nanoseconds) when the tick event occurred.
    pub ts_event: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl Ticker {
    #[must_use]
    pub fn new(instrument_id: InstrumentId, ts_event: UnixNanos, ts_init: UnixNanos) -> Self {
        Self {
            instrument_id,
            ts_event,
            ts_init,
        }
    }
}

impl Serializable for Ticker {}

impl Display for Ticker {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{}",
            self.instrument_id, self.ts_event, self.ts_init,
        )
    }
}
