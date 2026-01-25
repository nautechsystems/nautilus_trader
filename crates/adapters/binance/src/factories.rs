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

//! Factory functions for creating Binance clients and components.

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clients::{DataClient, ExecutionClient},
    clock::Clock,
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    enums::{AccountType, OmsType},
    identifiers::ClientId,
};
use nautilus_system::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};

use crate::{
    common::{
        consts::{BINANCE, BINANCE_VENUE},
        enums::BinanceProductType,
    },
    config::{BinanceDataClientConfig, BinanceExecClientConfig},
    futures::{data::BinanceFuturesDataClient, execution::BinanceFuturesExecutionClient},
    spot::{data::BinanceSpotDataClient, execution::BinanceSpotExecutionClient},
};

/// Factory for creating Binance data clients.
#[derive(Debug)]
pub struct BinanceDataClientFactory;

impl BinanceDataClientFactory {
    /// Creates a new [`BinanceDataClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BinanceDataClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DataClientFactory for BinanceDataClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        _cache: Rc<RefCell<Cache>>,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let binance_config = config
            .as_any()
            .downcast_ref::<BinanceDataClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BinanceDataClientFactory. Expected BinanceDataClientConfig, was {config:?}",
                )
            })?
            .clone();

        let client_id = ClientId::from(name);

        let product_type = binance_config
            .product_types
            .first()
            .copied()
            .unwrap_or(BinanceProductType::Spot);

        match product_type {
            BinanceProductType::Spot => {
                let client = BinanceSpotDataClient::new(client_id, binance_config)?;
                Ok(Box::new(client))
            }
            BinanceProductType::UsdM | BinanceProductType::CoinM => {
                let client =
                    BinanceFuturesDataClient::new(client_id, binance_config, product_type)?;
                Ok(Box::new(client))
            }
            _ => {
                anyhow::bail!("Unsupported product type for Binance data client: {product_type:?}")
            }
        }
    }

    fn name(&self) -> &'static str {
        BINANCE
    }

    fn config_type(&self) -> &'static str {
        stringify!(BinanceDataClientConfig)
    }
}

/// Factory for creating Binance Spot execution clients.
#[derive(Debug)]
pub struct BinanceExecutionClientFactory;

impl BinanceExecutionClientFactory {
    /// Creates a new [`BinanceExecutionClientFactory`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for BinanceExecutionClientFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionClientFactory for BinanceExecutionClientFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        let binance_config = config
            .as_any()
            .downcast_ref::<BinanceExecClientConfig>()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid config type for BinanceExecutionClientFactory. Expected BinanceExecClientConfig, was {config:?}",
                )
            })?
            .clone();

        let product_type = binance_config
            .product_types
            .first()
            .copied()
            .unwrap_or(BinanceProductType::Spot);

        match product_type {
            BinanceProductType::Spot => {
                // Spot uses cash account type and hedging OMS
                let account_type = AccountType::Cash;
                let oms_type = OmsType::Hedging;

                let core = ExecutionClientCore::new(
                    binance_config.trader_id,
                    ClientId::from(name),
                    *BINANCE_VENUE,
                    oms_type,
                    binance_config.account_id,
                    account_type,
                    None, // base_currency
                    clock,
                    cache,
                );

                let client = BinanceSpotExecutionClient::new(core, binance_config)?;
                Ok(Box::new(client))
            }
            BinanceProductType::UsdM | BinanceProductType::CoinM => {
                // Futures uses margin account type and netting OMS
                let account_type = AccountType::Margin;
                let oms_type = OmsType::Netting;

                let core = ExecutionClientCore::new(
                    binance_config.trader_id,
                    ClientId::from(name),
                    *BINANCE_VENUE,
                    oms_type,
                    binance_config.account_id,
                    account_type,
                    None, // base_currency
                    clock,
                    cache,
                );

                let client = BinanceFuturesExecutionClient::new(core, binance_config)?;
                Ok(Box::new(client))
            }
            _ => {
                anyhow::bail!(
                    "Unsupported product type for Binance execution client: {product_type:?}"
                )
            }
        }
    }

    fn name(&self) -> &'static str {
        BINANCE
    }

    fn config_type(&self) -> &'static str {
        stringify!(BinanceExecClientConfig)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_system::factories::DataClientFactory;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_binance_data_client_factory_creation() {
        let factory = BinanceDataClientFactory::new();
        assert_eq!(factory.name(), "BINANCE");
        assert_eq!(factory.config_type(), "BinanceDataClientConfig");
    }

    #[rstest]
    fn test_binance_data_client_factory_default() {
        let factory = BinanceDataClientFactory;
        assert_eq!(factory.name(), "BINANCE");
    }
}
