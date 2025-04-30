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

use serde::{Deserialize, de::DeserializeOwned};

/// A response structure received from a WebSocket JSON-RPC blockchain node subscription.
#[derive(Debug, Deserialize)]
pub struct RpcNodeWssResponse<T>
where
    T: DeserializeOwned,
{
    /// JSON-RPC version identifier.
    pub jsonrpc: String,
    /// Name of the RPC method that was called.
    pub method: String,
    /// Parameters containing subscription information and the deserialized result.
    #[serde(bound(deserialize = ""))]
    pub params: RpcNodeSubscriptionResponse<T>,
}

/// Container for subscription data within an RPC response, holding the subscription ID and the deserialized result.
#[derive(Debug, Deserialize)]
pub struct RpcNodeSubscriptionResponse<T>
where
    T: DeserializeOwned,
{
    /// ID of the subscription associated with the RPC response.
    pub subscription: String,
    /// Deserialized result.
    #[serde(bound(deserialize = ""))]
    pub result: T,
}

/// A response structure received from an HTTP JSON-RPC blockchain node request.
#[derive(Debug, Deserialize)]
pub struct RpcNodeHttpResponse<T>
where
    T: DeserializeOwned,
{
    /// JSON-RPC version identifier.
    pub jsonrpc: String,
    /// Request identifier returned by the server.
    pub id: u64,
    /// Deserialized result.
    #[serde(bound(deserialize = ""))]
    pub result: T,
}
