// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{any::Any, sync::Arc};

use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    data::DataType,
    identifiers::{ClientId, Venue},
};

pub struct DataRequest {
    pub request_id: UUID4,
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub venue: Venue,
    pub data_type: DataType,
}

pub struct DataResponse {
    pub response_id: UUID4,
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub venue: Venue,
    pub data_type: DataType,
    pub data: Arc<dyn Any + Send + Sync>,
}

impl DataResponse {
    pub fn new<T: Any + Send + Sync>(
        response_id: UUID4,
        correlation_id: UUID4,
        client_id: ClientId,
        venue: Venue,
        data_type: DataType,
        data: T,
    ) -> Self {
        Self {
            response_id,
            correlation_id,
            client_id,
            venue,
            data_type,
            data: Arc::new(data),
        }
    }
}

pub enum DataCommandAction {
    Subscribe,
    Unsubscibe,
}

pub struct DataCommand {
    pub client_id: ClientId,
    pub venue: Venue,
    pub data_type: DataType,
    pub action: DataCommandAction,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}
