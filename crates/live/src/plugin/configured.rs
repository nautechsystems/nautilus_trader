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

//! Safe wrappers for config-driven plug-in registration and adapter construction.

#![allow(unsafe_code)]

use nautilus_model::identifiers::ActorId;
use nautilus_plugin::{
    manifest::PluginManifest,
    surfaces::{actor::ActorVTable, strategy::StrategyVTable},
};
use nautilus_trading::strategy::StrategyConfig;

use crate::plugin::{
    PluginActorAdapter, PluginStrategyAdapter, host_vtable, register_custom_data_from_manifest,
};

/// Config-resolved plug-in component entry.
pub(crate) enum ConfiguredPluginEntry {
    Actor(ConfiguredActorEntry),
    Strategy(ConfiguredStrategyEntry),
}

/// Actor entry copied from a loaded manifest.
pub(crate) struct ConfiguredActorEntry {
    plugin_name: String,
    type_name: String,
    vtable: *const ActorVTable,
}

/// Strategy entry copied from a loaded manifest.
pub(crate) struct ConfiguredStrategyEntry {
    plugin_name: String,
    type_name: String,
    vtable: *const StrategyVTable,
}

impl ConfiguredActorEntry {
    /// Creates a host-side adapter for this configured actor entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in vtable rejects construction.
    pub(crate) fn create_adapter(
        &self,
        actor_id: ActorId,
        config_json: &str,
    ) -> anyhow::Result<PluginActorAdapter> {
        // SAFETY: entries come from a manifest owned by `PluginLoader`, and
        // `host_vtable()` is process-lifetime static.
        unsafe {
            PluginActorAdapter::new(
                actor_id,
                self.plugin_name.clone(),
                self.type_name.clone(),
                self.vtable,
                host_vtable(),
                config_json,
            )
        }
    }
}

impl ConfiguredStrategyEntry {
    /// Creates a host-side adapter for this configured strategy entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the plug-in vtable rejects construction.
    pub(crate) fn create_adapter(
        &self,
        strategy_config: StrategyConfig,
        config_json: &str,
    ) -> anyhow::Result<PluginStrategyAdapter> {
        // SAFETY: entries come from a manifest owned by `PluginLoader`, and
        // `host_vtable()` is process-lifetime static.
        unsafe {
            PluginStrategyAdapter::new(
                strategy_config,
                self.plugin_name.clone(),
                self.type_name.clone(),
                self.vtable,
                host_vtable(),
                config_json,
            )
        }
    }
}

/// Registers every custom data type declared by a loaded manifest.
///
/// # Errors
///
/// Returns an error if the host-side custom-data registry rejects an entry.
pub(crate) fn register_manifest_custom_data(manifest: &PluginManifest) -> anyhow::Result<usize> {
    // SAFETY: the manifest originates from `PluginLoader`, which keeps the
    // owning cdylib live for the process lifetime.
    unsafe { register_custom_data_from_manifest(manifest) }
}

/// Resolves an actor or strategy entry from a loaded manifest by type name.
///
/// # Errors
///
/// Returns an error when the type is missing or ambiguous.
pub(crate) fn configured_entry(
    manifest: &PluginManifest,
    path: &str,
    type_name: &str,
) -> anyhow::Result<ConfiguredPluginEntry> {
    let plugin_name = manifest_plugin_name(manifest);
    let actor_entry = find_actor_entry(manifest, type_name);
    let strategy_entry = find_strategy_entry(manifest, type_name);

    match (actor_entry, strategy_entry) {
        (Some(vtable), None) => Ok(ConfiguredPluginEntry::Actor(ConfiguredActorEntry {
            plugin_name,
            type_name: type_name.to_string(),
            vtable,
        })),
        (None, Some(vtable)) => Ok(ConfiguredPluginEntry::Strategy(ConfiguredStrategyEntry {
            plugin_name,
            type_name: type_name.to_string(),
            vtable,
        })),
        (None, None) => {
            anyhow::bail!("plug-in '{path}' does not expose actor or strategy type '{type_name}'")
        }
        (Some(_), Some(_)) => {
            anyhow::bail!("plug-in '{path}' exposes type '{type_name}' as both actor and strategy")
        }
    }
}

fn manifest_plugin_name(manifest: &PluginManifest) -> String {
    // SAFETY: manifest strings live in static cdylib storage.
    unsafe { manifest.plugin_name.as_str() }.to_string()
}

