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

use std::{any::Any, ops::Deref, sync::Arc};

use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
use nautilus_model::{
    data::DataType,
    identifiers::{ClientId, Venue},
};

pub struct DataRequest {
    pub req_id: UUID4,
    pub client_id: ClientId,
    pub venue: Venue,
    pub data_type: DataType,
    pub ts_init: UnixNanos,
}

pub struct DataResponse {
    pub req: DataRequest,
    pub data: Arc<dyn Any + Send + Sync>,
}

impl DataRequest {
    pub fn new(
        req_id: UUID4,
        client_id: ClientId,
        venue: Venue,
        data_type: DataType,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            req_id,
            client_id,
            venue,
            data_type,
            ts_init,
        }
    }
}

impl DataResponse {
    // TODO: remove this. It should not be possible to create a data response
    // without a data request.
    pub fn new<T: Any + Send + Sync>(
        req_id: UUID4,
        client_id: ClientId,
        venue: Venue,
        data_type: DataType,
        ts_init: UnixNanos,
        data: T,
    ) -> Self {
        let req = DataRequest::new(req_id, client_id, venue, data_type, ts_init);
        Self {
            req,
            data: Arc::new(data),
        }
    }

    pub fn new_with_req<T: Any + Send + Sync>(req: DataRequest, data: T) -> Self {
        Self {
            req,
            data: Arc::new(data),
        }
    }
}

impl Deref for DataResponse {
    type Target = DataRequest;

    fn deref(&self) -> &Self::Target {
        &self.req
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
