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

use std::fmt::{Display, Formatter};

use crate::defi::chain::SharedChain;

/// Represents a cryptocurrency token on a blockchain network.
#[derive(Debug, Clone)]
pub struct Token {
    /// The blockchain network where this token exists.
    pub chain: SharedChain,
    /// The blockchain address of the token contract.
    pub address: String,
    /// The full name of the token.
    pub name: String,
    /// The token's ticker symbol.
    pub symbol: String,
    /// The number of decimal places used to represent fractional token amounts.
    pub decimals: u8,
}

impl Token {
    /// Creates a new [`Token`] instance with the specified properties.
    #[must_use]
    pub fn new(
        chain: SharedChain,
        address: String,
        name: String,
        symbol: String,
        decimals: u8,
    ) -> Self {
        Self {
            chain,
            address,
            name,
            symbol,
            decimals,
        }
    }
}

impl Display for Token {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Token(symbol={}, name={})", self.symbol, self.name)
    }
}
