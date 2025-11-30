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

//! Type definitions for dYdX v4 gRPC operations.

use std::str::FromStr;

use cosmrs::tendermint::{Error, chain::Id};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display};

/// [Chain ID](https://docs.dydx.xyz/nodes/network-constants#chain-id)
/// serves as a unique chain identifier to prevent replay attacks.
///
/// See also [Cosmos ecosystem](https://cosmos.directory/).
#[derive(Debug, Eq, PartialEq, Clone, Display, AsRefStr, Deserialize, Serialize)]
pub enum ChainId {
    /// Testnet.
    #[strum(serialize = "dydx-testnet-4")]
    #[serde(rename = "dydx-testnet-4")]
    Testnet4,
    /// Mainnet.
    #[strum(serialize = "dydx-mainnet-1")]
    #[serde(rename = "dydx-mainnet-1")]
    Mainnet1,
}

impl FromStr for ChainId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dydx-testnet-4" | "testnet" => Ok(Self::Testnet4),
            "dydx-mainnet-1" | "mainnet" => Ok(Self::Mainnet1),
            _ => anyhow::bail!("Invalid chain ID: {s}"),
        }
    }
}

impl TryFrom<ChainId> for Id {
    type Error = Error;

    fn try_from(chain_id: ChainId) -> Result<Self, Self::Error> {
        chain_id.as_ref().parse()
    }
}
