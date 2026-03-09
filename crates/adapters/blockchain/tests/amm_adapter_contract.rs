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

use alloy::primitives::{U256, address};
use nautilus_blockchain::{
    contracts::pancakeswap_v2_router::{
        PancakeSwapV2RouterContract, SELECTOR_GET_AMOUNTS_IN, SELECTOR_GET_AMOUNTS_OUT,
        SELECTOR_SWAP_EXACT_TOKENS_FOR_TOKENS, SELECTOR_SWAP_TOKENS_FOR_EXACT_TOKENS,
    },
    execution::amm::{
        AmmAdapterRegistry, AmmProtocolAdapter, AmmRegistryError,
        pancakeswap_v2::PancakeSwapV2Adapter,
    },
    rpc::http::BlockchainHttpRpcClient,
};
use nautilus_model::defi::DexType;

fn make_adapter() -> PancakeSwapV2Adapter {
    let client = Arc::new(BlockchainHttpRpcClient::new(
        String::from("https://bsc.example.com"),
        None,
    ));
    let router = address!("0x10ED43C718714eb63d5aA57B78B54704E256024E");
    let wallet = address!("0x3333333333333333333333333333333333333333");
    PancakeSwapV2Adapter::new(client, router, wallet)
}

#[test]
fn test_pancakeswap_v2_adapter_capabilities_match_mvp() {
    let adapter = make_adapter();
    let capabilities = adapter.capabilities();

    assert!(capabilities.supports_quote_exact_in);
    assert!(capabilities.supports_quote_exact_out);
    assert!(capabilities.supports_single_hop);
    assert!(!capabilities.supports_multi_hop);
    assert!(capabilities.supports_deadline_arg);
    assert!(!capabilities.supports_recipient_override);
    assert!(capabilities.swap_call_returns_amounts);
}

#[test]
fn test_pancakeswap_v2_adapter_selector_constants_match_expected() {
    let path = vec![
        address!("0x1111111111111111111111111111111111111111"),
        address!("0x2222222222222222222222222222222222222222"),
    ];

    let get_out = PancakeSwapV2RouterContract::encode_get_amounts_out_call(
        U256::from(1_000u64),
        path.clone(),
    )
    .expect("encoding should succeed");
    let get_in =
        PancakeSwapV2RouterContract::encode_get_amounts_in_call(U256::from(1_000u64), path.clone())
            .expect("encoding should succeed");
    let swap_exact = PancakeSwapV2RouterContract::encode_swap_exact_tokens_for_tokens_call(
        U256::from(1_000u64),
        U256::from(900u64),
        path.clone(),
        address!("0x3333333333333333333333333333333333333333"),
        U256::from(1_234_567u64),
    )
    .expect("encoding should succeed");
    let swap_for_exact = PancakeSwapV2RouterContract::encode_swap_tokens_for_exact_tokens_call(
        U256::from(900u64),
        U256::from(1_000u64),
        path,
        address!("0x3333333333333333333333333333333333333333"),
        U256::from(1_234_567u64),
    )
    .expect("encoding should succeed");

    assert_eq!(&get_out[..4], SELECTOR_GET_AMOUNTS_OUT.as_slice());
    assert_eq!(&get_in[..4], SELECTOR_GET_AMOUNTS_IN.as_slice());
    assert_eq!(
        &swap_exact[..4],
        SELECTOR_SWAP_EXACT_TOKENS_FOR_TOKENS.as_slice()
    );
    assert_eq!(
        &swap_for_exact[..4],
        SELECTOR_SWAP_TOKENS_FOR_EXACT_TOKENS.as_slice()
    );
}

#[test]
fn test_amm_registry_returns_adapter_for_pancakeswap_v2() {
    let mut registry = AmmAdapterRegistry::new();
    registry
        .register(Arc::new(make_adapter()))
        .expect("registration should succeed");

    let adapter = registry
        .get(DexType::PancakeSwapV2)
        .expect("adapter should exist");

    assert_eq!(adapter.dex_type(), DexType::PancakeSwapV2);
}

#[test]
fn test_amm_registry_unknown_dex_type_returns_error() {
    let registry = AmmAdapterRegistry::new();

    let err = registry
        .get(DexType::UniswapV3)
        .expect_err("unknown adapter should fail");

    assert_eq!(err, AmmRegistryError::AdapterNotFound(DexType::UniswapV3));
}

#[test]
fn test_amm_registry_duplicate_registration_fails_fast() {
    let mut registry = AmmAdapterRegistry::new();
    registry
        .register(Arc::new(make_adapter()))
        .expect("initial registration should succeed");

    let err = registry
        .register(Arc::new(make_adapter()))
        .expect_err("duplicate registration should fail");

    assert_eq!(
        err,
        AmmRegistryError::DuplicateRegistration(DexType::PancakeSwapV2)
    );
}
