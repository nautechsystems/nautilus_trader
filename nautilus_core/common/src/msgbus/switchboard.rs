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

use ustr::Ustr;

/// Represents a switchboard of built-in messaging endpoint names.
#[derive(Clone, Debug)]
pub struct MessagingSwitchboard {
    pub data_engine_execute: Ustr,
    pub data_engine_process: Ustr,
    pub exec_engine_execute: Ustr,
    pub exec_engine_process: Ustr,
}

impl Default for MessagingSwitchboard {
    fn default() -> Self {
        Self {
            data_engine_execute: Ustr::from("DataEngine.execute"),
            data_engine_process: Ustr::from("DataEngine.process"),
            exec_engine_execute: Ustr::from("ExecEngine.execute"),
            exec_engine_process: Ustr::from("ExecEngine.process"),
        }
    }
}
