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

use ahash::AHashMap;
use nautilus_network::websocket::SubscriptionState;

#[derive(Debug)]
pub(crate) enum PendingSubscriptionRequest {
    Subscribe(Vec<String>),
    Unsubscribe(Vec<String>),
}

impl PendingSubscriptionRequest {
    pub(crate) fn subscribe(streams: Vec<String>) -> Self {
        Self::Subscribe(streams)
    }

    pub(crate) fn unsubscribe(streams: Vec<String>) -> Self {
        Self::Unsubscribe(streams)
    }

    pub(crate) fn confirm(&self, subscriptions: &SubscriptionState) {
        match self {
            Self::Subscribe(streams) => {
                for stream in streams {
                    subscriptions.confirm_subscribe(stream);
                }
            }
            Self::Unsubscribe(streams) => {
                for stream in streams {
                    subscriptions.confirm_unsubscribe(stream);
                }
            }
        }
    }

    pub(crate) fn mark_failure(&self, subscriptions: &SubscriptionState) {
        if let Self::Subscribe(streams) = self {
            for stream in streams {
                subscriptions.mark_failure(stream);
            }
        }
    }

    fn streams(&self) -> &[String] {
        match self {
            Self::Subscribe(streams) | Self::Unsubscribe(streams) => streams,
        }
    }

    fn retain_streams(&mut self, mut keep: impl FnMut(&String) -> bool) {
        match self {
            Self::Subscribe(streams) | Self::Unsubscribe(streams) => streams.retain(&mut keep),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct PendingSubscriptionRequests {
    requests: AHashMap<u64, PendingSubscriptionRequest>,
    request_ids: AHashMap<String, u64>,
}

impl PendingSubscriptionRequests {
    pub(crate) fn insert(&mut self, request_id: u64, request: PendingSubscriptionRequest) {
        debug_assert!(!self.requests.contains_key(&request_id));

        for stream in request.streams() {
            if let Some(previous_id) = self.request_ids.insert(stream.clone(), request_id)
                && previous_id != request_id
            {
                let remove_previous = self.requests.get_mut(&previous_id).is_some_and(|previous| {
                    previous.retain_streams(|previous_stream| previous_stream != stream);
                    previous.streams().is_empty()
                });

                if remove_previous {
                    self.requests.remove(&previous_id);
                }
            }
        }

        self.requests.insert(request_id, request);
    }

    pub(crate) fn take(&mut self, request_id: u64) -> Option<PendingSubscriptionRequest> {
        let request = self.requests.remove(&request_id)?;

        for stream in request.streams() {
            if self.request_ids.get(stream) == Some(&request_id) {
                self.request_ids.remove(stream);
            }
        }

        Some(request)
    }

    pub(crate) fn clear(&mut self) {
        self.requests.clear();
        self.request_ids.clear();
    }

    pub(crate) fn len(&self) -> usize {
        self.requests.len()
    }
}

pub(crate) fn reset_requests_after_reconnect(
    pending_requests: &mut PendingSubscriptionRequests,
    subscriptions: &SubscriptionState,
) {
    pending_requests.clear();
    subscriptions.reset_after_reconnect();
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_reset_requests_after_reconnect_preserves_only_subscribe_intent() {
        let subscriptions = SubscriptionState::new('@');
        let subscribe_topic = "btcusdt@trade";
        let unsubscribe_topic = "ethusdt@trade";
        subscriptions.mark_subscribe(subscribe_topic);
        subscriptions.mark_subscribe(unsubscribe_topic);
        subscriptions.confirm_subscribe(unsubscribe_topic);
        subscriptions.mark_unsubscribe(unsubscribe_topic);

        let mut pending_requests = PendingSubscriptionRequests::default();
        pending_requests.insert(
            1,
            PendingSubscriptionRequest::subscribe(vec![subscribe_topic.to_string()]),
        );
        pending_requests.insert(
            2,
            PendingSubscriptionRequest::unsubscribe(vec![unsubscribe_topic.to_string()]),
        );

        reset_requests_after_reconnect(&mut pending_requests, &subscriptions);

        assert_eq!(pending_requests.len(), 0);
        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            [subscribe_topic.to_string()]
        );
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
        assert_eq!(subscriptions.len(), 0);
    }

    #[rstest]
    fn test_newer_request_supersedes_same_stream_in_older_batch() {
        let mut pending_requests = PendingSubscriptionRequests::default();
        pending_requests.insert(
            1,
            PendingSubscriptionRequest::subscribe(vec![
                "btcusdt@trade".to_string(),
                "ethusdt@trade".to_string(),
            ]),
        );
        pending_requests.insert(
            2,
            PendingSubscriptionRequest::unsubscribe(vec!["btcusdt@trade".to_string()]),
        );

        let older = pending_requests.take(1).unwrap();
        assert_eq!(older.streams(), ["ethusdt@trade"]);
        assert_eq!(
            pending_requests.take(2).unwrap().streams(),
            ["btcusdt@trade"]
        );
        assert_eq!(pending_requests.len(), 0);
    }

    #[rstest]
    fn test_newer_request_removes_fully_superseded_request() {
        let mut pending_requests = PendingSubscriptionRequests::default();
        pending_requests.insert(
            1,
            PendingSubscriptionRequest::subscribe(vec!["btcusdt@trade".to_string()]),
        );
        pending_requests.insert(
            2,
            PendingSubscriptionRequest::unsubscribe(vec!["btcusdt@trade".to_string()]),
        );

        assert!(pending_requests.take(1).is_none());
        assert_eq!(pending_requests.len(), 1);
    }

    #[rstest]
    fn test_unsubscribe_failure_preserves_intent_until_reconnect() {
        let subscriptions = SubscriptionState::new('@');
        let topic = "btcusdt@trade";
        subscriptions.mark_subscribe(topic);
        subscriptions.confirm_subscribe(topic);
        subscriptions.mark_unsubscribe(topic);

        let request = PendingSubscriptionRequest::unsubscribe(vec![topic.to_string()]);
        request.mark_failure(&subscriptions);

        assert_eq!(subscriptions.pending_unsubscribe_topics(), [topic]);
        assert!(subscriptions.all_topics().is_empty());

        subscriptions.reset_after_reconnect();

        assert!(subscriptions.is_empty());
    }
}
