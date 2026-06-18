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

//! Conformance test for the unified write-once authoring surface.
//!
//! One authored strategy body ([`DemoStrategy::on_quote`]) runs unchanged against two capability
//! backends: a native backend that clones owned values out of live state, and a plug-in backend
//! that reaches host state only through owned round trips over explicit capability slots. The
//! authored body is byte-identical across both; this test preserves that proof. The mock backends
//! stand in for possible native / cdylib / PyO3 backends without making those APIs public.
//!
//! The surface definitions in this file are spike-local on purpose: they preserve the proof without
//! committing `common` or `trading` to a public authoring API. This test only exercises the minimal
//! vertical slice: identity, clock, one cache point-read, one portfolio scalar, and one market
//! submit.

#![cfg(feature = "host")]

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use nautilus_model::{
    enums::OrderSide,
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
    orders::{Order, OrderAny},
    types::{Currency, Money, Quantity},
};
use rstest::rstest;

use crate::spike_authoring::{
    ActorCapabilities, ActorOps, CapabilityResult, StrategyCapabilities, StrategyOps,
};

mod spike_authoring {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{ContingencyType, OrderSide, TimeInForce},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
        orders::{MarketOrder, OrderAny},
        types::{Money, Quantity},
    };
    use serde::{Deserialize, Serialize};

    pub(super) trait ActorCapabilities {
        fn trader_id(&self) -> TraderId;

        fn now_ns(&self) -> UnixNanos;

        fn cache_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<Option<OrderAny>>;
    }

    pub(super) trait ActorOps: ActorCapabilities + Sized {
        fn clock(&self) -> ClockOps<'_, Self> {
            ClockOps { caps: self }
        }

        fn cache(&self) -> CacheOps<'_, Self> {
            CacheOps { caps: self }
        }
    }

    impl<C: ActorCapabilities> ActorOps for C {}

    #[derive(Debug)]
    pub(super) struct ClockOps<'a, C: ActorCapabilities> {
        caps: &'a C,
    }

    impl<C: ActorCapabilities> ClockOps<'_, C> {
        pub(super) fn timestamp_ns(&self) -> UnixNanos {
            self.caps.now_ns()
        }
    }

    #[derive(Debug)]
    pub(super) struct CacheOps<'a, C: ActorCapabilities> {
        caps: &'a C,
    }

    impl<C: ActorCapabilities> CacheOps<'_, C> {
        pub(super) fn order(
            &self,
            client_order_id: &ClientOrderId,
        ) -> anyhow::Result<Option<OrderAny>> {
            self.caps.cache_order(client_order_id)
        }
    }

    pub(super) trait StrategyCapabilities: ActorCapabilities {
        fn strategy_id(&self) -> StrategyId;

        fn generate_client_order_id(&mut self) -> ClientOrderId;

        fn submit_order(&mut self, order: OrderAny) -> anyhow::Result<()>;

        fn net_exposure(&self, instrument_id: InstrumentId) -> anyhow::Result<Option<Money>>;
    }

    pub(super) trait StrategyOps: StrategyCapabilities + ActorOps + Sized {
        fn order(&mut self) -> OrderOps<'_, Self> {
            OrderOps { caps: self }
        }

        fn portfolio(&mut self) -> PortfolioOps<'_, Self> {
            PortfolioOps { caps: self }
        }

        fn submit(&mut self, order: OrderAny) -> anyhow::Result<()> {
            self.submit_order(order)
        }
    }

    impl<C: StrategyCapabilities> StrategyOps for C {}

    #[derive(Debug)]
    pub(super) struct OrderOps<'a, C: StrategyCapabilities> {
        caps: &'a mut C,
    }

    impl<C: StrategyCapabilities> OrderOps<'_, C> {
        pub(super) fn market(
            self,
            instrument_id: InstrumentId,
            order_side: OrderSide,
            quantity: Quantity,
        ) -> anyhow::Result<OrderAny> {
            let client_order_id = self.caps.generate_client_order_id();
            let order = MarketOrder::new_checked(
                self.caps.trader_id(),
                self.caps.strategy_id(),
                instrument_id,
                client_order_id,
                order_side,
                quantity,
                TimeInForce::Gtc,
                UUID4::new(),
                self.caps.now_ns(),
                false,
                false,
                Some(ContingencyType::NoContingency),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )?;
            Ok(OrderAny::Market(order))
        }
    }

    #[derive(Debug)]
    pub(super) struct PortfolioOps<'a, C: StrategyCapabilities> {
        caps: &'a mut C,
    }

    impl<C: StrategyCapabilities> PortfolioOps<'_, C> {
        pub(super) fn net_exposure(
            &self,
            instrument_id: InstrumentId,
        ) -> anyhow::Result<Option<Money>> {
            self.caps.net_exposure(instrument_id)
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(super) struct CapabilityResult<T> {
        ok: bool,
        value: Option<T>,
        err_message: Option<String>,
    }

    impl<T> CapabilityResult<T> {
        pub(super) fn ok(value: T) -> Self {
            Self {
                ok: true,
                value: Some(value),
                err_message: None,
            }
        }

        pub(super) fn into_result(self) -> anyhow::Result<T> {
            if self.ok {
                self.value.ok_or_else(|| {
                    anyhow::anyhow!("capability response marked ok but carried no value")
                })
            } else {
                Err(anyhow::anyhow!(
                    self.err_message
                        .unwrap_or_else(|| "capability error".to_string())
                ))
            }
        }
    }
}

