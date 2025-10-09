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

//! Authentication coordination for the Bybit WebSocket client.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use super::error::BybitWsError;

pub(crate) type AuthResultSender = tokio::sync::oneshot::Sender<Result<(), String>>;
pub(crate) type AuthResultReceiver = tokio::sync::oneshot::Receiver<Result<(), String>>;

pub(crate) const AUTHENTICATION_TIMEOUT_SECS: u64 = 10;

#[derive(Clone, Debug)]
pub(crate) struct AuthTracker {
    tx: Arc<Mutex<Option<AuthResultSender>>>,
}

#[allow(dead_code)]
impl AuthTracker {
    pub(crate) fn new() -> Self {
        Self {
            tx: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn begin(&self) -> AuthResultReceiver {
        let (sender, receiver) = tokio::sync::oneshot::channel();

        if let Ok(mut guard) = self.tx.lock() {
            if let Some(old) = guard.take() {
                tracing::warn!("New authentication request superseding previous pending request");
                let _ = old.send(Err("Authentication attempt superseded".to_string()));
            } else {
                tracing::debug!("Starting new authentication request");
            }
            *guard = Some(sender);
        }

        receiver
    }

    pub(crate) fn succeed(&self) {
        if let Ok(mut guard) = self.tx.lock()
            && let Some(sender) = guard.take()
        {
            let _ = sender.send(Ok(()));
        }
    }

    pub(crate) fn fail(&self, error: impl Into<String>) {
        let message = error.into();
        if let Ok(mut guard) = self.tx.lock()
            && let Some(sender) = guard.take()
        {
            let _ = sender.send(Err(message));
        }
    }

    pub(crate) async fn wait_for_result(
        &self,
        timeout: Duration,
        receiver: AuthResultReceiver,
    ) -> Result<(), BybitWsError> {
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(msg))) => Err(BybitWsError::Authentication(msg)),
            Ok(Err(_)) => Err(BybitWsError::Authentication(
                "Authentication channel closed".to_string(),
            )),
            Err(_) => {
                if let Ok(mut guard) = self.tx.lock() {
                    guard.take();
                }
                Err(BybitWsError::Authentication(
                    "Authentication timed out".to_string(),
                ))
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[tokio::test]
    async fn begin_supersedes_previous_sender() {
        let tracker = AuthTracker::new();

        let first = tracker.begin();
        let second = tracker.begin();

        // Completing the first receiver should yield an error indicating it was superseded.
        let result = first.await.expect("oneshot closed unexpectedly");
        assert_eq!(result, Err("Authentication attempt superseded".to_string()));

        // Fulfil the second attempt to keep the mutex state clean.
        tracker.succeed();
        tracker
            .wait_for_result(Duration::from_secs(1), second)
            .await
            .expect("expected successful authentication");
    }
}
