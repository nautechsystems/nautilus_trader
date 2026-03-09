//! Factory for creating sandbox execution clients.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache, clients::ExecutionClient, clock::Clock, live::clock::LiveClock,
};
use nautilus_execution::client::core::ExecutionClientCore;
use nautilus_model::identifiers::ClientId;
use nautilus_system::factories::{ClientConfig, ExecutionClientFactory};

use crate::{config::SandboxExecutionClientConfig, execution::SandboxExecutionClient};

impl ClientConfig for SandboxExecutionClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating sandbox execution clients.
#[derive(Debug, Default)]
pub struct SandboxExecutionClientFactory;

impl SandboxExecutionClientFactory {
    /// Creates a new [`SandboxExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ExecutionClientFactory for SandboxExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let sandbox_config = config
            .as_any()
            .downcast_ref::<SandboxExecutionClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for SandboxExecutionClientFactory. Expected SandboxExecutionClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(LiveClock::default()));

        let core = ExecutionClientCore::new(
            sandbox_config.trader_id,
            client_id,
            sandbox_config.venue,
            sandbox_config.oms_type,
            sandbox_config.account_id,
            sandbox_config.account_type,
            sandbox_config.base_currency,
            cache.clone(),
        );

        let client = SandboxExecutionClient::new(core, sandbox_config, clock, cache);
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        "SANDBOX"
    }

    fn config_type(&self) -> &'static str {
        "SandboxExecutionClientConfig"
    }
}
