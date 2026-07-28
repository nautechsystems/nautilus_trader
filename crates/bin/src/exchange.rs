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

//! Exchange selection and node building.

pub mod bybit;
pub mod dydx;

use std::str::FromStr;

use nautilus_live::node::LiveNode;
use nautilus_model::identifiers::TraderId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exchange {
    Dydx,
    Bybit,
}

impl FromStr for Exchange {
    type Err = Box<dyn std::error::Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "dydx" => Ok(Self::Dydx),
            "bybit" => Ok(Self::Bybit),
            other => {
                Err(format!("Unknown exchange '{other}'. Expected 'dydx' or 'bybit'").into())
            }
        }
    }
}

impl Exchange {
    pub fn build_node(
        self,
        trader_id: TraderId,
    ) -> Result<LiveNode, Box<dyn std::error::Error>> {
        match self {
            Self::Dydx => {
                let node = dydx::build_node(trader_id)?;
                Ok(node)
            }
            Self::Bybit => {
                let node = bybit::build_node(trader_id)?;
                Ok(node)
            }
        }
    }
}