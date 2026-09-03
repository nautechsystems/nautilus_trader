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

//! Authentication state and token refresh for Deribit WebSocket connections.

use std::time::Duration;

use nautilus_core::{UUID4, string::secret::SecretString, time::get_atomic_clock_realtime};
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::{
    handler::HandlerCommand,
    messages::{DeribitAuthParams, DeribitAuthResult, DeribitRefreshTokenParams},
};
use crate::common::credential::Credential;

/// Session name for Deribit WebSocket data client authentication.
pub const DERIBIT_DATA_SESSION_NAME: &str = "nautilus-data";

/// Session name for Deribit WebSocket execution client authentication.
pub const DERIBIT_EXECUTION_SESSION_NAME: &str = "nautilus-execution";

/// Authentication state storing OAuth tokens.
#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop)]
pub struct AuthState {
    /// Access token for API requests.
    pub access_token: SecretString,
    /// Refresh token for obtaining new access tokens.
    pub refresh_token: SecretString,
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
/// after reconnection. It generates the signature and sends auth params via the command channel.
/// The handler is responsible for generating the request ID.
///
/// # Arguments
///
/// - `credential` - API credentials for signing the request
/// - `scope` - Optional scope (e.g., "session:nautilus" for session-based auth)
/// - `cmd_tx` - Command channel to send the authentication request
pub fn send_auth_request(
    credential: &Credential,
    scope: Option<String>,
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
) {
    let timestamp = get_atomic_clock_realtime().get_time_ms();
    let nonce = UUID4::new().to_string();
    let signature = credential.sign_ws_auth(timestamp, &nonce, "");

    let mut auth_params = DeribitAuthParams {
        grant_type: "client_signature".to_string(),
        client_id: SecretString::from(credential.api_key()),
        timestamp,
        signature: SecretString::from(signature),
        nonce,
        data: SecretString::default(),
        scope,
    };

    let serialized = serde_json::to_string(&auth_params).map(SecretString::from);
    auth_params.zeroize();

    match serialized {
        Ok(auth_params) => {
            if let Err(e) = cmd_tx.send(HandlerCommand::Authenticate { auth_params }) {
                log::error!("Failed to send auth command: {e}");
            }
        }
        Err(e) => {
            log::error!("Failed to serialize auth params: {e}");
        }
    }
}

/// Refreshes the authentication token after 80% of its lifetime has elapsed.
///
/// When the refresh succeeds, a new `Authenticated` message will be received, which triggers
/// another delayed refresh and creates a continuous refresh cycle.
///
/// The `cancel_token` allows the owning handler to cancel a stale refresh when a new
/// authentication cycle begins.
pub async fn refresh_token_after_delay(
    expires_in: u64,
    refresh_token: SecretString,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    cancel_token: CancellationToken,
) {
    let refresh_delay_secs = (expires_in as f64 * 0.8) as u64;

    log::debug!(
        "Token refresh scheduled in {refresh_delay_secs}s (token expires in {expires_in}s)"
    );

    tokio::select! {
        () = tokio::time::sleep(Duration::from_secs(refresh_delay_secs)) => {}
        () = cancel_token.cancelled() => {
            log::debug!("Token refresh cancelled");
            return;
        }
    }

    log::debug!("Refreshing authentication token...");
    let mut refresh_params = DeribitRefreshTokenParams {
        grant_type: "refresh_token".to_string(),
        refresh_token,
    };

    let serialized = serde_json::to_string(&refresh_params).map(SecretString::from);
    refresh_params.zeroize();

    if let Ok(auth_params) = serialized {
        let _ = cmd_tx.send(HandlerCommand::Authenticate { auth_params });
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn assert_zeroize_on_drop<T: ZeroizeOnDrop>() {}

    #[rstest]
    fn test_auth_state_zeroizes_on_drop() {
        assert_zeroize_on_drop::<AuthState>();
    }

    #[rstest]
    #[tokio::test]
    async fn test_refresh_token_serialization_is_unchanged() {
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let refresh_token = ["refresh-", "token-value"].concat();

        refresh_token_after_delay(
            0,
            SecretString::from(refresh_token.clone()),
            cmd_tx,
            CancellationToken::new(),
        )
        .await;

        let command = cmd_rx.try_recv().expect("refresh command");
        let HandlerCommand::Authenticate { auth_params } = command else {
            panic!("expected authenticate command");
        };
        let auth_params: serde_json::Value =
            serde_json::from_str(auth_params.expose_secret()).expect("valid auth params");
        assert_eq!(
            auth_params,
            serde_json::json!({
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
            })
        );
    }
}
