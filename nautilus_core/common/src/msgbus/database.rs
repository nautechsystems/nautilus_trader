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

use std::collections::HashMap;

use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::trader_id::TraderId;

/// A generic message bus database facade.
///
/// The main operations take a consistent `key` and `payload` which should provide enough
/// information to implement the message bus database in many different technologies.
///
/// Delete operations may need a `payload` to target specific values.
pub trait MessageBusDatabaseAdapter {
    type DatabaseType;

    fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        config: HashMap<String, serde_json::Value>,
    ) -> anyhow::Result<Self::DatabaseType>;
    fn publish(&self, topic: String, payload: Vec<u8>) -> anyhow::Result<()>;
    fn close(&mut self) -> anyhow::Result<()>;
}
