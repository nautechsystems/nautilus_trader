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

//! Parquet-specific path, filename, and identifier operations.
//!
//! Cross-backend primitives (the `CatalogPathPrefix` trait, object-store path constructors,
//! identifier sanitization) live in [`crate::common::paths`] and are re-exported below for
//! parquet internal callers.

use std::path::{Path, PathBuf};

use nautilus_core::{
    UnixNanos,
    datetime::{iso8601_to_unix_nanos, unix_nanos_to_iso8601},
};

pub use crate::common::paths::{
    CatalogPathPrefix, extract_identifier_from_path, make_object_store_path,
    make_sql_safe_identifier, safe_directory_identifier, urisafe_instrument_id,
};

/// Converts timestamps to a filename using ISO 8601 format.
///
/// Returns a filename string in the format: "`iso_timestamp_1_iso_timestamp_2.parquet`".
#[must_use]
pub fn timestamps_to_filename(timestamp_1: UnixNanos, timestamp_2: UnixNanos) -> String {
    let datetime_1 = iso_timestamp_to_file_timestamp(&unix_nanos_to_iso8601(timestamp_1));
    let datetime_2 = iso_timestamp_to_file_timestamp(&unix_nanos_to_iso8601(timestamp_2));

    format!("{datetime_1}_{datetime_2}.parquet")
}

/// Converts an ISO 8601 timestamp to a filesystem-safe format.
pub(crate) fn iso_timestamp_to_file_timestamp(iso_timestamp: &str) -> String {
    iso_timestamp.replace([':', '.'], "-")
}

/// Converts a filesystem-safe timestamp back to ISO 8601 format.
pub(crate) fn file_timestamp_to_iso_timestamp(file_timestamp: &str) -> String {
    let (date_part, time_part) = file_timestamp
        .split_once('T')
        .unwrap_or((file_timestamp, ""));
    let time_part = time_part.strip_suffix('Z').unwrap_or(time_part);

    // Find the last hyphen to separate nanoseconds
    if let Some(last_hyphen_idx) = time_part.rfind('-') {
        let time_with_dot_for_nanos = format!(
            "{}.{}",
            &time_part[..last_hyphen_idx],
            &time_part[last_hyphen_idx + 1..]
        );
        let final_time_part = time_with_dot_for_nanos.replace('-', ":");
        format!("{date_part}T{final_time_part}Z")
    } else {
        // Fallback if no nanoseconds part found
        let final_time_part = time_part.replace('-', ":");
        format!("{date_part}T{final_time_part}Z")
    }
}

/// Converts an ISO 8601 timestamp string to Unix nanoseconds.
pub(crate) fn iso_to_unix_nanos(iso_timestamp: &str) -> anyhow::Result<u64> {
    Ok(iso8601_to_unix_nanos(iso_timestamp)?.into())
}

// Extract the instrument ID portion from a bar type directory name.
// Handles both standard and composite formats:
//   {id}-{step}-{agg}-{price}-{source}
//   {id}-{step}-{agg}-{price}-{source}@{step}-{agg}-{source}
// Strips the composite suffix before parsing with rsplitn(5, '-').
pub(crate) fn extract_bar_type_instrument_id(bar_type_dir: &str) -> Option<&str> {
    let standard = bar_type_dir.split('@').next().unwrap_or(bar_type_dir);
    let pieces: Vec<&str> = standard.rsplitn(5, '-').collect();
    // pieces (reversed): [source, price_type, agg, step, instrument_id]
    if pieces.len() == 5 && pieces[3].chars().all(|c| c.is_ascii_digit()) {
        Some(pieces[4])
    } else {
        None
    }
}

/// Extracts the filename from a file path and makes it SQL-safe.
#[must_use]
pub fn extract_sql_safe_filename(file_path: &str) -> String {
    if file_path.is_empty() {
        return "unknown_file".to_string();
    }

    let filename = file_path.split('/').next_back().unwrap_or("unknown_file");

    // Remove .parquet extension
    let name_without_ext = if let Some(dot_pos) = filename.rfind(".parquet") {
        &filename[..dot_pos]
    } else {
        filename
    };

    // Remove characters that can pose problems: hyphens, colons, etc.
    name_without_ext
        .replace(['-', ':', '.'], "_")
        .to_lowercase()
}

/// Creates a platform-appropriate local path using `PathBuf`.
pub fn make_local_path<P: AsRef<Path>>(base_path: P, components: &[&str]) -> PathBuf {
    let mut path = PathBuf::from(base_path.as_ref());
    for component in components {
        path.push(component);
    }
    path
}

/// Converts a local `PathBuf` to an object store path string.
#[must_use]
pub fn local_to_object_store_path(local_path: &Path) -> String {
    local_path.to_string_lossy().replace('\\', "/")
}

/// Extracts path components using platform-appropriate path parsing.
#[must_use]
pub fn extract_path_components(path_str: &str) -> Vec<String> {
    // Normalize separators and split
    let normalized = path_str.replace('\\', "/");
    normalized
        .split('/')
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Checks if a filename's timestamp range intersects with a query interval.
pub(crate) fn query_intersects_filename(
    filename: &str,
    start: Option<u64>,
    end: Option<u64>,
) -> bool {
    if let Some((file_start, file_end)) = parse_filename_timestamps(filename) {
        start.is_none_or(|start| start <= file_end) && end.is_none_or(|end| file_start <= end)
    } else {
        true
    }
}

/// Parses timestamps from a Parquet filename.
///
/// Extracts the start and end timestamps from filenames that follow the ISO 8601 format:
/// "`iso_timestamp_1_iso_timestamp_2.parquet`".
#[must_use]
pub fn parse_filename_timestamps(filename: &str) -> Option<(u64, u64)> {
    let path = Path::new(filename);
    let base_name = path.file_name()?.to_str()?;
    let base_filename = base_name.strip_suffix(".parquet")?;
    let mut parts = base_filename.split('_');
    let first_part = parts.next()?;
    let second_part = parts.next()?;
    if let Some(replay_identity) = parts.next()
        && (replay_identity.is_empty()
            || !replay_identity
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric()))
    {
        return None;
    }

    if parts.next().is_some() {
        return None;
    }

    let first_iso = file_timestamp_to_iso_timestamp(first_part);
    let second_iso = file_timestamp_to_iso_timestamp(second_part);

    let first_ts = iso_to_unix_nanos(&first_iso).ok()?;
    let second_ts = iso_to_unix_nanos(&second_iso).ok()?;

    Some((first_ts, second_ts))
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::{parse_filename_timestamps, timestamps_to_filename};

    #[rstest]
    fn parse_filename_timestamps_accepts_replay_identity_suffix() {
        let base = timestamps_to_filename(UnixNanos::from(1), UnixNanos::from(2));
        let filename = base.replace(".parquet", "_replay.parquet");

        assert_eq!(parse_filename_timestamps(&filename), Some((1, 2)));
    }

    #[rstest]
    fn parse_filename_timestamps_rejects_non_timestamp_segments() {
        let base = timestamps_to_filename(UnixNanos::from(1), UnixNanos::from(2));
        let filename = base.replace(".parquet", "_bad-suffix.parquet");

        assert_eq!(parse_filename_timestamps(&filename), None);
    }
}
