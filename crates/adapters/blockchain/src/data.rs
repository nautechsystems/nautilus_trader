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

use nautilus_model::defi::chain::{Blockchain, Chain};

use crate::{
    config::BlockchainAdapterConfig,
    rpc::{
        BlockchainRpcClient, BlockchainRpcClientAny,
        chains::{
            arbitrum::ArbitrumRpcClient, base::BaseRpcClient, ethereum::EthereumRpcClient,
            polygon::PolygonRpclient,
        },
        error::BlockchainRpcClientError,
        types::BlockchainRpcMessage,
    },
};

pub struct BlockchainDataClient {
    pub chain: Chain,
    rpc_client: BlockchainRpcClientAny,
}

impl BlockchainDataClient {
    #[must_use]
    pub fn new(chain: Chain, config: BlockchainAdapterConfig) -> Self {
        let rpc_client = Self::initialize_rpc_client(chain.name, config.wss_rpc_url);
        Self { chain, rpc_client }
    }

    fn initialize_rpc_client(
        blockchain: Blockchain,
        wss_rpc_url: String,
    ) -> BlockchainRpcClientAny {
        match blockchain {
            Blockchain::Ethereum => {
                BlockchainRpcClientAny::Ethereum(EthereumRpcClient::new(wss_rpc_url))
            }
            Blockchain::Polygon => {
                BlockchainRpcClientAny::Polygon(PolygonRpclient::new(wss_rpc_url))
            }
            Blockchain::Base => BlockchainRpcClientAny::Base(BaseRpcClient::new(wss_rpc_url)),
            Blockchain::Arbitrum => {
                BlockchainRpcClientAny::Arbitrum(ArbitrumRpcClient::new(wss_rpc_url))
            }
            _ => panic!("Unsupported blockchain {blockchain} for RPC connection"),
        }
    }

    pub async fn connect(&mut self) -> anyhow::Result<()> {
        self.rpc_client.connect().await
    }

    pub async fn process_rpc_message(&mut self) {
        loop {
            match self.rpc_client.next_rpc_message().await {
                Ok(msg) => match msg {
                    BlockchainRpcMessage::Block(block) => {
                        log::info!("{block}");
                    }
                },
                Err(e) => {
                    log::error!("Error processing rpc message: {e}");
                }
            }
        }
    }

    pub async fn subscribe_blocks(&mut self) -> Result<(), BlockchainRpcClientError> {
        self.rpc_client.subscribe_blocks().await
    }

    pub async fn unsubscribe_blocks(&mut self) -> Result<(), BlockchainRpcClientError> {
        self.rpc_client.unsubscribe_blocks().await
    }
}
