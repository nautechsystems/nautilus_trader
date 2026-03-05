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
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use alloy::primitives::Address;
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    factories::OrderEventFactory,
    live::get_runtime,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
    msgbus::{self, MessagingSwitchboard},
};
use nautilus_core::{UnixNanos, time::nanos_since_unix_epoch};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::AccountAny,
    defi::{SharedChain, validation::validate_address},
    enums::{AccountType, OmsType},
    identifiers::{AccountId, ClientId, Venue},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use tokio::{runtime::RuntimeFlavor, sync::Mutex as AsyncMutex, task::block_in_place};

use crate::{
    config::BlockchainExecutionClientConfig,
    contracts::erc20::Erc20Contract,
    execution::{
        metadata_store::{InMemoryMetadataStore, MetadataStore},
        wallet::{WalletTracker, WalletTrackerConfig},
    },
    rpc::http::BlockchainHttpRpcClient,
};

/// Execution client for blockchain interactions including balance tracking and order execution.
#[derive(Debug)]
pub struct BlockchainExecutionClient {
    /// Core execution client providing base functionality.
    core: ExecutionClientCore,
    /// Metadata store for token and pool details required during execution.
    _metadata_store: Mutex<Box<dyn MetadataStore>>,
    /// The blockchain network configuration.
    chain: SharedChain,
    /// Tracks deterministic wallet snapshots and allowance state.
    wallet_tracker: AsyncMutex<WalletTracker>,
    /// Whether connect should refresh wallet state immediately.
    wallet_refresh_on_connect: bool,
    /// Contract interface for ERC-20 token interactions.
    erc20_contract: Erc20Contract,
    /// HTTP RPC client for blockchain queries.
    http_rpc_client: Arc<BlockchainHttpRpcClient>,
}

impl BlockchainExecutionClient {
    /// Creates a new [`BlockchainExecutionClient`] instance for the specified configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the wallet address or any token address in the config is invalid.
    pub fn new(
        core_client: ExecutionClientCore,
        config: BlockchainExecutionClientConfig,
    ) -> anyhow::Result<Self> {
        Self::with_metadata_store(core_client, config, Box::new(InMemoryMetadataStore::new()))
    }

