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

use alloy::primitives::B256;

pub mod client;
pub mod core;
pub mod subscription;

// An event without an observed hash carries no constraint and always matches.
pub(crate) fn block_hash_matches(observed: Option<B256>, cached: B256) -> bool {
    observed.is_none_or(|hash| hash == cached)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;

    fn fixture_hash(hash: &str) -> B256 {
        B256::from_str(hash).expect("fixture block hash must be valid")
    }

    #[rstest]
    #[case(
        None,
        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        true
    )]
    #[case(
        Some("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        true
    )]
    #[case(
        Some("0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        true
    )]
    #[case(
        Some("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        true
    )]
    #[case(
        Some("0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"),
        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        false
    )]
    fn test_block_hash_matches(
        #[case] observed: Option<&str>,
        #[case] cached: &str,
        #[case] expected: bool,
    ) {
        let observed = observed.map(fixture_hash);
        assert_eq!(block_hash_matches(observed, fixture_hash(cached)), expected);
    }
}
