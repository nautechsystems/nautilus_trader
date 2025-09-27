//! Authentication coordination for the BitMEX WebSocket client.
//!
//! [`AuthTracker`] keeps the in-flight auth attempt and reports success or timeout so
//! the reconnect loop can proceed deterministically.
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

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::sync::Notify;

use crate::websocket::error::BitmexWsError;

pub(crate) const AUTHENTICATION_TIMEOUT_SECS: u64 = 5;

#[derive(Clone, Debug)]
pub(crate) struct AuthTracker {
    inflight: Arc<AtomicBool>,
    succeeded: Arc<AtomicBool>,
    notify: Arc<Notify>,
    last_error: Arc<Mutex<Option<String>>>,
}

impl AuthTracker {
    pub(crate) fn new() -> Self {
        Self {
            inflight: Arc::new(AtomicBool::new(false)),
            succeeded: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
            last_error: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn begin(&self) {
        if let Ok(mut guard) = self.last_error.lock() {
            *guard = None;
        }
        self.succeeded.store(false, Ordering::SeqCst);
        self.inflight.store(true, Ordering::SeqCst);
    }

    pub(crate) fn succeed(&self) {
        self.succeeded.store(true, Ordering::SeqCst);
        self.inflight.store(false, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub(crate) fn fail(&self, error: impl Into<String>) {
        if let Ok(mut guard) = self.last_error.lock() {
            *guard = Some(error.into());
        }
        self.succeeded.store(false, Ordering::SeqCst);
        self.inflight.store(false, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    fn inflight(&self) -> bool {
        self.inflight.load(Ordering::SeqCst)
    }

    fn succeeded(&self) -> bool {
        self.succeeded.load(Ordering::SeqCst)
    }

    fn error_message(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|guard| guard.clone())
    }

    pub(crate) async fn wait_for_result(&self, timeout: Duration) -> Result<(), BitmexWsError> {
        let wait_future = async {
            loop {
                if self.inflight() {
                    self.notify.notified().await;
                    continue;
                }

                if self.succeeded() {
                    return Ok(());
                }

                let error = self
                    .error_message()
                    .unwrap_or_else(|| "Authentication failed".to_string());
                return Err(BitmexWsError::AuthenticationError(error));
            }
        };

        match tokio::time::timeout(timeout, wait_future).await {
            Ok(result) => result,
            Err(_) => {
                self.fail("Authentication timed out");
                Err(BitmexWsError::AuthenticationError(
                    "Authentication timed out".to_string(),
                ))
            }
        }
    }
}
