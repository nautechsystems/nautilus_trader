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

use std::path::PathBuf;

#[must_use]
pub fn get_workspace_root_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Failed to get project root")
        .to_path_buf()
}

#[must_use]
pub fn get_project_root_path() -> PathBuf {
    get_workspace_root_path()
        .parent()
        .expect("Failed to get workspace root")
        .to_path_buf()
}

#[must_use]
pub fn get_project_testdata_path() -> PathBuf {
    get_project_root_path().join("tests").join("test_data")
}

#[must_use]
pub fn get_testdata_large_checksums_filepath() -> PathBuf {
    get_project_testdata_path()
        .join("large")
        .join("checksums.json")
}
