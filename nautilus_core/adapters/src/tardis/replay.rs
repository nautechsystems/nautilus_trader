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

use std::{fs, path::Path};

use crate::tardis::config::TardisReplayConfig;

pub async fn run_tardis_machine_replay(config_filepath: &Path) {
    let config_data = fs::read_to_string(config_filepath).expect("Failed to read config file");
    let config: TardisReplayConfig =
        serde_json::from_str(&config_data).expect("Failed to parse config JSON");

    println!("{config:?}"); // TODO: WIP
}
