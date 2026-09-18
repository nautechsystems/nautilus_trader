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
//! Holds primitives shared by catalog backends: separator normalization, local
//! path to URI conversion, object-store path constructors, identifier
//! sanitization, and persistence-owned type-to-prefix traits for
//! streaming/session writers. Backend-specific catalog paths are mapped in
//! `crate::catalog::types`.
//!
//! All path splitting in this module normalizes Windows `\` separators first,
//! so parsing behaves identically on every platform. Call sites must use these
//! helpers instead of splitting raw paths on `/`.

/// Persistence-owned static path prefix used by streaming/session writers.
pub trait CatalogPathPrefix {
    /// Returns the record family prefix.
    fn path_prefix() -> &'static str;
}

/// Normalizes Windows `\` separators to `/`.
///
/// Object stores and URIs always use forward slashes, while Windows local paths
/// use backslashes. Normalizing before splitting or joining keeps path parsing
/// identical on every platform.
#[must_use]
pub fn normalize_path_separators(path: &str) -> String {
    path.replace('\\', "/")
}

/// Extracts path components using platform-appropriate path parsing.
#[must_use]
pub fn extract_path_components(path_str: &str) -> Vec<String> {
    // Normalize separators and split
    let normalized = normalize_path_separators(path_str);
    normalized
        .split('/')
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Converts a local `PathBuf` to an object store path string.
#[must_use]
pub fn local_to_object_store_path(local_path: &std::path::Path) -> String {
    normalize_path_separators(&local_path.to_string_lossy())
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
        let normalized_base = normalize_path_separators(base_path)
            .trim_end_matches('/')
            .to_string();

        if !normalized_base.is_empty() {
            parts.push(normalized_base);
        }
    }

    for component in components {
        let normalized_component = normalize_path_separators(component.as_ref())
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
/// Backslashes are normalized first so Windows separators cannot bypass the filter.
#[must_use]
pub fn safe_directory_identifier(identifier: &str) -> String {
    let normalized = normalize_path_separators(identifier).replace("//", "/");
    let segments: Vec<&str> = normalized
        .split('/')
        .filter(|s| !s.is_empty() && *s != "..")
        .collect();
    segments.join("/")
}

/// Extracts the identifier from a file path: typically the second-to-last path component.
///
/// For example, from `data/quotes/EURUSD/file.parquet`, extracts `EURUSD`.
/// Both `/` and `\` separators are recognized so Windows paths resolve identically.
#[must_use]
pub fn extract_identifier_from_path(file_path: &str) -> Option<&str> {
    let parent = file_path.rfind(['/', '\\']).map(|idx| &file_path[..idx])?;
    let identifier = parent
        .rfind(['/', '\\'])
        .map_or(parent, |idx| &parent[idx + 1..]);
    (!identifier.is_empty()).then_some(identifier)
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

/// Normalizes a path to URI format for consistent object store usage.
///
/// If the path is already a URI (contains "://"), returns it as-is.
/// Otherwise, converts local paths to file:// URIs with proper cross-platform handling.
///
/// Supported URI schemes:
/// - `s3://` for AWS S3
/// - `gs://` or `gcs://` for Google Cloud Storage
/// - `az://` or `abfs://` for Azure Blob Storage
/// - `http://` or `https://` for HTTP/WebDAV
/// - `file://` for local files
///
/// # Cross-platform Path Handling
///
/// - Unix absolute paths: `/path/to/file` → `file:///path/to/file`
/// - Windows drive paths: `C:\path\to\file` → `file:///C:/path/to/file`
/// - Windows UNC paths: `\\server\share\file` → `file://server/share/file`
/// - Relative paths: converted to absolute using current directory
///
/// # Errors
///
/// Returns an error if the path is relative and the current working directory cannot be
/// resolved.
pub fn normalize_path_to_uri(path: &str) -> anyhow::Result<String> {
    if path.contains("://") {
        // Already a URI - return as-is
        Ok(path.to_string())
    } else if is_absolute_path(path) {
        Ok(path_to_file_uri(path))
    } else {
        // Relative path - make it absolute first
        let current_dir = std::env::current_dir().map_err(|e| {
            anyhow::anyhow!("Failed to resolve current directory for relative path '{path}': {e}")
        })?;

        let absolute_path = current_dir.join(path);
        Ok(path_to_file_uri(&absolute_path.to_string_lossy()))
    }
}

/// Checks if a path is absolute on any supported platform.
#[must_use]
fn is_absolute_path(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with("\\\\")
        || (path.len() >= 3
            && path.chars().nth(1) == Some(':')
            && matches!(path.chars().nth(2), Some('\\' | '/')))
}

/// Converts an absolute path to a file:// URI with proper platform handling.
#[must_use]
pub(crate) fn path_to_file_uri(path: &str) -> String {
    if path.starts_with('/') {
        // Unix absolute path
        format!("file://{path}")
    } else if path.len() >= 3 && path.chars().nth(1) == Some(':') {
        // Windows drive path - normalize separators and add proper prefix
        let normalized = normalize_path_separators(path);
        format!("file:///{normalized}")
    } else if let Some(without_prefix) = path.strip_prefix("\\\\") {
        // Windows UNC path \\server\share -> file://server/share
        let normalized = normalize_path_separators(without_prefix);
        format!("file://{normalized}")
    } else {
        // Fallback - treat as relative to root
        format!("file://{path}")
    }
}

/// Converts a file:// URI to a native path for the current platform.
/// On Windows, "file:///C:/x/y" becomes "C:\x\y" so LocalFileSystem and std::fs work correctly.
#[cfg(windows)]
pub(crate) fn file_uri_to_native_path(uri: &str) -> String {
    let without_scheme = uri
        .strip_prefix("file://")
        .or_else(|| uri.strip_prefix("file:"))
        .unwrap_or(uri);
    // Strip leading slash so "/C:/x/y" -> "C:/x/y", then use native separators
    let without_leading = without_scheme.trim_start_matches('/');
    without_leading.replace('/', "\\")
}

/// Converts a file:// URI to a path string for Unix (no-op; `object_store` accepts slash paths).
#[cfg(not(windows))]
pub(crate) fn file_uri_to_native_path(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
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
    let normalized = normalize_path_separators(path);
    let components: Vec<&str> = normalized
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
    let normalized = normalize_path_separators(path);
    let components: Vec<&str> = normalized
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
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn normalize_path_separators_converts_backslashes() {
        assert_eq!(
            normalize_path_separators(r"C:\catalog\backtest\run-1"),
            "C:/catalog/backtest/run-1",
        );
        assert_eq!(
            normalize_path_separators("C:/catalog/backtest/run-1"),
            "C:/catalog/backtest/run-1",
        );
        assert_eq!(
            normalize_path_separators(r"\\server\share\live\run-2"),
            "//server/share/live/run-2",
        );
    }

    #[rstest]
    fn extract_path_components_handles_platform_separators() {
        assert_eq!(
            extract_path_components(r"C:\catalog\backtest\run-1"),
            vec!["C:", "catalog", "backtest", "run-1"],
        );
        assert_eq!(
            extract_path_components("/catalog/backtest/run-1/"),
            vec!["catalog", "backtest", "run-1"],
        );
        assert!(extract_path_components("").is_empty());
    }

    #[rstest]
    fn extract_identifier_from_path_handles_platform_separators() {
        assert_eq!(
            extract_identifier_from_path("data/quotes/EURUSD/file.parquet"),
            Some("EURUSD"),
        );
        assert_eq!(
            extract_identifier_from_path(r"data\quotes\EURUSD\file.parquet"),
            Some("EURUSD"),
        );
        assert_eq!(
            extract_identifier_from_path(r"C:\data\quotes\EURUSD\file.parquet"),
            Some("EURUSD"),
        );
        assert_eq!(extract_identifier_from_path("file.parquet"), None);
        assert_eq!(extract_identifier_from_path(""), None);
    }

    #[rstest]
    fn safe_directory_identifier_blocks_windows_traversal() {
        assert_eq!(safe_directory_identifier(r"..\\..\\etc"), "etc");
        assert_eq!(safe_directory_identifier("../../etc"), "etc");
        assert_eq!(safe_directory_identifier("run-1"), "run-1");
    }

    #[rstest]
    fn test_normalize_path_to_uri() {
        // Unix absolute paths
        assert_eq!(
            normalize_path_to_uri("/tmp/test").unwrap(),
            "file:///tmp/test"
        );

        // Windows drive paths
        assert_eq!(
            normalize_path_to_uri("C:\\tmp\\test").unwrap(),
            "file:///C:/tmp/test"
        );
        assert_eq!(
            normalize_path_to_uri("C:/tmp/test").unwrap(),
            "file:///C:/tmp/test"
        );
        assert_eq!(
            normalize_path_to_uri("D:\\data\\file.txt").unwrap(),
            "file:///D:/data/file.txt"
        );

        // Windows UNC paths
        assert_eq!(
            normalize_path_to_uri("\\\\server\\share\\file").unwrap(),
            "file://server/share/file"
        );

        // Already URIs - should remain unchanged
        assert_eq!(
            normalize_path_to_uri("s3://bucket/path").unwrap(),
            "s3://bucket/path"
        );
        assert_eq!(
            normalize_path_to_uri("file:///tmp/test").unwrap(),
            "file:///tmp/test"
        );
        assert_eq!(
            normalize_path_to_uri("https://example.com/path").unwrap(),
            "https://example.com/path"
        );
    }

    #[rstest]
    fn test_is_absolute_path() {
        // Unix absolute paths
        assert!(is_absolute_path("/tmp/test"));
        assert!(is_absolute_path("/"));

        // Windows drive paths
        assert!(is_absolute_path("C:\\tmp\\test"));
        assert!(is_absolute_path("C:/tmp/test"));
        assert!(is_absolute_path("D:\\"));
        assert!(is_absolute_path("Z:/"));

        // Windows UNC paths
        assert!(is_absolute_path("\\\\server\\share"));
        assert!(is_absolute_path("\\\\localhost\\c$"));

        // Relative paths
        assert!(!is_absolute_path("tmp/test"));
        assert!(!is_absolute_path("./test"));
        assert!(!is_absolute_path("../test"));
        assert!(!is_absolute_path("test.txt"));

        // Edge cases
        assert!(!is_absolute_path(""));
        assert!(!is_absolute_path("C"));
        assert!(!is_absolute_path("C:"));
        assert!(!is_absolute_path("\\"));
    }

    #[rstest]
    fn test_path_to_file_uri() {
        // Unix absolute paths
        assert_eq!(path_to_file_uri("/tmp/test"), "file:///tmp/test");
        assert_eq!(path_to_file_uri("/"), "file:///");

        // Windows drive paths
        assert_eq!(path_to_file_uri("C:\\tmp\\test"), "file:///C:/tmp/test");
        assert_eq!(path_to_file_uri("C:/tmp/test"), "file:///C:/tmp/test");
        assert_eq!(path_to_file_uri("D:\\"), "file:///D:/");

        // Windows UNC paths
        assert_eq!(
            path_to_file_uri("\\\\server\\share\\file"),
            "file://server/share/file"
        );
        assert_eq!(
            path_to_file_uri("\\\\localhost\\c$\\test"),
            "file://localhost/c$/test"
        );
    }

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

    #[rstest]
    fn session_feather_paths_recover_type_and_identifier_with_backslashes() {
        let path = r"backtest\run-1\quotes\EURUSD.SIM\0001.feather";
        assert_eq!(
            type_name_from_session_feather_path(path, "backtest", "run-1").unwrap(),
            "quotes",
        );
        assert_eq!(
            identifier_from_session_feather_path(path, "backtest", "run-1").as_deref(),
            Some("EURUSD.SIM"),
        );
    }
}
