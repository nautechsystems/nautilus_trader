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

#[cfg(feature = "redis")]
pub(crate) fn redis_read_error(error: impl Into<anyhow::Error>) -> anyhow::Error {
    let error = error.into();
    if let Some(backend) = error.downcast_ref::<redis::RedisError>()
        && backend.code() != Some("WRONGTYPE")
        && backend.kind() != redis::ErrorKind::UnexpectedReturnType
    {
        return error;
    }

    integrity_error(error)
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

#[cfg(all(test, feature = "redis"))]
mod tests {
    use redis::{ErrorKind, RedisError, make_extension_error};
    use rstest::rstest;

    use super::{IntegrityError, redis_read_error};

    #[rstest]
    fn test_redis_history_read_preserves_error_category() {
        let transport = RedisError::from(std::io::Error::from(std::io::ErrorKind::ConnectionReset));
        let wrong_type = make_extension_error(
            "WRONGTYPE".to_string(),
            Some("invalid history type".to_string()),
        );
        let invalid_value =
            RedisError::from((ErrorKind::UnexpectedReturnType, "invalid history value"));

        let transport = redis_read_error(transport);
        let wrong_type = redis_read_error(wrong_type);
        let invalid_value = redis_read_error(invalid_value);

        assert!(transport.is::<RedisError>());
        assert!(!transport.is::<IntegrityError>());
        assert!(wrong_type.is::<IntegrityError>());
        assert!(invalid_value.is::<IntegrityError>());
    }
}
