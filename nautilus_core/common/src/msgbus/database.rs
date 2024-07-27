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

use std::{collections::HashMap, fmt};

use bytes::Bytes;
use nautilus_core::uuid::UUID4;
use nautilus_model::identifiers::TraderId;
use serde::{Deserialize, Serialize};

/// Represents a bus message including a topic and payload.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common")
)]
pub struct BusMessage {
    /// The topic to publish on.
    pub topic: String,
    /// The serialized payload for the message.
    pub payload: Bytes,
}

impl fmt::Display for BusMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}",
            self.topic,
            String::from_utf8_lossy(&self.payload)
        )
    }
}

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
    fn publish(&self, topic: String, payload: Bytes) -> anyhow::Result<()>;
    fn close(&mut self) -> anyhow::Result<()>;
}
