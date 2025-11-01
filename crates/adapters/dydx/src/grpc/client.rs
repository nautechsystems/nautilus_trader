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

//! gRPC client implementation for dYdX v4 protocol.
//!
//! This module provides the main gRPC client for interacting with dYdX v4 validator nodes.
//! It handles transaction signing, broadcasting, and querying account state.

use crate::error::DydxError;
use dydx_proto::cosmos_sdk_proto::cosmos::{
    auth::v1beta1::{BaseAccount, QueryAccountRequest, query_client::QueryClient as AuthClient},
    bank::v1beta1::{QueryAllBalancesRequest, query_client::QueryClient as BankClient},
    base::{
        tendermint::v1beta1::{
            Block, GetLatestBlockRequest, GetNodeInfoRequest, GetNodeInfoResponse,
            service_client::ServiceClient as BaseClient,
        },
        v1beta1::Coin,
    },
    tx::v1beta1::{
        BroadcastMode, BroadcastTxRequest, GetTxRequest, SimulateRequest,
        service_client::ServiceClient as TxClient,
    },
};
use dydx_proto::dydxprotocol::{
    clob::{ClobPair, QueryAllClobPairRequest, query_client::QueryClient as ClobClient},
    perpetuals::{
        Perpetual, QueryAllPerpetualsRequest, query_client::QueryClient as PerpetualsClient,
    },
    subaccounts::{
        QueryGetSubaccountRequest, Subaccount as SubaccountInfo,
        query_client::QueryClient as SubaccountsClient,
    },
};
use prost::Message as ProstMessage;
use tonic::transport::Channel;

/// Transaction hash type (internally uses tendermint::Hash).
pub type TxHash = String;

/// Block height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Height(pub u32);

/// gRPC client for dYdX v4 protocol operations.
///
/// This client handles:
/// - Transaction signing and broadcasting.
/// - Account query operations.
/// - Order placement and management via Cosmos SDK messages.
///
/// # Examples
///
/// ```no_run
/// use nautilus_dydx::grpc::DydxGrpcClient;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let grpc_url = "https://dydx-grpc.publicnode.com:443".to_string();
/// let client = DydxGrpcClient::new(grpc_url).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct DydxGrpcClient {
    channel: Channel,
    auth: AuthClient<Channel>,
    bank: BankClient<Channel>,
    base: BaseClient<Channel>,
    tx: TxClient<Channel>,
    clob: ClobClient<Channel>,
    perpetuals: PerpetualsClient<Channel>,
    subaccounts: SubaccountsClient<Channel>,
}

impl DydxGrpcClient {
    /// Create a new gRPC client.
    ///
    /// # Errors
    ///
    /// Returns an error if the gRPC connection cannot be established.
    pub async fn new(grpc_url: String) -> Result<Self, DydxError> {
        let channel = Channel::from_shared(grpc_url)
            .map_err(|e| DydxError::Config(format!("Invalid gRPC URL: {e}")))?
            .connect()
            .await
            .map_err(|e| {
                DydxError::Grpc(tonic::Status::unavailable(format!(
                    "Connection failed: {e}"
                )))
            })?;

        Ok(Self {
            auth: AuthClient::new(channel.clone()),
            bank: BankClient::new(channel.clone()),
            base: BaseClient::new(channel.clone()),
            tx: TxClient::new(channel.clone()),
            clob: ClobClient::new(channel.clone()),
            perpetuals: PerpetualsClient::new(channel.clone()),
            subaccounts: SubaccountsClient::new(channel.clone()),
            channel,
        })
    }

    /// Get the underlying gRPC channel.
    ///
    /// This can be used to create custom gRPC service clients.
    #[must_use]
    pub fn channel(&self) -> &Channel {
        &self.channel
    }

    /// Query account information for a given address.
    ///
    /// Returns the account number and sequence number needed for transaction signing.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails or the account does not exist.
    pub async fn query_address(&mut self, address: &str) -> Result<(u64, u64), anyhow::Error> {
        let req = QueryAccountRequest {
            address: address.to_string(),
        };
        let resp = self
            .auth
            .account(req)
            .await?
            .into_inner()
            .account
            .ok_or_else(|| {
                anyhow::anyhow!("Query account request failure, account should exist")
            })?;

        let account = BaseAccount::decode(&*resp.value)?;
        Ok((account.account_number, account.sequence))
    }

    /// Query for [an account](https://github.com/cosmos/cosmos-sdk/tree/main/x/auth#account-1)
    /// by its address.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails or the account does not exist.
    pub async fn get_account(&mut self, address: &str) -> Result<BaseAccount, anyhow::Error> {
        let req = QueryAccountRequest {
            address: address.to_string(),
        };
        let resp = self
            .auth
            .account(req)
            .await?
            .into_inner()
            .account
            .ok_or_else(|| {
                anyhow::anyhow!("Query account request failure, account should exist")
            })?;

        Ok(BaseAccount::decode(&*resp.value)?)
    }

