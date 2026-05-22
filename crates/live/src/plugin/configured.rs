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
use nautilus_plugin::manifest::{
    ValidatedActorRegistration, ValidatedActorVTable, ValidatedPluginManifest,
    ValidatedStrategyRegistration, ValidatedStrategyVTable,
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
    vtable: ValidatedActorVTable,
}

/// Strategy entry copied from a loaded manifest.
pub(crate) struct ConfiguredStrategyEntry {
    plugin_name: String,
    type_name: String,
    vtable: ValidatedStrategyVTable,
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
pub(crate) fn register_manifest_custom_data(
    manifest: ValidatedPluginManifest<'_>,
) -> anyhow::Result<usize> {
    register_custom_data_from_manifest(manifest)
}

/// Resolves an actor or strategy entry from a loaded manifest by type name.
///
/// # Errors
///
/// Returns an error when the type is missing or ambiguous.
pub(crate) fn configured_entry(
    manifest: ValidatedPluginManifest<'_>,
    path: &str,
    type_name: &str,
) -> anyhow::Result<ConfiguredPluginEntry> {
    let plugin_name = manifest.plugin_name().to_string();
    let actor_entry = find_actor_entry(manifest, type_name);
    let strategy_entry = find_strategy_entry(manifest, type_name);

    match (actor_entry, strategy_entry) {
        (Some(entry), None) => Ok(ConfiguredPluginEntry::Actor(ConfiguredActorEntry {
            plugin_name,
            type_name: entry.type_name().to_string(),
            vtable: entry.vtable(),
        })),
        (None, Some(entry)) => Ok(ConfiguredPluginEntry::Strategy(ConfiguredStrategyEntry {
            plugin_name,
            type_name: entry.type_name().to_string(),
            vtable: entry.vtable(),
        })),
        (None, None) => {
            anyhow::bail!("plug-in '{path}' does not expose actor or strategy type '{type_name}'")
        }
        (Some(_), Some(_)) => {
            anyhow::bail!("plug-in '{path}' exposes type '{type_name}' as both actor and strategy")
        }
    }
}

fn find_actor_entry(
    manifest: ValidatedPluginManifest<'_>,
    type_name: &str,
) -> Option<ValidatedActorRegistration> {
    manifest
        .actors()
        .find(|entry| entry.type_name() == type_name)
}

fn find_strategy_entry(
    manifest: ValidatedPluginManifest<'_>,
    type_name: &str,
) -> Option<ValidatedStrategyRegistration> {
    manifest
        .strategies()
        .find(|entry| entry.type_name() == type_name)
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use nautilus_plugin::{
        NAUTILUS_PLUGIN_ABI_VERSION,
        boundary::{BorrowedStr, Slice},
        host::{HostContext, HostVTable},
        manifest::{ActorRegistration, PluginBuildId, PluginManifest, StrategyRegistration},
        surfaces::{
            actor::{PluginActor, actor_vtable},
            strategy::{PluginStrategy, strategy_vtable},
        },
    };
    use rstest::rstest;

    use super::*;

    struct ExampleActor;

    impl PluginActor for ExampleActor {
        const TYPE_NAME: &'static str = "ExampleActor";

        fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
            Self
        }
    }

    struct ExampleStrategy;

    impl PluginStrategy for ExampleStrategy {
        const TYPE_NAME: &'static str = "ExampleStrategy";

        fn new(_host: *const HostVTable, _ctx: *const HostContext, _config_json: &str) -> Self {
            Self
        }
    }

    static ACTOR_REGISTRATIONS: LazyLock<[ActorRegistration; 1]> = LazyLock::new(|| {
        [ActorRegistration {
            type_name: BorrowedStr::from_str("ExampleActor"),
            vtable: actor_vtable::<ExampleActor>(),
        }]
    });
    static STRATEGY_REGISTRATIONS: LazyLock<[StrategyRegistration; 1]> = LazyLock::new(|| {
        [StrategyRegistration {
            type_name: BorrowedStr::from_str("ExampleStrategy"),
            vtable: strategy_vtable::<ExampleStrategy>(),
        }]
    });
    static AMBIGUOUS_ACTOR_REGISTRATIONS: LazyLock<[ActorRegistration; 1]> = LazyLock::new(|| {
        [ActorRegistration {
            type_name: BorrowedStr::from_str("DuplicateType"),
            vtable: actor_vtable::<ExampleActor>(),
        }]
    });
    static AMBIGUOUS_STRATEGY_REGISTRATIONS: LazyLock<[StrategyRegistration; 1]> =
        LazyLock::new(|| {
            [StrategyRegistration {
                type_name: BorrowedStr::from_str("DuplicateType"),
                vtable: strategy_vtable::<ExampleStrategy>(),
            }]
        });

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
            Slice::from_slice(&*ACTOR_REGISTRATIONS),
            Slice::from_slice(&*STRATEGY_REGISTRATIONS),
        );
        let manifest = ValidatedPluginManifest::new(&manifest)
            .expect("configured actor lookup uses a loader-valid manifest");

        let entry = configured_entry(manifest, "./libexample.so", "ExampleActor").unwrap();

        let ConfiguredPluginEntry::Actor(entry) = entry else {
            panic!("expected actor entry");
        };
        assert_eq!(entry.plugin_name, "test-plugin");
        assert_eq!(entry.type_name, "ExampleActor");
        assert_eq!(entry.vtable.as_ptr(), ACTOR_REGISTRATIONS[0].vtable);
    }

    #[rstest]
    fn configured_entry_resolves_strategy_by_type_name() {
        let manifest = manifest(
            Slice::from_slice(&*ACTOR_REGISTRATIONS),
            Slice::from_slice(&*STRATEGY_REGISTRATIONS),
        );
        let manifest = ValidatedPluginManifest::new(&manifest)
            .expect("configured strategy lookup uses a loader-valid manifest");

        let entry = configured_entry(manifest, "./libexample.so", "ExampleStrategy").unwrap();

        let ConfiguredPluginEntry::Strategy(entry) = entry else {
            panic!("expected strategy entry");
        };
        assert_eq!(entry.plugin_name, "test-plugin");
        assert_eq!(entry.type_name, "ExampleStrategy");
        assert_eq!(entry.vtable.as_ptr(), STRATEGY_REGISTRATIONS[0].vtable);
    }

    #[rstest]
    fn configured_entry_rejects_missing_type_name() {
        let manifest = manifest(
            Slice::from_slice(&*ACTOR_REGISTRATIONS),
            Slice::from_slice(&*STRATEGY_REGISTRATIONS),
        );
        let manifest = ValidatedPluginManifest::new(&manifest)
            .expect("missing configured type test uses a loader-valid manifest");

        let error = match configured_entry(manifest, "./libexample.so", "MissingType") {
            Ok(_) => panic!("configured entry should reject missing type"),
            Err(e) => e.to_string(),
        };

        assert!(error.contains("does not expose actor or strategy type"));
        assert!(error.contains("MissingType"));
    }

    #[rstest]
    fn validated_manifest_rejects_ambiguous_type_name() {
        let manifest = manifest(
            Slice::from_slice(&*AMBIGUOUS_ACTOR_REGISTRATIONS),
            Slice::from_slice(&*AMBIGUOUS_STRATEGY_REGISTRATIONS),
        );
        let validation_error = ValidatedPluginManifest::new(&manifest)
            .expect_err("loader rejects ambiguous manifest type names");
        assert!(
            validation_error
                .to_string()
                .contains("type name 'DuplicateType' appears in both actors[0] and strategies[0]")
        );
    }
}
