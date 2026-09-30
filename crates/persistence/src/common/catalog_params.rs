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

//! Validation of the `params` keys a built-in catalog accepts.

use nautilus_core::Params;
use serde_json::Value;

/// The JSON type a catalog param holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogParamKind {
    /// An object, such as `storage_options`.
    Object,
    /// A non-negative integer, such as `batch_size`.
    Count,
    /// A string, such as a codec name.
    Text,
    /// An array, such as `cluster_key_rules`.
    List,
}

impl CatalogParamKind {
    fn accepts(self, value: &Value) -> bool {
        match self {
            Self::Object => value.is_object(),
            Self::Count => value.is_u64(),
            Self::Text => value.is_string(),
            Self::List => value.is_array(),
        }
    }

    const fn expected(self) -> &'static str {
        match self {
            Self::Object => "an object",
            Self::Count => "a non-negative integer",
            Self::Text => "a string",
            Self::List => "a list",
        }
    }
}

/// Codec names the catalog `compression` param accepts, in lowercase.
pub const COMPRESSION_CODECS: &[&str] = &[
    "uncompressed",
    "snappy",
    "gzip",
    "brotli",
    "lz4",
    "lz4_raw",
    "zstd",
];

/// Reads the `compression` param as a lowercase codec name; a `null` value counts as absent.
///
/// # Errors
///
/// Returns an error if the value is not a string or names a codec outside [`COMPRESSION_CODECS`].
pub fn compression_name_from_params(params: Option<&Params>) -> anyhow::Result<Option<String>> {
    let Some(value) = params
        .and_then(|params| params.get("compression"))
        .filter(|value| !value.is_null())
    else {
        return Ok(None);
    };
    let given = value
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid catalog compression: expected a codec name"))?;
    let name = given.to_ascii_lowercase();
    anyhow::ensure!(
        COMPRESSION_CODECS.contains(&name.as_str()),
        "unknown compression `{given}`; valid values: {}",
        // `lz4_raw` is an alias of `lz4`
        COMPRESSION_CODECS
            .iter()
            .filter(|codec| **codec != "lz4_raw")
            .copied()
            .collect::<Vec<_>>()
            .join(", ")
    );

    Ok(Some(name))
}

/// Checks that the row-count params of `catalog` are positive when set.
///
/// # Errors
///
/// Returns an error naming the catalog and the key if `batch_size` or `max_row_group_size` is zero.
pub fn validate_catalog_counts(catalog: &str, params: Option<&Params>) -> anyhow::Result<()> {
    for key in ["batch_size", "max_row_group_size"] {
        anyhow::ensure!(
            params.and_then(|params| params.get_u64(key)) != Some(0),
            "Invalid {catalog} catalog param '{key}': must be a positive number of rows; omit the field for the backend default"
        );
    }
    Ok(())
}

