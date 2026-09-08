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

//! Cross-backend path and identifier operations.
//!
//! Holds primitives shared by catalog backends: object-store path constructors,
//! identifier sanitization, and persistence-owned type-to-prefix traits for
//! streaming/session writers. Backend-specific catalog paths are mapped in
//! `crate::catalog::types`.

/// Persistence-owned static path prefix used by streaming/session writers.
pub trait CatalogPathPrefix {
    /// Returns the record family prefix.
    fn path_prefix() -> &'static str;
}

/// Joins a base path and components into an object-store path with forward slashes.
///
/// Object stores (S3, GCS, etc.) always expect forward slashes regardless of platform.
#[must_use]
pub fn make_object_store_path<I, S>(base_path: &str, components: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut parts = Vec::new();

    if !base_path.is_empty() {
        let normalized_base = base_path
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string();

        if !normalized_base.is_empty() {
            parts.push(normalized_base);
        }
    }

    for component in components {
        let normalized_component = component
            .as_ref()
            .replace('\\', "/")
            .trim_start_matches('/')
            .trim_end_matches('/')
            .to_string();

        if !normalized_component.is_empty() {
            parts.push(normalized_component);
        }
    }

    parts.join("/")
}

/// Converts an instrument ID to a URI-safe format by removing forward slashes and replacing
/// carets with underscores. Some instrument IDs contain forward slashes (e.g., "BTC/USD") which
/// are not suitable for use in file paths.
#[must_use]
pub fn urisafe_instrument_id(instrument_id: &str) -> String {
    instrument_id.replace('/', "").replace('^', "_")
}

/// Normalizes a user-supplied identifier for use in directory paths.
///
/// Replaces `//` with `/` and filters out empty segments and `..` to prevent path traversal.
#[must_use]
pub fn safe_directory_identifier(identifier: &str) -> String {
    let normalized = identifier.replace("//", "/");
    let segments: Vec<&str> = normalized
        .split('/')
        .filter(|s| !s.is_empty() && *s != "..")
        .collect();
    segments.join("/")
}

/// Extracts the identifier from a file path: typically the second-to-last path component.
///
/// For example, from `data/quotes/EURUSD/file.parquet`, extracts `EURUSD`.
#[must_use]
pub fn extract_identifier_from_path(file_path: &str) -> Option<&str> {
    file_path.rsplit_once('/').and_then(|(parent, _)| {
        parent
            .rsplit_once('/')
            .map_or(Some(parent), |(_, identifier)| Some(identifier))
            .filter(|identifier| !identifier.is_empty())
    })
}

/// Makes an identifier safe for use in SQL table names.
///
/// Keeps ASCII alphanumerics and underscores; replaces everything else with `_`, then lowercases.
#[must_use]
pub fn make_sql_safe_identifier(identifier: &str) -> String {
    urisafe_instrument_id(identifier)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Returns the data-type segment of a Feather session path like
/// `backtest/{run_id}/data/{type}/{...}/file.feather`.
///
/// Custom data is encoded as `data/custom/{type_name}/...`; the returned value
/// is `custom/{type_name}` for those paths.
///
/// # Errors
///
/// Returns an error if the path does not contain the `{kind}/{instance_id}/...`
/// prefix or does not have a recognizable type segment.
pub(crate) fn type_name_from_session_feather_path(
    path: &str,
    kind: &str,
    instance_id: &str,
) -> anyhow::Result<String> {
    let components: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|component| !component.is_empty())
        .collect();
    let type_index = session_type_index(&components, kind, instance_id, path)?;
    if components.get(type_index) == Some(&"data")
        && components.get(type_index + 1) == Some(&"custom")
    {
        let type_name = components.get(type_index + 2).ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot infer custom data type from Feather session path '{path}' for {kind}/{instance_id}"
            )
        })?;
        return Ok(format!("custom/{type_name}"));
    }
    let type_segment = components[type_index];
    let file_name = components.last().copied().unwrap_or(type_segment);
    let type_name = if type_segment.ends_with(".feather") {
        file_name
            .strip_suffix(".feather")
            .and_then(|stem| stem.rsplit_once('_').map(|(type_name, _)| type_name))
            .unwrap_or(type_segment)
    } else {
        type_segment
    };
    Ok(type_name.to_string())
}

/// Returns the optional identifier (instrument id, bar type, ...) segment of a
/// Feather session path, or `None` if the path encodes only a type with no
/// identifier (e.g. catalog-level files).
pub(crate) fn identifier_from_session_feather_path(
    path: &str,
    kind: &str,
    instance_id: &str,
) -> Option<String> {
    let components: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|component| !component.is_empty())
        .collect();
    let type_index = session_type_index(&components, kind, instance_id, path).ok()?;
    if components.get(type_index) == Some(&"data")
        && components.get(type_index + 1) == Some(&"custom")
    {
        let identifier_start = type_index + 3;
        let file_index = components.len().checked_sub(1)?;
        if identifier_start >= file_index {
            return None;
        }
        return Some(components[identifier_start].to_string());
    }
    let identifier = components.get(type_index + 1)?;
    let file_name = components.last()?;

    if identifier.ends_with(".feather") {
        return None;
    }

    (identifier != file_name).then(|| (*identifier).to_string())
}

fn session_type_index(
    components: &[&str],
    kind: &str,
    instance_id: &str,
    path: &str,
) -> anyhow::Result<usize> {
    components
        .windows(2)
        .position(|window| window[0] == kind && window[1] == instance_id)
        .and_then(|kind_index| kind_index.checked_add(2))
        .filter(|type_index| *type_index < components.len())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot infer data type from Feather session path '{path}' for {kind}/{instance_id}"
            )
        })
}

#[cfg(test)]
mod session_path_tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn session_feather_paths_recover_type_and_identifier() {
        // Feather sessions are written under {kind}/{run_id}/{type}/{identifier?}/file.feather
        // (no `data/` segment: that prefix only appears under the Delta catalog target
        // layout, not under writer staging paths).
        let path = "backtest/run-1/quotes/EURUSD.SIM/0001.feather";
        assert_eq!(
            type_name_from_session_feather_path(path, "backtest", "run-1").unwrap(),
            "quotes",
        );
        assert_eq!(
            identifier_from_session_feather_path(path, "backtest", "run-1").as_deref(),
            Some("EURUSD.SIM"),
        );
        assert_eq!(
            type_name_from_session_feather_path(
                "backtest/run-1/quotes_1000-1.feather",
                "backtest",
                "run-1",
            )
            .unwrap(),
            "quotes",
        );

        let custom = "backtest/run-1/data/custom/MyType/inst/0001.feather";
        assert_eq!(
            type_name_from_session_feather_path(custom, "backtest", "run-1").unwrap(),
            "custom/MyType",
        );
        assert_eq!(
            identifier_from_session_feather_path(custom, "backtest", "run-1").as_deref(),
            Some("inst"),
        );
    }
}
