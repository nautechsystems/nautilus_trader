// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautilustrader.io
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

//! Semantic type selectors shared by catalog APIs.

use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::{
    data::{NautilusDataType, NautilusRecordType},
    instruments::NautilusInstrumentType,
};

/// A semantic catalog selector across data, record, and concrete instrument families.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum NautilusCatalogType {
    Data(NautilusDataType),
    Record(NautilusRecordType),
    Instrument(NautilusInstrumentType),
}

impl Display for NautilusCatalogType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Data(data_type) => Display::fmt(data_type, f),
            Self::Record(record_type) => Display::fmt(record_type, f),
            Self::Instrument(instrument_type) => Display::fmt(instrument_type, f),
        }
    }
}

impl From<NautilusDataType> for NautilusCatalogType {
    fn from(value: NautilusDataType) -> Self {
        Self::Data(value)
    }
}

impl From<NautilusRecordType> for NautilusCatalogType {
    fn from(value: NautilusRecordType) -> Self {
        Self::Record(value)
    }
}

impl From<NautilusInstrumentType> for NautilusCatalogType {
    fn from(value: NautilusInstrumentType) -> Self {
        Self::Instrument(value)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        NautilusCatalogType::from(NautilusDataType::QuoteTick),
        NautilusCatalogType::Data(NautilusDataType::QuoteTick)
    )]
    #[case(
        NautilusCatalogType::from(NautilusRecordType::AccountState),
        NautilusCatalogType::Record(NautilusRecordType::AccountState)
    )]
    #[case(
        NautilusCatalogType::from(NautilusInstrumentType::Equity),
        NautilusCatalogType::Instrument(NautilusInstrumentType::Equity)
    )]
    fn component_types_convert_to_catalog_type(
        #[case] actual: NautilusCatalogType,
        #[case] expected: NautilusCatalogType,
    ) {
        assert_eq!(actual, expected);
    }
}
