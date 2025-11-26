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

use std::cell::Ref;

use nautilus_common::{clock::Clock, messages::DataEvent};
use nautilus_data::client::DataClient;

#[async_trait::async_trait]
pub trait LiveDataClient: DataClient {
    /// Establishes a connection for live data.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    async fn connect(&mut self) -> anyhow::Result<()>;

    /// Disconnects the live data client.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnection fails.
    async fn disconnect(&mut self) -> anyhow::Result<()>;

    fn get_message_channel(&self) -> tokio::sync::mpsc::UnboundedSender<DataEvent>;

    fn get_clock(&self) -> Ref<'_, dyn Clock>;
}
