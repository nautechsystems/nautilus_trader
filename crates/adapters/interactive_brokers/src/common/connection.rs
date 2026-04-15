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

//! Connection management utilities for Interactive Brokers adapter.

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use ibapi::client::Client;
use nautilus_common::live::get_runtime;

/// Connection manager for Interactive Brokers clients.
///
/// Handles automatic reconnection with exponential backoff, connection monitoring,
/// and subscription resubscription on reconnect.
#[derive(Debug, Clone)]
pub struct ConnectionManager {
    /// Host address for IB Gateway/TWS.
    host: String,
    /// Port for IB Gateway/TWS.
    port: u16,
    /// Client ID.
    client_id: i32,
    /// Current connection state.
    is_connected: Arc<AtomicBool>,
    /// Connection attempt counter.
    attempt_count: Arc<AtomicU32>,
    /// Maximum connection attempts (0 = infinite).
    max_attempts: u32,
    /// Whether to retry indefinitely.
    retry_indefinitely: bool,
    /// Current backoff duration.
    current_backoff: Arc<std::sync::Mutex<Duration>>,
    /// Last disconnection time.
    last_disconnection: Arc<std::sync::Mutex<Option<tokio::time::Instant>>>,
}

impl ConnectionManager {
    /// Create a new connection manager.
    ///
    /// # Arguments
    ///
    /// * `host` - Host address
    /// * `port` - Port number
    /// * `client_id` - Client ID
    /// * `max_attempts` - Maximum connection attempts (0 = infinite)
    pub fn new(host: String, port: u16, client_id: i32, max_attempts: u32) -> Self {
        Self {
            host,
            port,
            client_id,
            is_connected: Arc::new(AtomicBool::new(false)),
            attempt_count: Arc::new(AtomicU32::new(0)),
            max_attempts,
            retry_indefinitely: max_attempts == 0,
            current_backoff: Arc::new(std::sync::Mutex::new(Duration::from_secs(1))),
            last_disconnection: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Connect to IB Gateway/TWS with automatic retry.
    ///
    /// # Returns
    ///
    /// Returns the connected client on success.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails after max attempts.
    ///
    /// # Panics
    ///
    /// Panics if the mutex is poisoned.
    pub async fn connect_with_retry(&self) -> anyhow::Result<Arc<Client>> {
        const MAX_BACKOFF: Duration = Duration::from_secs(60);
        let mut attempt = 0;
        let mut backoff = Duration::from_secs(1);

        loop {
            attempt += 1;
            self.attempt_count.store(attempt, Ordering::Relaxed);

            if !self.retry_indefinitely && attempt > self.max_attempts {
                anyhow::bail!("Failed to connect after {} attempts", self.max_attempts);
            }

            tracing::info!(
                "Connection attempt {} to {}:{} (client_id: {})",
                attempt,
                self.host,
                self.port,
                self.client_id
            );

            let address = format!("{}:{}", self.host, self.port);
            match Client::connect(&address, self.client_id).await {
                Ok(client) => {
                    tracing::info!(
                        "Successfully connected to IB Gateway/TWS at {} (client_id: {})",
                        address,
                        self.client_id
                    );

                    self.is_connected.store(true, Ordering::Relaxed);
                    self.attempt_count.store(0, Ordering::Relaxed);
                    *self.current_backoff.lock().unwrap() = Duration::from_secs(1);

                    return Ok(Arc::new(client));
                }
                Err(e) => {
                    tracing::warn!(
                        "Connection attempt {} failed: {} (backoff: {:?})",
                        attempt,
                        e,
                        backoff
                    );

                    if !self.retry_indefinitely && attempt >= self.max_attempts {
                        return Err(e).context(format!(
                            "Failed to connect after {} attempts",
                            self.max_attempts
                        ));
                    }

                    // Exponential backoff
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
                    *self.current_backoff.lock().unwrap() = backoff;
                }
            }
        }
    }

    /// Check if currently connected.
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    /// Mark connection as disconnected.
    ///
    /// # Panics
    ///
    /// Panics if the mutex is poisoned.
    pub fn mark_disconnected(&self) {
        self.is_connected.store(false, Ordering::Relaxed);
        *self.last_disconnection.lock().unwrap() = Some(tokio::time::Instant::now());
    }

    /// Get current attempt count.
    pub fn attempt_count(&self) -> u32 {
        self.attempt_count.load(Ordering::Relaxed)
    }

    /// Get current backoff duration.
    ///
    /// # Panics
    ///
    /// Panics if the mutex is poisoned.
    pub fn current_backoff(&self) -> Duration {
        *self.current_backoff.lock().unwrap()
    }

    /// Get time since last disconnection.
    ///
    /// # Panics
    ///
    /// Panics if the mutex is poisoned.
    pub fn time_since_disconnection(&self) -> Option<Duration> {
        self.last_disconnection
            .lock()
            .unwrap()
            .map(|time: tokio::time::Instant| time.elapsed())
    }
}

/// Connection watchdog for monitoring connection health.
///
/// Periodically checks connection status and triggers reconnection if needed.
#[derive(Clone)]
pub struct ConnectionWatchdog {
    /// Connection manager.
    manager: Arc<ConnectionManager>,
    /// Check interval.
    check_interval: Duration,
    /// Client reference (for health checks).
    client: Arc<std::sync::Mutex<Option<Arc<Client>>>>,
    /// Callback to call when reconnection is needed.
    reconnect_callback: Arc<
        dyn Fn() -> tokio::task::JoinHandle<anyhow::Result<Arc<Client>>> + Send + Sync + 'static,
    >,
    /// Whether the watchdog is running.
    is_running: Arc<AtomicBool>,
}

impl Debug for ConnectionWatchdog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ConnectionWatchdog))
            .field("check_interval", &self.check_interval)
            .field("is_running", &self.is_running.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl ConnectionWatchdog {
    /// Create a new connection watchdog.
    ///
    /// # Arguments
    ///
    /// * `manager` - Connection manager
    /// * `check_interval` - Interval between health checks
    /// * `reconnect_callback` - Callback to trigger reconnection
    pub fn new(
        manager: Arc<ConnectionManager>,
        check_interval: Duration,
        reconnect_callback: Arc<
            dyn Fn() -> tokio::task::JoinHandle<anyhow::Result<Arc<Client>>> + Send + Sync,
        >,
    ) -> Self {
        Self {
            manager,
            check_interval,
            client: Arc::new(std::sync::Mutex::new(None)),
            reconnect_callback,
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set the client reference for health checks.
    ///
    /// # Panics
    ///
    /// Panics if the mutex is poisoned.
    pub fn set_client(&self, client: Arc<Client>) {
        *self.client.lock().unwrap() = Some(client);
    }

    /// Start the watchdog.
    ///
    /// # Panics
    ///
    /// Panics if a mutex is poisoned.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let manager = Arc::clone(&self.manager);
        let client = Arc::clone(&self.client);
        let reconnect_callback = Arc::clone(&self.reconnect_callback);
        let check_interval = self.check_interval;
        let is_running = Arc::clone(&self.is_running);

        is_running.store(true, Ordering::Relaxed);

        get_runtime().spawn(async move {
            tracing::info!("Connection watchdog started");

            while is_running.load(Ordering::Relaxed) {
                tokio::time::sleep(check_interval).await;

                if !manager.is_connected() {
                    tracing::warn!(
                        "Connection watchdog detected disconnection, triggering reconnection"
                    );

                    // Trigger reconnection
                    let handle = reconnect_callback();

                    // Wait for reconnection attempt
                    match handle.await {
                        Ok(Ok(new_client)) => {
                            tracing::info!("Reconnection successful via watchdog");
                            *client.lock().unwrap() = Some(new_client);
                            manager.is_connected.store(true, Ordering::Relaxed);
                        }
                        Ok(Err(e)) => {
                            tracing::error!("Reconnection failed via watchdog: {}", e);
                        }
                        Err(e) => {
                            tracing::error!("Reconnection task panicked: {}", e);
                        }
                    }
                }
            }

            tracing::info!("Connection watchdog stopped");
        })
    }

    /// Stop the watchdog.
    pub fn stop(&self) {
        self.is_running.store(false, Ordering::Relaxed);
    }
}
