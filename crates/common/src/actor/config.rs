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

use nautilus_model::identifiers::ComponentId;

/// Configuration for Actor components.
#[derive(Debug, Clone)]
pub struct ActorConfig {
    /// The custom identifier for the Actor.
    pub actor_id: Option<ComponentId>, // TODO: Define ActorId
    /// Whether to log events.
    pub log_events: bool,
    /// Whether to log commands.
    pub log_commands: bool,
}

impl Default for ActorConfig {
    fn default() -> Self {
        Self {
            actor_id: None,
            log_events: true,
            log_commands: true,
        }
    }
}
