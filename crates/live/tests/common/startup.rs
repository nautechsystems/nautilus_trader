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
    time::Duration,
};

use async_trait::async_trait;
use nautilus_common::{
    actor::{DataActor, DataActorCore},
    cache::CacheView,
    clients::ExecutionClient,
    clock::Clock,
    factories::{ClientConfig, ExecutionClientFactory},
    live::{
        dst::time,
        runner::{get_data_event_sender, get_exec_event_sender},
    },
    messages::{DataEvent, ExecutionEvent, execution::CancelOrder},
    msgbus::{
        self, InstrumentSubscription, MessagingSwitchboard, TypedHandler,
        switchboard::get_instruments_pattern,
    },
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
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    reports::ExecutionMassStatus,
    types::{AccountBalance, Currency, MarginBalance, Money},
};
use parking_lot::Mutex;
use rust_decimal::Decimal;

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
    pub(crate) bulk_position_coverage_unavailable: Arc<AtomicBool>,
    pub(crate) disconnect_attempted: Arc<AtomicBool>,
    pub(crate) factory_trader_id: Arc<Mutex<Option<TraderId>>>,
    pub(crate) factory_clock_id: Arc<Mutex<Option<usize>>>,
    pub(crate) mass_status_requested: Arc<AtomicBool>,
    pub(crate) mass_status_delay: Duration,
    pub(crate) connect_account: Arc<Mutex<Option<AccountState>>>,
    pub(crate) connect_instrument: Arc<Mutex<Option<InstrumentAny>>>,
    pub(crate) recovered_instrument: Arc<Mutex<Option<InstrumentAny>>>,
    pub(crate) instrument_updates: Arc<AtomicUsize>,
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
    oms_type: OmsType,
    handles_all_order_venues: bool,
    cache: CacheView,
    instrument_subscription: Option<InstrumentSubscription>,
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
    oms_type: OmsType,
    handles_all_order_venues: bool,
}

impl StartupMassStatusExecutionClient {
    pub(crate) const CLIENT_ID: &'static str = "STARTUP-MASS-STATUS";
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
            oms_type: OmsType::Hedging,
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

    #[allow(dead_code, reason = "only the spec target selects NETTING accounting")]
    pub(crate) fn with_oms_type(mut self, oms_type: OmsType) -> Self {
        self.oms_type = oms_type;
        self
    }
}

impl ExecutionClientFactory for StartupMassStatusExecutionClientFactory {
    fn create(
        &self,
        trader_id: TraderId,
        _name: &str,
        _config: &dyn ClientConfig,
        cache: CacheView,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        *self.state.factory_trader_id.lock() = Some(trader_id);
        *self.state.factory_clock_id.lock() = Some(Rc::as_ptr(&clock).cast::<()>() as usize);
        Ok(Box::new(StartupMassStatusExecutionClient {
            state: self.state.clone(),
            behavior: self.behavior,
            client_id: self.client_id,
            account_id: self.account_id,
            venue: self.venue,
            oms_type: self.oms_type,
            handles_all_order_venues: self.handles_all_order_venues,
            cache,
            instrument_subscription: None,
        }))
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
        self.oms_type
    }

    fn provides_bulk_position_coverage(&self, _instrument_id: InstrumentId) -> bool {
        !self
            .state
            .bulk_position_coverage_unavailable
            .load(Ordering::Relaxed)
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
        let state = self.state.clone();
        let handler = TypedHandler::from(move |_: &InstrumentAny| {
            state.instrument_updates.fetch_add(1, Ordering::Relaxed);
        });

        self.instrument_subscription = Some(InstrumentSubscription::new(
            get_instruments_pattern(self.venue),
            handler,
        ));
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.instrument_subscription.take();
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
        let instrument = self.state.connect_instrument.lock().take();
        if let Some(instrument) = instrument {
            let event = DataEvent::Data(instrument.into());
            get_data_event_sender().send(event)?;
        }

        let account = self.state.connect_account.lock().take();
        if let Some(account) = account {
            let account_id = account.account_id;
            get_exec_event_sender().send(ExecutionEvent::Account(account))?;

            while self.cache.borrow().account(&account_id).is_none() {
                tokio::task::yield_now().await;
            }
        }

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

        if !self.state.mass_status_delay.is_zero() {
            time::sleep(self.state.mass_status_delay).await;
        }

        let recovered = self.state.recovered_instrument.lock().take();
        if let Some(instrument) = recovered {
            let endpoint = MessagingSwitchboard::data_engine_process_data();
            msgbus::send_data(endpoint, instrument.into());
        }

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
    add_startup_margin_account(node, account_id, Currency::USDT());
}

pub(crate) fn add_startup_margin_account(
    node: &LiveNode,
    account_id: AccountId,
    currency: Currency,
) {
    let total = Money::from_decimal(Decimal::from(1_000_000), currency).unwrap();
    let zero = Money::zero(currency);

    let account = MarginAccount::new(
        AccountState::new(
            account_id,
            AccountType::Margin,
            vec![AccountBalance::new(total, zero, total)],
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(currency),
        ),
        true,
    );

    node.kernel()
        .cache()
        .borrow_mut()
        .add_account(AccountAny::Margin(account))
        .unwrap();
}
