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

use nautilus_model::{enums::BarIntervalType, identifiers::ClientId};

/// Configuration for `DataEngine` instances.
#[derive(Clone, Debug)]
pub struct DataEngineConfig {
    pub time_bars_build_with_no_updates: bool,
    pub time_bars_timestamp_on_close: bool,
    pub time_bars_interval_type: BarIntervalType,
    pub validate_data_sequence: bool,
    pub buffer_deltas: bool,
    pub external_clients: Option<Vec<ClientId>>,
    pub debug: bool,
}

impl Default for DataEngineConfig {
    /// Creates a new default [`DataEngineConfig`] instance.
    fn default() -> Self {
        Self {
            time_bars_build_with_no_updates: true,
            time_bars_timestamp_on_close: true,
            time_bars_interval_type: BarIntervalType::LeftOpen,
            validate_data_sequence: false,
            buffer_deltas: false,
            external_clients: None,
            debug: false,
        }
    }
}
