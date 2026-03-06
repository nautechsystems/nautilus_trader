//! Factory functions for creating Tardis clients.

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, clients::DataClient, clock::Clock};
use nautilus_model::identifiers::ClientId;
use nautilus_system::factories::{ClientConfig, DataClientFactory};

use crate::{common::consts::TARDIS, config::TardisDataClientConfig, data::TardisDataClient};

impl ClientConfig for TardisDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Factory for creating Tardis data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.tardis", from_py_object)
)]
pub struct TardisDataClientFactory;

impl TardisDataClientFactory {
    /// Creates a new [`TardisDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for TardisDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for TardisDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let tardis_config = config
            .as_any()
            .downcast_ref::<TardisDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for TardisDataClientFactory. \
                     Expected TardisDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);
        let client = TardisDataClient::new(client_id, tardis_config)?;
        Ok(Box::new(client))
    }

    fn name(&self) -> &'static str {
        TARDIS
    }

    fn config_type(&self) -> &'static str {
        "TardisDataClientConfig"
    }
}
