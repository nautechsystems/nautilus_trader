// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Live market data client for the Bullet adapter.

use nautilus_common::clients::DataClient;
use nautilus_model::identifiers::{ClientId, Venue};

use crate::{common::consts::BULLET_VENUE, config::BulletDataClientConfig};

/// Live market data client for the Bullet exchange.
#[derive(Debug)]
pub struct BulletDataClient {
    client_id: ClientId,
    config: BulletDataClientConfig,
    connected: bool,
}

impl BulletDataClient {
    /// Create a new [`BulletDataClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if initialisation fails.
    pub fn new(client_id: ClientId, config: BulletDataClientConfig) -> anyhow::Result<Self> {
        Ok(Self { client_id, config, connected: false })
    }
}

impl DataClient for BulletDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*BULLET_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.connected = true;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.connected = false;
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn is_disconnected(&self) -> bool {
        !self.connected
    }
}
