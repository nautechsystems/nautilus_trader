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

use ahash::{AHashMap, AHashSet};
use alloy::primitives::{Address, keccak256};
use nautilus_model::defi::DexType;

/// Manages subscriptions to DeFi protocol events (swaps, mints, burns) across different DEXs.
///
/// This manager tracks which pool addresses are subscribed for each event type
/// and maintains the event signature encodings for efficient filtering.
#[derive(Debug)]
pub struct DefiDataSubscriptionManager {
    subscribed_pool_swaps: AHashMap<DexType, AHashSet<Address>>,
    pool_swap_event_encoded: AHashMap<DexType, String>,
    subscribed_pool_mints: AHashMap<DexType, AHashSet<Address>>,
    pool_mint_event_encoded: AHashMap<DexType, String>,
    subscribed_pool_burns: AHashMap<DexType, AHashSet<Address>>,
    pool_burn_event_encoded: AHashMap<DexType, String>,
}

impl Default for DefiDataSubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DefiDataSubscriptionManager {
    /// Creates a new [`DefiDataSubscriptionManager`] instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            subscribed_pool_burns: AHashMap::new(),
            subscribed_pool_mints: AHashMap::new(),
            subscribed_pool_swaps: AHashMap::new(),
            pool_swap_event_encoded: AHashMap::new(),
            pool_burn_event_encoded: AHashMap::new(),
            pool_mint_event_encoded: AHashMap::new(),
        }
    }

    /// Gets all unique contract addresses subscribed for any event type for a given DEX.
    #[must_use]
    pub fn get_subscribed_dex_contract_addresses(&self, dex: &DexType) -> Vec<Address> {
        let mut unique_addresses = AHashSet::new();

        if let Some(addresses) = self.subscribed_pool_swaps.get(dex) {
            unique_addresses.extend(addresses.iter().copied());
        }
        if let Some(addresses) = self.subscribed_pool_mints.get(dex) {
            unique_addresses.extend(addresses.iter().copied());
        }
        if let Some(addresses) = self.subscribed_pool_burns.get(dex) {
            unique_addresses.extend(addresses.iter().copied());
        }

        unique_addresses.into_iter().collect()
    }

    /// Gets all event signatures (keccak256 hashes) registered for a given DEX.
    #[must_use]
    pub fn get_subscribed_dex_event_signatures(&self, dex: &DexType) -> Vec<String> {
        let mut result = Vec::new();

        if let Some(swap_event_signature) = self.pool_swap_event_encoded.get(dex) {
            result.push(swap_event_signature.clone());
        }
        if let Some(mint_event_signature) = self.pool_mint_event_encoded.get(dex) {
            result.push(mint_event_signature.clone());
        }
        if let Some(burn_event_signature) = self.pool_burn_event_encoded.get(dex) {
            result.push(burn_event_signature.clone());
        }

        result
    }

    /// Gets the swap event signature for a specific DEX.
    #[must_use]
    pub fn get_dex_pool_swap_event_signature(&self, dex: &DexType) -> Option<String> {
        self.pool_swap_event_encoded.get(dex).cloned()
    }

    /// Gets the mint event signature for a specific DEX.
    #[must_use]
    pub fn get_dex_pool_mint_event_signature(&self, dex: &DexType) -> Option<String> {
        self.pool_mint_event_encoded.get(dex).cloned()
    }
    /// Gets the burn event signature for a specific DEX.
    #[must_use]
    pub fn get_dex_pool_burn_event_signature(&self, dex: &DexType) -> Option<String> {
        self.pool_burn_event_encoded.get(dex).cloned()
    }

    /// Registers a DEX with its event signatures for subscription management.
    ///
    /// This must be called before subscribing to any events for a DEX.
    /// The event signatures are hashed using keccak256 and stored in encoded format.
    pub fn register_dex_for_subscriptions(
        &mut self,
        dex: DexType,
        swap_event_signature: &str,
        mint_event_signature: &str,
        burn_event_signature: &str,
    ) {
        self.subscribed_pool_swaps.insert(dex, AHashSet::new());
        let swap_event_hash = keccak256(swap_event_signature.as_bytes());
        let encoded_swap_event = format!(
            "0x{encoded_hash}",
            encoded_hash = hex::encode(swap_event_hash)
        );
        self.pool_swap_event_encoded.insert(dex, encoded_swap_event);

        self.subscribed_pool_mints.insert(dex, AHashSet::new());
        let mint_event_hash = keccak256(mint_event_signature.as_bytes());
        let encoded_mint_event = format!(
            "0x{encoded_hash}",
            encoded_hash = hex::encode(mint_event_hash)
        );
        self.pool_mint_event_encoded.insert(dex, encoded_mint_event);

        self.subscribed_pool_burns.insert(dex, AHashSet::new());
        let burn_event_hash = keccak256(burn_event_signature.as_bytes());
        let encoded_burn_event = format!(
            "0x{encoded_hash}",
            encoded_hash = hex::encode(burn_event_hash)
        );
        self.pool_burn_event_encoded.insert(dex, encoded_burn_event);

        tracing::info!("Registered DEX for subscriptions: {dex:?}");
    }

    /// Subscribes to swap events for a specific pool address on a DEX.
    pub fn subscribe_swaps(&mut self, dex: DexType, address: Address) {
        if let Some(pool_set) = self.subscribed_pool_swaps.get_mut(&dex) {
            pool_set.insert(address);
        } else {
            tracing::error!("DEX not registered for swap subscriptions: {dex:?}");
        }
    }

    /// Subscribes to mint events for a specific pool address on a DEX.
    pub fn subscribe_mints(&mut self, dex: DexType, address: Address) {
        if let Some(pool_set) = self.subscribed_pool_mints.get_mut(&dex) {
            pool_set.insert(address);
        } else {
            tracing::error!("DEX not registered for mint subscriptions: {dex:?}");
        }
    }

    /// Subscribes to burn events for a specific pool address on a DEX.
    pub fn subscribe_burns(&mut self, dex: DexType, address: Address) {
        if let Some(pool_set) = self.subscribed_pool_burns.get_mut(&dex) {
            pool_set.insert(address);
        } else {
            tracing::warn!("DEX not registered for burn subscriptions: {dex:?}");
        }
    }

    /// Unsubscribes from swap events for a specific pool address on a DEX.
    pub fn unsubscribe_swaps(&mut self, dex: DexType, address: Address) {
        if let Some(pool_set) = self.subscribed_pool_swaps.get_mut(&dex) {
            pool_set.remove(&address);
        } else {
            tracing::error!("DEX not registered for swap subscriptions: {dex:?}");
        }
    }

    /// Unsubscribes from mint events for a specific pool address on a DEX.
    pub fn unsubscribe_mints(&mut self, dex: DexType, address: Address) {
        if let Some(pool_set) = self.subscribed_pool_mints.get_mut(&dex) {
            pool_set.remove(&address);
        } else {
            tracing::error!("DEX not registered for mint subscriptions: {dex:?}");
        }
    }

    /// Unsubscribes from burn events for a specific pool address on a DEX.
    pub fn unsubscribe_burns(&mut self, dex: DexType, address: Address) {
        if let Some(pool_set) = self.subscribed_pool_burns.get_mut(&dex) {
            pool_set.remove(&address);
        } else {
            tracing::error!("DEX not registered for burn subscriptions: {dex:?}");
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use alloy::primitives::address;
    use nautilus_model::defi::DexType;
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn manager() -> DefiDataSubscriptionManager {
        DefiDataSubscriptionManager::new()
    }

    #[fixture]
    fn registered_manager() -> DefiDataSubscriptionManager {
        let mut manager = DefiDataSubscriptionManager::new();
        manager.register_dex_for_subscriptions(
            DexType::UniswapV3,
            "Swap(address,address,int256,int256,uint160,uint128,int24)",
            "Mint(address,address,int24,int24,uint128,uint256,uint256)",
            "Burn(address,int24,int24,uint128,uint256,uint256)",
        );
        manager
    }

    #[rstest]
    fn test_new_creates_empty_manager(manager: DefiDataSubscriptionManager) {
        assert_eq!(
            manager
                .get_subscribed_dex_contract_addresses(&DexType::UniswapV3)
                .len(),
            0
        );
        assert_eq!(
            manager
                .get_subscribed_dex_event_signatures(&DexType::UniswapV3)
                .len(),
            0
        );
        assert!(
            manager
                .get_dex_pool_swap_event_signature(&DexType::UniswapV3)
                .is_none()
        );
        assert!(
            manager
                .get_dex_pool_mint_event_signature(&DexType::UniswapV3)
                .is_none()
        );
        assert!(
            manager
                .get_dex_pool_burn_event_signature(&DexType::UniswapV3)
                .is_none()
        );
    }

    #[rstest]
    fn test_register_dex_for_subscriptions(registered_manager: DefiDataSubscriptionManager) {
        // Should have all three event signatures
        let signatures =
            registered_manager.get_subscribed_dex_event_signatures(&DexType::UniswapV3);
        assert_eq!(signatures.len(), 3);

        // Each signature should be properly encoded
        assert!(
            registered_manager
                .get_dex_pool_swap_event_signature(&DexType::UniswapV3)
                .is_some()
        );
        assert!(
            registered_manager
                .get_dex_pool_mint_event_signature(&DexType::UniswapV3)
                .is_some()
        );
        assert!(
            registered_manager
                .get_dex_pool_burn_event_signature(&DexType::UniswapV3)
                .is_some()
        );
    }

    #[rstest]
    fn test_subscribe_and_get_addresses(mut registered_manager: DefiDataSubscriptionManager) {
        let pool_address = address!("1234567890123456789012345678901234567890");

        // Subscribe to swap events
        registered_manager.subscribe_swaps(DexType::UniswapV3, pool_address);

        let addresses =
            registered_manager.get_subscribed_dex_contract_addresses(&DexType::UniswapV3);
        assert_eq!(addresses.len(), 1);
        assert_eq!(addresses[0], pool_address);
    }

    #[rstest]
    fn test_subscribe_to_unregistered_dex(mut manager: DefiDataSubscriptionManager) {
        let pool_address = address!("1234567890123456789012345678901234567890");

        // Try to subscribe without registering - should log warning but not panic
        manager.subscribe_swaps(DexType::UniswapV3, pool_address);
        manager.subscribe_mints(DexType::UniswapV3, pool_address);
        manager.subscribe_burns(DexType::UniswapV3, pool_address);

        // Should return empty results
        let addresses = manager.get_subscribed_dex_contract_addresses(&DexType::UniswapV3);
        assert_eq!(addresses.len(), 0);
    }

    #[rstest]
    fn test_unsubscribe_removes_address(mut registered_manager: DefiDataSubscriptionManager) {
        let pool_address = address!("1234567890123456789012345678901234567890");

        // Subscribe
        registered_manager.subscribe_swaps(DexType::UniswapV3, pool_address);

        // Verify subscription
        assert_eq!(
            registered_manager
                .get_subscribed_dex_contract_addresses(&DexType::UniswapV3)
                .len(),
            1
        );

        // Unsubscribe
        registered_manager.unsubscribe_swaps(DexType::UniswapV3, pool_address);

        // Verify removal
        assert_eq!(
            registered_manager
                .get_subscribed_dex_contract_addresses(&DexType::UniswapV3)
                .len(),
            0
        );
    }

    #[rstest]
    fn test_get_event_signatures(registered_manager: DefiDataSubscriptionManager) {
        let swap_sig = registered_manager.get_dex_pool_swap_event_signature(&DexType::UniswapV3);
        let mint_sig = registered_manager.get_dex_pool_mint_event_signature(&DexType::UniswapV3);
        let burn_sig = registered_manager.get_dex_pool_burn_event_signature(&DexType::UniswapV3);

        // All should be Some and start with 0x
        assert!(swap_sig.is_some() && swap_sig.unwrap().starts_with("0x"));
        assert!(mint_sig.is_some() && mint_sig.unwrap().starts_with("0x"));
        assert!(burn_sig.is_some() && burn_sig.unwrap().starts_with("0x"));
    }

    #[rstest]
    fn test_multiple_subscriptions_same_pool(mut registered_manager: DefiDataSubscriptionManager) {
        let pool_address = address!("1234567890123456789012345678901234567890");

        // Subscribe same address multiple times to same event type
        registered_manager.subscribe_swaps(DexType::UniswapV3, pool_address);
        registered_manager.subscribe_swaps(DexType::UniswapV3, pool_address);

        // Should only appear once (HashSet behavior)
        let addresses =
            registered_manager.get_subscribed_dex_contract_addresses(&DexType::UniswapV3);
        assert_eq!(addresses.len(), 1);
    }

    #[rstest]
    fn test_get_combined_addresses_from_all_events(
        mut registered_manager: DefiDataSubscriptionManager,
    ) {
        let pool1 = address!("1111111111111111111111111111111111111111");
        let pool2 = address!("2222222222222222222222222222222222222222");
        let pool3 = address!("3333333333333333333333333333333333333333");

        // Subscribe different pools to different events
        registered_manager.subscribe_swaps(DexType::UniswapV3, pool1);
        registered_manager.subscribe_mints(DexType::UniswapV3, pool2);
        registered_manager.subscribe_burns(DexType::UniswapV3, pool3);

        // Should get all unique addresses
        let addresses =
            registered_manager.get_subscribed_dex_contract_addresses(&DexType::UniswapV3);
        assert_eq!(addresses.len(), 3);
        assert!(addresses.contains(&pool1));
        assert!(addresses.contains(&pool2));
        assert!(addresses.contains(&pool3));
    }

    #[rstest]
    fn test_event_signature_encoding(registered_manager: DefiDataSubscriptionManager) {
        // Known event signature and its expected keccak256 hash
        // Swap(address,address,int256,int256,uint160,uint128,int24) for UniswapV3
        let swap_sig = registered_manager
            .get_dex_pool_swap_event_signature(&DexType::UniswapV3)
            .unwrap();

        // Should be properly formatted hex string
        assert!(swap_sig.starts_with("0x"));
        assert_eq!(swap_sig.len(), 66); // 0x + 64 hex chars (32 bytes)

        // Verify it's valid hex
        let hex_part = &swap_sig[2..];
        assert!(hex_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[rstest]
    #[case(DexType::UniswapV3)]
    #[case(DexType::UniswapV2)]
    fn test_complete_subscription_workflow(#[case] dex_type: DexType) {
        let mut manager = DefiDataSubscriptionManager::new();
        let pool1 = address!("1111111111111111111111111111111111111111");
        let pool2 = address!("2222222222222222222222222222222222222222");

        // Step 1: Register DEX
        manager.register_dex_for_subscriptions(
            dex_type,
            "Swap(address,uint256,uint256)",
            "Mint(address,uint256)",
            "Burn(address,uint256)",
        );

        // Step 2: Subscribe to events
        manager.subscribe_swaps(dex_type, pool1);
        manager.subscribe_swaps(dex_type, pool2);
        manager.subscribe_mints(dex_type, pool1);
        manager.subscribe_burns(dex_type, pool2);

        // Step 3: Verify subscriptions
        let addresses = manager.get_subscribed_dex_contract_addresses(&dex_type);
        assert_eq!(addresses.len(), 2);
        assert!(addresses.contains(&pool1));
        assert!(addresses.contains(&pool2));

        // Step 4: Get event signatures
        let signatures = manager.get_subscribed_dex_event_signatures(&dex_type);
        assert_eq!(signatures.len(), 3);

        // Step 5: Unsubscribe from some events
        manager.unsubscribe_swaps(dex_type, pool1);
        manager.unsubscribe_burns(dex_type, pool2);

        // Step 6: Verify remaining subscriptions (only pool1 mint remains)
        let remaining = manager.get_subscribed_dex_contract_addresses(&dex_type);
        assert!(remaining.contains(&pool1)); // Still has mint subscription
        assert!(remaining.contains(&pool2)); // Still has swap subscription
    }
}
