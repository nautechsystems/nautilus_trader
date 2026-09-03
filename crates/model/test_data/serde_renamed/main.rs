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

use std::{fmt::Display, str::FromStr};

use nautilus_model::enum_strum_serde;

#[derive(Debug)]
enum ConsumerEnum {
    Value,
}

impl Display for ConsumerEnum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VALUE")
    }
}

impl FromStr for ConsumerEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "VALUE" => Ok(Self::Value),
            _ => Err("invalid value"),
        }
    }
}

enum_strum_serde!(ConsumerEnum);

fn assert_serde<T>()
where
    T: renamed_serde::Serialize + for<'de> renamed_serde::Deserialize<'de>,
{
}

fn main() {
    assert_serde::<ConsumerEnum>();
}
