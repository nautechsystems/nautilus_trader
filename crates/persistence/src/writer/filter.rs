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

//! Record-family filters for streaming writer backends.

use ahash::{AHashMap, AHashSet};
use nautilus_model::{
    data::NautilusRecordType,
    instruments::{InstrumentAny, NautilusInstrumentType},
};

use crate::{catalog::traits::NautilusRecordTypePrefix, common::paths::CatalogPathPrefix};

/// Typed record-family filter shared by streaming writer backends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriterRecordFilter {
    entries: AHashMap<String, Option<AHashSet<String>>>,
    instrument_types: AHashSet<String>,
}

impl WriterRecordFilter {
    /// Creates an empty filter which allows all records.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a filter allowing complete record families.
    #[must_use]
    pub fn from_record_types(record_types: impl IntoIterator<Item = NautilusRecordType>) -> Self {
        let mut filter = Self::new();
        for record_type in record_types {
            filter.insert(&record_type, None);
        }
        filter
    }

    /// Adds one record family with optional identifier restriction.
    pub fn insert(&mut self, record_type: &NautilusRecordType, identifiers: Option<Vec<String>>) {
        let prefix = record_type.path_prefix().into_owned();
        self.insert_prefix(prefix, identifiers);
    }

    /// Adds one catalog path prefix with optional identifier restriction.
    pub fn insert_prefix(&mut self, prefix: impl Into<String>, identifiers: Option<Vec<String>>) {
        let identifiers = identifiers.map(|values| values.into_iter().collect());
        self.entries.insert(prefix.into(), identifiers);
    }

    /// Adds one concrete instrument family.
    pub fn insert_instrument_type(&mut self, instrument_type: &NautilusInstrumentType) {
        self.instrument_types.insert(instrument_type.to_string());
    }

    /// Returns whether this filter carries no restrictions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.instrument_types.is_empty()
    }

    /// Returns whether this filter mentions a record prefix.
    #[must_use]
    pub fn contains_prefix(&self, record_prefix: &str) -> bool {
        self.is_empty()
            || self.entries.contains_key(record_prefix)
            || (!self.instrument_types.is_empty() && record_prefix == InstrumentAny::path_prefix())
    }

    /// Returns whether record prefix and optional identifier pass this filter.
    #[must_use]
    pub fn allows(
        &self,
        record_prefix: &str,
        identifier: Option<&str>,
        instrument_type: Option<&str>,
    ) -> bool {
        if self.is_empty() {
            return true;
        }

        let Some(identifiers) = self.entries.get(record_prefix) else {
            return record_prefix == InstrumentAny::path_prefix()
                && !self.instrument_types.is_empty()
                && instrument_type.is_some_and(|value| self.instrument_types.contains(value));
        };

        if record_prefix == InstrumentAny::path_prefix()
            && !self.instrument_types.is_empty()
            && !instrument_type.is_some_and(|value| self.instrument_types.contains(value))
        {
            return false;
        }

        match identifiers {
            None => true,
            Some(identifiers) => {
                identifier.is_some_and(|identifier| identifiers.contains(identifier))
            }
        }
    }
}
