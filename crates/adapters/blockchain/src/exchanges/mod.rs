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

use std::collections::HashMap;

use crate::exchanges::extended::DexExtended;

pub mod arbitrum;
pub mod base;
pub mod ethereum;
pub mod extended;

/// Returns a vector of all Dexes instances across all chains
#[must_use]
pub fn all_dex() -> Vec<&'static DexExtended> {
    let mut dexes = Vec::new();
    dexes.extend(arbitrum::all());
    dexes.extend(base::all());
    dexes.extend(ethereum::all());
    dexes
}

/// Returns a map of all DEX names to Dex instances across all chains
#[must_use]
pub fn dex_extended_map() -> HashMap<&'static str, &'static DexExtended> {
    let mut map = HashMap::new();
    map.extend(arbitrum::dex_map());
    map.extend(base::dex_map());
    map.extend(ethereum::dex_map());
    map
}
