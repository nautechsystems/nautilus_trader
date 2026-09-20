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

//! Builder for constructing [`LiveNode`] instances.

use std::{cell::RefCell, collections::HashMap, fmt::Debug, rc::Rc, time::Duration};

use ahash::{AHashMap, AHashSet};
use nautilus_common::{
    cache::{CacheConfig, database::CacheDatabaseFactory},
    clients::ExecutionClient,
    clock::Clock,
    enums::Environment,
    factories::{
        ClientConfig, DataClientFactory, ExecutionClientFactory, SimulatedExecutionClientFactory,
    },
    logging::logger::LoggerConfig,
    msgbus::{
        BusMessage, MessageBusBackingFactory, MessageBusConfig, MessageBusExternalEgress,
        MessageBusExternalIngress, external_egress_from_backing, external_io_from_backing,
    },
};
use nautilus_core::UUID4;
use nautilus_data::client::DataClientAdapter;
use nautilus_execution::engine::ExecutionEngine;
use nautilus_model::identifiers::{TraderId, Venue};
use nautilus_portfolio::config::PortfolioConfig;
#[cfg(feature = "streaming")]
use nautilus_system::config::StreamingConfig;
#[cfg(feature = "python")]
use nautilus_system::trader::Trader;
use nautilus_system::{
    clock_factory::ClockFactory,
    event_store::{EventStoreFactory, KernelEventStore},
    kernel::{NautilusKernel, NautilusKernelDependencies},
};
use nautilus_trading::ImportableControllerConfig;

use super::{
    LiveNode,
    config::{
        LiveDataEngineConfig, LiveExecutionEngineConfig, LiveNodeConfig, LiveRiskEngineConfig,
        RoutingConfig, validate_live_environment,
    },
};
use crate::{
    execution::{
        client::LiveExecutionClient,
        manager::{ExecutionManager, ExecutionManagerConfig},
    },
    runner::AsyncRunner,
    socket::SocketReconnectRegistry,
};

#[derive(Debug)]
enum ExecutionClientFactoryEntry {
    Adapter(Box<dyn ExecutionClientFactory>),
    Simulated(Box<dyn SimulatedExecutionClientFactory>),
}

pub(crate) struct ExternalMessageBusIngress(Box<dyn MessageBusExternalIngress>);

impl Debug for ExternalMessageBusIngress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ExternalMessageBusIngress))
            .finish_non_exhaustive()
    }
}

/// Builder for constructing a [`LiveNode`] with a fluent API.
///
/// Provides configuration options specific to live nodes, including client factory
/// registration, timeout settings, and optional event-store injection for run-lifecycle
/// audit and replay (see [`Self::with_event_store`]).
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.live", unsendable)
)]
pub struct LiveNodeBuilder {
    name: String,
    config: LiveNodeConfig,
    data_client_factories: HashMap<String, Box<dyn DataClientFactory>>,
    exec_client_factories: HashMap<String, ExecutionClientFactoryEntry>,
    data_client_configs: HashMap<String, Box<dyn ClientConfig>>,
    exec_client_configs: HashMap<String, Box<dyn ClientConfig>>,
    data_client_routing: HashMap<String, RoutingConfig>,
    exec_client_routing: HashMap<String, RoutingConfig>,
    event_store_factory: Option<EventStoreFactory>,
    clock_factory: Option<ClockFactory>,
    cache_database_factory: Option<Box<dyn CacheDatabaseFactory>>,
    external_msgbus_factory: Option<Box<dyn MessageBusBackingFactory>>,
    external_msgbus_egress: Option<Box<dyn MessageBusExternalEgress>>,
    external_msgbus_ingress: Option<ExternalMessageBusIngress>,
}

impl Debug for LiveNodeBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(LiveNodeBuilder))
            .field("name", &self.name)
            .field("config", &self.config)
            .field("data_client_factories", &self.data_client_factories.keys())
            .field("exec_client_factories", &self.exec_client_factories.keys())
            .field("data_client_configs", &self.data_client_configs.keys())
            .field("exec_client_configs", &self.exec_client_configs.keys())
            .field("event_store_factory", &self.event_store_factory.is_some())
            .field("clock_factory", &self.clock_factory.is_some())
            .field(
                "cache_database_factory",
                &self.cache_database_factory.is_some(),
            )
            .field(
                "external_msgbus_factory",
                &self.external_msgbus_factory.is_some(),
            )
            .field(
                "external_msgbus_egress",
                &self.external_msgbus_egress.is_some(),
            )
            .field(
                "external_msgbus_ingress",
                &self.external_msgbus_ingress.is_some(),
            )
            .finish_non_exhaustive()
    }
}