/// Checks that `params` holds only the `accepted` keys, each with its declared type.
///
/// A `null` value counts as absent.
///
/// # Errors
///
/// Returns an error naming the catalog and the key if a key is not accepted or its value has the
/// wrong type.
pub fn validate_catalog_params(
    catalog: &str,
    params: Option<&Params>,
    accepted: &[(&str, CatalogParamKind)],
) -> anyhow::Result<()> {
    let Some(params) = params else {
        return Ok(());
    };

    for (key, value) in params {
        let Some((_, kind)) = accepted.iter().find(|(name, _)| name == key) else {
            let names = accepted
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "Unknown {catalog} catalog param '{key}'{}",
                if names.is_empty() {
                    ": this catalog takes no params".to_string()
                } else {
                    format!(", expected one of {names}")
                }
            );
        };

        anyhow::ensure!(
            value.is_null() || kind.accepts(value),
            "Invalid {catalog} catalog param '{key}': expected {}",
            kind.expected()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    const ACCEPTED: &[(&str, CatalogParamKind)] = &[
        ("storage_options", CatalogParamKind::Object),
        ("batch_size", CatalogParamKind::Count),
        ("compression", CatalogParamKind::Text),
        ("cluster_key_rules", CatalogParamKind::List),
    ];

    fn params(key: &str, value: Value) -> Params {
        let mut params = Params::new();
        params.insert(key.to_string(), value);
        params
    }

    #[rstest]
    fn accepts_declared_keys_with_their_types() {
        let mut params = params("storage_options", json!({"region": "eu-west-1"}));
        params.insert("batch_size".to_string(), json!(1024));
        params.insert("compression".to_string(), json!("zstd"));
        params.insert("cluster_key_rules".to_string(), json!([]));

        assert!(validate_catalog_params("Test", Some(&params), ACCEPTED).is_ok());
        assert!(validate_catalog_params("Test", None, ACCEPTED).is_ok());
    }

    #[rstest]
    #[case::batch_size("batch_size")]
    #[case::max_row_group_size("max_row_group_size")]
    fn zero_counts_are_rejected(#[case] key: &str) {
        let error = validate_catalog_counts("Test", Some(&params(key, json!(0))))
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            format!(
                "Invalid Test catalog param '{key}': must be a positive number of rows; omit the field for the backend default"
            )
        );
    }

    #[rstest]
    fn lzo_is_not_an_accepted_codec() {
        let error = compression_name_from_params(Some(&params("compression", json!("lzo"))))
            .unwrap_err()
            .to_string();

        assert!(error.starts_with("unknown compression `lzo`"), "{error}");
    }

    #[rstest]
    fn null_counts_as_absent() {
        let params = params("batch_size", Value::Null);

        assert!(validate_catalog_params("Test", Some(&params), ACCEPTED).is_ok());
    }

    #[rstest]
    #[case::misspelled("no_such_param", json!(1), "Unknown Test catalog param 'no_such_param'")]
    #[case::wrong_count("batch_size", json!("big"), "Invalid Test catalog param 'batch_size': expected a non-negative integer")]
    #[case::negative_count("batch_size", json!(-1), "Invalid Test catalog param 'batch_size': expected a non-negative integer")]
    #[case::wrong_object("storage_options", json!("x"), "Invalid Test catalog param 'storage_options': expected an object")]
    #[case::wrong_text("compression", json!(6), "Invalid Test catalog param 'compression': expected a string")]
    fn rejects_a_misspelled_key_or_wrong_type(
        #[case] key: &str,
        #[case] value: Value,
        #[case] expected: &str,
    ) {
        let error = validate_catalog_params("Test", Some(&params(key, value)), ACCEPTED)
            .unwrap_err()
            .to_string();

        assert!(error.starts_with(expected), "{error}");
    }

    #[rstest]
    #[case::absent(None, None)]
    #[case::null(Some(Value::Null), None)]
    #[case::lowercase(Some(json!("zstd")), Some("zstd"))]
    #[case::mixed_case(Some(json!("Snappy")), Some("snappy"))]
    fn compression_name_is_lowercased(
        #[case] value: Option<Value>,
        #[case] expected: Option<&str>,
    ) {
        let params = value.map(|value| params("compression", value));

        assert_eq!(
            compression_name_from_params(params.as_ref()).unwrap(),
            expected.map(ToString::to_string)
        );
    }

    #[rstest]
    #[case::numeric_code(json!(6), "Invalid catalog compression: expected a codec name")]
    #[case::unknown_codec(
        json!("zip"),
        "unknown compression `zip`; valid values: uncompressed, snappy, gzip, brotli, lz4, zstd"
    )]
    fn compression_name_rejects_codes_and_unknown_codecs(
        #[case] value: Value,
        #[case] expected: &str,
    ) {
        let error = compression_name_from_params(Some(&params("compression", value)))
            .unwrap_err()
            .to_string();

        assert_eq!(error, expected);
    }
}
