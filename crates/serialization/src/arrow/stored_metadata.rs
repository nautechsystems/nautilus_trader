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

//! The metadata every catalog backend persists for a batch, and its inverse.
//!
//! A batch's schema metadata holds the type name, the identity (`instrument_id`, `bar_type`, or
//! `account_id`), and the precision. The storage location gives the type name and the
//! `identifier` column gives the identity, so a stored map keeps only what neither provides.

use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
};

use nautilus_model::data::BarType;

use super::{KEY_ACCOUNT_ID, KEY_BAR_TYPE, KEY_INSTRUMENT_ID, KEY_TYPE_NAME};

/// Arrow metadata keys whose values duplicate the catalog row identifier column.
///
/// Bars use `bar_type` as their identifier, account states and execution mass status use
/// `account_id`, and all other built-in catalog types use `instrument_id`.
const DERIVABLE_METADATA_KEYS: [&str; 3] = [KEY_INSTRUMENT_ID, KEY_BAR_TYPE, KEY_ACCOUNT_ID];

/// Returns `metadata` without the derivable entries whose value equals `identifier`.
///
/// When `identifier` parses as a bar type, an `instrument_id` entry equal to the bar type's
/// instrument id is also stripped; that arm pairs with the [`restore_derivable_metadata_key`]
/// `bar_type` restore, which re-derives the instrument id.
#[must_use]
pub fn strip_derivable_metadata_keys(
    metadata: &HashMap<String, String>,
    identifier: &str,
) -> HashMap<String, String> {
    let bar_instrument_id = BarType::from_str(identifier)
        .ok()
        .map(|bar_type| bar_type.instrument_id().to_string());

    metadata
        .iter()
        .filter(|(key, value)| {
            let derivable = match key.as_str() {
                KEY_BAR_TYPE | KEY_ACCOUNT_ID => value.as_str() == identifier,
                KEY_INSTRUMENT_ID => {
                    value.as_str() == identifier
                        || bar_instrument_id.as_deref() == Some(value.as_str())
                }
                _ => false,
            };
            !(DERIVABLE_METADATA_KEYS.contains(&key.as_str()) && derivable)
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Restores the identifier-derivable `key` into `metadata` from `identifier`.
pub fn restore_derivable_metadata_key(
    metadata: &mut HashMap<String, String>,
    key: &str,
    identifier: &str,
) {
    metadata.insert(key.to_string(), identifier.to_string());

    if key == KEY_BAR_TYPE
        && let Ok(bar_type) = BarType::from_str(identifier)
    {
        metadata.insert(
            KEY_INSTRUMENT_ID.to_string(),
            bar_type.instrument_id().to_string(),
        );
    }
}

/// Returns the metadata every backend persists for a batch.
///
/// `identifier` is the batch's `identifier` value and `location_type_name` the type name its
/// storage location (folder or table) already implies. Entries those two restore are dropped, so
/// what remains is what neither gives: precision and per-field custom entries.
#[must_use]
pub fn stored_metadata(
    metadata: &HashMap<String, String>,
    identifier: Option<&str>,
    location_type_name: Option<&str>,
) -> HashMap<String, String> {
    let mut stored = match identifier {
        Some(identifier) => strip_derivable_metadata_keys(metadata, identifier),
        None => metadata.clone(),
    };

    if location_type_name.is_some()
        && stored.get(KEY_TYPE_NAME).map(String::as_str) == location_type_name
    {
        stored.remove(KEY_TYPE_NAME);
    }

    stored
}

/// Returns the full metadata decoders expect from a stored map.
///
/// Puts back `type_name` from the storage location and the `derivable_key` entry from the
/// `identifier`, the inverse of [`stored_metadata`].
#[must_use]
pub fn restored_metadata(
    mut stored: HashMap<String, String>,
    derivable_key: Option<&str>,
    identifier: Option<&str>,
    location_type_name: Option<&str>,
) -> HashMap<String, String> {
    if let (Some(key), Some(identifier)) = (derivable_key, identifier) {
        restore_derivable_metadata_key(&mut stored, key, identifier);
    }

    if let Some(type_name) = location_type_name {
        stored
            .entry(KEY_TYPE_NAME.to_string())
            .or_insert_with(|| type_name.to_string());
    }

    stored
}

/// Returns the canonical JSON of a metadata map: keys serialized in sorted order.
///
/// # Errors
///
/// Returns an error if the map cannot be serialized.
pub fn canonical_metadata_json(metadata: &HashMap<String, String>) -> anyhow::Result<String> {
    Ok(serde_json::to_string(
        &metadata
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    )?)
}

/// Returns the hash of a canonical metadata JSON string, the reference rows use for their map.
#[must_use]
pub fn metadata_json_hash(metadata_json: &str) -> String {
    format!("blake3:{}", blake3::hash(metadata_json.as_bytes()).to_hex())
}

/// Returns the hash that links a row to its stored metadata map, shared by Feather staging,
/// DuckLake, and Timescale.
///
/// # Errors
///
/// Returns an error if the map cannot be serialized.
pub fn metadata_hash(stored: &HashMap<String, String>) -> anyhow::Result<String> {
    Ok(metadata_json_hash(&canonical_metadata_json(stored)?))
}
