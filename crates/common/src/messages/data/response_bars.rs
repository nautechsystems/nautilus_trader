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

use std::any::Any;

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{Bar, BarType},
    identifiers::ClientId,
};

#[derive(Clone, Debug)]
pub struct BarsResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub bar_type: BarType,
    pub data: Vec<Bar>,
    pub ts_init: UnixNanos,
    pub params: Option<IndexMap<String, String>>,
}

impl BarsResponse {
    /// Converts to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Creates a new [`BarsResponse`] instance.
    pub fn new(
        correlation_id: UUID4,
        client_id: ClientId,
        bar_type: BarType,
        data: Vec<Bar>,
        ts_init: UnixNanos,
        params: Option<IndexMap<String, String>>,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            bar_type,
            data,
            ts_init,
            params,
        }
    }
}