#[rstest]
fn spike_surface_exposes_no_borrow_guard_types() {
    let source = include_str!("authoring_conformance.rs");
    let spike_surface = source
        .split("mod spike_authoring {")
        .nth(1)
        .expect("spike surface module exists")
        .split("// The author's strategy")
        .next()
        .expect("spike surface module has an end marker");

    for banned in ["Ref<", "RefMut<", "Rc<", "dyn "] {
        assert!(
            !spike_surface.contains(banned),
            "spike authoring surface must not expose `{banned}`"
        );
    }
}

// The author's strategy: one macro-known backend field plus author state. A future macro could
// generate the capability delegation; here it is written once, generic over the backend `B`, to
// stand in for that codegen. The strategy carries its backend (Shape A):
// `self.cache()` / `self.order()` / `self.portfolio()` resolve through the blanket-implemented
// `ActorOps` / `StrategyOps` extension traits, not an engine-owned adapter.
struct DemoStrategy<B: StrategyCapabilities> {
    engine: B,
    submitted: Vec<ClientOrderId>,
}

impl<B: StrategyCapabilities> ActorCapabilities for DemoStrategy<B> {
    fn trader_id(&self) -> TraderId {
        self.engine.trader_id()
    }

    fn now_ns(&self) -> nautilus_core::UnixNanos {
        self.engine.now_ns()
    }

    fn cache_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<Option<OrderAny>> {
        self.engine.cache_order(client_order_id)
    }
}

impl<B: StrategyCapabilities> StrategyCapabilities for DemoStrategy<B> {
    fn strategy_id(&self) -> StrategyId {
        self.engine.strategy_id()
    }

    fn generate_client_order_id(&mut self) -> ClientOrderId {
        self.engine.generate_client_order_id()
    }

    fn submit_order(&mut self, order: OrderAny) -> anyhow::Result<()> {
        self.engine.submit_order(order)
    }

    fn net_exposure(&self, instrument_id: InstrumentId) -> anyhow::Result<Option<Money>> {
        self.engine.net_exposure(instrument_id)
    }
}

impl<B: StrategyCapabilities> DemoStrategy<B> {
    // Authored once. The body below is byte-identical regardless of backend `B`; that identity is
    // the whole claim the conformance test exists to keep true.
    fn on_quote(&mut self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let order = self
            .order()
            .market(instrument_id, OrderSide::Buy, Quantity::from("1.0"))?;
        let client_order_id = order.client_order_id();
        self.submit(order)?;

        // Load-bearing borrow sequence: portfolio() takes &mut self, cache() takes &self, and both
        // run back to back in one callback with no borrow-checker fight.
        let exposure = self.portfolio().net_exposure(instrument_id)?;
        anyhow::ensure!(
            exposure == Some(Money::zero(Currency::USD())),
            "flat book nets to zero exposure"
        );

        let fetched = self.cache().order(&client_order_id)?;
        anyhow::ensure!(fetched.is_some(), "submitted order should be readable back");

        // Clock read through the surface; the value is not asserted, only that it resolves owned.
        let _now = self.clock().timestamp_ns();

        self.submitted.push(client_order_id);
        Ok(())
    }
}

// Native backend: holds live state and serves reads by cloning out of it.
struct NativeBackend {
    trader_id: TraderId,
    strategy_id: StrategyId,
    clock_ns: u64,
    next_id: usize,
    cache: HashMap<ClientOrderId, OrderAny>,
}

impl NativeBackend {
    fn new() -> Self {
        Self {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            clock_ns: 1_700_000_000_000_000_000,
            next_id: 0,
            cache: HashMap::new(),
        }
    }
}

impl ActorCapabilities for NativeBackend {
    fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    fn now_ns(&self) -> nautilus_core::UnixNanos {
        nautilus_core::UnixNanos::from(self.clock_ns)
    }

    fn cache_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<Option<OrderAny>> {
        Ok(self.cache.get(client_order_id).cloned())
    }
}

impl StrategyCapabilities for NativeBackend {
    fn strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    fn generate_client_order_id(&mut self) -> ClientOrderId {
        self.next_id += 1;
        ClientOrderId::from(format!("O-{:03}", self.next_id).as_str())
    }

