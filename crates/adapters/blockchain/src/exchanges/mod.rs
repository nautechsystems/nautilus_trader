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

/// Chains with a registered DEX map, so `sync-dex` and `analyze-pool(s)` can operate on them.
///
/// Other chains (for example Polygon) are valid for block-level `sync-blocks` but have no DEX
/// registrations, so DEX commands reject them.
pub const DEX_SUPPORTED_CHAINS: [Blockchain; 4] = [
    Blockchain::Ethereum,
    Blockchain::Base,
    Blockchain::Arbitrum,
    Blockchain::Bsc,
];

/// The capability tiers a registered DEX reaches on a chain, derived from its parser presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DexCapability {
    /// The DEX type.
    pub dex_type: DexType,
    /// Whether `sync-dex` can discover pools (a HyperSync `PoolCreated` parser is registered).
    pub discovery: bool,
    /// Whether `analyze-pool(s)` can build snapshots (the full pool-event parser set is registered).
    pub snapshots: bool,
    /// Whether replay keeps `fee_protocol` correct (snapshot-capable plus a `SetFeeProtocol` parser).
    pub replay_ready: bool,
}

/// Returns the capability tier of every DEX registered on `blockchain`, sorted by DEX name.
///
/// Returns an empty vector for chains without a DEX map (see [`DEX_SUPPORTED_CHAINS`]).
#[must_use]
pub fn dex_capabilities_for_chain(blockchain: Blockchain) -> Vec<DexCapability> {
    let mut dex_types: Vec<DexType> = match blockchain {
        Blockchain::Ethereum => ETHEREUM_DEX_EXTENDED_MAP.keys().copied().collect(),
        Blockchain::Base => BASE_DEX_EXTENDED_MAP.keys().copied().collect(),
        Blockchain::Arbitrum => ARBITRUM_DEX_EXTENDED_MAP.keys().copied().collect(),
        Blockchain::Bsc => BSC_DEX_EXTENDED_MAP.keys().copied().collect(),
        _ => return Vec::new(),
    };
    dex_types.sort_by_key(ToString::to_string);

    dex_types
        .into_iter()
        .filter_map(|dex_type| {
            let dex = get_dex_extended(blockchain, &dex_type)?;
            Some(DexCapability {
                dex_type,
                discovery: dex.supports_pool_discovery(),
                snapshots: dex.missing_pool_analysis_parsers().is_empty(),
                replay_ready: dex.supports_fee_protocol_replay(),
            })
        })
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
    use nautilus_model::defi::pool_analysis::PoolEventKind;
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
    const KNOWN_PARSER_GAPS: &[(Blockchain, DexType, &str)] = &[];

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

        if dex.fee_protocol_update_event.is_some() {
            record(
                "SetFeeProtocol",
                dex_extended
                    .parse_fee_protocol_update_event_hypersync_fn
                    .is_some(),
            );
        }

        if dex.fee_protocol_collect_event.is_some() {
            record(
                "CollectProtocol",
                dex_extended
                    .parse_fee_protocol_collect_event_hypersync_fn
                    .is_some(),
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

    #[rstest]
    #[case(Blockchain::Bsc)]
    #[case(Blockchain::Base)]
    #[case(Blockchain::Arbitrum)]
    #[case(Blockchain::Ethereum)]
    fn test_pancakeswap_v3_supports_pool_analysis(#[case] blockchain: Blockchain) {
        let dex_extended = get_dex_extended(blockchain, &DexType::PancakeSwapV3)
            .expect("PancakeSwapV3 should be registered");
        assert!(dex_extended.supports_pool_discovery());
        assert!(
            dex_extended.missing_pool_analysis_parsers().is_empty(),
            "PancakeSwapV3 on {blockchain:?} is missing analysis parsers: {:?}",
            dex_extended.missing_pool_analysis_parsers()
        );
    }

    #[rstest]
    #[case(Blockchain::Ethereum, DexType::UniswapV3, true, true, true)]
    #[case(Blockchain::Base, DexType::UniswapV3, true, true, true)]
    #[case(Blockchain::Bsc, DexType::PancakeSwapV3, true, true, false)]
    #[case(Blockchain::Base, DexType::AerodromeSlipstream, false, true, false)]
    #[case(Blockchain::Ethereum, DexType::UniswapV2, true, false, false)]
    #[case(Blockchain::Arbitrum, DexType::SushiSwapV2, false, false, false)]
    fn test_dex_capability_tiers(
        #[case] blockchain: Blockchain,
        #[case] dex_type: DexType,
        #[case] discovery: bool,
        #[case] snapshots: bool,
        #[case] replay_ready: bool,
    ) {
        let capability = dex_capabilities_for_chain(blockchain)
            .into_iter()
            .find(|c| c.dex_type == dex_type)
            .unwrap_or_else(|| panic!("{dex_type:?} should be registered on {blockchain:?}"));

        assert_eq!(capability.discovery, discovery);
        assert_eq!(capability.snapshots, snapshots);
        assert_eq!(capability.replay_ready, replay_ready);
    }

    #[rstest]
    fn test_dex_capabilities_empty_for_chain_without_map() {
        assert!(dex_capabilities_for_chain(Blockchain::Polygon).is_empty());
    }

    #[rstest]
    fn test_dex_capabilities_sorted_by_name() {
        let names: Vec<String> = dex_capabilities_for_chain(Blockchain::Arbitrum)
            .iter()
            .map(|c| c.dex_type.to_string())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();

        assert_eq!(names, sorted);
    }

    #[rstest]
    fn test_dex_without_parsers_reports_unsupported() {
        // SushiSwapV3 on Arbitrum is registered for the pool set but has no event parsers.
        let dex_extended = get_dex_extended(Blockchain::Arbitrum, &DexType::SushiSwapV3)
            .expect("SushiSwapV3 should be registered on Arbitrum");
        assert!(!dex_extended.supports_pool_discovery());
        assert_eq!(
            dex_extended.missing_pool_analysis_parsers(),
            vec![
                PoolEventKind::Initialize,
                PoolEventKind::Swap,
                PoolEventKind::Mint,
                PoolEventKind::Burn,
                PoolEventKind::Collect,
            ]
        );
    }
}
