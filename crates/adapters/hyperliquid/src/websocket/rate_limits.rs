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

use std::{
    fmt::Debug,
    num::NonZeroU32,
    sync::{Arc, LazyLock, Weak},
};

use ahash::AHashMap;
use nautilus_network::ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota};
use parking_lot::Mutex;
use ustr::Ustr;

use super::{handler::subscription_to_key, messages::SubscriptionRequest};
use crate::common::{
    consts::{
        HYPERLIQUID_WS_CONNECTIONS_MAX, HYPERLIQUID_WS_CONNECTIONS_PER_MINUTE,
        HYPERLIQUID_WS_MESSAGES_PER_MINUTE, HYPERLIQUID_WS_POST_INFLIGHT_MAX,
        HYPERLIQUID_WS_SUBSCRIPTION_USERS_MAX, HYPERLIQUID_WS_SUBSCRIPTIONS_MAX,
    },
    enums::HyperliquidEnvironment,
    rate_limits::HyperliquidRouteScope,
};

const MESSAGE_RATE_KEY: &str = "hyperliquid:ws:messages";
const CONNECTION_RATE_KEY: &str = "hyperliquid:ws:connections";

type NetworkRateLimiter = RateLimiter<Ustr, MonotonicClock>;
type WebSocketLimitRegistry = Mutex<AHashMap<HyperliquidRouteScope, Weak<WebSocketRateLimits>>>;

static WS_LIMITS: LazyLock<WebSocketLimitRegistry> = LazyLock::new(|| Mutex::new(AHashMap::new()));

pub(super) struct WebSocketRateLimits {
    pub(super) messages: Arc<NetworkRateLimiter>,
    pub(super) connections: Arc<NetworkRateLimiter>,
    pub(super) connection_slots: Arc<tokio::sync::Semaphore>,
    pub(super) post_slots: Arc<tokio::sync::Semaphore>,
    subscriptions: Mutex<SubscriptionCounts>,
}

impl Debug for WebSocketRateLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(WebSocketRateLimits)).finish()
    }
}

#[derive(Debug, Default)]
struct SubscriptionCounts {
    clients: AHashMap<u64, AHashMap<String, Option<String>>>,
    user_references: AHashMap<String, usize>,
    total: usize,
}

impl WebSocketRateLimits {
    pub(super) fn new() -> Self {
        Self {
            messages: Arc::new(Self::network_limiter(
                MESSAGE_RATE_KEY,
                HYPERLIQUID_WS_MESSAGES_PER_MINUTE,
            )),
            connections: Arc::new(Self::network_limiter(
                CONNECTION_RATE_KEY,
                HYPERLIQUID_WS_CONNECTIONS_PER_MINUTE,
            )),
            connection_slots: Arc::new(tokio::sync::Semaphore::new(HYPERLIQUID_WS_CONNECTIONS_MAX)),
            post_slots: Arc::new(tokio::sync::Semaphore::new(
                HYPERLIQUID_WS_POST_INFLIGHT_MAX,
            )),
            subscriptions: Mutex::new(SubscriptionCounts::default()),
        }
    }

    fn network_limiter(key: &str, per_minute: u32) -> NetworkRateLimiter {
        RateLimiter::new_with_quota(
            None,
            vec![(
                Ustr::from(key),
                Quota::per_minute(NonZeroU32::new(per_minute).unwrap()),
            )],
        )
    }

    pub(super) fn message_key(&self) -> Ustr {
        Ustr::from(MESSAGE_RATE_KEY)
    }

    pub(super) fn connection_key(&self) -> Ustr {
        Ustr::from(CONNECTION_RATE_KEY)
    }

    pub(super) async fn acquire_message(&self) {
        self.messages.until_key_ready(&self.message_key()).await;
    }

    pub(super) fn reserve_subscription(
        &self,
        client_id: u64,
        subscription: &SubscriptionRequest,
    ) -> Result<bool, String> {
        let key = subscription_to_key(subscription);
        let user = subscription_user(subscription).map(str::to_lowercase);
        let mut counts = self.subscriptions.lock();

        if counts
            .clients
            .get(&client_id)
            .is_some_and(|subscriptions| subscriptions.contains_key(&key))
        {
            return Ok(false);
        }

        if counts.total >= HYPERLIQUID_WS_SUBSCRIPTIONS_MAX {
            return Err(format!(
                "Hyperliquid allows at most {HYPERLIQUID_WS_SUBSCRIPTIONS_MAX} WebSocket subscriptions per route"
            ));
        }

        if let Some(user) = &user
            && !counts.user_references.contains_key(user)
            && counts.user_references.len() >= HYPERLIQUID_WS_SUBSCRIPTION_USERS_MAX
        {
            return Err(format!(
                "Hyperliquid allows at most {HYPERLIQUID_WS_SUBSCRIPTION_USERS_MAX} unique users in WebSocket subscriptions per route"
            ));
        }

        counts
            .clients
            .entry(client_id)
            .or_default()
            .insert(key, user.clone());
        counts.total += 1;

        if let Some(user) = user {
            *counts.user_references.entry(user).or_default() += 1;
        }
        Ok(true)
    }