    fn submit_order(&mut self, order: OrderAny) -> anyhow::Result<()> {
        self.cache.insert(order.client_order_id(), order);
        Ok(())
    }

    fn net_exposure(&self, _instrument_id: InstrumentId) -> anyhow::Result<Option<Money>> {
        Ok(Some(Money::zero(Currency::USD())))
    }
}

// Stands in for host process state a plug-in cannot borrow into. The plug-in reaches it only
// through owned round trips, modelled here as explicit host capability slots behind interior
// mutability.
struct Host {
    trader_id: TraderId,
    strategy_id: StrategyId,
    clock_ns: u64,
    next_id: Cell<usize>,
    cache: RefCell<HashMap<ClientOrderId, String>>,
}

impl Host {
    fn new() -> Self {
        Self {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            clock_ns: 1_700_000_000_000_000_000,
            next_id: Cell::new(0),
            cache: RefCell::new(HashMap::new()),
        }
    }

    // Models an explicit `HostVTable::cache_order` slot.
    fn cache_order(&self, client_order_id: &ClientOrderId) -> String {
        let order = self
            .cache
            .borrow()
            .get(client_order_id)
            .map(|encoded| serde_json::from_str::<OrderAny>(encoded).expect("valid order"));
        serde_json::to_string(&CapabilityResult::ok(order)).expect("encodable response")
    }

    // Models an explicit `HostVTable::portfolio_net_exposure` slot.
    fn portfolio_net_exposure(&self, _instrument_id: InstrumentId) -> String {
        let exposure = Some(Money::zero(Currency::USD()));
        serde_json::to_string(&CapabilityResult::ok(exposure)).expect("encodable response")
    }
}

// Plug-in backend: no live borrow of host state is available, so every read decodes an owned value
// from a host round trip. The capability signatures are identical to `NativeBackend`.
struct PluginBackend<'h> {
    host: &'h Host,
}

impl ActorCapabilities for PluginBackend<'_> {
    fn trader_id(&self) -> TraderId {
        self.host.trader_id
    }

    fn now_ns(&self) -> nautilus_core::UnixNanos {
        nautilus_core::UnixNanos::from(self.host.clock_ns)
    }

    fn cache_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<Option<OrderAny>> {
        let response: CapabilityResult<Option<OrderAny>> =
            serde_json::from_str(&self.host.cache_order(client_order_id))?;
        response.into_result()
    }
}

impl StrategyCapabilities for PluginBackend<'_> {
    fn strategy_id(&self) -> StrategyId {
        self.host.strategy_id
    }

    fn generate_client_order_id(&mut self) -> ClientOrderId {
        let next = self.host.next_id.get() + 1;
        self.host.next_id.set(next);
        ClientOrderId::from(format!("O-{next:03}").as_str())
    }

    fn submit_order(&mut self, order: OrderAny) -> anyhow::Result<()> {
        let encoded = serde_json::to_string(&order)?;
        self.host
            .cache
            .borrow_mut()
            .insert(order.client_order_id(), encoded);
        Ok(())
    }

    fn net_exposure(&self, instrument_id: InstrumentId) -> anyhow::Result<Option<Money>> {
        let response: CapabilityResult<Option<Money>> =
            serde_json::from_str(&self.host.portfolio_net_exposure(instrument_id))?;
        response.into_result()
    }
}

#[rstest]
fn native_backend_runs_the_authored_body() {
    let mut strategy = DemoStrategy {
        engine: NativeBackend::new(),
        submitted: Vec::new(),
    };
    let instrument_id = InstrumentId::from("EUR/USD.SIM");

    strategy.on_quote(instrument_id).unwrap();
    strategy.on_quote(instrument_id).unwrap();

    assert_eq!(strategy.trader_id(), TraderId::from("TRADER-001"));
    assert_eq!(strategy.strategy_id(), StrategyId::from("S-001"));
    assert_eq!(strategy.submitted.len(), 2);
    assert_eq!(strategy.engine.cache.len(), 2);
}

#[rstest]
fn plugin_backend_runs_the_same_authored_body() {
    let host = Host::new();
    let mut strategy = DemoStrategy {
        engine: PluginBackend { host: &host },
        submitted: Vec::new(),
    };
    let instrument_id = InstrumentId::from("EUR/USD.SIM");

    strategy.on_quote(instrument_id).unwrap();
    strategy.on_quote(instrument_id).unwrap();

    assert_eq!(strategy.trader_id(), TraderId::from("TRADER-001"));
    assert_eq!(strategy.strategy_id(), StrategyId::from("S-001"));
    assert_eq!(strategy.submitted.len(), 2);
    // Orders crossed the boundary as JSON and decoded back to owned values in host state.
    assert_eq!(host.cache.borrow().len(), 2);
}
