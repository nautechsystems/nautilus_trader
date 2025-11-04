use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::common::enums::AsterdexWsChannel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionStatus {
    Pending,
    Subscribed,
    Unsubscribed,
}

pub struct SubscriptionManager {
    subscriptions: Arc<RwLock<HashMap<AsterdexWsChannel, SubscriptionStatus>>>,
}

impl SubscriptionManager {
    pub fn new() -> Self {
        Self {
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_pending(&self, channel: AsterdexWsChannel) {
        let mut subs = self.subscriptions.write().await;
        subs.insert(channel, SubscriptionStatus::Pending);
    }

    pub async fn mark_subscribed(&self, channel: &AsterdexWsChannel) {
        let mut subs = self.subscriptions.write().await;
        if let Some(status) = subs.get_mut(channel) {
            *status = SubscriptionStatus::Subscribed;
        }
    }

    pub async fn mark_unsubscribed(&self, channel: &AsterdexWsChannel) {
        let mut subs = self.subscriptions.write().await;
        subs.remove(channel);
    }

    pub async fn get_status(&self, channel: &AsterdexWsChannel) -> Option<SubscriptionStatus> {
        let subs = self.subscriptions.read().await;
        subs.get(channel).copied()
    }

    pub async fn is_subscribed(&self, channel: &AsterdexWsChannel) -> bool {
        matches!(
            self.get_status(channel).await,
            Some(SubscriptionStatus::Subscribed)
        )
    }

    pub async fn get_all(&self) -> Vec<AsterdexWsChannel> {
        let subs = self.subscriptions.read().await;
        subs.keys().cloned().collect()
    }
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_subscription_manager() {
        let manager = SubscriptionManager::new();
        let channel = AsterdexWsChannel::SpotAggTrade {
            symbol: "BTCUSDT".to_string(),
        };

        // Add pending
        manager.add_pending(channel.clone()).await;
        assert_eq!(
            manager.get_status(&channel).await,
            Some(SubscriptionStatus::Pending)
        );

        // Mark subscribed
        manager.mark_subscribed(&channel).await;
        assert_eq!(
            manager.get_status(&channel).await,
            Some(SubscriptionStatus::Subscribed)
        );
        assert!(manager.is_subscribed(&channel).await);

        // Mark unsubscribed
        manager.mark_unsubscribed(&channel).await;
        assert_eq!(manager.get_status(&channel).await, None);
        assert!(!manager.is_subscribed(&channel).await);
    }
}
