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

//! Execution client implementations for trading venue connectivity.

use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use async_trait::async_trait;
use nautilus_common::messages::execution::{
    BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, QueryOrder,
    SubmitOrder, SubmitOrderList,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    identifiers::{AccountId, ClientId, Venue},
    types::{AccountBalance, MarginBalance},
};

pub mod base;

/// Defines the interface for an execution client managing order operations.
///
/// # Thread safety
///
/// Client instances are not intended to be sent across threads. The `?Send` bound
/// allows implementations to hold non-Send state for any Python interop.
#[async_trait(?Send)]
pub trait ExecutionClient {
    fn is_connected(&self) -> bool;
    fn client_id(&self) -> ClientId;
    fn account_id(&self) -> AccountId;
    fn venue(&self) -> Venue;
    fn oms_type(&self) -> OmsType;
    fn get_account(&self) -> Option<AccountAny>;

    /// Generates and publishes the account state event.
    ///
    /// # Errors
    ///
    /// Returns an error if generating the account state fails.
    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()>;

    /// Starts the execution client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to start.
    fn start(&mut self) -> anyhow::Result<()>;

    /// Stops the execution client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to stop.
    fn stop(&mut self) -> anyhow::Result<()>;

    /// Connects the client to the execution venue.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    async fn connect(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Disconnects the client from the execution venue.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnection fails.
    async fn disconnect(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Submits a single order command to the execution venue.
    ///
    /// # Errors
    ///
    /// Returns an error if submission fails.
    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Submits a list of orders to the execution venue.
    ///
    /// # Errors
    ///
    /// Returns an error if submission fails.
    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Modifies an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if modification fails.
    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Cancels a specific order.
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails.
    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Cancels all orders.
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation fails.
    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Cancels a batch of orders.
    ///
    /// # Errors
    ///
    /// Returns an error if batch cancellation fails.
    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Queries the status of an account.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    fn query_account(&self, cmd: &QueryAccount) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }

    /// Queries the status of an order.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
        log_not_implemented(cmd);
        Ok(())
    }
}

#[inline(always)]
fn log_not_implemented<T: Debug>(cmd: &T) {
    log::warn!("{cmd:?} – handler not implemented");
}

/// Wraps an [`ExecutionClient`], managing its lifecycle and providing access to the client.
pub struct ExecutionClientAdapter {
    pub(crate) client: Box<dyn ExecutionClient>,
    pub client_id: ClientId,
    pub venue: Venue,
    pub account_id: AccountId,
    pub oms_type: OmsType,
}

impl Deref for ExecutionClientAdapter {
    type Target = Box<dyn ExecutionClient>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl DerefMut for ExecutionClientAdapter {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

impl Debug for ExecutionClientAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ExecutionClientAdapter))
            .field("client_id", &self.client_id)
            .field("venue", &self.venue)
            .field("account_id", &self.account_id)
            .field("oms_type", &self.oms_type)
            .finish()
    }
}

impl ExecutionClientAdapter {
    /// Creates a new [`ExecutionClientAdapter`] with the given client.
    #[must_use]
    pub fn new(client: Box<dyn ExecutionClient>) -> Self {
        let client_id = client.client_id();
        let venue = client.venue();
        let account_id = client.account_id();
        let oms_type = client.oms_type();

        Self {
            client,
            client_id,
            venue,
            account_id,
            oms_type,
        }
    }

    /// Connects the execution client to the venue.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        self.client.connect().await
    }

    /// Disconnects the execution client from the venue.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnection fails.
    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.client.disconnect().await
    }
}