impl LiveNodeBuilder {
    /// Creates a new [`LiveNodeBuilder`] with required parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if `environment` is invalid (BACKTEST).
    pub fn new(trader_id: TraderId, environment: Environment) -> anyhow::Result<Self> {
        validate_live_environment(environment)?;

        let config = LiveNodeConfig {
            environment,
            trader_id,
            ..Default::default()
        };

        Ok(Self {
            name: "LiveNode".to_string(),
            config,
            data_client_factories: HashMap::new(),
            exec_client_factories: HashMap::new(),
            data_client_configs: HashMap::new(),
            exec_client_configs: HashMap::new(),
            data_client_routing: HashMap::new(),
            exec_client_routing: HashMap::new(),
            event_store_factory: None,
            clock_factory: None,
            cache_database_factory: None,
            external_msgbus_factory: None,
            external_msgbus_egress: None,
            external_msgbus_ingress: None,
        })
    }

    /// Creates a new [`LiveNodeBuilder`] from an existing [`LiveNodeConfig`].
    ///
    /// # Errors
    ///
    /// Returns an error if the config's environment is invalid (BACKTEST).
    pub fn from_config(config: LiveNodeConfig) -> anyhow::Result<Self> {
        validate_live_environment(config.environment)?;

        Ok(Self {
            name: "LiveNode".to_string(),
            config,
            data_client_factories: HashMap::new(),
            exec_client_factories: HashMap::new(),
            data_client_configs: HashMap::new(),
            exec_client_configs: HashMap::new(),
            data_client_routing: HashMap::new(),
            exec_client_routing: HashMap::new(),
            event_store_factory: None,
            clock_factory: None,
            cache_database_factory: None,
            external_msgbus_factory: None,
            external_msgbus_egress: None,
            external_msgbus_ingress: None,
        })
    }

    /// Returns the name for the node.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Set the name for the node.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the instance ID for the node.
    #[must_use]
    pub const fn with_instance_id(mut self, instance_id: UUID4) -> Self {
        self.config.instance_id = Some(instance_id);
        self
    }

    /// Configure whether to load state on startup.
    #[must_use]
    pub const fn with_load_state(mut self, load_state: bool) -> Self {
        self.config.load_state = load_state;
        self
    }

    /// Configure whether to save state on shutdown.
    #[must_use]
    pub const fn with_save_state(mut self, save_state: bool) -> Self {
        self.config.save_state = save_state;
        self
    }

    /// Set the importable controller configuration for the node.
    ///
    /// The controller is instantiated and registered with the trader during
    /// [`LiveNodeBuilder::build`], enabling runtime strategy/actor management
    /// (create, start, stop, remove) without restarting the node. This mirrors
    /// the `controller` field on [`LiveNodeConfig`] used by the config-based
    /// [`LiveNode::build`] path, so a builder that also registers client
    /// factories can host a controller in a single node.
    #[must_use]
    pub fn with_controller(mut self, controller: ImportableControllerConfig) -> Self {
        self.config.controller = Some(controller);
        self
    }

