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

use std::sync::Arc;

use nautilus_blockchain::execution::metadata_store::{
    InMemoryMetadataStore, PoolMetadataStore, TokenMetadataStore,
};
use nautilus_core::UnixNanos;
use nautilus_model::defi::{
    AmmType, Dex, DexType, Pool, PoolIdentifier, Token, chain::chains, validation::validate_address,
};

fn make_token(address: &str, symbol: &str, decimals: u8) -> Token {
    Token::new(
        Arc::new(chains::ARBITRUM.clone()),
        validate_address(address).expect("token address should be valid"),
        symbol.to_string(),
        symbol.to_string(),
        decimals,
    )
}

fn make_pool() -> Pool {
    let chain = Arc::new(chains::ARBITRUM.clone());
    let pool_address =
        validate_address("0xd13040d4fe917EE704158CfCB3338dCd2838B245").expect("valid pool");

    let dex = Arc::new(Dex::new(
        (*chain).clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated(address,address,uint24,int24,address)",
        "Swap(address,address,int256,int256,uint160,uint128,int24)",
        "Mint(address,address,int24,int24,uint128,uint256,uint256)",
        "Burn(address,int24,int24,uint128,uint256,uint256)",
        "Collect(address,address,int24,int24,uint128,uint128)",
    ));

    let token0 = make_token("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1", "WETH", 18);
    let token1 = make_token("0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8", "USDC", 6);

    Pool::new(
        chain,
        dex,
        pool_address,
        PoolIdentifier::from_address(pool_address),
        0,
        token0,
        token1,
        Some(100),
        Some(1),
        UnixNanos::default(),
    )
}

#[test]
fn in_memory_store_roundtrips_token_metadata() {
    let mut store = InMemoryMetadataStore::new();
    let token = make_token("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1", "WETH", 18);

    assert!(store.get_token(&token.address).is_none());
    store.insert_token(token.clone());

    let stored = store
        .get_token(&token.address)
        .expect("token metadata should be stored");

    assert_eq!(stored.address, token.address);
    assert_eq!(stored.symbol, token.symbol);
    assert_eq!(stored.decimals, token.decimals);
}

#[test]
fn in_memory_store_roundtrips_pool_metadata() {
    let mut store = InMemoryMetadataStore::new();
    let pool = make_pool();
    let pool_identifier = pool.pool_identifier;

    assert!(store.get_pool(&pool_identifier).is_none());
    store.insert_pool(pool.clone());

    let stored = store
        .get_pool(&pool_identifier)
        .expect("pool metadata should be stored");

    assert_eq!(stored.pool_identifier, pool.pool_identifier);
    assert_eq!(stored.token0.address, pool.token0.address);
    assert_eq!(stored.token1.address, pool.token1.address);
}
