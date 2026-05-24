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

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use async_trait::async_trait;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::{Clock, TestClock},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, QueryOrder,
        SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    identifiers::{AccountId, ClientId, ClientOrderId, Venue},
    instruments::InstrumentAny,
    types::{AccountBalance, MarginBalance},
};

/// A stub execution client for testing purposes.
///
/// This client provides a minimal implementation of the `ExecutionClient` trait
/// that can be used in unit tests without requiring actual venue connectivity.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct StubExecutionClient {
    client_id: ClientId,
    account_id: AccountId,
    venue: Venue,
    oms_type: OmsType,
    is_connected: bool,
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    received_instruments: Rc<RefCell<Vec<InstrumentAny>>>,
    start_count: Rc<Cell<usize>>,
    stop_count: Rc<Cell<usize>>,
    reset_count: Rc<Cell<usize>>,
    dispose_count: Rc<Cell<usize>>,
    submitted_order_ids: Rc<RefCell<Vec<ClientOrderId>>>,
    handles_all_order_venues: bool,
}

impl StubExecutionClient {
    /// Creates a new [`StubExecutionClient`] instance.
    #[allow(dead_code)]
    pub fn new(
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        oms_type: OmsType,
        clock: Option<Rc<RefCell<dyn Clock>>>,
    ) -> Self {
        Self {
            client_id,
            account_id,
            venue,
            oms_type,
            is_connected: false,
            clock: clock.unwrap_or_else(|| Rc::new(RefCell::new(TestClock::new()))),
            cache: Rc::new(RefCell::new(Cache::new(None, None))),
            received_instruments: Rc::new(RefCell::new(Vec::new())),
            start_count: Rc::new(Cell::new(0)),
            stop_count: Rc::new(Cell::new(0)),
            reset_count: Rc::new(Cell::new(0)),
            dispose_count: Rc::new(Cell::new(0)),
            submitted_order_ids: Rc::new(RefCell::new(Vec::new())),
            handles_all_order_venues: false,
        }
    }

    /// Configures this stub to accept orders for any instrument venue.
    #[must_use]
    pub fn with_handles_all_order_venues(mut self) -> Self {
        self.handles_all_order_venues = true;
        self
    }

    /// Returns a shared handle to the instruments delivered via [`ExecutionClient::on_instrument`].
    #[must_use]
    pub fn received_instruments(&self) -> Rc<RefCell<Vec<InstrumentAny>>> {
        self.received_instruments.clone()
    }

    /// Returns a shared handle to the submitted order IDs.
    #[must_use]
    pub fn submitted_order_ids(&self) -> Rc<RefCell<Vec<ClientOrderId>>> {
        self.submitted_order_ids.clone()
    }

    /// Returns the number of times [`ExecutionClient::start`] was invoked.
    #[must_use]
    pub fn start_count(&self) -> usize {
        self.start_count.get()
    }

    /// Returns the number of times [`ExecutionClient::stop`] was invoked.
    #[must_use]
    pub fn stop_count(&self) -> usize {
        self.stop_count.get()
    }

    /// Returns the number of times [`ExecutionClient::reset`] was invoked.
    #[must_use]
    pub fn reset_count(&self) -> usize {
        self.reset_count.get()
    }

    /// Returns the number of times [`ExecutionClient::dispose`] was invoked.
    #[must_use]
    pub fn dispose_count(&self) -> usize {
        self.dispose_count.get()
    }
}

#[async_trait(?Send)]
impl ExecutionClient for StubExecutionClient {
    fn is_connected(&self) -> bool {
        self.is_connected
    }

    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn account_id(&self) -> AccountId {
        self.account_id
    }

    fn venue(&self) -> Venue {
        self.venue
    }

    fn handles_order_venue(&self, venue: Venue) -> bool {
        self.handles_all_order_venues || self.venue == venue
    }

    fn oms_type(&self) -> OmsType {
        self.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        None // Stub implementation returns None
    }

    fn generate_account_state(
        &self,
        _balances: Vec<AccountBalance>,
        _margins: Vec<MarginBalance>,
        _reported: bool,
        _ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.is_connected = true;
        self.start_count.set(self.start_count.get() + 1);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.is_connected = false;
        self.stop_count.set(self.stop_count.get() + 1);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.reset_count.set(self.reset_count.get() + 1);
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.dispose_count.set(self.dispose_count.get() + 1);
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        self.submitted_order_ids
            .borrow_mut()
            .push(cmd.client_order_id);

        Ok(()) // Stub implementation always succeeds
    }

    fn submit_order_list(&self, _cmd: SubmitOrderList) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn modify_order(&self, _cmd: ModifyOrder) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn cancel_order(&self, _cmd: CancelOrder) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn cancel_all_orders(&self, _cmd: CancelAllOrders) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn batch_cancel_orders(&self, _cmd: BatchCancelOrders) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn query_order(&self, _cmd: QueryOrder) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn on_instrument(&mut self, instrument: InstrumentAny) {
        self.received_instruments.borrow_mut().push(instrument);
    }
}