    /// Query for [account balances](https://github.com/cosmos/cosmos-sdk/tree/main/x/bank#allbalances)
    /// by address for all denominations.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn get_account_balances(
        &mut self,
        address: &str,
    ) -> Result<Vec<Coin>, anyhow::Error> {
        let req = QueryAllBalancesRequest {
            address: address.to_string(),
            resolve_denom: false,
            pagination: None,
        };
        let balances = self.bank.all_balances(req).await?.into_inner().balances;
        Ok(balances)
    }

    /// Query for node info.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn get_node_info(&mut self) -> Result<GetNodeInfoResponse, anyhow::Error> {
        let req = GetNodeInfoRequest {};
        let info = self.base.get_node_info(req).await?.into_inner();
        Ok(info)
    }

    /// Query for the latest block.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn latest_block(&mut self) -> Result<Block, anyhow::Error> {
        let req = GetLatestBlockRequest::default();
        let latest_block = self
            .base
            .get_latest_block(req)
            .await?
            .into_inner()
            .sdk_block
            .ok_or_else(|| anyhow::anyhow!("The latest block is empty"))?;
        Ok(latest_block)
    }

    /// Query for the latest block height.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn latest_block_height(&mut self) -> Result<Height, anyhow::Error> {
        let latest_block = self.latest_block().await?;
        let header = latest_block
            .header
            .ok_or_else(|| anyhow::anyhow!("The block doesn't contain a header"))?;
        let height = Height(header.height.try_into()?);
        Ok(height)
    }

    /// Query for all perpetual markets.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn get_perpetuals(&mut self) -> Result<Vec<Perpetual>, anyhow::Error> {
        let req = QueryAllPerpetualsRequest { pagination: None };
        let perpetuals = self
            .perpetuals
            .perpetual_all(req)
            .await?
            .into_inner()
            .perpetual;
        Ok(perpetuals)
    }

    /// Query for all CLOB pairs.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn get_clob_pairs(&mut self) -> Result<Vec<ClobPair>, anyhow::Error> {
        let req = QueryAllClobPairRequest { pagination: None };
        let pairs = self.clob.clob_pair_all(req).await?.into_inner().clob_pair;
        Ok(pairs)
    }

    /// Query for subaccount information.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn get_subaccount(
        &mut self,
        address: &str,
        number: u32,
    ) -> Result<SubaccountInfo, anyhow::Error> {
        let req = QueryGetSubaccountRequest {
            owner: address.to_string(),
            number,
        };
        let subaccount = self
            .subaccounts
            .subaccount(req)
            .await?
            .into_inner()
            .subaccount
            .ok_or_else(|| {
                anyhow::anyhow!("Subaccount query response does not contain subaccount")
            })?;
        Ok(subaccount)
    }

    /// Simulate a transaction to estimate gas usage.
    ///
    /// # Errors
    ///
    /// Returns an error if simulation fails.
    pub async fn simulate_tx(&mut self, tx_bytes: Vec<u8>) -> Result<u64, anyhow::Error> {
        let req = SimulateRequest { tx_bytes, tx: None };
        let gas_used = self
            .tx
            .simulate(req)
            .await?
            .into_inner()
            .gas_info
            .ok_or_else(|| anyhow::anyhow!("Simulation response does not contain gas info"))?
            .gas_used;
        Ok(gas_used)
    }

    /// Broadcast a signed transaction.
    ///
    /// # Errors
    ///
    /// Returns an error if broadcasting fails.
    pub async fn broadcast_tx(&mut self, tx_bytes: Vec<u8>) -> Result<TxHash, anyhow::Error> {
        let req = BroadcastTxRequest {
            tx_bytes,
            mode: BroadcastMode::Sync as i32,
        };
        let response = self.tx.broadcast_tx(req).await?.into_inner();

        if let Some(tx_response) = response.tx_response {
            if tx_response.code != 0 {
                return Err(anyhow::anyhow!(
                    "Transaction broadcast failed: code={}, log={}",
                    tx_response.code,
                    tx_response.raw_log
                ));
            }
            Ok(tx_response.txhash)
        } else {
            Err(anyhow::anyhow!(
                "Broadcast response does not contain tx_response"
            ))
        }
    }

    /// Query transaction by hash.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub async fn get_tx(&mut self, hash: &str) -> Result<cosmrs::Tx, anyhow::Error> {
        let req = GetTxRequest {
            hash: hash.to_string(),
        };
        let response = self.tx.get_tx(req).await?.into_inner();

        if let Some(tx) = response.tx {
            Ok(cosmrs::Tx::try_from(tx)?)
        } else {
            Err(anyhow::anyhow!("Transaction not found"))
        }
    }
}
