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

use std::{fs, sync::LazyLock};

use toml::Value;

use crate::paths::get_project_root_path;

// Reads the NautilusTrader version from the top-level `pyproject.toml`.
fn read_nautilus_version() -> String {
    let filepath = get_project_root_path().join("pyproject.toml");
    let data = fs::read_to_string(filepath).expect("Unable to read pyproject.toml");
    let parsed_toml: Value = toml::from_str(&data).expect("Unable to parse pyproject.toml");

    parsed_toml["tool"]["poetry"]["version"]
        .as_str()
        .expect("Unable to find version in pyproject.toml")
        .to_string()
}

/// The NautilusTrader version string read from the top-level `pyproject.toml`.
pub static NAUTILUS_VERSION: LazyLock<String> = LazyLock::new(read_nautilus_version);

/// The NautilusTrader common User-Agent string including the current version.
pub static USER_AGENT: LazyLock<String> =
    LazyLock::new(|| format!("NautilusTrader/{}", NAUTILUS_VERSION.clone()));
