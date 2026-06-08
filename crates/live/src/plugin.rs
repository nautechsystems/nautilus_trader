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

//! Live-node plug-in support.
//!
//! The public exports mirror `nautilus_plugin::bridge` so existing
//! `nautilus_live::plugin::*` imports keep working. The crate-local runtime
//! pieces below load configured plug-ins, build node adapters, and manage
//! plug-in controller lifecycle state.

use std::path::Path;

use ahash::AHashSet;
use anyhow::Context;
use aws_lc_rs::digest;
use nautilus_core::hex;
use nautilus_model::identifiers::{ActorId, StrategyId};
pub use nautilus_plugin::bridge::*;
use nautilus_plugin::loader::PluginLoader;
use nautilus_trading::strategy::StrategyConfig;

use crate::config::PluginConfig;

#[derive(Debug, Default)]
pub(crate) struct NodePlugins {
    loader: Option<PluginLoader>,
    controllers: Vec<PluginControllerAdapter>,
    controllers_started: bool,
}

impl NodePlugins {
    pub(crate) fn set_loader(&mut self, loader: PluginLoader) {
        self.loader = Some(loader);
    }

    pub(crate) fn push_controller(&mut self, controller: PluginControllerAdapter) {
        self.controllers.push(controller);
    }

    pub(crate) fn load_plugin(
        &mut self,
        config: &PluginConfig,
    ) -> anyhow::Result<NodePluginAdapter> {
        verify_plugin_sha256(config)?;

        let entry = {
            let loader = self.loader.get_or_insert_with(plugin_loader);
            let path = Path::new(&config.path);
            let already_loaded = loader.loaded().iter().any(|loaded| loaded.path() == path);

            if !already_loaded {
                let loaded = loader
                    .load(&config.path)
                    .with_context(|| format!("failed to load plug-in '{}'", config.path))?;
                let registered = register_manifest_custom_data(loaded.validated_manifest())
                    .with_context(|| {
                        format!(
                            "failed to register custom data from plug-in '{}'",
                            loaded.path().display()
                        )
                    })?;

                if registered > 0 {
                    log::info!(
                        "Registered {registered} custom data type(s) from plug-in {}",
                        loaded.path().display()
                    );
                }
            }

            let loaded = loader
                .loaded()
                .iter()
                .find(|loaded| loaded.path() == path)
                .ok_or_else(|| anyhow::anyhow!("plug-in '{}' was not loaded", config.path))?;
            configured_entry(loaded.validated_manifest(), &config.path, &config.type_name)?
        };

        configured_plugin_adapter(entry, config)
    }

    pub(crate) fn start_controllers(&mut self) -> anyhow::Result<()> {
        if self.controllers_started {
            return Ok(());
        }

        for index in 0..self.controllers.len() {
            let result = {
                let controller = &mut self.controllers[index];
                controller.on_start().with_context(|| {
                    format!(
                        "failed to start plug-in controller '{}' from plug-in '{}'",
                        controller.type_name(),
                        controller.plugin_name()
                    )
                })
            };

            if let Err(start_err) = result {
                for controller in self.controllers[..index].iter_mut().rev() {
                    if let Err(stop_err) = controller.on_stop() {
                        log::error!(
                            "Failed to roll back plug-in controller '{}' from plug-in '{}': {stop_err}",
                            controller.type_name(),
                            controller.plugin_name()
                        );
                    }
                }
                return Err(start_err);
            }
        }

        self.controllers_started = true;
        Ok(())
    }