    /// Set the connection timeout in seconds.
    #[must_use]
    pub const fn with_timeout_connection(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_connection = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the reconciliation timeout in seconds.
    #[must_use]
    pub const fn with_timeout_reconciliation(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_reconciliation = Duration::from_secs(timeout_secs);
        self
    }

    /// Configure whether to run startup reconciliation.
    #[must_use]
    pub fn with_reconciliation(mut self, reconciliation: bool) -> Self {
        self.config.exec_engine.reconciliation = reconciliation;
        self
    }

    /// Set the reconciliation lookback in minutes.
    #[must_use]
    pub fn with_reconciliation_lookback_mins(mut self, mins: u32) -> Self {
        self.config.exec_engine.reconciliation_lookback_mins = Some(mins);
        self
    }

    /// Set the portfolio initialization timeout in seconds.
    #[must_use]
    pub const fn with_timeout_portfolio(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_portfolio = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the disconnection timeout in seconds.
    #[must_use]
    pub const fn with_timeout_disconnection_secs(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_disconnection = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the post-stop delay in seconds.
    #[must_use]
    pub const fn with_delay_post_stop_secs(mut self, delay_secs: u64) -> Self {
        self.config.delay_post_stop = Duration::from_secs(delay_secs);
        self
    }

    /// Set the shutdown timeout in seconds.
    #[must_use]
    pub const fn with_delay_shutdown_secs(mut self, delay_secs: u64) -> Self {
        self.config.timeout_shutdown = Duration::from_secs(delay_secs);
        self
    }

    /// Inject a caller-supplied clock factory for the kernel and component clocks.
    #[must_use]
    pub fn with_clock_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn() -> Rc<RefCell<dyn Clock>> + 'static,
    {
        self.clock_factory = Some(ClockFactory::new(factory));
        self
    }

    /// Set the cache configuration.
    #[must_use]
    pub fn with_cache_config(mut self, config: CacheConfig) -> Self {
        self.config.cache = Some(config);
        self
    }

    /// Install the cache database backing from a factory.
    ///
    /// The node constructs and owns the adapter when it starts, so the `load_state` and
    /// `save_state` settings on [`LiveNodeConfig`] take effect.
    #[must_use]
    pub fn with_cache_database_factory(mut self, factory: Box<dyn CacheDatabaseFactory>) -> Self {
        self.cache_database_factory = Some(factory);
        self
    }

    /// Set the message bus configuration.
    ///
    /// External streams are consumed when an ingress implementation is injected with
    /// [`Self::with_external_ingress`] or built from [`Self::with_external_msgbus_factory`].
    #[must_use]
    pub fn with_msgbus_config(mut self, config: MessageBusConfig) -> Self {
        self.config.msgbus = Some(config);
        self
    }

    /// Set the portfolio configuration.
    #[must_use]
    pub fn with_portfolio_config(mut self, config: PortfolioConfig) -> Self {
        self.config.portfolio = Some(config);
        self
    }

    /// Set the streaming configuration.
    ///
    /// The Rust live runtime does not support this setting yet.
    /// `build()` returns an error when it is set.
    #[cfg(feature = "streaming")]
    #[must_use]
    pub fn with_streaming_config(mut self, config: StreamingConfig) -> Self {
        self.config.streaming = Some(config);
        self
    }

    /// Set the data engine configuration.
    ///
    /// The Rust live runtime currently supports only the default `qsize`.
    /// `build()` returns an error for other values.
    #[must_use]
    pub fn with_data_engine_config(mut self, config: LiveDataEngineConfig) -> Self {
        self.config.data_engine = config;
        self
    }

    /// Set the risk engine configuration.
    ///
    /// The Rust live runtime currently supports only the default `qsize`.
    /// `build()` returns an error for other values.
    #[must_use]
    pub fn with_risk_engine_config(mut self, config: LiveRiskEngineConfig) -> Self {
        self.config.risk_engine = config;
        self
    }

    /// Set the execution engine configuration.
    ///
    /// The Rust live runtime currently supports only the default `qsize`.
    /// `build()` returns an error for other values.
    #[must_use]
    pub fn with_exec_engine_config(mut self, config: LiveExecutionEngineConfig) -> Self {
        self.config.exec_engine = config;
        self
    }

    /// Inject an event-store implementation to drive run-lifecycle capture.
    ///
    /// The factory receives the kernel's instance id and clock so the returned
    /// `KernelEventStore` shares the same time source the kernel uses to stamp run
    /// lifecycle entries. The concrete implementation lives outside this crate;
    /// callers typically build it from
    /// [`LiveNodeConfig::event_store`](crate::config::LiveNodeConfig::event_store)
    /// inside the closure.
    #[must_use]
    pub fn with_event_store<F>(mut self, factory: F) -> Self
    where
        F: FnOnce(UUID4, Rc<RefCell<dyn Clock>>) -> anyhow::Result<Box<dyn KernelEventStore>>
            + 'static,
    {
        self.event_store_factory = Some(Box::new(factory));
        self
    }

    /// Inject external message bus egress for serialized message bus publications.
    #[must_use]
    pub fn with_external_msgbus_egress(
        mut self,
        external_egress: Box<dyn MessageBusExternalEgress>,
    ) -> Self {
        self.external_msgbus_egress = Some(external_egress);
        self
    }

    /// Build and inject external message bus egress and configured ingress from a factory.
    #[must_use]
    pub fn with_external_msgbus_factory(
        mut self,
        factory: Box<dyn MessageBusBackingFactory>,
    ) -> Self {
        self.external_msgbus_factory = Some(factory);
        self
    }

    /// Inject external message bus ingress for serialized inbound publications.
    #[must_use]
    pub fn with_external_ingress(
        mut self,
        external_ingress: Box<dyn MessageBusExternalIngress>,
    ) -> Self {
        self.external_msgbus_ingress = Some(ExternalMessageBusIngress(external_ingress));
        self
    }

    /// Set the logging configuration.
    #[must_use]
    pub fn with_logging(mut self, logging: LoggerConfig) -> Self {
        self.config.logging = logging;
        self
    }

    /// Adds a data client factory with configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if a client with the same name is already registered.
    pub fn add_data_client(
        self,
        name: Option<String>,
        factory: Box<dyn DataClientFactory>,
        config: Box<dyn ClientConfig>,
    ) -> anyhow::Result<Self> {
        self.add_data_client_with_routing(name, factory, config, RoutingConfig::default())
    }

    #[cfg(feature = "python")]
    pub(crate) fn has_data_client(&self, name: &str) -> bool {
        self.data_client_factories.contains_key(name)
    }

    #[cfg(feature = "python")]
    pub(crate) fn has_exec_client(&self, name: &str) -> bool {
        self.exec_client_factories.contains_key(name)
    }

    /// Adds a data client factory with configuration and explicit routing.
    ///
    /// # Errors
    ///
    /// Returns an error if a client with the same name is already registered.
    pub fn add_data_client_with_routing(
        mut self,
        name: Option<String>,
        factory: Box<dyn DataClientFactory>,
        config: Box<dyn ClientConfig>,
        routing: RoutingConfig,
    ) -> anyhow::Result<Self> {
        let name = name.unwrap_or_else(|| factory.name().to_string());

        if self.data_client_factories.contains_key(&name) {
            anyhow::bail!("Data client '{name}' is already registered");
        }

        self.data_client_factories.insert(name.clone(), factory);
        self.data_client_configs.insert(name.clone(), config);
        self.data_client_routing.insert(name, routing);
        Ok(self)
    }

    /// Adds an execution client factory with configuration.
    ///
    /// Equivalent to [`Self::add_exec_client_with_routing`] with default (empty)
    /// routing.
    ///
    /// # Errors
    ///
    /// Returns an error if a client with the same name is already registered.
    pub fn add_exec_client(
        self,
        name: Option<String>,
        factory: Box<dyn ExecutionClientFactory>,
        config: Box<dyn ClientConfig>,
    ) -> anyhow::Result<Self> {
        self.add_exec_client_with_routing(name, factory, config, RoutingConfig::default())
    }

    /// Adds an execution client factory with configuration and explicit routing.
    ///
    /// Explicit venue routes take precedence over automatic routes. The only client for a
    /// venue routes it automatically. Multiple clients for that venue require an explicit
    /// venue route or a default client.
    ///
    /// # Errors
    ///
    /// Returns an error if a client with the same name is already registered.
    pub fn add_exec_client_with_routing(
        mut self,
        name: Option<String>,
        factory: Box<dyn ExecutionClientFactory>,
        config: Box<dyn ClientConfig>,
        routing: RoutingConfig,
    ) -> anyhow::Result<Self> {
        let name = name.unwrap_or_else(|| factory.name().to_string());

        if self.exec_client_factories.contains_key(&name) {
            anyhow::bail!("Execution client '{name}' is already registered");
        }

        self.exec_client_factories
            .insert(name.clone(), ExecutionClientFactoryEntry::Adapter(factory));
        self.exec_client_configs.insert(name.clone(), config);
        self.exec_client_routing.insert(name, routing);
        Ok(self)
    }

    /// Add a simulated execution client factory.
    ///
    /// This path is for sync-core clients such as the sandbox matching engine, which owns cache
    /// mutation. Live venue adapters should use [`Self::add_exec_client`].
    ///
    /// # Errors
    ///
    /// Returns an error if a client with the same name is already registered.
    pub fn add_simulated_exec_client(
        mut self,
        name: Option<String>,
        factory: Box<dyn SimulatedExecutionClientFactory>,
        config: Box<dyn ClientConfig>,
    ) -> anyhow::Result<Self> {
        let name = name.unwrap_or_else(|| factory.name().to_string());

        if self.exec_client_factories.contains_key(&name) {
            anyhow::bail!("Execution client '{name}' is already registered");
        }

        self.exec_client_factories.insert(
            name.clone(),
            ExecutionClientFactoryEntry::Simulated(factory),
        );
        self.exec_client_configs.insert(name, config);
        Ok(self)
    }

    /// Build the [`LiveNode`] with the configured settings.
    ///
    /// This will:
    /// 1. Build the underlying kernel.
    /// 2. Create clients using factories.
    /// 3. Register clients with engines.
    ///
    /// # Errors
    ///
    /// Returns an error if node construction fails, including conflicting execution routes
    /// or multiple execution clients for a venue without an explicit route or default client.
    pub fn build(mut self) -> anyhow::Result<LiveNode> {
        self.build_in_place()
    }

    pub(crate) fn build_in_place(&mut self) -> anyhow::Result<LiveNode> {
        log::info!(
            "Building LiveNode with {} data clients and {} execution clients",
            self.data_client_factories.len(),
            self.exec_client_factories.len()
        );

        self.config.validate_runtime_support()?;

        if self.config.event_store.is_some() && self.event_store_factory.is_none() {
            anyhow::bail!(
                "LiveNodeConfig.event_store is set but no factory was registered; \
                 call LiveNodeBuilder::with_event_store(...) to install one"
            );
        }

        if self.external_msgbus_factory.is_some()
            && (self.external_msgbus_egress.is_some() || self.external_msgbus_ingress.is_some())
        {
            anyhow::bail!(
                "external message bus factory cannot be combined with injected egress or ingress"
            );
        }

        let runner = AsyncRunner::new();
        runner.bind_senders();

        let socket_registry = SocketReconnectRegistry::default();

        let kernel = NautilusKernel::new_with_dependencies(
            self.name.clone(),
            self.config.clone(),
            NautilusKernelDependencies::default()
                .with_clock_factory(self.clock_factory.clone())
                .with_event_store_factory(self.event_store_factory.take()),
        )?;
        #[cfg(feature = "python")]
        if let Some(controller) = self.config.controller.as_ref() {
            Trader::add_controller_from_importable_config(&kernel.trader, controller)?;
        }

        #[cfg(not(feature = "python"))]
        if let Some(controller) = self.config.controller.as_ref() {
            anyhow::bail!(
                "LiveNodeConfig.controller for importable controller '{}' requires the python feature",
                controller.controller_path
            );
        }

        let (external_egress, external_ingress) = self.create_external_msgbus(&kernel)?;

        if let Some(external_egress) = external_egress {
            let config = self.config.msgbus.clone().unwrap_or_default();
            nautilus_common::msgbus::get_message_bus()
                .borrow_mut()
                .set_external_egress_config(external_egress, &config)?;
        }

        for (name, factory) in &self.data_client_factories {
            if let Some(config) = self.data_client_configs.get(name) {
                log::debug!("Creating data client {name}");

                let client = socket_registry.scope(|| {
                    factory.create(name, config.as_ref(), kernel.cache().into(), kernel.clock())
                })?;

                let client_id = client.client_id();
                let venue = client.venue();
                socket_registry.register_client(client_id);

                let adapter = DataClientAdapter::new(
                    client_id, venue, true, // handles_order_book_deltas
                    true, // handles_order_book_snapshots
                    client,
                );

                let routing = self
                    .data_client_routing
                    .get(name)
                    .cloned()
                    .unwrap_or_default();

                {
                    let mut data_engine = kernel.data_engine.borrow_mut();
                    data_engine.register_client(adapter, venue);

                    if routing.default {
                        data_engine.set_default_client(client_id)?;
                    }

                    if let Some(venues) = &routing.venues {
                        for venue_str in venues {
                            data_engine.register_venue_routing(
                                client_id,
                                Venue::new(venue_str.as_str()),
                            )?;
                        }
                    }
                }

                log::info!("Registered DataClient-{client_id}");
            } else {
                log::warn!("No config found for data client factory {name}");
            }
        }

        let mut exec_clients = Vec::new();
        let mut venue_candidates = AHashMap::<Venue, Vec<_>>::new();
        let mut venues_explicit = AHashSet::new();
        let mut has_default_client = false;
        let mut instrument_venues = AHashSet::new();

        for (name, factory) in &self.exec_client_factories {
            if let Some(config) = self.exec_client_configs.get(name) {
                log::debug!("Creating execution client {name}");

                let client = socket_registry.scope(|| match factory {
                    ExecutionClientFactoryEntry::Adapter(factory) => factory.create(
                        self.config.trader_id,
                        name,
                        config.as_ref(),
                        kernel.cache().into(),
                        kernel.clock(),
                    ),
                    ExecutionClientFactoryEntry::Simulated(factory) => {
                        factory.create(self.config.trader_id, name, config.as_ref(), kernel.cache())
                    }
                })?;

                let client = LiveExecutionClient::new(client);
                let client_id = client.client_id();
                let venue = client.venue();
                socket_registry.register_client(client_id);

                let routing = self
                    .exec_client_routing
                    .get(name)
                    .cloned()
                    .unwrap_or_default();

                {
                    let mut exec_engine = kernel.exec_engine.borrow_mut();
                    exec_engine.register_client(Box::new(client.clone()))?;

                    if routing.default {
                        exec_engine.set_default_client(client_id)?;
                        has_default_client = true;
                    }

                    if let Some(venues) = &routing.venues {
                        for venue_str in venues {
                            let route_venue = Venue::new(venue_str.as_str());
                            exec_engine.register_venue_routing(client_id, route_venue)?;
                            venues_explicit.insert(route_venue);
                            instrument_venues.insert(route_venue);
                        }
                    }
                }

                venue_candidates.entry(venue).or_default().push(client_id);
                instrument_venues.insert(venue);
                exec_clients.push(client);

                log::info!("Registered ExecutionClient-{client_id}");
            } else {
                log::warn!("No config found for execution client factory {name}");
            }
        }

        {
            let mut exec_engine = kernel.exec_engine.borrow_mut();

            for (venue, candidates) in venue_candidates {
                if venues_explicit.contains(&venue) {
                    continue;
                }

                if let [client_id] = candidates.as_slice() {
                    exec_engine.register_venue_routing(*client_id, venue)?;
                } else if !has_default_client {
                    anyhow::bail!(
                        "Multiple execution clients for venue {venue}: configure an explicit venue route or default client"
                    );
                }
            }
        }

        for venue in instrument_venues {
            ExecutionEngine::subscribe_venue_instruments(&kernel.exec_engine, venue);
        }

        let exec_manager_config = ExecutionManagerConfig::from(&self.config.exec_engine)
            .with_trader_id(self.config.trader_id);

        let mut exec_manager = ExecutionManager::new(
            kernel.clock.clone(),
            kernel.cache.clone(),
            exec_manager_config,
        )?;

        for client in &exec_clients {
            exec_manager.set_position_reconciliation_tolerance(
                client.account_id(),
                client.position_reconciliation_tolerance(),
            );
        }

        let mut node = LiveNode::new_from_builder(
            kernel,
            runner,
            self.config.clone(),
            exec_manager,
            exec_clients,
            socket_registry,
            None,
            external_ingress,
        );
        node.load_configured_plugins()?;
        node.cache_database_factory = self.cache_database_factory.take();

        log::info!("Built successfully");

        Ok(node)
    }

    #[expect(
        clippy::type_complexity,
        reason = "external backing returns its paired egress and ingress"
    )]
    fn create_external_msgbus(
        &mut self,
        kernel: &NautilusKernel,
    ) -> anyhow::Result<(
        Option<Box<dyn MessageBusExternalEgress>>,
        Option<ExternalMessageBusIngress>,
    )> {
        let Some(factory) = self.external_msgbus_factory.as_ref() else {
            return Ok((
                self.external_msgbus_egress.take(),
                self.external_msgbus_ingress.take(),
            ));
        };

        let config = self.config.msgbus.clone().unwrap_or_default();
        let has_external_streams = config
            .external_streams
            .as_ref()
            .is_some_and(|streams| !streams.is_empty());
        config.validate()?;
        let backing = factory.create(self.config.trader_id, kernel.instance_id, config)?;

        if has_external_streams {
            let (external_egress, external_ingress) = external_io_from_backing(backing);
            Ok((
                Some(external_egress),
                Some(ExternalMessageBusIngress(external_ingress)),
            ))
        } else {
            Ok((Some(external_egress_from_backing(backing)), None))
        }
    }
}

impl ExternalMessageBusIngress {
    pub(crate) fn is_closed(&self) -> bool {
        self.0.is_closed()
    }

