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

use std::{
    fmt::{Display, Formatter},
    sync::Arc,
};

use alloy_primitives::Address;
use serde::{Deserialize, Serialize};

use crate::defi::chain::SharedChain;

/// Represents a cryptocurrency token on a blockchain network.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Token {
    /// The blockchain network where this token exists.
    pub chain: SharedChain,
    /// The blockchain address of the token contract.
    pub address: Address,
    /// The full name of the token.
    pub name: String,
    /// The token's ticker symbol.
    pub symbol: String,
    /// The number of decimal places used to represent fractional token amounts.
    pub decimals: u8,
}

/// A thread-safe shared pointer to a `Token`, enabling efficient reuse across multiple components.
pub type SharedToken = Arc<Token>;

impl Token {
    /// Creates a new [`Token`] instance with the specified properties.
    #[must_use]
    pub fn new(
        chain: SharedChain,
        address: Address,
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rstest::rstest;

    use super::*;
    use crate::defi::chain::chains;

    #[rstest]
    fn test_token_constructor() {
        let chain = Arc::new(chains::ETHEREUM.clone());
        let address = "0xA0b86a33E6441b936662bb6B5d1F8Fb0E2b57A5D"
            .parse()
            .unwrap();

        let token = Token::new(
            chain.clone(),
            address,
            "Wrapped Ether".to_string(),
            "WETH".to_string(),
            18,
        );

        assert_eq!(token.chain.chain_id, chain.chain_id);
        assert_eq!(token.address, address);
        assert_eq!(token.name, "Wrapped Ether");
        assert_eq!(token.symbol, "WETH");
        assert_eq!(token.decimals, 18);
    }

    #[rstest]
    fn test_token_display_with_special_characters() {
        // Test edge case where token names/symbols contain formatting characters
        let chain = Arc::new(chains::ETHEREUM.clone());
        let token = Token::new(
            chain,
            "0xA0b86a33E6441b936662bb6B5d1F8Fb0E2b57A5D"
                .parse()
                .unwrap(),
            "Test Token (with parentheses)".to_string(),
            "TEST-1".to_string(),
            18,
        );

        let display = token.to_string();
        assert_eq!(
            display,
            "Token(symbol=TEST-1, name=Test Token (with parentheses))"
        );
    }
}