    pub(crate) fn stop_controllers(&mut self) -> anyhow::Result<()> {
        if !self.controllers_started {
            return Ok(());
        }

        let mut first_error = None;

        for controller in self.controllers.iter_mut().rev() {
            if let Err(e) = controller.on_stop().with_context(|| {
                format!(
                    "failed to stop plug-in controller '{}' from plug-in '{}'",
                    controller.type_name(),
                    controller.plugin_name()
                )
            }) {
                log::error!("{e}");
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }

        self.controllers_started = false;

        if let Some(e) = first_error {
            Err(e)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug)]
pub(crate) enum NodePluginAdapter {
    Actor(Box<PluginActorAdapter>),
    Strategy(Box<PluginStrategyAdapter>),
    Controller(PluginControllerAdapter),
}

#[derive(Debug)]
pub(crate) struct NodePluginBatch {
    loader: PluginLoader,
    adapters: Vec<NodePluginAdapter>,
}

impl NodePluginBatch {
    pub(crate) fn into_parts(self) -> (PluginLoader, Vec<NodePluginAdapter>) {
        (self.loader, self.adapters)
    }
}

pub(crate) fn load_configured_plugin_batch(
    configs: &[PluginConfig],
) -> anyhow::Result<NodePluginBatch> {
    let mut loader = plugin_loader();
    let mut loaded_paths = AHashSet::new();

    for config in configs {
        verify_plugin_sha256(config)?;
        if loaded_paths.insert(config.path.clone()) {
            loader
                .load(&config.path)
                .with_context(|| format!("failed to load plug-in '{}'", config.path))?;
        }
    }

    for loaded in loader.loaded() {
        let registered =
            register_manifest_custom_data(loaded.validated_manifest()).with_context(|| {
                format!(
                    "failed to register custom data from plug-in '{}'",
                    loaded.path().display()
                )
            })?;

        if registered > 0 {
            log::info!(
                "Registered {registered} custom data type(s) from plug-in {}",
                loaded.path().display()
            );
        }
    }

    let adapters = configs
        .iter()
        .map(|config| configured_plugin_adapter_from_loader(&loader, config))
        .collect::<anyhow::Result<_>>()?;

    Ok(NodePluginBatch { loader, adapters })
}

fn configured_plugin_adapter_from_loader(
    loader: &PluginLoader,
    config: &PluginConfig,
) -> anyhow::Result<NodePluginAdapter> {
    let loaded = loader
        .loaded()
        .iter()
        .find(|loaded| loaded.path() == Path::new(&config.path))
        .ok_or_else(|| anyhow::anyhow!("plug-in '{}' was not loaded", config.path))?;

    let entry = configured_entry(loaded.validated_manifest(), &config.path, &config.type_name)?;
    configured_plugin_adapter(entry, config)
}

fn configured_plugin_adapter(
    entry: ConfiguredPluginEntry,
    config: &PluginConfig,
) -> anyhow::Result<NodePluginAdapter> {
    let config_json = serde_json::to_string(&config.config)?;

    match entry {
        ConfiguredPluginEntry::Actor(entry) => {
            let actor_id = plugin_actor_id(config)?;
            let adapter = entry
                .create_adapter(actor_id, &config_json)
                .with_context(|| {
                    format!(
                        "failed to instantiate plug-in actor '{}' from {}",
                        config.type_name, config.path
                    )
                })?;
            Ok(NodePluginAdapter::Actor(Box::new(adapter)))
        }
        ConfiguredPluginEntry::Strategy(entry) => {
            let strategy_config = plugin_strategy_config(config)?;
            let adapter = entry
                .create_adapter(strategy_config, &config_json)
                .with_context(|| {
                    format!(
                        "failed to instantiate plug-in strategy '{}' from {}",
                        config.type_name, config.path
                    )
                })?;
            Ok(NodePluginAdapter::Strategy(Box::new(adapter)))
        }
        ConfiguredPluginEntry::Controller(entry) => {
            let adapter = entry.create_adapter(&config_json).with_context(|| {
                format!(
                    "failed to instantiate plug-in controller '{}' from {}",
                    config.type_name, config.path
                )
            })?;
            Ok(NodePluginAdapter::Controller(adapter))
        }
    }
}

fn verify_plugin_sha256(config: &PluginConfig) -> anyhow::Result<()> {
    let Some(expected) = &config.sha256 else {
        return Ok(());
    };

    let bytes = std::fs::read(&config.path)
        .with_context(|| format!("failed to read plug-in '{}'", config.path))?;
    let actual = hex::encode(digest::digest(&digest::SHA256, &bytes).as_ref());
    if actual.eq_ignore_ascii_case(expected) {
        return Ok(());
    }

    anyhow::bail!(
        "plug-in '{}' SHA-256 mismatch: expected {}, actual {}",
        config.path,
        expected,
        actual
    )
}

fn plugin_actor_id(config: &PluginConfig) -> anyhow::Result<ActorId> {
    let actor_id = plugin_config_string(config, "actor_id")?.unwrap_or(&config.type_name);
    ActorId::new_checked(actor_id)
        .map_err(|e| anyhow::anyhow!("invalid actor_id for plug-in '{}': {e}", config.type_name))
}

fn plugin_strategy_config(config: &PluginConfig) -> anyhow::Result<StrategyConfig> {
    let mut strategy_config = if let Some(value) = config.config.get("strategy_config") {
        serde_json::from_value::<StrategyConfig>(value.clone()).with_context(|| {
            format!(
                "invalid strategy_config for plug-in strategy '{}'",
                config.type_name
            )
        })?
    } else {
        StrategyConfig::default()
    };

    if strategy_config.strategy_id.is_none() {
        let strategy_id = plugin_config_string(config, "strategy_id")?
            .map_or_else(|| format!("{}-001", config.type_name), str::to_string);
        strategy_config.strategy_id = Some(StrategyId::new_checked(&strategy_id).map_err(|e| {
            anyhow::anyhow!(
                "invalid strategy_id for plug-in strategy '{}': {e}",
                config.type_name
            )
        })?);
    }

    if strategy_config.order_id_tag.is_none()
        && let Some(order_id_tag) = plugin_config_string(config, "order_id_tag")?
    {
        strategy_config.order_id_tag = Some(order_id_tag.to_string());
    }

    Ok(strategy_config)
}

fn plugin_config_string<'a>(
    config: &'a PluginConfig,
    key: &'static str,
) -> anyhow::Result<Option<&'a str>> {
    match config.config.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value.as_str())),
        Some(_) => anyhow::bail!(
            "plug-in '{}' config field '{key}' must be a string",
            config.type_name
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf};

    use nautilus_core::UUID4;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_verify_plugin_sha256_accepts_matching_uppercase_digest() {
        let bytes = b"plugin bytes";
        let path = write_plugin_bytes(bytes);
        let config = PluginConfig {
            path: path.to_string_lossy().into_owned(),
            type_name: "ExampleActor".to_string(),
            config: HashMap::new(),
            sha256: Some(sha256_hex(bytes).to_uppercase()),
        };

        let result = verify_plugin_sha256(&config);
        std::fs::remove_file(path).unwrap();

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_verify_plugin_sha256_rejects_mismatch() {
        let path = write_plugin_bytes(b"plugin bytes");
        let config = PluginConfig {
            path: path.to_string_lossy().into_owned(),
            type_name: "ExampleActor".to_string(),
            config: HashMap::new(),
            sha256: Some("0".repeat(64)),
        };

        let error = verify_plugin_sha256(&config).unwrap_err().to_string();
        std::fs::remove_file(path).unwrap();

        assert!(error.contains("SHA-256 mismatch"));
    }

    #[rstest]
    fn test_verify_plugin_sha256_reports_missing_file() {
        let path = plugin_test_path();
        let config = PluginConfig {
            path: path.to_string_lossy().into_owned(),
            type_name: "ExampleActor".to_string(),
            config: HashMap::new(),
            sha256: Some("0".repeat(64)),
        };

        let error = verify_plugin_sha256(&config).unwrap_err().to_string();

        assert!(error.contains("failed to read plug-in"));
    }

    #[rstest]
    fn test_verify_plugin_sha256_skips_missing_digest() {
        let path = plugin_test_path();
        let config = PluginConfig {
            path: path.to_string_lossy().into_owned(),
            type_name: "ExampleActor".to_string(),
            config: HashMap::new(),
            sha256: None,
        };

        assert!(verify_plugin_sha256(&config).is_ok());
    }

    #[rstest]
    fn test_plugin_actor_id_rejects_non_string_actor_id() {
        let config = PluginConfig {
            path: "./libexample.so".to_string(),
            type_name: "ExampleActor".to_string(),
            config: HashMap::from([("actor_id".to_string(), serde_json::json!(42))]),
            sha256: None,
        };

        let error = plugin_actor_id(&config).unwrap_err().to_string();

        assert!(error.contains("actor_id"));
        assert!(error.contains("must be a string"));
    }

    #[rstest]
    fn test_plugin_strategy_config_accepts_nested_strategy_config() {
        let config = PluginConfig {
            path: "./libexample.so".to_string(),
            type_name: "ExampleStrategy".to_string(),
            config: HashMap::from([(
                "strategy_config".to_string(),
                serde_json::json!({
                    "strategy_id": "NestedStrategy-001",
                    "order_id_tag": "NEST",
                }),
            )]),
            sha256: None,
        };

        let strategy_config = plugin_strategy_config(&config).unwrap();

        assert_eq!(
            strategy_config.strategy_id,
            Some(StrategyId::from("NestedStrategy-001"))
        );
        assert_eq!(strategy_config.order_id_tag.as_deref(), Some("NEST"));
    }

    #[rstest]
    fn test_plugin_strategy_config_uses_default_strategy_id() {
        let config = PluginConfig {
            path: "./libexample.so".to_string(),
            type_name: "ExampleStrategy".to_string(),
            config: HashMap::new(),
            sha256: None,
        };

        let strategy_config = plugin_strategy_config(&config).unwrap();

        assert_eq!(
            strategy_config.strategy_id,
            Some(StrategyId::from("ExampleStrategy-001"))
        );
    }

    #[rstest]
    fn test_plugin_strategy_config_uses_top_level_strategy_id_and_order_id_tag() {
        let config = PluginConfig {
            path: "./libexample.so".to_string(),
            type_name: "ExampleStrategy".to_string(),
            config: HashMap::from([
                (
                    "strategy_id".to_string(),
                    serde_json::json!("TopLevelStrategy-001"),
                ),
                ("order_id_tag".to_string(), serde_json::json!("TOP")),
            ]),
            sha256: None,
        };

        let strategy_config = plugin_strategy_config(&config).unwrap();

        assert_eq!(
            strategy_config.strategy_id,
            Some(StrategyId::from("TopLevelStrategy-001"))
        );
        assert_eq!(strategy_config.order_id_tag.as_deref(), Some("TOP"));
    }

    fn write_plugin_bytes(bytes: &[u8]) -> PathBuf {
        let path = plugin_test_path();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn plugin_test_path() -> PathBuf {
        std::env::temp_dir().join(format!("nautilus-live-plugin-{}.bin", UUID4::new()))
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        hex::encode(digest::digest(&digest::SHA256, bytes).as_ref())
    }
}
