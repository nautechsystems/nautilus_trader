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

use thiserror::Error;

/// Represents errors that can occur when interacting with a blockchain RPC client.
#[derive(Debug, Error)]
pub enum BlockchainRpcClientError {
    /// Occurs when the RPC client encounters a client-level error, such as connection failures.
    #[error("Client error: {0}")]
    ClientError(String),
    /// Occurs when input parameters to an RPC call are invalid.
    #[error("Invalid RPC parameters: {0}")]
    InvalidParameters(String),
    /// Occurs when decoding contract ABI data fails.
    #[error("Decoding error: {0}")]
    AbiDecodingError(String),
    /// Occurs when parsing an RPC message fails.
    #[error("Parsing error: {0}")]
    MessageParsingError(String),
    /// Occurs when receiving an unsupported RPC response type.
    #[error("Unsupported rpc response type of message {0}")]
    UnsupportedRpcResponseType(String),
    /// Occurs when an internal RPC client error is encountered.
    #[error("Internal Rpc client error: {0}")]
    InternalRpcClientError(String),
    /// Indicates that no message was received from the RPC channel.
    #[error("No message received")]
    NoMessageReceived,
}
