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

//! FIX Client for the Coinbase International Drop Copy Endpoint.
//!
//! This implementation focuses specifically on processing execution reports
//! via the FIX protocol, leveraging the existing `SocketClient` for TCP/TLS connectivity.
//!
//! # Warning
//!
//! **Not a full FIX engine**: This client supports only the Coinbase International Drop Copy
//! endpoint and lacks general-purpose FIX functionality.
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use aws_lc_rs::hmac;
use base64::prelude::*;
use nautilus_common::logging::{log_task_started, log_task_stopped};
#[cfg(feature = "python")]
use nautilus_core::python::IntoPyObjectNautilusExt;
use nautilus_core::{env::get_or_env_var, time::get_atomic_clock_realtime};
use nautilus_model::identifiers::AccountId;
use nautilus_network::socket::{SocketClient, SocketConfig, WriterCommand};
#[cfg(feature = "python")]
use pyo3::prelude::*;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::stream::Mode;

use super::{
    messages::{FIX_DELIMITER, FixMessage},
    parse::convert_to_order_status_report,
};
use crate::{
    common::consts::COINBASE_INTX,
    fix::{
        messages::{fix_exec_type, fix_message_type, fix_tag},
        parse::convert_to_fill_report,
    },
};

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
#[derive(Debug, Clone)]
pub struct CoinbaseIntxFixClient {
    endpoint: String,
    api_key: String,
    api_secret: String,
    api_passphrase: String,
    portfolio_id: String,
    sender_comp_id: String,
    target_comp_id: String,
    socket: Option<Arc<SocketClient>>,
    connected: Arc<AtomicBool>,
    logged_on: Arc<AtomicBool>,
    seq_num: Arc<AtomicUsize>,
    received_seq_num: Arc<AtomicUsize>,
    heartbeat_secs: u64,
    processing_task: Option<Arc<JoinHandle<()>>>,
    heartbeat_task: Option<Arc<JoinHandle<()>>>,
}

impl CoinbaseIntxFixClient {
    /// Creates a new [`CoinbaseIntxFixClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables or parameters are missing.
    pub fn new(
        endpoint: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        portfolio_id: Option<String>,
    ) -> anyhow::Result<Self> {
        let endpoint = endpoint.unwrap_or("fix.international.coinbase.com:6130".to_string());
        let api_key = get_or_env_var(api_key, "COINBASE_INTX_API_KEY")?;
        let api_secret = get_or_env_var(api_secret, "COINBASE_INTX_API_SECRET")?;
        let api_passphrase = get_or_env_var(api_passphrase, "COINBASE_INTX_API_PASSPHRASE")?;
        let portfolio_id = get_or_env_var(portfolio_id, "COINBASE_INTX_PORTFOLIO_ID")?;
        let sender_comp_id = api_key.clone();
        let target_comp_id = "CBINTLDC".to_string(); // Drop Copy endpoint

        Ok(Self {
            endpoint,
            api_key,
            api_secret,
            api_passphrase,
            portfolio_id,
            sender_comp_id,
            target_comp_id,
            socket: None,
            connected: Arc::new(AtomicBool::new(false)),
            logged_on: Arc::new(AtomicBool::new(false)),
            seq_num: Arc::new(AtomicUsize::new(1)),
            received_seq_num: Arc::new(AtomicUsize::new(0)),
            heartbeat_secs: 10, // Default (probably no need to change)
            processing_task: None,
            heartbeat_task: None,
        })
    }

