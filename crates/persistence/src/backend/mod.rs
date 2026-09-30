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

//! Provides an Apache Parquet backend powered by [DataFusion](https://arrow.apache.org/datafusion).

use std::sync::Arc;

use indexmap::{IndexMap, map::Entry};

use crate::{
    catalog::factory as catalog_factory,
    common::paths,
    writer::{factory as writer_factory, feather as feather_writer, traits as writer_traits},
};

pub mod migration;
pub mod parquet;

/// Returns the persistence-owned catalog-factory registry.
///
/// The neutral registry types live in [`catalog_factory`]. Concrete built-in
/// backend registrations are assembled here so consumers such as backtest, live,
/// CLI tools, and Python bindings can share the same defaults without making the
/// shared factory module depend on backend-specific modules.
#[must_use]
pub fn default_catalog_factories() -> catalog_factory::CatalogFactoryRegistry {
    let mut registry = catalog_factory::CatalogFactoryRegistry::new();
    register_builtin_catalog_factories(&mut registry);
    registry
}

/// Merges user-provided factories into the persistence default registry.
///
/// # Errors
///
/// Returns an error if a user-provided name collides with an existing built-in
/// or user-provided entry.
pub fn extend_catalog_factories(
    extra: impl IntoIterator<Item = (String, catalog_factory::CatalogFactory)>,
) -> anyhow::Result<catalog_factory::CatalogFactoryRegistry> {
    extend_factories(default_catalog_factories(), extra, "Catalog")
}

/// Returns the persistence-owned writer-factory registry.
///
/// Mirrors [`default_catalog_factories`]: the neutral registry types live in
/// [`writer_factory`], and concrete built-in registrations are assembled here.
#[must_use]
pub fn default_writer_factories() -> writer_factory::WriterFactoryRegistry {
    let mut registry = writer_factory::WriterFactoryRegistry::new();
    register_builtin_writer_factories(&mut registry);
    registry
}

/// Merges user-provided writer factories into the persistence default registry.
///
/// # Errors
///
/// Returns an error if a user-provided name collides with an existing entry.
pub fn extend_writer_factories(
    extra: impl IntoIterator<Item = (String, writer_factory::WriterFactory)>,
) -> anyhow::Result<writer_factory::WriterFactoryRegistry> {
    extend_factories(default_writer_factories(), extra, "Writer")
}

fn extend_factories<T>(
    mut registry: IndexMap<String, T>,
    extra: impl IntoIterator<Item = (String, T)>,
    kind: &str,
) -> anyhow::Result<IndexMap<String, T>> {
    for (name, factory) in extra {
        match registry.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(factory);
            }
            Entry::Occupied(entry) => {
                anyhow::bail!("{kind} factory already registered: {}", entry.key());
            }
        }
    }

    Ok(registry)
}

fn register_builtin_catalog_factories(registry: &mut catalog_factory::CatalogFactoryRegistry) {
    parquet::register_catalog_factory(registry);
}

fn register_builtin_writer_factories(registry: &mut writer_factory::WriterFactoryRegistry) {
    parquet::writer::register_factory(registry);
    registry.insert(
        writer_factory::FEATHER_WRITER_FACTORY_NAME.to_string(),
        Arc::new(
            |config: &writer_factory::WriterConnectConfig, clock: feather_writer::WriterClock| {
                let directory = paths::local_writer_directory(&config.uri)?;
                feather_writer::recover_partial_feather_files(&directory);

                Ok(Box::new(
                    feather_writer::FeatherWriter::new(
                        directory,
                        clock,
                        config.rotation_config.clone(),
                        None,
                        config.flush_interval_ms,
                    )
                    .with_record_filter(config.record_filter.clone()),
                ) as writer_traits::StreamingDataSink)
            },
        ),
    );
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn unused_catalog_factory() -> catalog_factory::CatalogFactory {
        Arc::new(|_| anyhow::bail!("not invoked"))
    }

    #[rstest]
    fn extend_catalog_factories_accepts_external_factory() {
        let factories =
            extend_catalog_factories(vec![("ExternalTest".to_string(), unused_catalog_factory())])
                .unwrap();

        assert_eq!(
            factories.keys().map(String::as_str).collect::<Vec<_>>(),
            vec![
                catalog_factory::PARQUET_CATALOG_FACTORY_NAME,
                "ExternalTest"
            ],
        );
    }

    #[rstest]
    #[case::builtin(vec!["Parquet"], "Parquet")]
    #[case::user(vec!["Duplicate", "Duplicate"], "Duplicate")]
    fn extend_catalog_factories_rejects_duplicate_names(
        #[case] names: Vec<&str>,
        #[case] duplicate: &str,
    ) {
        let extra = names
            .into_iter()
            .map(|name| (name.to_string(), unused_catalog_factory()));

        let error = extend_catalog_factories(extra)
            .err()
            .expect("duplicate registration should fail");

        assert_eq!(
            error.to_string(),
            format!("Catalog factory already registered: {duplicate}"),
        );
    }
}
