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
    data::{Data, DataType},
    identifiers::{ClientId, Venue},
};

// TODO: redesign data messages for a tighter model
#[derive(Debug)]
pub struct DataRequest {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub venue: Venue,
    pub data_type: DataType,
    pub ts_init: UnixNanos,
}

pub type Payload = Arc<dyn Any + Send + Sync>;

#[derive(Debug)]
pub struct DataResponse {
    pub correlation_id: UUID4,
    pub client_id: ClientId,
    pub venue: Venue,
    pub data_type: DataType,
    pub data: Payload,
    pub ts_init: UnixNanos,
}

impl DataResponse {
    pub fn new<T: Any + Send + Sync>(
        correlation_id: UUID4,
        client_id: ClientId,
        venue: Venue,
        data_type: DataType,
        data: T,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            correlation_id,
            client_id,
            venue,
            data_type,
            data: Arc::new(data),
            ts_init,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Subscribe,
    Unsubscribe,
}

#[derive(Debug, Clone)]
pub struct SubscriptionCommand {
    pub client_id: ClientId,
    pub venue: Venue,
    pub data_type: DataType,
    pub action: Action,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl SubscriptionCommand {
    #[must_use]
    pub const fn new(
        client_id: ClientId,
        venue: Venue,
        data_type: DataType,
        action: Action,
        command_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            client_id,
            venue,
            data_type,
            action,
            command_id,
            ts_init,
        }
    }
}

pub enum DataEngineRequest {
    Request(DataRequest),
    SubscriptionCommand(SubscriptionCommand),
}

// TODO: Refine this to reduce disparity between enum sizes
#[allow(clippy::large_enum_variant)]
pub enum DataClientResponse {
    Response(DataResponse),
    Data(Data),
}
