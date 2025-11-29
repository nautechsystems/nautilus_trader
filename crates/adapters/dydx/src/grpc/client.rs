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

use prost::Message as ProstMessage;
use tonic::transport::Channel;

use crate::{
    error::DydxError,
    proto::{
        cosmos_sdk_proto::cosmos::{
            auth::v1beta1::{
                BaseAccount, QueryAccountRequest, query_client::QueryClient as AuthClient,
            },
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
        },
        dydxprotocol::{
            clob::{ClobPair, QueryAllClobPairRequest, query_client::QueryClient as ClobClient},
            perpetuals::{
                Perpetual, QueryAllPerpetualsRequest, query_client::QueryClient as PerpetualsClient,
            },
            subaccounts::{
                QueryGetSubaccountRequest, Subaccount as SubaccountInfo,
                query_client::QueryClient as SubaccountsClient,
            },
        },
    },
};

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
/// - Connection management and automatic failover to fallback nodes.
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
    current_url: String,
}

impl DydxGrpcClient {
    /// Create a new gRPC client with a single URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the gRPC connection cannot be established.
    pub async fn new(grpc_url: String) -> Result<Self, DydxError> {
        let mut endpoint = Channel::from_shared(grpc_url.clone())
            .map_err(|e| DydxError::Config(format!("Invalid gRPC URL: {e}")))?;

        // Enable TLS for HTTPS URLs (required for public gRPC nodes)
        if grpc_url.starts_with("https://") {
            let tls = tonic::transport::ClientTlsConfig::new().with_enabled_roots();
            endpoint = endpoint
                .tls_config(tls)
                .map_err(|e| DydxError::Config(format!("TLS config failed: {e}")))?;
        }

        let channel = endpoint.connect().await.map_err(|e| {
            DydxError::Grpc(Box::new(tonic::Status::unavailable(format!(
                "Connection failed: {e}"
            ))))
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
            current_url: grpc_url,
        })
    }

    /// Create a new gRPC client with fallback support.
    ///
    /// Attempts to connect to each URL in the provided list until a successful
    /// connection is established. This is useful for DEX environments where nodes
    /// can fail and fallback options are needed.
    ///
    /// # Errors
    ///
    /// Returns an error if none of the provided URLs can establish a connection.
    pub async fn new_with_fallback(grpc_urls: &[impl AsRef<str>]) -> Result<Self, DydxError> {
        if grpc_urls.is_empty() {
            return Err(DydxError::Config("No gRPC URLs provided".to_string()));
        }

        let mut last_error = None;

        for (idx, url) in grpc_urls.iter().enumerate() {
            let url_str = url.as_ref();
            tracing::debug!(
                "Attempting to connect to gRPC node: {url_str} (attempt {}/{})",
                idx + 1,
                grpc_urls.len()
            );

            match Self::new(url_str.to_string()).await {
                Ok(client) => {
                    tracing::info!("Successfully connected to gRPC node: {url_str}");
                    return Ok(client);
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to gRPC node {url_str}: {e}");
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            DydxError::Grpc(Box::new(tonic::Status::unavailable(
                "All gRPC connection attempts failed".to_string(),
            )))
        }))
    }

    /// Reconnect to a different gRPC node from the fallback list.
    ///
    /// Attempts to establish a new connection to each URL in the provided list
    /// until successful. This is useful when the current node fails and you need
    /// to failover to a different validator node.
    ///
    /// # Errors
    ///
    /// Returns an error if none of the provided URLs can establish a connection.
    pub async fn reconnect_with_fallback(
        &mut self,
        grpc_urls: &[impl AsRef<str>],
    ) -> Result<(), DydxError> {
        if grpc_urls.is_empty() {
            return Err(DydxError::Config("No gRPC URLs provided".to_string()));
        }

        let mut last_error = None;

        for (idx, url) in grpc_urls.iter().enumerate() {
            let url_str = url.as_ref();

            // Skip if it's the same URL we're currently connected to
            if url_str == self.current_url {
                tracing::debug!("Skipping current URL: {url_str}");
                continue;
            }

            tracing::debug!(
                "Attempting to reconnect to gRPC node: {url_str} (attempt {}/{})",
                idx + 1,
                grpc_urls.len()
            );

            let channel = match Channel::from_shared(url_str.to_string())
                .map_err(|e| DydxError::Config(format!("Invalid gRPC URL: {e}")))
            {
                Ok(ch) => ch,
                Err(e) => {
                    last_error = Some(e);
                    continue;
                }
            };

            match channel.connect().await {
                Ok(connected_channel) => {
                    tracing::info!("Successfully reconnected to gRPC node: {url_str}");

                    // Update all service clients with the new channel
                    self.channel = connected_channel.clone();
                    self.auth = AuthClient::new(connected_channel.clone());
                    self.bank = BankClient::new(connected_channel.clone());
                    self.base = BaseClient::new(connected_channel.clone());
                    self.tx = TxClient::new(connected_channel.clone());
                    self.clob = ClobClient::new(connected_channel.clone());
                    self.perpetuals = PerpetualsClient::new(connected_channel.clone());
                    self.subaccounts = SubaccountsClient::new(connected_channel);
                    self.current_url = url_str.to_string();

                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("Failed to reconnect to gRPC node {url_str}: {e}");
                    last_error = Some(DydxError::Grpc(Box::new(tonic::Status::unavailable(
                        format!("Connection failed: {e}"),
                    ))));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            DydxError::Grpc(Box::new(tonic::Status::unavailable(
                "All gRPC reconnection attempts failed".to_string(),
            )))
        }))
    }

    /// Get the currently connected gRPC node URL.
    #[must_use]
    pub fn current_url(&self) -> &str {
        &self.current_url
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
        let response = self.perpetuals.all_perpetuals(req).await?.into_inner();
        Ok(response.perpetual)
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
    #[allow(deprecated)]
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
                anyhow::bail!(
                    "Transaction broadcast failed: code={}, log={}",
                    tx_response.code,
                    tx_response.raw_log
                );
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
            // Convert through bytes since the types are incompatible
            let tx_bytes = tx.encode_to_vec();
            cosmrs::Tx::try_from(tx_bytes.as_slice()).map_err(|e| anyhow::anyhow!("{e}"))
        } else {
            anyhow::bail!("Transaction not found")
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_height_ordering() {
        let h1 = Height(100);
        let h2 = Height(200);
        assert!(h1 < h2);
        assert_eq!(h1, Height(100));
    }

    #[tokio::test]
    async fn test_new_with_fallback_empty_urls() {
        let result = DydxGrpcClient::new_with_fallback(&[] as &[&str]).await;
        assert!(result.is_err());
        if let Err(DydxError::Config(msg)) = result {
            assert_eq!(msg, "No gRPC URLs provided");
        } else {
            panic!("Expected Config error");
        }
    }

    #[tokio::test]
    async fn test_new_with_fallback_invalid_urls() {
        // Test with invalid URLs that will fail to connect
        let invalid_urls = vec!["invalid://bad-url", "http://0.0.0.0:1"];
        let result = DydxGrpcClient::new_with_fallback(&invalid_urls).await;

        // Should fail with either Config or Grpc error
        assert!(result.is_err());
    }
}
