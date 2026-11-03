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

use std::fmt::Display;

pub(crate) use nautilus_common::defi::cache::IntegrityError;
use nautilus_model::{defi::currency, instruments::Instrument};
#[cfg(feature = "redis")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "redis")]
use serde_json::Value;

#[cfg(feature = "redis")]
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstrumentRecord {
    pub defi_currency_codes: Vec<String>,
    pub instrument: Value,
}

pub(crate) fn integrity_error(error: impl Display) -> anyhow::Error {
    IntegrityError(format!("{error:#}")).into()
}

pub(crate) fn contract_conflict(code: &str) -> anyhow::Error {
    integrity_error(format!(
        "Token contract is already persisted as {code}; restore existing definitions before converting tokens"
    ))
}

pub(crate) fn validate_marker(instrument: &dyn Instrument, codes: &[String]) -> anyhow::Result<()> {
    let expected = currency::instrument_codes(instrument).map_err(integrity_error)?;
    if expected != codes {
        return Err(integrity_error(format!(
            "Instrument {} token dependencies differ from its marker",
            instrument.id(),
        )));
    }

    Ok(())
}

pub(crate) fn currency_references(instrument: &dyn Instrument) -> Vec<String> {
    let mut references = [
        instrument.base_currency(),
        Some(instrument.quote_currency()),
        Some(instrument.settlement_currency()),
    ]
    .into_iter()
    .flatten()
    .map(|currency| currency.code.to_string())
    .collect::<Vec<_>>();
    references.sort_unstable();
    references.dedup();
    references
}
