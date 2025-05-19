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

/// Returns the environment variable for the given `key`.
///
/// # Errors
///
/// Returns an error if the environment variable is not set.
pub fn get_env_var(key: &str) -> anyhow::Result<String> {
    match std::env::var(key) {
        Ok(var) => Ok(var),
        Err(_) => anyhow::bail!("environment variable '{key}' must be set"),
    }
}
