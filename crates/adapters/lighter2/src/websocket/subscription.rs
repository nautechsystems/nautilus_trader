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

//! Subscription management for Lighter WebSocket client.

use std::collections::HashSet;

use crate::common::enums::LighterWsChannel;

/// Manages WebSocket subscriptions.
#[derive(Debug, Clone, Default)]
pub struct SubscriptionManager {
    /// Set of confirmed subscriptions.
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
    pub fn add(&mut self, channel: LighterWsChannel) {
        self.subscriptions.insert(channel.to_string());
    }

    /// Removes a subscription.
    pub fn remove(&mut self, channel: &LighterWsChannel) {
        self.subscriptions.remove(&channel.to_string());
    }

    /// Checks if a channel is subscribed.
    #[must_use]
    pub fn is_subscribed(&self, channel: &LighterWsChannel) -> bool {
        self.subscriptions.contains(&channel.to_string())
    }

    /// Returns the number of active subscriptions.
    #[must_use]
    pub fn count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Clears all subscriptions.
    pub fn clear(&mut self) {
        self.subscriptions.clear();
    }

    /// Returns all subscription keys.
    #[must_use]
    pub fn get_all(&self) -> Vec<String> {
        self.subscriptions.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_manager() {
        let mut manager = SubscriptionManager::new();
        let channel = LighterWsChannel::OrderBook { market_id: 0 };

        assert_eq!(manager.count(), 0);

        manager.add(channel.clone());
        assert_eq!(manager.count(), 1);
        assert!(manager.is_subscribed(&channel));

        manager.remove(&channel);
        assert_eq!(manager.count(), 0);
        assert!(!manager.is_subscribed(&channel));
    }
}