    /// Creates a new [`BlockchainExecutionClient`] instance with a caller-supplied metadata store.
    ///
    /// # Errors
    ///
    /// Returns an error if the wallet address or any token address in the config is invalid.
    pub fn with_metadata_store(
        core_client: ExecutionClientCore,
        config: BlockchainExecutionClientConfig,
        metadata_store: Box<dyn MetadataStore>,
    ) -> anyhow::Result<Self> {
        let chain = Arc::new(config.chain.clone());
        let http_rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            config.http_rpc_url.clone(),
            config.rpc_requests_per_second,
        ));
        let wallet_address = validate_address(config.wallet_address.as_str())?;
        let erc20_contract = Erc20Contract::new(http_rpc_client.clone(), true);

        // Initialize token universe so wallet snapshots are deterministic and bounded.
        let mut token_universe = HashSet::new();
        if let Some(specified_tokens) = &config.tokens {
            for token in specified_tokens {
                let token_address = validate_address(token.as_str())?;
                token_universe.insert(token_address);
            }
        }

        for pool in metadata_store.all_pools() {
            token_universe.insert(pool.token0.address);
            token_universe.insert(pool.token1.address);
        }

        if let Some(wnative_address) = &config.wallet_wnative_address {
            let parsed = validate_address(wnative_address.as_str())?;
            token_universe.insert(parsed);
        }

        for token in &config.wallet_extra_tokens {
            let token_address = validate_address(token.as_str())?;
            token_universe.insert(token_address);
        }

        let allowance_spenders: Vec<Address> = config
            .wallet_allowance_spenders
            .iter()
            .map(|address| validate_address(address.as_str()))
            .collect::<anyhow::Result<Vec<_>>>()?;

        let max_batch_size = usize::try_from(config.multicall_max_batch_size)
            .unwrap_or(64)
            .max(1);
        let min_batch_size = usize::try_from(config.multicall_min_batch_size)
            .unwrap_or(4)
            .max(1)
            .min(max_batch_size);
        let max_tokens_per_refresh = usize::try_from(config.wallet_max_tokens_per_refresh)
            .unwrap_or(256)
            .max(1);

        let wallet_tracker_config = WalletTrackerConfig {
            allowance_spenders,
            snapshot_ttl: Duration::from_secs(u64::from(config.wallet_snapshot_ttl_secs.max(1))),
            max_tokens_per_refresh,
            multicall_max_batch_size: max_batch_size,
            multicall_min_batch_size: min_batch_size,
        };
        let wallet_tracker = WalletTracker::new(
            chain.clone(),
            wallet_address,
            token_universe,
            wallet_tracker_config,
        );

        Ok(Self {
            core: core_client,
            chain,
            _metadata_store: Mutex::new(metadata_store),
            wallet_tracker: AsyncMutex::new(wallet_tracker),
            wallet_refresh_on_connect: config.wallet_refresh_on_connect,
            erc20_contract,
            http_rpc_client,
        })
    }

    async fn refresh_wallet_snapshot(&self, force: bool) -> anyhow::Result<Vec<AccountBalance>> {
        let mut tracker = self.wallet_tracker.lock().await;
        if force || tracker.needs_refresh() {
            let summary = tracker
                .refresh(self.http_rpc_client.as_ref(), &self.erc20_contract)
                .await?;
            log::info!(
                "Wallet snapshot refreshed on {}: tokens={}, spenders={}",
                self.chain.name,
                summary.token_count,
                summary.spender_count
            );
        }
        tracker.account_balances()
    }

    fn refresh_wallet_snapshot_blocking(&self, force: bool) -> anyhow::Result<Vec<AccountBalance>> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => match handle.runtime_flavor() {
                RuntimeFlavor::CurrentThread => {
                    anyhow::bail!(
                        "query_account wallet refresh cannot block on a current-thread runtime"
                    )
                }
                _ => block_in_place(|| handle.block_on(self.refresh_wallet_snapshot(force))),
            },
            Err(_) => get_runtime().block_on(async { self.refresh_wallet_snapshot(force).await }),
        }
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BlockchainExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        self.core.venue
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account(&self.core.account_id).cloned()
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        let factory = OrderEventFactory::new(
            self.core.trader_id,
            self.core.account_id,
            AccountType::Cash,
            self.core.base_currency,
        );
        let account_state =
            factory.generate_account_state(balances, margins, reported, ts_event, ts_event);
        let endpoint = MessagingSwitchboard::portfolio_update_account();
        msgbus::send_account_state(endpoint, &account_state);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }

        self.core.set_started();
        log::info!(
            "Blockchain execution client started: client_id={}, account_id={}, venue={}",
            self.core.client_id,
            self.core.account_id,
            self.core.venue,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        self.core.set_stopped();
        self.core.set_disconnected();
        log::info!(
            "Blockchain execution client stopped: client_id={}",
            self.core.client_id
        );
        Ok(())
    }

    fn submit_order(&self, _cmd: &SubmitOrder) -> anyhow::Result<()> {
        todo!("implement submit_order")
    }

    fn submit_order_list(&self, _cmd: &SubmitOrderList) -> anyhow::Result<()> {
        todo!("implement submit_order_list")
    }

    fn modify_order(&self, _cmd: &ModifyOrder) -> anyhow::Result<()> {
        todo!("implement modify_order")
    }

    fn cancel_order(&self, _cmd: &CancelOrder) -> anyhow::Result<()> {
        todo!("implement cancel_order")
    }

    fn cancel_all_orders(&self, _cmd: &CancelAllOrders) -> anyhow::Result<()> {
        todo!("implement cancel_all_orders")
    }

    fn batch_cancel_orders(&self, _cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        todo!("implement batch_cancel_orders")
    }

    fn query_account(&self, cmd: &QueryAccount) -> anyhow::Result<()> {
        let balances = self.refresh_wallet_snapshot_blocking(true)?;
        self.generate_account_state(balances, Vec::new(), true, cmd.ts_init)?;
        Ok(())
    }

    fn query_order(&self, _cmd: &QueryOrder) -> anyhow::Result<()> {
        todo!("implement query_order")
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            log::warn!("Blockchain execution client already connected");
            return Ok(());
        }

        log::info!(
            "Connecting to blockchain execution client on chain {}",
            self.chain.name
        );

        if self.wallet_refresh_on_connect {
            let balances = self.refresh_wallet_snapshot(true).await?;
            let ts_event = UnixNanos::from(nanos_since_unix_epoch());
            self.generate_account_state(balances, Vec::new(), false, ts_event)?;
        }

        self.core.set_connected();
        log::info!(
            "Blockchain execution client connected on chain {}",
            self.chain.name
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.core.set_disconnected();
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        todo!("implement generate_order_status_report")
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        todo!("implement generate_order_status_reports")
    }

    async fn generate_fill_reports(
        &self,
        _cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        todo!("implement generate_fill_reports")
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        todo!("implement generate_position_status_reports")
    }

    async fn generate_mass_status(
        &self,
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        todo!("implement generate_mass_status")
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, sync::Arc};

    use super::BlockchainExecutionClient;
    use crate::{
        config::BlockchainExecutionClientConfig,
        execution::metadata_store::{InMemoryMetadataStore, PoolMetadataStore},
    };
    use nautilus_common::cache::Cache;
    use nautilus_core::UnixNanos;
    use nautilus_live::ExecutionClientCore;
    use nautilus_model::{
        defi::{
            AmmType, Dex, DexType, Pool, PoolIdentifier, Token, chain::chains,
            validation::validate_address,
        },
        enums::{AccountType, OmsType},
        identifiers::{AccountId, ClientId, TraderId, Venue},
        stubs::TestDefault,
    };

    fn make_token(address: &str, symbol: &str, decimals: u8) -> Token {
        Token::new(
            Arc::new(chains::BSC.clone()),
            validate_address(address).expect("token address should be valid"),
            symbol.to_string(),
            symbol.to_string(),
            decimals,
        )
    }

    fn make_pool() -> Pool {
        let chain = Arc::new(chains::BSC.clone());
        let pool_address =
            validate_address("0xd13040d4fe917EE704158CfCB3338dCd2838B245").expect("valid pool");

        let dex = Arc::new(Dex::new(
            (*chain).clone(),
            DexType::PancakeSwapV2,
            "0x10ED43C718714eb63d5aA57B78B54704E256024E",
            0,
            AmmType::CPAMM,
            "PairCreated(address,address,address,uint256)",
            "Swap(address,uint256,uint256,uint256,uint256,address)",
            "Mint(address,uint256,uint256)",
            "Burn(address,uint256,uint256,address)",
            "Sync(uint112,uint112)",
        ));

        let token0 = make_token("0x55d398326f99059fF775485246999027B3197955", "USDT", 18);
        let token1 = make_token("0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d", "USDC", 18);

        Pool::new(
            chain,
            dex,
            pool_address,
            PoolIdentifier::from_address(pool_address),
            0,
            token0,
            token1,
            Some(2500),
            None,
            UnixNanos::default(),
        )
    }

    #[test]
    fn test_token_universe_derives_pool_wnative_and_extra_tokens() {
        let mut metadata_store = InMemoryMetadataStore::new();
        let pool = make_pool();
        let pool_token0 = pool.token0.address;
        let pool_token1 = pool.token1.address;
        metadata_store.insert_pool(pool);

        let trader_id = TraderId::test_default();
        let account_id = AccountId::new("BINANCE-001");
        let mut config = BlockchainExecutionClientConfig::new(
            trader_id,
            account_id,
            Venue::new("Bsc:PancakeSwapV2"),
            chains::BSC.clone(),
            String::from("0x1111111111111111111111111111111111111111"),
            Some(vec![String::from(
                "0x0000000000000000000000000000000000000001",
            )]),
            String::from("https://bsc.example.com"),
            None,
        );
        config.wallet_extra_tokens =
            vec![String::from("0x0000000000000000000000000000000000000002")];
        config.wallet_wnative_address =
            Some(String::from("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c"));

        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = ExecutionClientCore::new(
            trader_id,
            ClientId::new("BLOCKCHAIN"),
            config.venue,
            OmsType::Netting,
            account_id,
            AccountType::Cash,
            None,
            cache,
        );

        let client =
            BlockchainExecutionClient::with_metadata_store(core, config, Box::new(metadata_store))
                .expect("client should construct");
        let tracker = nautilus_common::live::get_runtime().block_on(client.wallet_tracker.lock());
        let token_universe = &tracker.wallet_balance().token_universe;

        assert!(token_universe.contains(&pool_token0));
        assert!(token_universe.contains(&pool_token1));
        assert!(token_universe.contains(
            &validate_address("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c").expect("valid wnative")
        ));
        assert!(
            token_universe.contains(
                &validate_address("0x0000000000000000000000000000000000000001")
                    .expect("valid configured token")
            )
        );
        assert!(
            token_universe.contains(
                &validate_address("0x0000000000000000000000000000000000000002")
                    .expect("valid extra token")
            )
        );
        assert_eq!(token_universe.len(), 5);
    }
}
