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

use nautilus_model::defi::{Blockchain, Chain, DexType};

use crate::exchanges::{
    arbitrum::ARBITRUM_DEX_EXTENDED_MAP, base::BASE_DEX_EXTENDED_MAP,
    ethereum::ETHEREUM_DEX_EXTENDED_MAP, extended::DexExtended,
};

pub mod arbitrum;
pub mod base;
pub mod ethereum;
pub mod extended;
mod parsing;

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
