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

//! Tardis base URL constants and resolution helpers.

use super::consts::TARDIS_MACHINE_WS_URL;

/// Default Tardis REST API base URL.
pub const TARDIS_HTTP_BASE_URL: &str = "https://api.tardis.dev/v1";

/// Resolves the Tardis Machine WebSocket base URL from an explicit value or the
/// `TARDIS_MACHINE_WS_URL` environment variable.
///
/// # Errors
///
/// Returns an error if neither `url` nor the environment variable is set.
pub fn resolve_ws_base_url(url: Option<&str>) -> anyhow::Result<String> {
    url.map(ToString::to_string)
        .or_else(|| std::env::var(TARDIS_MACHINE_WS_URL).ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Tardis Machine `base_url` must be provided or \
                 set in the '{TARDIS_MACHINE_WS_URL}' environment variable"
            )
        })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_resolve_ws_base_url_with_explicit_value() {
        let result = resolve_ws_base_url(Some("ws://localhost:8001")).unwrap();
        assert_eq!(result, "ws://localhost:8001");
    }

    #[rstest]
    fn test_resolve_ws_base_url_prefers_explicit_value() {
        let result = resolve_ws_base_url(Some("ws://custom:9999")).unwrap();
        assert_eq!(result, "ws://custom:9999");
    }
}
