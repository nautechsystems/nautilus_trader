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

//! Catalog factory registry conformance.

use std::sync::Arc;

use nautilus_persistence::{
    backend::extend_catalog_factories, catalog::factory::CatalogFactoryRegistry,
};
use rstest::rstest;

/// Regression: built-in registration must not prevent arbitrary external
/// catalog factories from being added by consumers.
#[rstest]
fn persistence_default_registry_accepts_external_catalog_factory() {
    let mut extra = CatalogFactoryRegistry::new();
    extra.insert(
        "ExternalTest".to_string(),
        Arc::new(|_config| anyhow::bail!("not invoked")),
    );

    let factories = extend_catalog_factories(extra).unwrap();
    assert!(factories.contains_key("ExternalTest"));
}
