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

//! WebSocket subscription management.

use std::collections::HashSet;

use crate::common::enums::GateioWsChannel;

/// Manages WebSocket channel subscriptions.
#[derive(Debug, Default)]
pub struct SubscriptionManager {
    subscriptions: HashSet<String>,
}

impl SubscriptionManager {
    /// Creates a new subscription manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            subscriptions: HashSet::new(),
        }
    }

    /// Adds a subscription.
    pub fn subscribe(&mut self, channel: &GateioWsChannel) {
        self.subscriptions.insert(channel.to_string());
    }

    /// Removes a subscription.
    pub fn unsubscribe(&mut self, channel: &GateioWsChannel) {
        self.subscriptions.remove(&channel.to_string());
    }

    /// Checks if a channel is subscribed.
    #[must_use]
    pub fn is_subscribed(&self, channel: &GateioWsChannel) -> bool {
        self.subscriptions.contains(&channel.to_string())
    }

    /// Returns the number of subscriptions.
    #[must_use]
    pub fn count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Returns all active subscriptions.
    #[must_use]
    pub fn subscriptions(&self) -> Vec<String> {
        self.subscriptions.iter().cloned().collect()
    }

    /// Clears all subscriptions.
    pub fn clear(&mut self) {
        self.subscriptions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_manager() {
        let mut manager = SubscriptionManager::new();
        let channel = GateioWsChannel::SpotTicker {
            currency_pair: "BTC_USDT".to_string(),
        };

        assert_eq!(manager.count(), 0);
        assert!(!manager.is_subscribed(&channel));

        manager.subscribe(&channel);
        assert_eq!(manager.count(), 1);
        assert!(manager.is_subscribed(&channel));

        manager.unsubscribe(&channel);
        assert_eq!(manager.count(), 0);
        assert!(!manager.is_subscribed(&channel));
    }

    #[test]
    fn test_multiple_subscriptions() {
        let mut manager = SubscriptionManager::new();
        let channel1 = GateioWsChannel::SpotTicker {
            currency_pair: "BTC_USDT".to_string(),
        };
        let channel2 = GateioWsChannel::SpotTrades {
            currency_pair: "ETH_USDT".to_string(),
        };

        manager.subscribe(&channel1);
        manager.subscribe(&channel2);
        assert_eq!(manager.count(), 2);
    }

    #[test]
    fn test_clear() {
        let mut manager = SubscriptionManager::new();
        let channel = GateioWsChannel::SpotTicker {
            currency_pair: "BTC_USDT".to_string(),
        };

        manager.subscribe(&channel);
        assert_eq!(manager.count(), 1);

        manager.clear();
        assert_eq!(manager.count(), 0);
    }
}
