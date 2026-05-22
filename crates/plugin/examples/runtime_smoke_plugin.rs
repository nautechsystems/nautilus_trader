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

//! Plug-in cdylib used by live-node runtime smoke tests.

use std::{fs, path::PathBuf};

use nautilus_plugin::prelude::*;

#[derive(Default)]
pub struct RuntimeSmokeActor {
    callback_path: Option<PathBuf>,
    label: String,
}

impl PluginActor for RuntimeSmokeActor {
    const TYPE_NAME: &'static str = "RuntimeSmokeActor";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, config_json: &str) -> Self {
        let config = serde_json::from_str::<serde_json::Value>(config_json)
            .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
        let callback_path = config
            .get("callback_path")
            .and_then(serde_json::Value::as_str)
            .map(PathBuf::from);
        let label = config
            .get("label")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("runtime-smoke")
            .to_string();

        Self {
            callback_path,
            label,
        }
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        if let Some(path) = &self.callback_path {
            fs::write(path, format!("{}:on_start\n", self.label))?;
        }
        Ok(())
    }
}

nautilus_plugin::nautilus_plugin! {
    name: "runtime-smoke-plugin",
    vendor: "Nautech",
    version: env!("CARGO_PKG_VERSION"),
    actors: [RuntimeSmokeActor],
}

#[allow(dead_code)]
fn main() {}
