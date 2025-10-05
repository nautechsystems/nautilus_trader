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

//! Authentication coordination for the OKX WebSocket client.
//!
//! This module ensures each login attempt produces a fresh success or
//! failure signal before subscriptions resume.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use super::error::OKXWsError;

pub(crate) type AuthResultSender = tokio::sync::oneshot::Sender<Result<(), String>>;
pub(crate) type AuthResultReceiver = tokio::sync::oneshot::Receiver<Result<(), String>>;

pub(crate) const AUTHENTICATION_TIMEOUT_SECS: u64 = 10;

#[derive(Clone, Debug)]
pub(crate) struct AuthTracker {
    tx: Arc<Mutex<Option<AuthResultSender>>>,
}

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
                let _ = old.send(Err("Authentication attempt superseded".to_string()));
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
    ) -> Result<(), OKXWsError> {
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(msg))) => Err(OKXWsError::AuthenticationError(msg)),
            Ok(Err(_)) => Err(OKXWsError::AuthenticationError(
                "Authentication channel closed".to_string(),
            )),
            Err(_) => {
                if let Ok(mut guard) = self.tx.lock() {
                    guard.take();
                }
                Err(OKXWsError::AuthenticationError(
                    "Authentication timed out".to_string(),
                ))
            }
        }
    }
}
