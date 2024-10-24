// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{fs, path::PathBuf, sync::LazyLock};

use toml::Value;

fn read_nautilus_version() -> String {
    let filepath = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Failed to get workspace root")
        .parent()
        .expect("Failed to get project root")
        .join("pyproject.toml");

    let data = fs::read_to_string(filepath).expect("Unable to read pyproject.toml");
    let parsed_toml: Value = toml::from_str(&data).expect("Unable to parse pyproject.toml");

    parsed_toml["tool"]["poetry"]["version"]
        .as_str()
        .expect("Unable to find version in pyproject.toml")
        .to_string()
}

/// Common User-Agent value for `NautilusTrader`.
pub static USER_AGENT: LazyLock<String> =
    LazyLock::new(|| format!("NautilusTrader/{}", read_nautilus_version()));
