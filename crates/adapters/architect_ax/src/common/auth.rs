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

//! Authentication-token refresh lifecycle shared by AX clients.

use std::{fmt::Display, time::Instant};

use nautilus_common::live::get_runtime;
use tokio::task::JoinHandle;

use super::{
    consts::{
        AX_AUTH_TOKEN_REFRESH_INTERVAL, AX_AUTH_TOKEN_REFRESH_RETRY_DELAY,
        AX_AUTH_TOKEN_REQUEST_TIMEOUT, AX_AUTH_TOKEN_TTL_SECS,
    },
    credential::Credential,
};
use crate::http::client::AxHttpClient;

/// Spawns an AX authentication-token refresh task.
///
/// A refreshed token becomes the HTTP session token during authentication, then `update_token`
/// makes it available to future WebSocket reconnect handshakes.
pub fn spawn_auth_token_refresh<E>(
    http_client: AxHttpClient,
    credential: Credential,
    update_token: impl Fn(String) -> Result<(), E> + Send + 'static,
) -> JoinHandle<()>
where
    E: Display + Send + 'static,
{
    get_runtime().spawn(async move {
        let conservative_ttl = std::time::Duration::from_secs(AX_AUTH_TOKEN_TTL_SECS as u64)
            .saturating_sub(AX_AUTH_TOKEN_REQUEST_TIMEOUT);
        let mut fully_propagated_expiry = Instant::now() + conservative_ttl;
        let mut next_delay = AX_AUTH_TOKEN_REFRESH_INTERVAL;

        loop {
            tokio::time::sleep(next_delay).await;
            let request_started = Instant::now();

            let result = tokio::time::timeout(
                AX_AUTH_TOKEN_REQUEST_TIMEOUT,
                http_client.authenticate(
                    credential.api_key(),
                    credential.api_secret(),
                    AX_AUTH_TOKEN_TTL_SECS,
                ),
            )
            .await;

            let error = match result {
                Ok(Ok(token)) => match update_token(token) {
                    Ok(()) => {
                        fully_propagated_expiry = request_started
                            + std::time::Duration::from_secs(AX_AUTH_TOKEN_TTL_SECS as u64);
                        next_delay = AX_AUTH_TOKEN_REFRESH_INTERVAL;
                        log::debug!("AX authentication token refreshed");
                        continue;
                    }
                    Err(e) => format!("failed to update WebSocket reconnect authentication: {e}"),
                },
                Ok(Err(e)) => format!("authentication request failed: {e}"),
                Err(_) => format!(
                    "authentication request timed out after {}s",
                    AX_AUTH_TOKEN_REQUEST_TIMEOUT.as_secs()
                ),
            };

            if Instant::now() >= fully_propagated_expiry {
                log::error!(
                    "AX authentication token refresh failed after the last fully propagated token expired: {error}"
                );
            } else {
                log::warn!("AX authentication token refresh failed: {error}");
            }
            next_delay = AX_AUTH_TOKEN_REFRESH_RETRY_DELAY;
        }
    })
}
