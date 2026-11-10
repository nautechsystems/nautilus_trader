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

//! FinFeed credential resolution at construction.

use nautilus_core::string::secret::SecretString;

use crate::http::error::Error;

/// Default environment source for prediction-market access.
pub const FINFEED_API_KEY: &str = "FINFEED_API_KEY";

/// Resolves an explicit key or reads `FINFEED_API_KEY` once.
///
/// # Errors
///
/// Returns an error if the key is missing, empty, or contains anything other than visible ASCII characters.
pub fn resolve_api_key(api_key: Option<SecretString>) -> Result<SecretString, Error> {
    let api_key = match api_key {
        Some(api_key) => api_key,
        None => std::env::var(FINFEED_API_KEY)
            .map(SecretString::from)
            .map_err(|_| Error::Validation("set api_key or FINFEED_API_KEY".into()))?,
    };

    if api_key.expose_secret().is_empty()
        || !api_key
            .expose_secret()
            .bytes()
            .all(|b| (33..=126).contains(&b))
    {
        return Err(Error::Validation(
            "API key must contain visible ASCII".into(),
        ));
    }

    Ok(api_key)
}
