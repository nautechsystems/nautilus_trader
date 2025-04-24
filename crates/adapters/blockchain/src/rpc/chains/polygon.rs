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

use nautilus_model::defi::chain::chains;

use crate::rpc::{
    BlockchainRpcClient, core::CoreBlockchainRpcClient, error::BlockchainRpcClientError,
};

pub struct PolygonRpclient {
    base_client: CoreBlockchainRpcClient,
}

impl PolygonRpclient {
    pub fn new(wss_rpc_url: String) -> Self {
        let base_client = CoreBlockchainRpcClient::new(chains::POLYGON.clone(), wss_rpc_url);

        Self { base_client }
    }
}

impl BlockchainRpcClient for PolygonRpclient {
    fn subscribe_live_blocks(&self) -> Result<(), BlockchainRpcClientError> {
        self.base_client.subscribe_live_blocks()
    }

    fn unsubscribe_live_blocks(&self) -> Result<(), BlockchainRpcClientError> {
        self.base_client.unsubscribe_live_blocks()
    }
}
