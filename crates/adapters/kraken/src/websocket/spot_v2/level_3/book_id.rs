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

//! Stable venue order-id → u64 mapping for L3 book orders.

use std::hash::{BuildHasher, Hasher};

/// Maps Kraken venue order-id strings to stable `u64` values for `BookOrder.order_id`.
///
/// Kraken WS v2 uses alphanumeric venue IDs (e.g. `O6ZQNQ-BXL4E-5WGINO`).
/// Uses fixed-seed `ahash` for cross-process determinism (replay/backtest safe).
pub(crate) struct BookOrderIdHasher(ahash::RandomState);

impl BookOrderIdHasher {
    /// Creates a new hasher with deterministic fixed seeds.
    pub(crate) fn new() -> Self {
        Self(ahash::RandomState::with_seeds(
            0x4B52_414B_454E_5F4C,
            0x3330_5F4F_5244_4552,
            0x4944_5F48_4153_4831,
            0x5631_305F_5F5F_5F5F,
        ))
    }

    /// Maps `venue_id` to a stable `u64` via fixed-seed hash.
    pub(crate) fn hash(&self, venue_id: &str) -> u64 {
        let mut h = self.0.build_hasher();
        h.write(venue_id.as_bytes());
        h.finish()
    }
}

impl Default for BookOrderIdHasher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_alphanumeric_id_is_stable_across_calls() {
        let hasher = BookOrderIdHasher::new();
        let id = "O6ZQNQ-BXL4E-5WGINO";
        assert_eq!(hasher.hash(id), hasher.hash(id));
    }

    #[rstest]
    fn test_different_ids_produce_different_hashes() {
        let hasher = BookOrderIdHasher::new();
        assert_ne!(hasher.hash("order-a"), hasher.hash("order-b"));
    }

    #[rstest]
    fn test_two_instances_same_seeds_agree() {
        let h1 = BookOrderIdHasher::new();
        let h2 = BookOrderIdHasher::new();
        assert_eq!(
            h1.hash("some-venue-order-id"),
            h2.hash("some-venue-order-id")
        );
    }
}
