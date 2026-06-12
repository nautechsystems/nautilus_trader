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

use std::{fs, io::Write, path::PathBuf};

use nautilus_plugin::prelude::*;

struct CallbackConfig {
    callback_path: Option<PathBuf>,
    label: String,
    fail_on_start: bool,
}

#[derive(Default)]
pub struct RuntimeSmokeActor {
    callback_path: Option<PathBuf>,
    label: String,
}

impl PluginActor for RuntimeSmokeActor {
    const TYPE_NAME: &'static str = "RuntimeSmokeActor";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, config_json: &str) -> Self {
        let config = parse_callback_config(config_json, "runtime-smoke");
        Self {
            callback_path: config.callback_path,
            label: config.label,
        }
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        if let Some(path) = &self.callback_path {
            append_callback(path, &self.label, "on_start")?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct RuntimeSmokeStrategy {
    callback_path: Option<PathBuf>,
    label: String,
}

impl PluginStrategy for RuntimeSmokeStrategy {
    const TYPE_NAME: &'static str = "RuntimeSmokeStrategy";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, config_json: &str) -> Self {
        let config = parse_callback_config(config_json, "runtime-smoke-strategy");
        Self {
            callback_path: config.callback_path,
            label: config.label,
        }
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        if let Some(path) = &self.callback_path {
            append_callback(path, &self.label, "on_start")?;
        }
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        if let Some(path) = &self.callback_path {
            append_callback(path, &self.label, "on_stop")?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct RuntimeSmokeController {
    callback_path: Option<PathBuf>,
    label: String,
    fail_on_start: bool,
}

impl PluginController for RuntimeSmokeController {
    const TYPE_NAME: &'static str = "RuntimeSmokeController";

    fn new(
        _host: *const ControllerHostVTable,
        _ctx: *const ControllerHostContext,
        config_json: &str,
    ) -> Self {
        let config = parse_callback_config(config_json, "runtime-smoke-controller");
        Self {
            callback_path: config.callback_path,
            label: config.label,
            fail_on_start: config.fail_on_start,
        }
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        if let Some(path) = &self.callback_path {
            append_callback(path, &self.label, "on_start")?;
        }

        if self.fail_on_start {
            anyhow::bail!("configured controller start failure")
        }

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        if let Some(path) = &self.callback_path {
            append_callback(path, &self.label, "on_stop")?;
        }
        Ok(())
    }
}

fn parse_callback_config(config_json: &str, default_label: &str) -> CallbackConfig {
    let config = serde_json::from_str::<serde_json::Value>(config_json)
        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default()));
    let callback_path = config
        .get("callback_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let label = config
        .get("label")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(default_label)
        .to_string();
    let fail_on_start = config
        .get("fail_on_start")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    CallbackConfig {
        callback_path,
        label,
        fail_on_start,
    }
}

fn append_callback(path: &PathBuf, label: &str, hook: &str) -> anyhow::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{label}:{hook}")?;
    Ok(())
}

nautilus_plugin::nautilus_plugin! {
    name: "runtime-smoke-plugin",
    vendor: "Nautech",
    version: env!("CARGO_PKG_VERSION"),
    actors: [RuntimeSmokeActor],
    strategies: [RuntimeSmokeStrategy],
    controllers: [RuntimeSmokeController],
}

#[allow(dead_code)]
fn main() {}
