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

use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::ClientId;

pub struct DataRequest {
    pub actor_id: UUID4,
    pub req_id: UUID4,
    pub client_id: ClientId,
}

pub struct DataResponse {
    pub actor_id: UUID4,
    pub req_id: UUID4,
    pub client_id: ClientId,
}
