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

use std::{cell::RefCell, fmt::Debug, rc::Rc, time::Duration};

use nautilus_common::{
    cache::{CacheConfig, database::CacheDatabaseAdapter},
    clock::Clock,
    enums::Environment,
    logging::logger::LoggerConfig,
};
use nautilus_core::UUID4;
use nautilus_data::engine::config::DataEngineConfig;
use nautilus_execution::engine::config::ExecutionEngineConfig;
use nautilus_model::identifiers::TraderId;
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_risk::engine::config::RiskEngineConfig;

use crate::{
    config::KernelConfig,
    event_store::{EventStoreFactory, KernelEventStore},
    kernel::NautilusKernel,
};

/// Builder for constructing a [`NautilusKernel`] with a fluent API.
///
/// Provides a convenient way to configure and build a kernel instance with
/// optional components and settings.
pub struct NautilusKernelBuilder {
    name: String,
    trader_id: TraderId,
    environment: Environment,
    instance_id: Option<UUID4>,
    load_state: bool,
    save_state: bool,
    logging: Option<LoggerConfig>,
    timeout_connection: Duration,
    timeout_reconciliation: Duration,
    timeout_portfolio: Duration,
    timeout_disconnection: Duration,
    delay_post_stop: Duration,
    timeout_shutdown: Duration,
    cache: Option<CacheConfig>,
    cache_database: Option<Box<dyn CacheDatabaseAdapter>>,
    data_engine: Option<DataEngineConfig>,
    risk_engine: Option<RiskEngineConfig>,
    exec_engine: Option<ExecutionEngineConfig>,
    portfolio: Option<PortfolioConfig>,
    event_store_factory: Option<EventStoreFactory>,
}

impl Debug for NautilusKernelBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(NautilusKernelBuilder))
            .field("name", &self.name)
            .field("trader_id", &self.trader_id)
            .field("environment", &self.environment)
            .field("instance_id", &self.instance_id)
            .field("load_state", &self.load_state)
            .field("save_state", &self.save_state)
            .field("logging", &self.logging)
            .field("timeout_connection", &self.timeout_connection)
            .field("timeout_reconciliation", &self.timeout_reconciliation)
            .field("timeout_portfolio", &self.timeout_portfolio)
            .field("timeout_disconnection", &self.timeout_disconnection)
            .field("delay_post_stop", &self.delay_post_stop)
            .field("timeout_shutdown", &self.timeout_shutdown)
            .field("cache", &self.cache)
            .field("cache_database", &self.cache_database.is_some())
            .field("data_engine", &self.data_engine)
            .field("risk_engine", &self.risk_engine)
            .field("exec_engine", &self.exec_engine)
            .field("portfolio", &self.portfolio)
            .field("event_store_factory", &self.event_store_factory.is_some())
            .finish_non_exhaustive()
    }
}

impl NautilusKernelBuilder {
    /// Creates a new [`NautilusKernelBuilder`] with required parameters.
    #[must_use]
    pub const fn new(name: String, trader_id: TraderId, environment: Environment) -> Self {
        Self {
            name,
            trader_id,
            environment,
            instance_id: None,
            load_state: true,
            save_state: true,
            logging: None,
            timeout_connection: Duration::from_secs(60),
            timeout_reconciliation: Duration::from_secs(30),
            timeout_portfolio: Duration::from_secs(10),
            timeout_disconnection: Duration::from_secs(10),
            delay_post_stop: Duration::from_secs(10),
            timeout_shutdown: Duration::from_secs(5),
            cache: None,
            cache_database: None,
            data_engine: None,
            risk_engine: None,
            exec_engine: None,
            portfolio: None,
            event_store_factory: None,
        }
    }

    /// Set the instance ID for the kernel.
    #[must_use]
    pub const fn with_instance_id(mut self, instance_id: UUID4) -> Self {
        self.instance_id = Some(instance_id);
        self
    }

    /// Configure whether to load state on startup.
    #[must_use]
    pub const fn with_load_state(mut self, load_state: bool) -> Self {
        self.load_state = load_state;
        self
    }