    pub(super) fn release_subscription(&self, client_id: u64, key: &str) {
        let mut counts = self.subscriptions.lock();
        let removed = counts
            .clients
            .get_mut(&client_id)
            .and_then(|subscriptions| subscriptions.remove(key));
        let remove_client = counts
            .clients
            .get(&client_id)
            .is_some_and(|subscriptions| subscriptions.is_empty());

        if remove_client {
            counts.clients.remove(&client_id);
        }

        if let Some(user) = removed {
            counts.total -= 1;
            decrement_user_reference(&mut counts.user_references, user.as_deref());
        }
    }

    pub(super) fn release_subscriptions<'a>(
        &self,
        client_id: u64,
        keys: impl IntoIterator<Item = &'a str>,
    ) {
        for key in keys {
            self.release_subscription(client_id, key);
        }
    }

    pub(super) fn release_client(&self, client_id: u64) {
        let mut counts = self.subscriptions.lock();
        let Some(subscriptions) = counts.clients.remove(&client_id) else {
            return;
        };

        counts.total -= subscriptions.len();
        for user in subscriptions.into_values() {
            decrement_user_reference(&mut counts.user_references, user.as_deref());
        }
    }

    #[cfg(test)]
    fn subscription_count(&self) -> usize {
        self.subscriptions.lock().total
    }

    #[cfg(test)]
    fn subscription_user_count(&self) -> usize {
        self.subscriptions.lock().user_references.len()
    }
}

pub(super) fn shared_websocket_limits(
    environment: HyperliquidEnvironment,
    endpoint_url: &str,
    proxy_url: Option<&str>,
) -> Arc<WebSocketRateLimits> {
    let scope = HyperliquidRouteScope::new(environment, endpoint_url, proxy_url);
    let mut registry = WS_LIMITS.lock();

    if let Some(limits) = registry.get(&scope).and_then(Weak::upgrade) {
        return limits;
    }

    let limits = Arc::new(WebSocketRateLimits::new());
    registry.insert(scope, Arc::downgrade(&limits));
    limits
}

fn subscription_user(subscription: &SubscriptionRequest) -> Option<&str> {
    match subscription {
        SubscriptionRequest::Notification { user }
        | SubscriptionRequest::WebData2 { user }
        | SubscriptionRequest::OrderUpdates { user }
        | SubscriptionRequest::UserEvents { user }
        | SubscriptionRequest::UserFills { user, .. }
        | SubscriptionRequest::UserFundings { user }
        | SubscriptionRequest::UserNonFundingLedgerUpdates { user }
        | SubscriptionRequest::UserTwapSliceFills { user }
        | SubscriptionRequest::UserTwapHistory { user }
        | SubscriptionRequest::ActiveAssetData { user, .. } => Some(user),
        SubscriptionRequest::AllMids { .. }
        | SubscriptionRequest::AllDexsAssetCtxs
        | SubscriptionRequest::Candle { .. }
        | SubscriptionRequest::L2Book { .. }
        | SubscriptionRequest::Trades { .. }
        | SubscriptionRequest::ActiveAssetCtx { .. }
        | SubscriptionRequest::ActiveSpotAssetCtx { .. }
        | SubscriptionRequest::Bbo { .. } => None,
    }
}

