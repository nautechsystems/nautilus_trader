use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::Clock,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, QueryOrder,
        SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    identifiers::{AccountId, ClientId, Venue},
    types::{AccountBalance, MarginBalance},
};

use crate::client::ExecutionClient;

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
            clock: clock
                .unwrap_or_else(|| Rc::new(RefCell::new(nautilus_common::clock::TestClock::new()))),
            cache: Rc::new(RefCell::new(Cache::new(None, None))),
        }
    }
}

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
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.is_connected = false;
        Ok(())
    }

    fn submit_order(&self, _cmd: &SubmitOrder) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn submit_order_list(&self, _cmd: &SubmitOrderList) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn modify_order(&self, _cmd: &ModifyOrder) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn cancel_order(&self, _cmd: &CancelOrder) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn cancel_all_orders(&self, _cmd: &CancelAllOrders) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn batch_cancel_orders(&self, _cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }

    fn query_order(&self, _cmd: &QueryOrder) -> anyhow::Result<()> {
        Ok(()) // Stub implementation always succeeds
    }
}