    /// Creates a new authenticated [`CoinbaseIntxFixClient`] instance using
    /// environment variables and the default Coinbase International FIX drop copy endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are not set.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::new(None, None, None, None, None)
    }

    /// Returns the FIX endpoint being used by the client.
    #[must_use]
    pub const fn endpoint(&self) -> &str {
        self.endpoint.as_str()
    }

    /// Returns the public API key being used by the client.
    #[must_use]
    pub const fn api_key(&self) -> &str {
        self.api_key.as_str()
    }

    /// Returns the Coinbase International portfolio ID being used by the client.
    #[must_use]
    pub const fn portfolio_id(&self) -> &str {
        self.portfolio_id.as_str()
    }

    /// Returns the sender company ID being used by the client.
    #[must_use]
    pub const fn sender_comp_id(&self) -> &str {
        self.sender_comp_id.as_str()
    }

    /// Returns the target company ID being used by the client.
    #[must_use]
    pub const fn target_comp_id(&self) -> &str {
        self.target_comp_id.as_str()
    }

    /// Checks if the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Checks if the client is logged on.
    #[must_use]
    pub fn is_logged_on(&self) -> bool {
        self.logged_on.load(Ordering::SeqCst)
    }

    /// Connects to the Coinbase International FIX Drop Copy endpoint.
    ///
    /// # Panics
    ///
    /// Panics if time calculation or unwrap logic inside fails during logon retry setup.
    ///
    /// # Errors
    ///
    /// Returns an error if network connection or FIX logon fails.
    pub async fn connect(
        &mut self,
        #[cfg(feature = "python")] handler: Py<PyAny>,
        #[cfg(not(feature = "python"))] _handler: (),
    ) -> anyhow::Result<()> {
        let logged_on = self.logged_on.clone();
        let seq_num = self.seq_num.clone();
        let received_seq_num = self.received_seq_num.clone();
        let account_id = AccountId::new(format!("{COINBASE_INTX}-{}", self.portfolio_id));

        let handle_message = Arc::new(move |data: &[u8]| {
            if let Ok(message) = FixMessage::parse(data) {
                // Update received sequence number
                if let Some(msg_seq) = message.msg_seq_num() {
                    received_seq_num.store(msg_seq, Ordering::SeqCst);
                }

                // Process message based on type
                if let Some(msg_type) = message.msg_type() {
                    match msg_type {
                        fix_message_type::LOGON => {
                            tracing::info!("Logon successful");
                            logged_on.store(true, Ordering::SeqCst);
                        }
                        fix_message_type::LOGOUT => {
                            tracing::info!("Received logout");
                            logged_on.store(false, Ordering::SeqCst);
                        }
                        fix_message_type::EXECUTION_REPORT => {
                            if let Some(exec_type) = message.get_field(fix_tag::EXEC_TYPE) {
                                if matches!(
                                    exec_type,
                                    fix_exec_type::REJECTED
                                        | fix_exec_type::NEW
                                        | fix_exec_type::PENDING_NEW
                                ) {
                                    // These order events are already handled by the client
                                    tracing::debug!(
                                        "Received execution report for EXEC_TYPE {exec_type} (not handling here)"
                                    );
                                } else if matches!(
                                    exec_type,
                                    fix_exec_type::CANCELED
                                        | fix_exec_type::EXPIRED
                                        | fix_exec_type::REPLACED
                                ) {
                                    let clock = get_atomic_clock_realtime(); // TODO: Optimize
                                    let ts_init = clock.get_time_ns();
                                    match convert_to_order_status_report(
                                        &message, account_id, ts_init,
                                    ) {
                                        #[cfg(feature = "python")]
                                        Ok(report) => Python::attach(|py| {
                                            call_python(
                                                py,
                                                &handler,
                                                report.into_py_any_unwrap(py),
                                            );
                                        }),
                                        #[cfg(not(feature = "python"))]
                                        Ok(_report) => {
                                            tracing::debug!(
                                                "Order status report handled (Python disabled)"
                                            );
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to parse FIX execution report: {e}"
                                            );
                                        }
                                    }
                                } else if exec_type == fix_exec_type::PARTIAL_FILL
                                    || exec_type == fix_exec_type::FILL
                                {
                                    let clock = get_atomic_clock_realtime(); // TODO: Optimize
                                    let ts_init = clock.get_time_ns();
                                    match convert_to_fill_report(&message, account_id, ts_init) {
                                        #[cfg(feature = "python")]
                                        Ok(report) => Python::attach(|py| {
                                            call_python(
                                                py,
                                                &handler,
                                                report.into_py_any_unwrap(py),
                                            );
                                        }),
                                        #[cfg(not(feature = "python"))]
                                        Ok(_report) => {
                                            tracing::debug!(
                                                "Fill report handled (Python disabled)"
                                            );
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to parse FIX execution report: {e}"
                                            );
                                        }
                                    }
                                } else {
                                    tracing::warn!("Unhandled EXEC_TYPE {exec_type}: {message:?}");
                                }
                            }
                        }
                        // These can be HEARTBEAT or TEST_REQUEST messages,
                        // ideally we'd respond to these with a heartbeat
                        // including tag 112 TestReqID.
                        _ => tracing::trace!("Received unexpected {message:?}"),
                    }
                }
            } else {
                tracing::error!("Failed to parse FIX message");
            }
        });

        let config = SocketConfig {
            url: self.endpoint.clone(),
            mode: Mode::Tls,
            suffix: vec![FIX_DELIMITER],
            message_handler: Some(handle_message),
            heartbeat: None, // Using FIX heartbeats
            reconnect_timeout_ms: Some(10000),
            reconnect_delay_initial_ms: Some(5000),
            reconnect_delay_max_ms: Some(30000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(500),
            certs_dir: None,
        };

        let socket = match SocketClient::connect(
            config, None, // post_connection
            None, // post_reconnection
            None, // post_disconnection
        )
        .await
        {
            Ok(socket) => socket,
            Err(e) => anyhow::bail!("Failed to connect to FIX endpoint: {e:?}"),
        };

        let writer_tx = socket.writer_tx.clone();

        self.socket = Some(Arc::new(socket));

        self.send_logon().await?;

        // Create task to monitor connection and send logon after reconnect
        let connected_clone = self.connected.clone();
        let logged_on_clone = self.logged_on.clone();
        let heartbeat_secs = self.heartbeat_secs;
        let client_clone = self.clone();

        self.processing_task = Some(Arc::new(tokio::spawn(async move {
            log_task_started("maintain-fix-connection");

            let mut last_logon_attempt = std::time::Instant::now()
                .checked_sub(Duration::from_secs(10))
                .unwrap();

            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;

                // Check if connected but not logged on
                if connected_clone.load(Ordering::SeqCst) && !logged_on_clone.load(Ordering::SeqCst)
                {
                    // Rate limit logon attempts
                    if last_logon_attempt.elapsed() > Duration::from_secs(10) {
                        tracing::info!("Connected without logon");
                        last_logon_attempt = std::time::Instant::now();

                        if let Err(e) = client_clone.send_logon().await {
                            tracing::error!("Failed to send logon: {e}");
                        }
                    }
                }
            }
        })));

        let logged_on_clone = self.logged_on.clone();
        let sender_comp_id = self.sender_comp_id.clone();
        let target_comp_id = self.target_comp_id.clone();

        self.heartbeat_task = Some(Arc::new(tokio::spawn(async move {
            log_task_started("heartbeat");
            tracing::debug!("Heartbeat at {heartbeat_secs}s intervals");

            let interval = Duration::from_secs(heartbeat_secs);

            loop {
                if logged_on_clone.load(Ordering::SeqCst) {
                    // Create new heartbeat message
                    let seq = seq_num.fetch_add(1, Ordering::SeqCst) + 1;
                    let now = chrono::Utc::now();
                    let msg =
                        FixMessage::create_heartbeat(seq, &sender_comp_id, &target_comp_id, &now);

                    if let Err(e) = writer_tx.send(WriterCommand::Send(msg.to_bytes().into())) {
                        tracing::error!("Failed to send heartbeat: {e}");
                        break;
                    }

                    tracing::trace!("Sent heartbeat");
                } else {
                    // No longer logged on
                    tracing::debug!("No longer logged on, stopping heartbeat task");
                    break;
                }

                tokio::time::sleep(interval).await;
            }

            log_task_stopped("heartbeat");
        })));

        Ok(())
    }

    /// Closes the connection.
    ///
    /// # Errors
    ///
    /// Returns an error if logout or socket closure fails.
    pub async fn close(&mut self) -> anyhow::Result<()> {
        // Send logout message if connected
        if self.is_logged_on()
            && let Err(e) = self.send_logout("Normal logout").await
        {
            tracing::warn!("Failed to send logout message: {e}");
        }

        // Close socket
        if let Some(socket) = &self.socket {
            socket.close().await;
        }

        // Cancel processing task
        if let Some(task) = self.processing_task.take() {
            task.abort();
        }

        // Cancel heartbeat task
        if let Some(task) = self.heartbeat_task.take() {
            task.abort();
        }

        self.connected.store(false, Ordering::SeqCst);
        self.logged_on.store(false, Ordering::SeqCst);

        Ok(())
    }

    /// Send a logon message
    async fn send_logon(&self) -> anyhow::Result<()> {
        if self.socket.is_none() {
            anyhow::bail!("Socket not connected".to_string());
        }

        // Reset sequence number
        self.seq_num.store(1, Ordering::SeqCst);

        let now = chrono::Utc::now();
        let timestamp = now.format("%Y%m%d-%H:%M:%S.%3f").to_string();
        let passphrase = self.api_passphrase.clone();

        let message = format!(
            "{}{}{}{}",
            timestamp, self.api_key, self.target_comp_id, passphrase
        );

        // Create signature
        let decoded_secret = BASE64_STANDARD
            .decode(&self.api_secret)
            .map_err(|e| anyhow::anyhow!("Invalid base64 secret key: {e}"))?;

        let key = hmac::Key::new(hmac::HMAC_SHA256, &decoded_secret);
        let tag = hmac::sign(&key, message.as_bytes());
        let encoded_signature = BASE64_STANDARD.encode(tag.as_ref());

        let logon_msg = FixMessage::create_logon(
            1, // Always use 1 for new logon with reset
            &self.sender_comp_id,
            &self.target_comp_id,
            self.heartbeat_secs,
            &self.api_key,
            &passphrase,
            &encoded_signature,
            &now,
        );

        if let Some(socket) = &self.socket {
            tracing::info!("Logging on...");

            match socket.send_bytes(logon_msg.to_bytes()).await {
                Ok(()) => tracing::debug!("Sent logon message"),
                Err(e) => tracing::error!("Error on logon: {e}"),
            }
        } else {
            anyhow::bail!("Socket not connected".to_string());
        }

        let start = std::time::Instant::now();
        while !self.is_logged_on() {
            tokio::time::sleep(Duration::from_millis(100)).await;

            if start.elapsed() > Duration::from_secs(10) {
                anyhow::bail!("Logon timeout".to_string());
            }
        }

        self.logged_on.store(true, Ordering::SeqCst);

        Ok(())
    }

    /// Sends a logout message.
    async fn send_logout(&self, text: &str) -> anyhow::Result<()> {
        if self.socket.is_none() {
            anyhow::bail!("Socket not connected".to_string());
        }

        let seq_num = self.seq_num.fetch_add(1, Ordering::SeqCst);
        let now = chrono::Utc::now();

        let logout_msg = FixMessage::create_logout(
            seq_num,
            &self.sender_comp_id,
            &self.target_comp_id,
            Some(text),
            &now,
        );

        if let Some(socket) = &self.socket {
            match socket.send_bytes(logout_msg.to_bytes()).await {
                Ok(()) => tracing::debug!("Sent logout message"),
                Err(e) => tracing::error!("Error on logout: {e}"),
            }
        } else {
            anyhow::bail!("Socket not connected".to_string());
        }

        Ok(())
    }
}

// Can't be moved to core because we don't want to depend on tracing there
#[cfg(feature = "python")]
pub fn call_python(py: Python, callback: &Py<PyAny>, py_obj: Py<PyAny>) {
    if let Err(e) = callback.call1(py, (py_obj,)) {
        tracing::error!("Error calling Python: {e}");
    }
}