fn decrement_user_reference(references: &mut AHashMap<String, usize>, user: Option<&str>) {
    let Some(user) = user else {
        return;
    };
    let Some(count) = references.get_mut(user) else {
        return;
    };

    *count -= 1;
    if *count == 0 {
        references.remove(user);
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn trades_subscription(index: usize) -> SubscriptionRequest {
        SubscriptionRequest::Trades {
            coin: Ustr::from(&format!("COIN-{index}")),
        }
    }

    fn user_subscription(index: usize) -> SubscriptionRequest {
        SubscriptionRequest::UserEvents {
            user: format!("0x{index:040x}"),
        }
    }

    #[rstest]
    fn websocket_limits_share_route_scope() {
        let first = shared_websocket_limits(
            HyperliquidEnvironment::Testnet,
            "wss://ws-share.example/ws",
            None,
        );
        let second = shared_websocket_limits(
            HyperliquidEnvironment::Testnet,
            "wss://ws-share.example/another-path",
            None,
        );
        let proxied = shared_websocket_limits(
            HyperliquidEnvironment::Testnet,
            "wss://ws-share.example/ws",
            Some("http://proxy.example:8080"),
        );

        assert!(Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(&first, &proxied));
    }

    #[rstest]
    fn websocket_message_and_connection_rates_match_official_limits() {
        let limits = WebSocketRateLimits::new();
        let message_key = limits.message_key();
        let connection_key = limits.connection_key();

        for _ in 0..HYPERLIQUID_WS_MESSAGES_PER_MINUTE {
            assert!(limits.messages.check_key(&message_key).is_ok());
        }
        assert!(limits.messages.check_key(&message_key).is_err());

        for _ in 0..HYPERLIQUID_WS_CONNECTIONS_PER_MINUTE {
            assert!(limits.connections.check_key(&connection_key).is_ok());
        }
        assert!(limits.connections.check_key(&connection_key).is_err());
    }

    #[rstest]
    fn websocket_connection_slots_match_official_limit() {
        let limits = WebSocketRateLimits::new();
        let mut permits = Vec::new();

        for _ in 0..HYPERLIQUID_WS_CONNECTIONS_MAX {
            permits.push(
                Arc::clone(&limits.connection_slots)
                    .try_acquire_owned()
                    .unwrap(),
            );
        }

        assert!(
            Arc::clone(&limits.connection_slots)
                .try_acquire_owned()
                .is_err()
        );
        assert_eq!(permits.len(), HYPERLIQUID_WS_CONNECTIONS_MAX);
    }

    #[rstest]
    fn websocket_subscription_limit_is_shared_across_clients() {
        let limits = WebSocketRateLimits::new();

        for index in 0..HYPERLIQUID_WS_SUBSCRIPTIONS_MAX {
            let client_id = (index % 2) as u64;
            assert!(
                limits
                    .reserve_subscription(client_id, &trades_subscription(index))
                    .unwrap()
            );
        }

        assert!(
            limits
                .reserve_subscription(2, &trades_subscription(HYPERLIQUID_WS_SUBSCRIPTIONS_MAX))
                .is_err()
        );
    }

    #[rstest]
    fn websocket_duplicate_subscription_uses_one_reservation() {
        let limits = WebSocketRateLimits::new();
        let subscription = trades_subscription(0);

        assert!(limits.reserve_subscription(1, &subscription).unwrap());
        assert!(!limits.reserve_subscription(1, &subscription).unwrap());
        assert_eq!(limits.subscription_count(), 1);
    }

    #[rstest]
    fn websocket_subscription_users_are_case_insensitive() {
        let limits = WebSocketRateLimits::new();
        let lower = SubscriptionRequest::UserEvents {
            user: "0xabcdef".to_string(),
        };
        let upper = SubscriptionRequest::OrderUpdates {
            user: "0xABCDEF".to_string(),
        };

        assert!(limits.reserve_subscription(1, &lower).unwrap());
        assert!(limits.reserve_subscription(1, &upper).unwrap());
        assert_eq!(limits.subscription_count(), 2);
        assert_eq!(limits.subscription_user_count(), 1);
    }

    #[rstest]
    fn websocket_unique_user_limit_releases_on_unsubscribe() {
        let limits = WebSocketRateLimits::new();

        for index in 0..HYPERLIQUID_WS_SUBSCRIPTION_USERS_MAX {
            assert!(
                limits
                    .reserve_subscription(1, &user_subscription(index))
                    .unwrap()
            );
        }
        assert!(
            limits
                .reserve_subscription(2, &user_subscription(HYPERLIQUID_WS_SUBSCRIPTION_USERS_MAX))
                .is_err()
        );

        let released = user_subscription(0);
        limits.release_subscription(1, &subscription_to_key(&released));

        assert!(
            limits
                .reserve_subscription(2, &user_subscription(HYPERLIQUID_WS_SUBSCRIPTION_USERS_MAX))
                .unwrap()
        );
        assert_eq!(
            limits.subscription_count(),
            HYPERLIQUID_WS_SUBSCRIPTION_USERS_MAX
        );
    }
}
