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

//! Minimal plug-in cdylib showcasing the `nautilus_plugin!` macro.
//!
//! Build with `cargo build -p nautilus-plugin --example custom_data_plugin`. The
//! resulting artifact lives at
//! `target/<profile>/examples/libcustom_data_plugin.<ext>` and can be loaded
//! via [`nautilus_plugin::loader::PluginLoader`].
//!
//! This example registers a single custom-data type and uses sentinel byte
//! encodings instead of real Arrow IPC so it can stand alone without an Arrow
//! dependency. Production plug-ins serialize their schema via
//! `arrow::ipc::writer::StreamWriter`.

use std::{fs, path::PathBuf};

use nautilus_model::data::QuoteTick;
use nautilus_plugin::prelude::*;

#[derive(Default)]
pub struct ExampleActor {
    quotes_seen: u64,
    callback_path: Option<PathBuf>,
}

impl PluginActor for ExampleActor {
    const TYPE_NAME: &'static str = "ExampleActor";

    fn new(_host: *const HostVTable, _ctx: *const HostContext, config_json: &str) -> Self {
        Self {
            quotes_seen: 0,
            callback_path: config_callback_path(config_json),
        }
    }

    fn on_data(&mut self, data: PluginCustomDataRef) -> anyhow::Result<()> {
        let tick = data
            .downcast_ref::<ExampleTick>()
            .ok_or_else(|| anyhow::anyhow!("expected ExampleTick custom data"))?;

        if let Some(path) = &self.callback_path {
            fs::write(path, tick.value.to_string())?;
        }
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        self.quotes_seen += 1;
        Ok(())
    }
}

pub struct ExampleStrategy {
    _host: *const HostVTable,
    _ctx: *const HostContext,
    quotes_seen: u64,
    callback_path: Option<PathBuf>,
}

// SAFETY: ExampleStrategy holds opaque host pointers the host commits to
// keeping live for the strategy's lifetime; the trait is `Send`.
unsafe impl Send for ExampleStrategy {}

impl PluginStrategy for ExampleStrategy {
    const TYPE_NAME: &'static str = "ExampleStrategy";

    fn new(host: *const HostVTable, ctx: *const HostContext, config_json: &str) -> Self {
        Self {
            _host: host,
            _ctx: ctx,
            quotes_seen: 0,
            callback_path: config_callback_path(config_json),
        }
    }

    fn on_data(&mut self, data: PluginCustomDataRef) -> anyhow::Result<()> {
        let tick = data
            .downcast_ref::<ExampleTick>()
            .ok_or_else(|| anyhow::anyhow!("expected ExampleTick custom data"))?;

        if let Some(path) = &self.callback_path {
            fs::write(path, tick.value.to_string())?;
        }
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        self.quotes_seen += 1;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExampleTick {
    pub value: f64,
    pub ts_event: u64,
    pub ts_init: u64,
}

impl PluginCustomData for ExampleTick {
    const TYPE_NAME: &'static str = "ExampleTick";

    fn ts_event(&self) -> u64 {
        self.ts_event
    }

    fn ts_init(&self) -> u64 {
        self.ts_init
    }

    fn to_json(&self) -> anyhow::Result<Vec<u8>> {
        Ok(format!(
            r#"{{"value":{},"ts_event":{},"ts_init":{}}}"#,
            self.value, self.ts_event, self.ts_init
        )
        .into_bytes())
    }

    fn from_json(payload: &[u8]) -> anyhow::Result<Self> {
        let text = std::str::from_utf8(payload)?;
        let mut value = 0.0;
        let mut ts_event = 0u64;
        let mut ts_init = 0u64;

        for part in text.trim_matches(['{', '}']).split(',') {
            let mut kv = part.splitn(2, ':');
            let key = kv.next().unwrap_or("").trim_matches('"');
            let v = kv.next().unwrap_or("");
            match key {
                "value" => value = v.parse()?,
                "ts_event" => ts_event = v.parse()?,
                "ts_init" => ts_init = v.parse()?,
                _ => {}
            }
        }
        Ok(Self {
            value,
            ts_event,
            ts_init,
        })
    }

    fn schema_ipc() -> anyhow::Result<Vec<u8>> {
        Ok(b"example-schema".to_vec())
    }

    fn encode_batch(items: &[&Self]) -> anyhow::Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(&u32::try_from(items.len()).unwrap().to_le_bytes());
        for it in items {
            let json = it.to_json()?;
            out.extend_from_slice(&u32::try_from(json.len()).unwrap().to_le_bytes());
            out.extend_from_slice(&json);
        }
        Ok(out)
    }

    fn decode_batch(ipc_bytes: &[u8], _metadata: &[(String, String)]) -> anyhow::Result<Vec<Self>> {
        let mut cursor = 0;
        let count = u32::from_le_bytes(ipc_bytes[cursor..cursor + 4].try_into()?) as usize;
        cursor += 4;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let len = u32::from_le_bytes(ipc_bytes[cursor..cursor + 4].try_into()?) as usize;
            cursor += 4;
            let chunk = &ipc_bytes[cursor..cursor + len];
            cursor += len;
            out.push(Self::from_json(chunk)?);
        }
        Ok(out)
    }
}

fn config_callback_path(config_json: &str) -> Option<PathBuf> {
    let config = serde_json::from_str::<serde_json::Value>(config_json).ok()?;
    config
        .get("callback_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
}

nautilus_plugin::nautilus_plugin! {
    name: "example-custom-data-plugin",
    vendor: "Nautech",
    version: env!("CARGO_PKG_VERSION"),
    custom_data: [ExampleTick],
    actors: [ExampleActor],
    strategies: [ExampleStrategy],
}

// The `[[example]]` cdylib still needs a `main` to satisfy cargo's example
// build pipeline. It is never called when the artifact is loaded as a cdylib.
#[allow(dead_code)]
fn main() {}
