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

/// Loads and deserializes a JSON test fixture from the `test_data/` directory.
#[expect(clippy::missing_panics_doc, reason = "test-only helper")]
pub fn load_test_data<T>(filename: &str) -> T
where
    T: serde::de::DeserializeOwned,
{
    let path = format!("test_data/{filename}");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read test data at {path}: {e}"));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse test data at {path}: {e}"))
}