    /// Configure whether to save state on shutdown.
    #[must_use]
    pub const fn with_save_state(mut self, save_state: bool) -> Self {
        self.save_state = save_state;
        self
    }

    /// Set the logging configuration.
    #[must_use]
    pub fn with_logging_config(mut self, config: LoggerConfig) -> Self {
        self.logging = Some(config);
        self
    }

    /// Set the connection timeout in seconds.
    #[must_use]
    pub const fn with_timeout_connection(mut self, timeout_secs: u64) -> Self {
        self.timeout_connection = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the reconciliation timeout in seconds.
    #[must_use]
    pub const fn with_timeout_reconciliation(mut self, timeout_secs: u64) -> Self {
        self.timeout_reconciliation = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the portfolio initialization timeout in seconds.
    #[must_use]
    pub const fn with_timeout_portfolio(mut self, timeout_secs: u64) -> Self {
        self.timeout_portfolio = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the disconnection timeout in seconds.
    #[must_use]
    pub const fn with_timeout_disconnection(mut self, timeout_secs: u64) -> Self {
        self.timeout_disconnection = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the post-stop delay in seconds.
    #[must_use]
    pub const fn with_delay_post_stop(mut self, delay_secs: u64) -> Self {
        self.delay_post_stop = Duration::from_secs(delay_secs);
        self
    }

    /// Set the shutdown timeout in seconds.
    #[must_use]
    pub const fn with_timeout_shutdown(mut self, timeout_secs: u64) -> Self {
        self.timeout_shutdown = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the cache configuration.
    #[must_use]
    pub fn with_cache_config(mut self, config: CacheConfig) -> Self {
        self.cache = Some(config);
        self
    }

    /// Inject a durable cache database adapter.
    ///
    /// The adapter is passed straight to [`nautilus_common::cache::Cache::new`] so
    /// generic cache state (including event-store snapshot blobs) is restored on
    /// startup without an external caller pre-seeding the in-memory cache. Adapter
    /// construction lives outside this crate to keep `nautilus-system` decoupled
    /// from concrete backing stores such as Redis or Postgres.
    #[must_use]
    pub fn with_cache_database(mut self, adapter: Box<dyn CacheDatabaseAdapter>) -> Self {
        self.cache_database = Some(adapter);
        self
    }

    /// Set the data engine configuration.
    #[must_use]
    pub fn with_data_engine_config(mut self, config: DataEngineConfig) -> Self {
        self.data_engine = Some(config);
        self
    }

    /// Set the risk engine configuration.
    #[must_use]
    pub fn with_risk_engine_config(mut self, config: RiskEngineConfig) -> Self {
        self.risk_engine = Some(config);
        self
    }

    /// Set the execution engine configuration.
    #[must_use]
    pub fn with_exec_engine_config(mut self, config: ExecutionEngineConfig) -> Self {
        self.exec_engine = Some(config);
        self
    }

    /// Set the portfolio configuration.
    #[must_use]
    pub const fn with_portfolio_config(mut self, config: PortfolioConfig) -> Self {
        self.portfolio = Some(config);
        self
    }

    /// Inject an event-store implementation to drive run-lifecycle capture.
    ///
    /// The factory is invoked with the kernel's instance id and clock during
    /// construction, so the returned [`KernelEventStore`] scans the same run directory
    /// and shares the same time source the kernel uses for `RunStarted`/`RunEnded` and
    /// any drop-seal fallback. The concrete implementation lives outside this crate
    /// (typically in `nautilus-event-store`); callers build it inside the closure.
    #[must_use]
    pub fn with_event_store<F>(mut self, factory: F) -> Self
    where
        F: FnOnce(UUID4, Rc<RefCell<dyn Clock>>) -> anyhow::Result<Box<dyn KernelEventStore>>
            + 'static,
    {
        self.event_store_factory = Some(Box::new(factory));
        self
    }

    /// Build the [`NautilusKernel`] with the configured settings.
    ///
    /// # Errors
    ///
    /// Returns an error if kernel initialization fails.
    pub fn build(self) -> anyhow::Result<NautilusKernel> {
        let config = KernelConfig {
            environment: self.environment,
            trader_id: self.trader_id,
            load_state: self.load_state,
            save_state: self.save_state,
            logging: self.logging.unwrap_or_default(),
            instance_id: self.instance_id,
            timeout_connection: self.timeout_connection,
            timeout_reconciliation: self.timeout_reconciliation,
            timeout_portfolio: self.timeout_portfolio,
            timeout_disconnection: self.timeout_disconnection,
            delay_post_stop: self.delay_post_stop,
            timeout_shutdown: self.timeout_shutdown,
            cache: self.cache,
            msgbus: None, // msgbus config - not exposed in builder yet
            data_engine: self.data_engine,
            risk_engine: self.risk_engine,
            exec_engine: self.exec_engine,
            portfolio: self.portfolio,
            streaming: None,
        };

        NautilusKernel::new_with(
            self.name,
            config,
            self.cache_database,
            self.event_store_factory,
        )
    }
}

impl Default for NautilusKernelBuilder {
    /// Create a default builder with minimal configuration for testing/development.
    fn default() -> Self {
        Self::new(
            "NautilusKernel".to_string(),
            TraderId::default(),
            Environment::Backtest,
        )
    }
}

#[cfg(test)]
mod tests {
    use ahash::AHashMap;
    use bytes::Bytes;
    use nautilus_common::{
        cache::{
            Cache,
            database::{CacheDatabaseAdapter, CacheMap},
        },
        clock::Clock,
        signal::Signal,
    };
    use nautilus_core::UnixNanos;
    use nautilus_execution::engine::SnapshotAnchorer;
    use nautilus_model::{
        accounts::AccountAny,
        data::{
            Bar, CustomData, DataType, FundingRateUpdate, QuoteTick, TradeTick,
            greeks::{GreeksData, YieldCurveData},
        },
        events::{OrderEventAny, OrderSnapshot, position::snapshot::PositionSnapshot},
        identifiers::{
            AccountId, ClientId, ClientOrderId, ComponentId, InstrumentId, PositionId, StrategyId,
            TraderId, VenueOrderId,
        },
        instruments::{InstrumentAny, SyntheticInstrument},
        orderbook::OrderBook,
        orders::OrderAny,
        position::Position,
        types::Currency,
    };
    use rstest::*;
    use ustr::Ustr;

    use super::*;
    use crate::event_store::RegisteredComponents;

    #[rstest]
    fn test_builder_default() {
        let builder = NautilusKernelBuilder::default();
        assert_eq!(builder.name, "NautilusKernel");
        assert_eq!(builder.environment, Environment::Backtest);
        assert!(builder.load_state);
        assert!(builder.save_state);
    }

    #[rstest]
    fn test_builder_fluent_api() {
        let trader_id = TraderId::from("TRADER-001");
        let instance_id = UUID4::new();

        let builder =
            NautilusKernelBuilder::new("TestKernel".to_string(), trader_id, Environment::Live)
                .with_instance_id(instance_id)
                .with_load_state(false)
                .with_save_state(false)
                .with_timeout_connection(30);

        assert_eq!(builder.name, "TestKernel");
        assert_eq!(builder.trader_id, trader_id);
        assert_eq!(builder.environment, Environment::Live);
        assert_eq!(builder.instance_id, Some(instance_id));
        assert!(!builder.load_state);
        assert!(!builder.save_state);
        assert_eq!(builder.timeout_connection, Duration::from_secs(30));
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_builder_build() {
        let result = NautilusKernelBuilder::default().build();
        assert!(result.is_ok());

        let kernel = result.unwrap();
        assert_eq!(kernel.name(), "NautilusKernel".to_string());
        assert_eq!(kernel.environment(), Environment::Backtest);
    }

    #[rstest]
    fn test_builder_with_configs() {
        let cache_config = CacheConfig::default();
        let data_engine_config = DataEngineConfig::default();

        let builder = NautilusKernelBuilder::default()
            .with_cache_config(cache_config)
            .with_data_engine_config(data_engine_config);

        assert!(builder.cache.is_some());
        assert!(builder.data_engine.is_some());
    }

    #[rstest]
    fn test_builder_with_cache_database() {
        let builder = NautilusKernelBuilder::default().with_cache_database(Box::new(NoopAdapter));

        assert!(builder.cache_database.is_some());
    }

    #[rstest]
    fn test_builder_default_has_no_event_store() {
        let kernel = NautilusKernelBuilder::default()
            .build()
            .expect("kernel builds without an event store");

        assert!(kernel.event_store().is_none());
    }

    #[rstest]
    fn test_builder_with_event_store_invokes_factory_with_kernel_args() {
        type FactoryArgs = (UUID4, Rc<RefCell<dyn Clock>>);

        let known_id = UUID4::new();
        let captured: Rc<RefCell<Option<FactoryArgs>>> = Rc::new(RefCell::new(None));
        let captured_for_closure = captured.clone();

        let kernel = NautilusKernelBuilder::default()
            .with_instance_id(known_id)
            .with_event_store(move |instance_id, clock| {
                *captured_for_closure.borrow_mut() = Some((instance_id, clock));
                Ok(Box::new(NoopKernelEventStore))
            })
            .build()
            .expect("kernel");

        let (received_id, received_clock) =
            captured.borrow_mut().take().expect("factory invoked once");

        assert_eq!(
            received_id, known_id,
            "factory must receive kernel instance_id"
        );
        assert!(
            Rc::ptr_eq(&received_clock, &kernel.clock()),
            "factory must receive the kernel's clock Rc, not a fresh allocation",
        );
    }

    #[rstest]
    fn test_builder_with_event_store_propagates_factory_error() {
        let result = NautilusKernelBuilder::default()
            .with_event_store(|_instance_id, _clock| Err(anyhow::anyhow!("factory boom")))
            .build();

        let err = result.expect_err("factory error must surface from build()");

        assert!(
            err.to_string().contains("factory boom"),
            "error must propagate the factory's message; got: {err}",
        );
    }

    #[rstest]
    fn test_builder_with_all_engine_configs() {
        let builder = NautilusKernelBuilder::default()
            .with_data_engine_config(DataEngineConfig::default())
            .with_risk_engine_config(RiskEngineConfig::default())
            .with_exec_engine_config(ExecutionEngineConfig::default())
            .with_portfolio_config(PortfolioConfig::default());

        assert!(builder.data_engine.is_some());
        assert!(builder.risk_engine.is_some());
        assert!(builder.exec_engine.is_some());
        assert!(builder.portfolio.is_some());
    }

    #[rstest]
    fn test_builder_with_all_timeouts() {
        let builder = NautilusKernelBuilder::default()
            .with_timeout_connection(10)
            .with_timeout_reconciliation(20)
            .with_timeout_portfolio(30)
            .with_timeout_disconnection(40)
            .with_delay_post_stop(50)
            .with_timeout_shutdown(60);

        assert_eq!(builder.timeout_connection, Duration::from_secs(10));
        assert_eq!(builder.timeout_reconciliation, Duration::from_secs(20));
        assert_eq!(builder.timeout_portfolio, Duration::from_secs(30));
        assert_eq!(builder.timeout_disconnection, Duration::from_secs(40));
        assert_eq!(builder.delay_post_stop, Duration::from_secs(50));
        assert_eq!(builder.timeout_shutdown, Duration::from_secs(60));
    }

    #[rstest]
    fn test_builder_default_timeouts() {
        let builder = NautilusKernelBuilder::default();

        assert_eq!(builder.timeout_connection, Duration::from_secs(60));
        assert_eq!(builder.timeout_reconciliation, Duration::from_secs(30));
        assert_eq!(builder.timeout_portfolio, Duration::from_secs(10));
        assert_eq!(builder.timeout_disconnection, Duration::from_secs(10));
        assert_eq!(builder.delay_post_stop, Duration::from_secs(10));
        assert_eq!(builder.timeout_shutdown, Duration::from_secs(5));
    }

    struct NoopAdapter;

    #[async_trait::async_trait]
    impl CacheDatabaseAdapter for NoopAdapter {
        fn close(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn flush(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn load_all(&self) -> anyhow::Result<CacheMap> {
            Ok(CacheMap::default())
        }

        fn load(&self) -> anyhow::Result<AHashMap<String, Bytes>> {
            Ok(AHashMap::new())
        }

        async fn load_currencies(&self) -> anyhow::Result<AHashMap<Ustr, Currency>> {
            Ok(AHashMap::new())
        }

        async fn load_instruments(&self) -> anyhow::Result<AHashMap<InstrumentId, InstrumentAny>> {
            Ok(AHashMap::new())
        }

        async fn load_synthetics(
            &self,
        ) -> anyhow::Result<AHashMap<InstrumentId, SyntheticInstrument>> {
            Ok(AHashMap::new())
        }

        async fn load_accounts(&self) -> anyhow::Result<AHashMap<AccountId, AccountAny>> {
            Ok(AHashMap::new())
        }

        async fn load_orders(&self) -> anyhow::Result<AHashMap<ClientOrderId, OrderAny>> {
            Ok(AHashMap::new())
        }

        async fn load_positions(&self) -> anyhow::Result<AHashMap<PositionId, Position>> {
            Ok(AHashMap::new())
        }

        fn load_index_order_position(&self) -> anyhow::Result<AHashMap<ClientOrderId, Position>> {
            Ok(AHashMap::new())
        }

        fn load_index_order_client(&self) -> anyhow::Result<AHashMap<ClientOrderId, ClientId>> {
            Ok(AHashMap::new())
        }

        async fn load_currency(&self, _code: &Ustr) -> anyhow::Result<Option<Currency>> {
            Ok(None)
        }

        async fn load_instrument(
            &self,
            _instrument_id: &InstrumentId,
        ) -> anyhow::Result<Option<InstrumentAny>> {
            Ok(None)
        }

        async fn load_synthetic(
            &self,
            _instrument_id: &InstrumentId,
        ) -> anyhow::Result<Option<SyntheticInstrument>> {
            Ok(None)
        }

        async fn load_account(
            &self,
            _account_id: &AccountId,
        ) -> anyhow::Result<Option<AccountAny>> {
            Ok(None)
        }

        async fn load_order(
            &self,
            _client_order_id: &ClientOrderId,
        ) -> anyhow::Result<Option<OrderAny>> {
            Ok(None)
        }

        async fn load_position(
            &self,
            _position_id: &PositionId,
        ) -> anyhow::Result<Option<Position>> {
            Ok(None)
        }

        fn load_actor(
            &self,
            _component_id: &ComponentId,
        ) -> anyhow::Result<AHashMap<String, Bytes>> {
            Ok(AHashMap::new())
        }

        fn load_strategy(
            &self,
            _strategy_id: &StrategyId,
        ) -> anyhow::Result<AHashMap<String, Bytes>> {
            Ok(AHashMap::new())
        }

        fn load_signals(&self, _name: &str) -> anyhow::Result<Vec<Signal>> {
            Ok(Vec::new())
        }

        fn load_custom_data(&self, _data_type: &DataType) -> anyhow::Result<Vec<CustomData>> {
            Ok(Vec::new())
        }

        fn load_order_snapshot(
            &self,
            _client_order_id: &ClientOrderId,
        ) -> anyhow::Result<Option<OrderSnapshot>> {
            Ok(None)
        }

        fn load_position_snapshot(
            &self,
            _position_id: &PositionId,
        ) -> anyhow::Result<Option<PositionSnapshot>> {
            Ok(None)
        }

        fn load_quotes(&self, _instrument_id: &InstrumentId) -> anyhow::Result<Vec<QuoteTick>> {
            Ok(Vec::new())
        }

        fn load_trades(&self, _instrument_id: &InstrumentId) -> anyhow::Result<Vec<TradeTick>> {
            Ok(Vec::new())
        }

        fn load_funding_rates(
            &self,
            _instrument_id: &InstrumentId,
        ) -> anyhow::Result<Vec<FundingRateUpdate>> {
            Ok(Vec::new())
        }

        fn load_bars(&self, _instrument_id: &InstrumentId) -> anyhow::Result<Vec<Bar>> {
            Ok(Vec::new())
        }

        fn add(&self, _key: String, _value: Bytes) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_currency(&self, _currency: &Currency) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_instrument(&self, _instrument: &InstrumentAny) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_synthetic(&self, _synthetic: &SyntheticInstrument) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_account(&self, _account: &AccountAny) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_order(&self, _order: &OrderAny, _client_id: Option<ClientId>) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_order_snapshot(&self, _snapshot: &OrderSnapshot) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_position(&self, _position: &Position) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_position_snapshot(&self, _snapshot: &PositionSnapshot) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_order_book(&self, _order_book: &OrderBook) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_signal(&self, _signal: &Signal) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_custom_data(&self, _data: &CustomData) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_quote(&self, _quote: &QuoteTick) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_trade(&self, _trade: &TradeTick) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_funding_rate(&self, _funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_bar(&self, _bar: &Bar) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_greeks(&self, _greeks: &GreeksData) -> anyhow::Result<()> {
            Ok(())
        }

        fn add_yield_curve(&self, _yield_curve: &YieldCurveData) -> anyhow::Result<()> {
            Ok(())
        }

        fn delete_actor(&self, _component_id: &ComponentId) -> anyhow::Result<()> {
            Ok(())
        }

        fn delete_strategy(&self, _component_id: &StrategyId) -> anyhow::Result<()> {
            Ok(())
        }

        fn delete_order(&self, _client_order_id: &ClientOrderId) -> anyhow::Result<()> {
            Ok(())
        }

        fn delete_position(&self, _position_id: &PositionId) -> anyhow::Result<()> {
            Ok(())
        }

        fn delete_account_event(
            &self,
            _account_id: &AccountId,
            _event_id: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn index_venue_order_id(
            &self,
            _client_order_id: ClientOrderId,
            _venue_order_id: VenueOrderId,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn index_order_position(
            &self,
            _client_order_id: ClientOrderId,
            _position_id: PositionId,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn update_actor(&self) -> anyhow::Result<()> {
            Ok(())
        }

        fn update_strategy(&self) -> anyhow::Result<()> {
            Ok(())
        }

        fn update_account(&self, _account: &AccountAny) -> anyhow::Result<()> {
            Ok(())
        }

        fn update_order(&self, _order_event: &OrderEventAny) -> anyhow::Result<()> {
            Ok(())
        }

        fn update_position(&self, _position: &Position) -> anyhow::Result<()> {
            Ok(())
        }

        fn snapshot_order_state(&self, _order: &OrderAny) -> anyhow::Result<()> {
            Ok(())
        }

        fn snapshot_position_state(&self, _position: &Position) -> anyhow::Result<()> {
            Ok(())
        }

        fn heartbeat(&self, _timestamp: UnixNanos) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct NoopKernelEventStore;

    impl KernelEventStore for NoopKernelEventStore {
        fn restore_parent_cache(
            &mut self,
            _instance_id: UUID4,
            _cache: &mut Cache,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn open(
            &mut self,
            _instance_id: UUID4,
            _components: &RegisteredComponents,
            _environment: Environment,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn snapshot_anchorer(&self) -> Option<SnapshotAnchorer> {
            None
        }

        fn seal(&mut self, _ts_init: UnixNanos) {}

        fn run_id(&self) -> Option<&str> {
            None
        }

        fn parent_run_id(&self) -> Option<&str> {
            None
        }

        fn is_halted(&self) -> bool {
            false
        }
    }
}
