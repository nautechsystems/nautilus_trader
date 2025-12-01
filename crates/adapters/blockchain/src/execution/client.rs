// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::HashSet, sync::Arc};

use alloy::primitives::Address;
use async_trait::async_trait;
use nautilus_common::messages::execution::{
    BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
    GenerateOrderStatusReport, GeneratePositionReports, ModifyOrder, QueryAccount, QueryOrder,
    SubmitOrder, SubmitOrderList,
};
use nautilus_core::UnixNanos;
use nautilus_execution::client::{ExecutionClient, base::ExecutionClientCore};
use nautilus_live::execution::client::LiveExecutionClient;
use nautilus_model::{
    accounts::AccountAny,
    defi::{
        SharedChain, Token,
        validation::validate_address,
        wallet::{TokenBalance, WalletBalance},
    },
    enums::OmsType,
    identifiers::{AccountId, ClientId, Venue},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money},
};

use crate::{
    cache::BlockchainCache, config::BlockchainExecutionClientConfig,
    contracts::erc20::Erc20Contract, rpc::http::BlockchainHttpRpcClient,
};

/// Execution client for blockchain interactions including balance tracking and order execution.
#[derive(Debug)]
pub struct BlockchainExecutionClient {
    /// Core execution client providing base functionality.
    core: ExecutionClientCore,
    /// Cache for storing token metadata and other blockchain data.
    cache: BlockchainCache,
    /// The blockchain network configuration.
    chain: SharedChain,
    /// The wallet address used for transactions and balance queries.
    wallet_address: Address,
    /// Tracks native currency and ERC-20 token balances.
    wallet_balance: WalletBalance,
    /// Contract interface for ERC-20 token interactions.
    erc20_contract: Erc20Contract,
    /// Whether the client is currently connected.
    connected: bool,
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
        let chain = Arc::new(config.chain);
        let cache = BlockchainCache::new(chain.clone());
        let http_rpc_client = Arc::new(BlockchainHttpRpcClient::new(
            config.http_rpc_url.clone(),
            config.rpc_requests_per_second,
        ));
        let wallet_address = validate_address(config.wallet_address.as_str())?;
        let erc20_contract = Erc20Contract::new(http_rpc_client.clone(), true);

        // Initialize token universe, so we can fetch them from the blockchain later.
        let mut token_universe = HashSet::new();
        if let Some(specified_tokens) = config.tokens {
            for token in specified_tokens {
                let token_address = validate_address(token.as_str())?;
                token_universe.insert(token_address);
            }
        }
        let wallet_balance = WalletBalance::new(token_universe);

        Ok(Self {
            core: core_client,
            connected: false,
            wallet_balance,
            chain,
            cache,
            erc20_contract,
            http_rpc_client,
            wallet_address,
        })
    }

    /// Fetches the native currency balance (e.g., ETH) for the wallet from the blockchain.
    async fn fetch_native_currency_balance(&self) -> anyhow::Result<Money> {
        let balance_u256 = self
            .http_rpc_client
            .get_balance(&self.wallet_address, None)
            .await?;

        let native_currency = self.chain.native_currency();

        // Convert from wei (18 decimals on-chain) to Money
        let balance = Money::from_wei(balance_u256, native_currency);

        Ok(balance)
    }

    /// Fetches the balance of a specific ERC-20 token for the wallet.
    async fn fetch_token_balance(
        &mut self,
        token_address: &Address,
    ) -> anyhow::Result<TokenBalance> {
        // Get the cached token or fetch it from the blockchain and cache it.
        let token = if let Some(token) = self.cache.get_token(token_address) {
            token.to_owned()
        } else {
            let token_info = self.erc20_contract.fetch_token_info(token_address).await?;
            let token = Token::new(
                self.chain.clone(),
                *token_address,
                token_info.name,
                token_info.symbol,
                token_info.decimals,
            );
            self.cache.add_token(token.clone()).await?;
            token
        };

        let amount = self
            .erc20_contract
            .balance_of(token_address, &self.wallet_address)
            .await?;
        let token_balance = TokenBalance::new(amount, token);

        // TODO: Use price oracle here and cache, to get the latest price then convert to USD
        // then use token_balance.set_amount_usd(amount_usd) to set the amount_usd value.

        Ok(token_balance)
    }

    /// Refreshes all wallet balances including native currency and tracked ERC-20 tokens.
    async fn refresh_wallet_balances(&mut self) -> anyhow::Result<()> {
        let native_currency_balance = self.fetch_native_currency_balance().await?;
        tracing::info!(
            "Initializing wallet balance with native currency balance: {} {}",
            native_currency_balance.as_decimal(),
            native_currency_balance.currency
        );
        self.wallet_balance
            .set_native_currency_balance(native_currency_balance);

        // Fetch token balances from the blockchain.
        if !self.wallet_balance.is_token_universe_initialized() {
            // TODO sync from transfer events for tokens that wallet interacted with.
        } else {
            let tokens: Vec<Address> = self
                .wallet_balance
                .token_universe
                .clone()
                .into_iter()
                .collect();
            for token in tokens {
                if let Ok(token_balance) = self.fetch_token_balance(&token).await {
                    tracing::info!("Adding token balance to the wallet: {}", token_balance);
                    self.wallet_balance.add_token_balance(token_balance);
                }
            }
        }

        Ok(())
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BlockchainExecutionClient {
    fn is_connected(&self) -> bool {
        self.connected
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
        todo!("implement get_account")
    }

    fn generate_account_state(
        &self,
        _balances: Vec<AccountBalance>,
        _margins: Vec<MarginBalance>,
        _reported: bool,
        _ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        todo!("implement generate_account_state")
    }

    fn start(&mut self) -> anyhow::Result<()> {
        todo!("implement start")
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        todo!("implement stop")
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

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        todo!("implement query_account")
    }

    fn query_order(&self, _cmd: &QueryOrder) -> anyhow::Result<()> {
        todo!("implement query_order")
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected {
            tracing::warn!("Blockchain execution client already connected");
            return Ok(());
        }

        tracing::info!(
            "Connecting to blockchain execution client on chain {}",
            self.chain.name
        );

        self.refresh_wallet_balances().await?;

        self.connected = true;
        tracing::info!(
            "Blockchain execution client connected on chain {}",
            self.chain.name
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.connected = false;
        Ok(())
    }
}

#[async_trait(?Send)]
impl LiveExecutionClient for BlockchainExecutionClient {
    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        todo!("implement generate_order_status_report")
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReport,
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
        _cmd: &GeneratePositionReports,
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