fn find_actor_entry(manifest: &PluginManifest, type_name: &str) -> Option<*const ActorVTable> {
    // SAFETY: manifest slices live in static cdylib storage.
    for entry in unsafe { manifest.actors.as_slice() } {
        // SAFETY: entry strings live in static cdylib storage.
        if unsafe { entry.type_name.as_str() } == type_name {
            return Some(entry.vtable);
        }
    }
    None
}

fn find_strategy_entry(
    manifest: &PluginManifest,
    type_name: &str,
) -> Option<*const StrategyVTable> {
    // SAFETY: manifest slices live in static cdylib storage.
    for entry in unsafe { manifest.strategies.as_slice() } {
        // SAFETY: entry strings live in static cdylib storage.
        if unsafe { entry.type_name.as_str() } == type_name {
            return Some(entry.vtable);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use nautilus_plugin::{
        NAUTILUS_PLUGIN_ABI_VERSION,
        boundary::{BorrowedStr, Slice},
        manifest::{ActorRegistration, PluginBuildId, PluginManifest, StrategyRegistration},
    };
    use rstest::rstest;

    use super::*;

    static ACTOR_REGISTRATIONS: [ActorRegistration; 1] = [ActorRegistration {
        type_name: BorrowedStr::from_str("ExampleActor"),
        vtable: std::ptr::null(),
    }];
    static STRATEGY_REGISTRATIONS: [StrategyRegistration; 1] = [StrategyRegistration {
        type_name: BorrowedStr::from_str("ExampleStrategy"),
        vtable: std::ptr::null(),
    }];
    static AMBIGUOUS_ACTOR_REGISTRATIONS: [ActorRegistration; 1] = [ActorRegistration {
        type_name: BorrowedStr::from_str("DuplicateType"),
        vtable: std::ptr::null(),
    }];
    static AMBIGUOUS_STRATEGY_REGISTRATIONS: [StrategyRegistration; 1] = [StrategyRegistration {
        type_name: BorrowedStr::from_str("DuplicateType"),
        vtable: std::ptr::null(),
    }];

    fn manifest(
        actors: Slice<'static, ActorRegistration>,
        strategies: Slice<'static, StrategyRegistration>,
    ) -> PluginManifest {
        PluginManifest {
            abi_version: NAUTILUS_PLUGIN_ABI_VERSION,
            plugin_name: BorrowedStr::from_str("test-plugin"),
            plugin_vendor: BorrowedStr::from_str("nautech"),
            plugin_version: BorrowedStr::from_str("0.0.0"),
            build_id: PluginBuildId::current(),
            custom_data: Slice::empty(),
            actors,
            strategies,
        }
    }

    #[rstest]
    fn configured_entry_resolves_actor_by_type_name() {
        let manifest = manifest(
            Slice::from_slice(&ACTOR_REGISTRATIONS),
            Slice::from_slice(&STRATEGY_REGISTRATIONS),
        );

        let entry = configured_entry(&manifest, "./libexample.so", "ExampleActor").unwrap();

        assert!(matches!(entry, ConfiguredPluginEntry::Actor(_)));
    }

    #[rstest]
    fn configured_entry_resolves_strategy_by_type_name() {
        let manifest = manifest(
            Slice::from_slice(&ACTOR_REGISTRATIONS),
            Slice::from_slice(&STRATEGY_REGISTRATIONS),
        );

        let entry = configured_entry(&manifest, "./libexample.so", "ExampleStrategy").unwrap();

        assert!(matches!(entry, ConfiguredPluginEntry::Strategy(_)));
    }

    #[rstest]
    fn configured_entry_rejects_missing_type_name() {
        let manifest = manifest(
            Slice::from_slice(&ACTOR_REGISTRATIONS),
            Slice::from_slice(&STRATEGY_REGISTRATIONS),
        );

        let error = match configured_entry(&manifest, "./libexample.so", "MissingType") {
            Ok(_) => panic!("configured entry should reject missing type"),
            Err(e) => e.to_string(),
        };

        assert!(error.contains("does not expose actor or strategy type"));
        assert!(error.contains("MissingType"));
    }

    #[rstest]
    fn configured_entry_rejects_ambiguous_type_name() {
        let manifest = manifest(
            Slice::from_slice(&AMBIGUOUS_ACTOR_REGISTRATIONS),
            Slice::from_slice(&AMBIGUOUS_STRATEGY_REGISTRATIONS),
        );

        let error = match configured_entry(&manifest, "./libexample.so", "DuplicateType") {
            Ok(_) => panic!("configured entry should reject ambiguous type"),
            Err(e) => e.to_string(),
        };

        assert!(error.contains("exposes type 'DuplicateType' as both actor and strategy"));
    }
}
