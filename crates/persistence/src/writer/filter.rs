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
    data::{NautilusDataType, NautilusRecordType},
    instruments::NautilusInstrumentType,
};

use crate::catalog::types::CatalogDataType;

/// Typed record-family filter shared by streaming writer backends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WriterRecordFilter {
    entries: AHashMap<CatalogDataType, Option<AHashSet<String>>>,
    instrument_types: AHashSet<NautilusInstrumentType>,
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
            filter.insert(record_type, None);
        }

        filter
    }

    /// Adds one data or record family with optional identifier restriction.
    pub fn insert(&mut self, family: impl Into<CatalogDataType>, identifiers: Option<Vec<String>>) {
        let identifiers = identifiers.map(|values| values.into_iter().collect());
        self.entries.insert(family.into(), identifiers);
    }

    /// Adds one concrete instrument family.
    pub fn insert_instrument_type(&mut self, instrument_type: NautilusInstrumentType) {
        self.instrument_types.insert(instrument_type);
    }

    /// Returns whether this filter carries no restrictions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.instrument_types.is_empty()
    }

    /// Returns whether this filter allows records of `family`.
    #[must_use]
    pub fn contains(&self, family: &CatalogDataType) -> bool {
        self.is_empty()
            || self.entries.contains_key(family)
            || (!self.instrument_types.is_empty() && is_instrument_family(family))
    }

    /// Returns whether a record of `data_type` and optional identifier passes this filter.
    ///
    /// An instrument record passes its class as [`CatalogDataType::Instrument`].
    #[must_use]
    pub fn allows(&self, data_type: &CatalogDataType, identifier: Option<&str>) -> bool {
        if self.is_empty() {
            return true;
        }

        let instrument_type = match data_type {
            CatalogDataType::Instrument(instrument_type) => Some(instrument_type),
            _ => None,
        };

        let family = catalog_family(data_type);

        let Some(identifiers) = self.entries.get(&family) else {
            return is_instrument_family(&family)
                && instrument_type.is_some_and(|value| self.instrument_types.contains(value));
        };

        if is_instrument_family(&family)
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

/// Returns the family a writer stages `data_type` under; every instrument class is one family.
#[must_use]
pub(crate) fn catalog_family(data_type: &CatalogDataType) -> CatalogDataType {
    match data_type {
        CatalogDataType::Instrument(_) => CatalogDataType::Data(NautilusDataType::Instrument),
        other => other.clone(),
    }
}

fn is_instrument_family(family: &CatalogDataType) -> bool {
    *family == CatalogDataType::Data(NautilusDataType::Instrument)
}
