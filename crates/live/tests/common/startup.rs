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
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use nautilus_common::{
    actor::{DataActor, DataActorCore},
    cache::CacheView,
    clients::ExecutionClient,
    clock::Clock,
    factories::{ClientConfig, ExecutionClientFactory},
    messages::execution::CancelOrder,
    nautilus_actor,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType},
    events::account::state::AccountState,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue, VenueOrderId,
    },
    instruments::{Instrument, stubs::crypto_perpetual_ethusdt},
    reports::ExecutionMassStatus,
    types::{AccountBalance, Currency, MarginBalance, Money},
};
use parking_lot::Mutex;

#[derive(Debug)]
pub(crate) struct StartupActor {
    pub(crate) core: DataActorCore,
    pub(crate) starts: Rc<Cell<usize>>,
}

impl DataActor for StartupActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.starts.set(self.starts.get() + 1);
        Ok(())
    }
}

nautilus_actor!(StartupActor);

#[derive(Clone, Debug, Default)]
pub(crate) struct StartupMassStatusClientState {
    pub(crate) connected: Arc<AtomicBool>,
    pub(crate) disconnect_attempted: Arc<AtomicBool>,
    pub(crate) factory_trader_id: Arc<Mutex<Option<TraderId>>>,
    pub(crate) factory_clock_id: Arc<Mutex<Option<usize>>>,
    pub(crate) mass_status_requested: Arc<AtomicBool>,
    pub(crate) mass_status: Arc<Mutex<Option<ExecutionMassStatus>>>,
    pub(crate) registered_external_orders: Arc<Mutex<Vec<ClientOrderId>>>,
    pub(crate) cancel_orders_received: Arc<AtomicUsize>,
    pub(crate) cancel_orders_while_connected: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum StartupMassStatusBehavior {
    Available,
    Unavailable,
    #[allow(dead_code, reason = "only the spec target constructs request errors")]
    Error,
    Pending,
}

pub(crate) struct StartupMassStatusExecutionClient {
    state: StartupMassStatusClientState,
    behavior: StartupMassStatusBehavior,
    client_id: ClientId,
    account_id: AccountId,
    venue: Venue,
    handles_all_order_venues: bool,
}

#[derive(Debug)]
pub(crate) struct StartupMassStatusExecutionClientConfig;

#[derive(Debug)]
pub(crate) struct StartupMassStatusExecutionClientFactory {
    state: StartupMassStatusClientState,
    behavior: StartupMassStatusBehavior,
    client_id: ClientId,
    account_id: AccountId,
    venue: Venue,
    handles_all_order_venues: bool,
}

impl StartupMassStatusExecutionClient {
    pub(crate) const CLIENT_ID: &'static str = "STARTUP-MASS-STATUS";

    fn new(
        state: StartupMassStatusClientState,
        behavior: StartupMassStatusBehavior,
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
        handles_all_order_venues: bool,
    ) -> Self {
        Self {
            state,
            behavior,
            client_id,
            account_id,
            venue,
            handles_all_order_venues,
        }
    }
}

impl ClientConfig for StartupMassStatusExecutionClientConfig {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl StartupMassStatusExecutionClientFactory {
    pub(crate) fn new(
        state: StartupMassStatusClientState,
        behavior: StartupMassStatusBehavior,
    ) -> Self {
        Self {
            state,
            behavior,
            client_id: ClientId::from(StartupMassStatusExecutionClient::CLIENT_ID),
            account_id: AccountId::from("STARTUP-MASS-STATUS-001"),
            venue: crypto_perpetual_ethusdt().id().venue,
            handles_all_order_venues: false,
        }
    }

    pub(crate) fn with_identity(
        mut self,
        client_id: ClientId,
        account_id: AccountId,
        venue: Venue,
    ) -> Self {
        self.client_id = client_id;
        self.account_id = account_id;
        self.venue = venue;
        self
    }

    pub(crate) fn with_handles_all_order_venues(mut self) -> Self {
        self.handles_all_order_venues = true;
        self
    }
}

impl ExecutionClientFactory for StartupMassStatusExecutionClientFactory {
    fn create(
        &self,
        trader_id: TraderId,
        _name: &str,
        _config: &dyn ClientConfig,
        _cache: CacheView,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        *self.state.factory_trader_id.lock() = Some(trader_id);
        *self.state.factory_clock_id.lock() = Some(Rc::as_ptr(&clock).cast::<()>() as usize);
        Ok(Box::new(StartupMassStatusExecutionClient::new(
            self.state.clone(),
            self.behavior,
            self.client_id,
            self.account_id,
            self.venue,
            self.handles_all_order_venues,
        )))
    }

    fn name(&self) -> &'static str {
        "startup-mass-status"
    }

    fn config_type(&self) -> &'static str {
        stringify!(StartupMassStatusExecutionClientConfig)
    }
}

#[async_trait(?Send)]
impl ExecutionClient for StartupMassStatusExecutionClient {
    fn is_connected(&self) -> bool {
        self.state.connected.load(Ordering::Relaxed)
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
        OmsType::Hedging
    }

    fn get_account(&self) -> Option<AccountAny> {
        None
    }

    fn generate_account_state(
        &self,
        _balances: Vec<AccountBalance>,
        _margins: Vec<MarginBalance>,
        _reported: bool,
        _ts_event: UnixNanos,
        _info: Option<Params>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn cancel_order(&self, _cmd: CancelOrder) -> anyhow::Result<()> {
        self.state
            .cancel_orders_received
            .fetch_add(1, Ordering::Relaxed);
        self.state.cancel_orders_while_connected.store(
            self.state.connected.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        self.state.connected.store(true, Ordering::Relaxed);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.state
            .disconnect_attempted
            .store(true, Ordering::Relaxed);
        self.state.connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    async fn generate_mass_status(
        &self,
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        self.state
            .mass_status_requested
            .store(true, Ordering::Relaxed);

        match self.behavior {
            StartupMassStatusBehavior::Available => Ok(self.state.mass_status.lock().clone()),
            StartupMassStatusBehavior::Unavailable => Ok(None),
            StartupMassStatusBehavior::Error => Err(anyhow::anyhow!("mass status failed")),
            StartupMassStatusBehavior::Pending => {
                std::future::pending::<anyhow::Result<Option<ExecutionMassStatus>>>().await
            }
        }
    }

    fn register_external_order(
        &self,
        client_order_id: ClientOrderId,
        _venue_order_id: VenueOrderId,
        _instrument_id: InstrumentId,
        _strategy_id: StrategyId,
        _ts_init: UnixNanos,
    ) {
        self.state
            .registered_external_orders
            .lock()
            .push(client_order_id);
    }
}

pub(crate) fn add_reconciliation_test_account(node: &LiveNode) {
    let account_id = AccountId::from("BLOCKING-REPORT-001");

    let account = MarginAccount::new(
        AccountState::new(
            account_id,
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::from("1000000 USDT"),
                Money::from("0 USDT"),
                Money::from("1000000 USDT"),
            )],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(Currency::USDT()),
        ),
        true,
    );
    node.kernel()
        .cache()
        .borrow_mut()
        .add_account(AccountAny::Margin(account))
        .unwrap();
}
