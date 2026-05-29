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

//! DeFi data input for the backtest engine.

use nautilus_model::{data::Data, defi::DefiData, identifiers::ClientId};

use crate::engine::BacktestEngine;

impl BacktestEngine {
    /// Adds DeFi data to the engine for replay during the backtest run.
    ///
    /// The `client_id` registers a backtest data client for DeFi startup
    /// subscriptions and pool snapshot requests. When `client_id` is `None`, a
    /// default `BACKTEST` client is registered.
    ///
    /// # Errors
    ///
    /// Returns an error if `data` is empty.
    pub fn add_defi_data(
        &mut self,
        data: Vec<DefiData>,
        client_id: Option<ClientId>,
        sort: bool,
    ) -> anyhow::Result<()> {
        self.add_defi_data_iterator(data, client_id, sort)
    }

    /// Adds DeFi data from an iterator for replay during the backtest run.
    ///
    /// # Errors
    ///
    /// Returns an error if `data` is empty.
    pub fn add_defi_data_iterator<I>(
        &mut self,
        data: I,
        client_id: Option<ClientId>,
        sort: bool,
    ) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = DefiData>,
    {
        let data: Vec<Data> = data
            .into_iter()
            .map(|defi| Data::Defi(Box::new(defi)))
            .collect();
        self.add_data(data, client_id, false, sort)
    }

    pub(crate) fn add_defi_data_client_if_not_exists(&mut self, client_id: Option<ClientId>) {
        let client_id = client_id.unwrap_or_else(|| ClientId::from("BACKTEST"));
        self.add_data_client_if_not_exists(client_id);
    }
}
