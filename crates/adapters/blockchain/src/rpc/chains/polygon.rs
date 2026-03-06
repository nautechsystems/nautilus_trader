use nautilus_model::defi::chain::chains;

use crate::rpc::{
    BlockchainRpcClient, core::CoreBlockchainRpcClient, error::BlockchainRpcClientError,
    types::BlockchainMessage,
};

#[derive(Debug)]
pub struct PolygonRpcClient {
    base_client: CoreBlockchainRpcClient,
}

impl PolygonRpcClient {
    pub fn new(wss_rpc_url: String) -> Self {
        let base_client = CoreBlockchainRpcClient::new(chains::POLYGON.clone(), wss_rpc_url);

        Self { base_client }
    }
}

#[async_trait::async_trait]
impl BlockchainRpcClient for PolygonRpcClient {
    async fn connect(&mut self) -> anyhow::Result<()> {
        self.base_client.connect().await
    }

    async fn subscribe_blocks(&mut self) -> Result<(), BlockchainRpcClientError> {
        self.base_client.subscribe_blocks().await
    }

    async fn unsubscribe_blocks(&mut self) -> Result<(), BlockchainRpcClientError> {
        self.base_client.unsubscribe_blocks().await
    }

    async fn next_rpc_message(&mut self) -> Result<BlockchainMessage, BlockchainRpcClientError> {
        self.base_client.next_rpc_message().await
    }
}