    pub(crate) fn take_receiver(
        &mut self,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
        self.0.take_receiver()
    }

    pub(crate) fn close(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, rc::Rc};

    use nautilus_common::{
        cache::CacheView,
        clients::ExecutionClient,
        clock::Clock,
        enums::Environment,
        factories::{ClientConfig, ExecutionClientFactory},
        messages::execution::{SubmitOrder, TradingCommand},
        msgbus::{self, switchboard},
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_execution::engine::stubs::StubExecutionClient;
    use nautilus_model::{
        enums::{OmsType, OrderType},
        identifiers::{AccountId, ClientId, ClientOrderId, TraderId, Venue},
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        orders::{Order, OrderTestBuilder},
        stubs::TestDefault,
        types::Quantity,
    };
    use nautilus_trading::ImportableControllerConfig;
    use rstest::rstest;

    use super::LiveNodeBuilder;
    use crate::node::config::RoutingConfig;

    #[rstest]
    fn test_with_controller_sets_config_controller() {
        let controller = ImportableControllerConfig {
            controller_path: "module:Controller".to_string(),
            config_path: "module:ControllerConfig".to_string(),
            config: HashMap::new(),
        };

        let builder = LiveNodeBuilder::new(TraderId::from("TRADER-001"), Environment::Live)
            .unwrap()
            .with_controller(controller);

        assert!(builder.config.controller.is_some());
    }

    #[rstest]
    #[case::single(1, false, false, false)]
    #[case::explicit(2, true, false, false)]
    #[case::default(2, false, true, false)]
    #[case::explicit_over_default(2, true, true, false)]
    #[case::single_empty_venues(1, false, false, true)]
    #[case::default_empty_venues(2, false, true, true)]
    fn test_execution_client_routing_and_instruments(
        #[case] count: usize,
        #[case] explicit: bool,
        #[case] default: bool,
        #[case] empty_venues: bool,
        #[values(false, true)] reverse: bool,
    ) {
        let clients: Vec<_> = (0..count)
            .map(|i| {
                StubExecutionClient::new(
                    ClientId::new(format!("CLIENT-{i}")),
                    AccountId::new(format!("ACCOUNT-{i}")),
                    Venue::from("SIM"),
                    OmsType::Netting,
                    None,
                )
            })
            .collect();

        let mut builder =
            LiveNodeBuilder::new(TraderId::test_default(), Environment::Live).unwrap();
        let mut indices: Vec<_> = (0..count).collect();
        if reverse {
            indices.reverse();
        }

        for i in indices {
            builder = builder
                .add_exec_client_with_routing(
                    Some(format!("client-{i}")),
                    Box::new(RoutingClientFactory(clients[i].clone())),
                    Box::new(RoutingClientConfig),
                    RoutingConfig {
                        default: default && i == 0,
                        venues: if empty_venues {
                            Some(vec![])
                        } else {
                            (explicit && i == 1).then(|| vec!["SIM".to_string()])
                        },
                    },
                )
                .unwrap();
        }

        let node = builder.build().unwrap();
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        msgbus::publish_instrument(
            switchboard::get_instrument_topic(instrument.id()),
            &instrument,
        );
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .quantity(Quantity::from(1))
            .build();
        node.kernel()
            .cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
        node.kernel()
            .cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        let engine = node.kernel().exec_engine.borrow();
        engine.execute(TradingCommand::SubmitOrder(SubmitOrder::from_order(
            &order,
            TraderId::test_default(),
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
        )));
        let routed = engine.get_clients_for_orders(std::slice::from_ref(&order));
        let explicit_index = count - 1 - usize::from(explicit);
        let explicit_order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-EXPLICIT"))
            .quantity(Quantity::from(2))
            .build();
        node.kernel()
            .cache
            .borrow_mut()
            .add_order(explicit_order.clone(), None, None, false)
            .unwrap();
        engine.execute(TradingCommand::SubmitOrder(SubmitOrder::from_order(
            &explicit_order,
            TraderId::test_default(),
            Some(clients[explicit_index].client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        )));

        assert_eq!(engine.client_ids().len(), count);
        assert_eq!(routed.len(), 1);
        assert_eq!(
            routed[0].client_id(),
            clients[usize::from(explicit)].client_id()
        );

        for (i, client) in clients.iter().enumerate() {
            let mut expected_orders = if i == usize::from(explicit) {
                vec![order.client_order_id()]
            } else {
                vec![]
            };

            if i == explicit_index {
                expected_orders.push(explicit_order.client_order_id());
            }

            assert_eq!(*client.submitted_order_ids().borrow(), expected_orders);
            assert_eq!(
                *client.received_instruments().borrow(),
                vec![instrument.clone()]
            );
        }
    }

    #[rstest]
    fn test_execution_client_keeps_native_route_with_extra_venue(
        #[values(false, true)] other_native_client: bool,
    ) {
        let client = StubExecutionClient::new(
            ClientId::from("CLIENT"),
            AccountId::from("CLIENT-001"),
            Venue::from("SIM"),
            OmsType::Netting,
            None,
        )
        .with_handles_all_order_venues();

        let other_client = StubExecutionClient::new(
            ClientId::from("OTHER_CLIENT"),
            AccountId::from("OTHER_CLIENT-002"),
            Venue::from("OTHER"),
            OmsType::Netting,
            None,
        );

        let mut builder = LiveNodeBuilder::new(TraderId::test_default(), Environment::Live)
            .unwrap()
            .add_exec_client_with_routing(
                Some("client".to_string()),
                Box::new(RoutingClientFactory(client.clone())),
                Box::new(RoutingClientConfig),
                RoutingConfig {
                    default: false,
                    venues: Some(vec!["OTHER".to_string(), "OTHER".to_string()]),
                },
            )
            .unwrap();

        if other_native_client {
            builder = builder
                .add_exec_client_with_routing(
                    Some("other-client".to_string()),
                    Box::new(RoutingClientFactory(other_client.clone())),
                    Box::new(RoutingClientConfig),
                    RoutingConfig::default(),
                )
                .unwrap();
        }

        let node = builder.build().unwrap();

        let native = audusd_sim();
        let mut other = native.clone();
        other.id.venue = Venue::from("OTHER");
        let instruments = [
            InstrumentAny::CurrencyPair(native),
            InstrumentAny::CurrencyPair(other),
        ];

        for instrument in &instruments {
            let order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(instrument.id())
                .client_order_id(ClientOrderId::new(format!("O-{}", instrument.id().venue)))
                .quantity(Quantity::from(1))
                .build();
            let engine = node.kernel().exec_engine.borrow();
            node.kernel()
                .cache
                .borrow_mut()
                .add_instrument(instrument.clone())
                .unwrap();
            node.kernel()
                .cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
            engine.execute(TradingCommand::SubmitOrder(SubmitOrder::from_order(
                &order,
                TraderId::test_default(),
                None,
                None,
                UUID4::new(),
                UnixNanos::default(),
            )));
            let routed = engine.get_clients_for_orders(&[order]);
            assert_eq!(
                routed
                    .iter()
                    .map(|client| client.client_id())
                    .collect::<Vec<_>>(),
                vec![client.client_id()]
            );
            drop(engine);
            msgbus::publish_instrument(
                switchboard::get_instrument_topic(instrument.id()),
                instrument,
            );
        }

        assert_eq!(*client.received_instruments().borrow(), instruments);
        assert_eq!(
            *client.submitted_order_ids().borrow(),
            vec![ClientOrderId::from("O-SIM"), ClientOrderId::from("O-OTHER")]
        );
        assert_eq!(*other_client.submitted_order_ids().borrow(), vec![]);

        let expected_other = if other_native_client {
            vec![instruments[1].clone()]
        } else {
            vec![]
        };

        assert_eq!(
            *other_client.received_instruments().borrow(),
            expected_other
        );
    }

    #[rstest]
    #[case::ambiguous(
        false,
        false,
        false,
        "Multiple execution clients for venue SIM: configure an explicit venue route or default client"
    )]
    #[case::duplicate_id(true, false, false, "Client already registered with ID CLIENT-0")]
    #[case::duplicate_default(false, true, false, "default client already registered")]
    #[case::duplicate_route(false, false, true, "cannot re-route")]
    fn test_execution_client_routing_rejects_ambiguity(
        #[case] duplicate_id: bool,
        #[case] default: bool,
        #[case] explicit: bool,
        #[case] expected: &str,
    ) {
        let mut builder =
            LiveNodeBuilder::new(TraderId::test_default(), Environment::Live).unwrap();

        for i in 0..2 {
            let id = if duplicate_id { 0 } else { i };

            let client = StubExecutionClient::new(
                ClientId::new(format!("CLIENT-{id}")),
                AccountId::new(format!("ACCOUNT-{i}")),
                Venue::from("SIM"),
                OmsType::Netting,
                None,
            );
            builder = builder
                .add_exec_client_with_routing(
                    Some(format!("client-{i}")),
                    Box::new(RoutingClientFactory(client)),
                    Box::new(RoutingClientConfig),
                    RoutingConfig {
                        default,
                        venues: explicit.then(|| vec!["SIM".to_string()]),
                    },
                )
                .unwrap();
        }

        let error = builder.build().unwrap_err().to_string();
        assert!(error.contains(expected), "Unexpected error: {error}");
    }

    #[derive(Debug)]
    struct RoutingClientFactory(StubExecutionClient);

    impl ExecutionClientFactory for RoutingClientFactory {
        fn create(
            &self,
            _trader_id: TraderId,
            _name: &str,
            _config: &dyn ClientConfig,
            _cache: CacheView,
            _clock: Rc<RefCell<dyn Clock>>,
        ) -> anyhow::Result<Box<dyn ExecutionClient>> {
            Ok(Box::new(self.0.clone()))
        }

        fn name(&self) -> &'static str {
            "routing"
        }

        fn config_type(&self) -> &'static str {
            "RoutingClientConfig"
        }
    }

    #[derive(Debug)]
    struct RoutingClientConfig;

    impl ClientConfig for RoutingClientConfig {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
}
