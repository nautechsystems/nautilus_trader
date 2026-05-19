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

use nautilus_model::defi::{Blockchain, Chain, DexType};

use crate::exchanges::{
    arbitrum::ARBITRUM_DEX_EXTENDED_MAP, base::BASE_DEX_EXTENDED_MAP, bsc::BSC_DEX_EXTENDED_MAP,
    ethereum::ETHEREUM_DEX_EXTENDED_MAP, extended::DexExtended,
};

pub mod arbitrum;
pub mod base;
pub mod bsc;
pub mod ethereum;
pub mod extended;
pub mod parsing;

/// Returns a map of all DEX names to Dex instances across all chains
#[must_use]
pub fn get_dex_extended(
    blockchain: Blockchain,
    dex_type: &DexType,
) -> Option<&'static DexExtended> {
    match blockchain {
        Blockchain::Ethereum => ETHEREUM_DEX_EXTENDED_MAP.get(dex_type).copied(),
        Blockchain::Base => BASE_DEX_EXTENDED_MAP.get(dex_type).copied(),
        Blockchain::Arbitrum => ARBITRUM_DEX_EXTENDED_MAP.get(dex_type).copied(),
        Blockchain::Bsc => BSC_DEX_EXTENDED_MAP.get(dex_type).copied(),
        _ => None,
    }
}

/// Returns the supported DEX names for a given blockchain.
#[must_use]
pub fn get_supported_dexes_for_chain(blockchain: Blockchain) -> Vec<String> {
    let dex_types: Vec<DexType> = match blockchain {
        Blockchain::Ethereum => ETHEREUM_DEX_EXTENDED_MAP.keys().copied().collect(),
        Blockchain::Base => BASE_DEX_EXTENDED_MAP.keys().copied().collect(),
        Blockchain::Arbitrum => ARBITRUM_DEX_EXTENDED_MAP.keys().copied().collect(),
        Blockchain::Bsc => BSC_DEX_EXTENDED_MAP.keys().copied().collect(),
        _ => vec![],
    };

    dex_types
        .into_iter()
        .map(|dex_type| format!("{dex_type}"))
        .collect()
}

/// Attempts to match a DEX name in a case-insensitive manner.
pub fn find_dex_type_case_insensitive(dex_name: &str, chain: &Chain) -> Option<DexType> {
    let supported_dexes = get_supported_dexes_for_chain(chain.name);

    // First try exact match (for performance)
    if let Some(dex_type) = DexType::from_dex_name(dex_name) {
        return Some(dex_type);
    }

    // Try case-insensitive match
    for supported_dex in supported_dexes {
        if supported_dex.to_lowercase() == dex_name.to_lowercase() {
            return DexType::from_dex_name(&supported_dex);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use alloy::primitives::keccak256;
    use nautilus_core::hex;
    use rstest::rstest;

    use super::*;

    /// keccak256 of the empty string, used as the sentinel hash when a DEX
    /// registration leaves an event signature blank.
    fn empty_signature_hash() -> String {
        hex::encode_prefixed(keccak256("".as_bytes()))
    }

    /// Pre-existing (chain, dex, event) parser gaps that predate the structured-error
    /// patch. New gaps must not be added without also wiring the parser; legacy gaps
    /// belong to a separate cleanup.
    const KNOWN_PARSER_GAPS: &[(Blockchain, DexType, &str)] = &[
        (Blockchain::Base, DexType::UniswapV3, "Swap"),
        (Blockchain::Base, DexType::UniswapV3, "Mint"),
        (Blockchain::Base, DexType::UniswapV3, "Burn"),
        (Blockchain::Base, DexType::UniswapV3, "Collect"),
    ];

    fn is_known_gap(blockchain: Blockchain, dex_type: DexType, event: &str) -> bool {
        KNOWN_PARSER_GAPS
            .iter()
            .any(|(b, d, e)| *b == blockchain && *d == dex_type && *e == event)
    }

    fn collect_parity_gaps(blockchain: Blockchain, dex_type: DexType, gaps: &mut Vec<String>) {
        let dex_extended = get_dex_extended(blockchain, &dex_type).unwrap_or_else(|| {
            panic!("{blockchain:?}:{dex_type:?} should be registered in the DEX map")
        });
        let empty = empty_signature_hash();
        let dex = &dex_extended.dex;

        let mut record = |event: &str, has_parser: bool| {
            if !has_parser && !is_known_gap(blockchain, dex_type, event) {
                gaps.push(format!(
                    "{blockchain:?}:{dex_type:?} advertises {event} but has no HyperSync parser"
                ));
            }
        };

        if dex.pool_created_event.as_ref() != empty {
            record(
                "PoolCreated",
                dex_extended.parse_pool_created_event_hypersync_fn.is_some(),
            );
        }

        if dex.swap_created_event.as_ref() != empty {
            record("Swap", dex_extended.parse_swap_event_hypersync_fn.is_some());
        }

        if dex.mint_created_event.as_ref() != empty {
            record("Mint", dex_extended.parse_mint_event_hypersync_fn.is_some());
        }

        if dex.burn_created_event.as_ref() != empty {
            record("Burn", dex_extended.parse_burn_event_hypersync_fn.is_some());
        }

        if dex.collect_created_event.as_ref() != empty {
            record(
                "Collect",
                dex_extended.parse_collect_event_hypersync_fn.is_some(),
            );
        }

        if dex.initialize_event.is_some() {
            record(
                "Initialize",
                dex_extended.parse_initialize_event_hypersync_fn.is_some(),
            );
        }

        if dex.flash_created_event.is_some() {
            record(
                "Flash",
                dex_extended.parse_flash_event_hypersync_fn.is_some(),
            );
        }
    }

    #[rstest]
    #[case(Blockchain::Ethereum)]
    #[case(Blockchain::Base)]
    #[case(Blockchain::Arbitrum)]
    #[case(Blockchain::Bsc)]
    fn test_dex_signature_parser_parity_for_chain(#[case] blockchain: Blockchain) {
        let dex_types: Vec<DexType> = match blockchain {
            Blockchain::Ethereum => ETHEREUM_DEX_EXTENDED_MAP.keys().copied().collect(),
            Blockchain::Base => BASE_DEX_EXTENDED_MAP.keys().copied().collect(),
            Blockchain::Arbitrum => ARBITRUM_DEX_EXTENDED_MAP.keys().copied().collect(),
            Blockchain::Bsc => BSC_DEX_EXTENDED_MAP.keys().copied().collect(),
            _ => panic!("unsupported chain in test"),
        };
        assert!(
            !dex_types.is_empty(),
            "{blockchain:?} should register at least one DEX"
        );

        let mut gaps = Vec::new();
        for dex_type in dex_types {
            collect_parity_gaps(blockchain, dex_type, &mut gaps);
        }
        assert!(
            gaps.is_empty(),
            "DEX parser parity violations for {blockchain:?}:\n{}",
            gaps.join("\n")
        );
    }

    #[rstest]
    #[case(Blockchain::Bsc, DexType::UniswapV3)]
    #[case(Blockchain::Bsc, DexType::PancakeSwapV3)]
    fn test_bsc_dispatch_returns_registered_dex(
        #[case] blockchain: Blockchain,
        #[case] dex_type: DexType,
    ) {
        let dex_extended = get_dex_extended(blockchain, &dex_type)
            .expect("BSC dispatch should return the registered DEX");
        assert_eq!(dex_extended.dex.chain.name, blockchain);
        assert_eq!(dex_extended.dex.name, dex_type);
    }
}
