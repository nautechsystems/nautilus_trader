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

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{data::DataType, identifiers::ClientId};

#[derive(Clone, Debug)]
pub struct RequestCustomData {
    pub client_id: ClientId,
    pub data_type: DataType,
    pub request_id: UUID4,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl RequestCustomData {
    /// Creates a new [`RequestCustomData`] instance.
    pub fn new(
        client_id: ClientId,
        data_type: DataType,
        request_id: UUID4,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            client_id,
            data_type,
            request_id,
            ts_init,
            params,
        }
    }
}
