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

//! Authentication state and token refresh for Deribit WebSocket connections.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use nautilus_common::live::get_runtime;
use nautilus_core::{UUID4, time::get_atomic_clock_realtime};

use super::{
    handler::HandlerCommand,
    messages::{DeribitAuthParams, DeribitAuthResult, DeribitRefreshTokenParams},
};
use crate::common::{credential::Credential, rpc::DeribitJsonRpcRequest};

/// Default session name for Deribit WebSocket authentication.
pub const DEFAULT_SESSION_NAME: &str = "nautilus";

/// Authentication state storing OAuth tokens.
#[derive(Debug, Clone)]
pub struct AuthState {
    /// Access token for API requests.
    pub access_token: String,
    /// Refresh token for obtaining new access tokens.
    pub refresh_token: String,
    /// Token expiration time in seconds from authentication.
    pub expires_in: u64,
    /// Timestamp when tokens were obtained (Unix milliseconds).
    pub obtained_at: u64,
    /// Scope used for authentication.
    pub scope: String,
}

impl AuthState {
    /// Creates a new [`AuthState`] from an authentication result.
    #[must_use]
    pub fn from_auth_result(result: &DeribitAuthResult, obtained_at: u64) -> Self {
        Self {
            access_token: result.access_token.clone(),
            refresh_token: result.refresh_token.clone(),
            expires_in: result.expires_in,
            obtained_at,
            scope: result.scope.clone(),
        }
    }

    /// Returns the expiration timestamp in Unix milliseconds.
    #[must_use]
    pub fn expires_at_ms(&self) -> u64 {
        self.obtained_at + (self.expires_in * 1000)
    }

    /// Returns whether the token is expired or near expiry (within 60 seconds).
    #[must_use]
    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        // Consider expired if within 60 seconds of expiry
        current_time_ms + 60_000 >= self.expires_at_ms()
    }

    /// Returns whether this is a session-scoped authentication.
    #[must_use]
    pub fn is_session_scoped(&self) -> bool {
        self.scope.starts_with("session:")
    }
}

/// Sends an authentication request using client_signature grant type.
///
/// This is a helper function used by both initial authentication and re-authentication
/// after reconnection. It generates the signature, creates the JSON-RPC request, and
/// sends it via the command channel.
///
/// # Arguments
///
/// * `credential` - API credentials for signing the request
/// * `scope` - Optional scope (e.g., "session:nautilus" for session-based auth)
/// * `cmd_tx` - Command channel to send the authentication request
/// * `request_id_counter` - Counter for generating unique request IDs
pub fn send_auth_request(
    credential: &Credential,
    scope: Option<String>,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    request_id_counter: &Arc<AtomicU64>,
) {
    let timestamp = get_atomic_clock_realtime().get_time_ms();
    let nonce = UUID4::new().to_string();
    let signature = credential.sign_ws_auth(timestamp, &nonce, "");

    let auth_params = DeribitAuthParams {
        grant_type: "client_signature".to_string(),
        client_id: credential.api_key.to_string(),
        timestamp,
        signature,
        nonce,
        data: String::new(),
        scope,
    };

    let request_id = request_id_counter.fetch_add(1, Ordering::Relaxed);
    let request = DeribitJsonRpcRequest::new(request_id, "public/auth", auth_params);

    if let Ok(payload) = serde_json::to_string(&request) {
        let _ = cmd_tx.send(HandlerCommand::Authenticate { payload });
    }
}

/// Spawns a background task to refresh the authentication token before it expires.
///
/// The task sleeps until 80% of the token lifetime has passed, then sends a refresh request.
/// When the refresh succeeds, a new `Authenticated` message will be received, which triggers
/// another refresh task - creating a continuous refresh cycle.
pub fn spawn_token_refresh_task(
    expires_in: u64,
    refresh_token: String,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    request_id_counter: Arc<AtomicU64>,
) {
    // Refresh at 80% of token lifetime to ensure we never expire
    let refresh_delay_secs = (expires_in as f64 * 0.8) as u64;

    get_runtime().spawn(async move {
        tracing::debug!(
            "Token refresh scheduled in {}s (token expires in {}s)",
            refresh_delay_secs,
            expires_in
        );
        tokio::time::sleep(Duration::from_secs(refresh_delay_secs)).await;

        tracing::info!("Refreshing authentication token...");
        let refresh_params = DeribitRefreshTokenParams {
            grant_type: "refresh_token".to_string(),
            refresh_token,
        };

        let request_id = request_id_counter.fetch_add(1, Ordering::Relaxed);
        let request = DeribitJsonRpcRequest::new(request_id, "public/auth", refresh_params);

        if let Ok(payload) = serde_json::to_string(&request) {
            let _ = cmd_tx.send(HandlerCommand::Authenticate { payload });
        }
    });
}
