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

//! Object-store paths, listing, and file administration for the Parquet catalog.

#![expect(
    clippy::missing_errors_doc,
    reason = "catalog store functions forward object-store errors"
)]

use nautilus_common::live::block_on_nautilus_with;
use object_store::ObjectMeta;

use super::{
    HashSet, ObjectPath, ObjectStore, ObjectStoreExt, ParquetDataCatalog, PathBuf, StreamExt,
    UnixNanos, append_path_to_file_uri, are_intervals_disjoint, decode_object_store_segment,
    is_remote_uri_scheme, make_object_store_path, parse_filename_timestamps,
    query::{filter_identifier_files, is_parquet_bar_prefix},
    query_intersects_filename, remote_full_uri, remote_store_root_url, timestamps_to_filename,
    urisafe_instrument_id,
};
use crate::{
    catalog::types::{
        CatalogDataType, custom_data_read_prefixes, custom_type_name,
        parquet_catalog_data_type_path_prefixes,
    },
    common::paths::normalize_path_separators,
};

impl ParquetDataCatalog {
    /// Extends the timestamp range of an existing Parquet file by renaming it.
    ///
    /// This method finds an existing file that is adjacent to the specified time range
    /// and renames it to include the new range. This is useful when appending data
    /// that extends the time coverage of existing files.
    /// The proposed extension is validated against the other files before renaming,
    /// so a rejected extension leaves existing files unchanged.
    ///
    /// If no file is adjacent to the specified range, this method does nothing and
    /// returns `Ok(())` after confirming the existing intervals are disjoint.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The stored family to target.
    /// - `identifier`: Optional identifier to target a specific instrument's data. Can be an `instrument_id` (e.g., "EUR/USD.SIM") or a `bar_type` (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    /// - `start`: Start timestamp of the new range to extend to.
    /// - `end`: End timestamp of the new range to extend to.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The range is reversed (`start` is after `end`).
    /// - The directory path cannot be constructed.
    /// - The proposed extension would overlap another file.
    /// - The existing intervals are already overlapping.
    /// - File rename operations fail.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_core::UnixNanos;
    /// use nautilus_model::data::NautilusDataType;
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    ///
    /// // Extend a file's range backwards or forwards
    /// catalog.extend_file_name(
    ///     &NautilusDataType::QuoteTick.into(),
    ///     Some("BTC/USD.SIM"),
    ///     UnixNanos::from(1609459200000000000),
    ///     UnixNanos::from(1609545600000000000),
    /// )?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn extend_file_name(
        &self,
        data_type: &CatalogDataType,
        identifier: Option<&str>,
        start: UnixNanos,
        end: UnixNanos,
    ) -> anyhow::Result<()> {
        let prefixes = parquet_catalog_data_type_path_prefixes(data_type);

        if let [data_cls] = prefixes.as_slice() {
            let directory = self.make_path(data_cls.as_ref(), identifier)?;
            return self.extend_file_name_in_directory(&directory, start, end);
        }

        // The aggregate instrument family spans every class directory, and one identifier can be
        // stored under several classes, so extend each directory that already holds it rather
        // than inventing a class for it.
        let mut extended = false;

        for data_cls in &prefixes {
            let directory = self.make_path(data_cls.as_ref(), identifier)?;
            if !self.get_directory_intervals(&directory)?.is_empty() {
                self.extend_file_name_in_directory(&directory, start, end)?;
                extended = true;
            }
        }

        anyhow::ensure!(
            extended,
            "Cannot extend file name for {data_type}: no instrument class holds {}; \
             name the class with a NautilusInstrumentType",
            identifier.unwrap_or("any identifier"),
        );

        Ok(())
    }

    pub(super) fn extend_file_name_in_directory(
        &self,
        directory: &str,
        start: UnixNanos,
        end: UnixNanos,
    ) -> anyhow::Result<()> {
        let start = start.as_u64();
        let end = end.as_u64();

        anyhow::ensure!(
            start <= end,
            "Cannot extend file in directory '{directory}': reversed range ({start}, {end})",
        );

        let intervals = self.get_directory_intervals(directory)?;

        anyhow::ensure!(
            are_intervals_disjoint(&intervals),
            "Intervals are not disjoint in directory '{directory}': {intervals:?}",
        );

        let adjacent = intervals.iter().enumerate().find_map(|(index, interval)| {
            if end.checked_add(1) == Some(interval.0) {
                // Extend backwards: new file covers [start, interval.1]
                Some((index, *interval, (start, interval.1)))
            } else if start.checked_sub(1) == Some(interval.1) {
                // Extend forwards: new file covers [interval.0, end]
                Some((index, *interval, (interval.0, end)))
            } else {
                None
            }
        });

        let Some((index, original, proposed)) = adjacent else {
            return Ok(());
        };

        let mut extended = intervals.clone();
        extended[index] = proposed;

        anyhow::ensure!(
            are_intervals_disjoint(&extended),
            "Extending file interval {original:?} to {proposed:?} in directory '{directory}' \
            with range ({start}, {end}) would create non-disjoint intervals. \
            Existing intervals: {intervals:?}",
        );

        self.rename_parquet_file(directory, original.0, original.1, proposed.0, proposed.1)
    }

    /// Helper method to rename a parquet file by moving it via object store operations
    fn rename_parquet_file(
        &self,
        directory: &str,
        old_start: u64,
        old_end: u64,
        new_start: u64,
        new_end: u64,
    ) -> anyhow::Result<()> {
        let new_filename =
            timestamps_to_filename(UnixNanos::from(new_start), UnixNanos::from(new_end));
        let new_path = format!("{directory}/{new_filename}");
        let matches = self
            .list_parquet_files(directory)?
            .into_iter()
            .filter(|file| parse_filename_timestamps(file) == Some((old_start, old_end)))
            .collect::<Vec<_>>();

        anyhow::ensure!(
            matches.len() == 1,
            "expected one parquet file for interval ({old_start}, {old_end}) in {directory}, \
             found {}",
            matches.len()
        );

        let old_path = &matches[0];
        if old_path.ends_with(&new_filename) {
            return Ok(());
        }

        let old_object_path = self.to_object_path_parsed(old_path)?;
        let new_object_path = self.to_object_path(&new_path)?;
        self.move_file(&old_object_path, &new_object_path)
    }

    /// Lists all Parquet files in a specified directory.
    ///
    /// This method scans a directory and returns the full paths of all files with the `.parquet`
    /// extension. It works with both local filesystems and remote object stores, making it
    /// suitable for various storage backends.
    ///
    /// # Parameters
    ///
    /// - `directory`: The directory path to scan for Parquet files.
    ///
    /// # Returns
    ///
    /// Returns a vector of full file paths (as strings) for all Parquet files found in the directory.
    /// The paths are relative to the object store root and suitable for use with object store operations.
    /// Returns an empty vector if the directory doesn't exist or contains no Parquet files.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    /// - Network issues occur (for remote object stores).
    ///
    /// # Notes
    ///
    /// - Only files ending with `.parquet` are included.
    /// - Subdirectories are not recursively scanned.
    /// - File paths are returned in the order provided by the object store.
    /// - Works with all supported object store backends (local, S3, GCS, Azure, etc.).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    /// let files = catalog.list_parquet_files("data/quotes/EURUSD")?;
    ///
    /// for file in files {
    ///     println!("Found Parquet file: {}", file);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn list_parquet_files(&self, directory: &str) -> anyhow::Result<Vec<String>> {
        self.execute_async(|| async {
            let prefix = ObjectPath::from(format!("{directory}/"));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut files = Vec::new();

            while let Some(object) = stream.next().await {
                let object = object?;
                if object.location.as_ref().ends_with(".parquet") {
                    files.push(object.location.to_string());
                }
            }

            Ok::<Vec<String>, anyhow::Error>(files)
        })
    }

    /// Lists all instrument identifiers for a specific data type.
    ///
    /// This method scans the data directory for a given data type and extracts
    /// all unique instrument identifiers from the directory structure.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The stored family to target.
    ///
    /// # Returns
    ///
    /// Returns a vector of instrument identifier strings.
    ///
    /// # Errors
    ///
    /// Returns an error if directory listing fails.
    pub fn list_instruments(&self, data_type: &CatalogDataType) -> anyhow::Result<Vec<String>> {
        let prefixes = match custom_type_name(data_type) {
            Some(type_name) => Vec::from(custom_data_read_prefixes(type_name)),
            None => parquet_catalog_data_type_path_prefixes(data_type),
        };

        let mut instruments = Vec::new();

        for prefix in prefixes {
            instruments.extend(self.list_prefix_instruments(prefix.as_ref())?);
        }

        // The same identifier can live under more than one instrument class.
        instruments.sort();
        instruments.dedup();

        Ok(instruments)
    }

    fn list_prefix_instruments(&self, data_type: &str) -> anyhow::Result<Vec<String>> {
        let prefix = ObjectPath::from(self.make_path(data_type, None)?);

        self.execute_async(|| async {
            let mut stream = self.object_store.list(Some(&prefix));
            let mut instruments = HashSet::new();

            while let Some(object) = stream.next().await {
                let object = object?;

                // Relative to the prefix a datum is `{identifier}/{filename}.parquet`
                let Some(relative) = object.location.prefix_match(&prefix) else {
                    continue;
                };
                let segments: Vec<_> = relative.collect();

                if let [identifier, filename] = segments.as_slice()
                    && filename.as_ref().ends_with(".parquet")
                {
                    instruments.insert(decode_object_store_segment(identifier.as_ref()));
                }
            }

            Ok::<Vec<String>, anyhow::Error>(instruments.into_iter().collect())
        })
    }

    /// Lists Parquet files matching specific criteria (data type, identifiers, time range).
    ///
    /// This method finds all Parquet files that match the specified criteria by filtering
    /// files based on their directory structure and filename timestamps.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The stored family to target.
    /// - `identifiers`: Optional list of identifiers to filter by.
    /// - `start`: Optional start timestamp to filter files by their time range.
    /// - `end`: Optional end timestamp to filter files by their time range.
    ///
    /// # Returns
    ///
    /// Returns a vector of file paths that match the criteria.
    ///
    /// # Errors
    ///
    /// Returns an error if directory listing or file filtering fails.
    pub fn list_parquet_files_with_criteria(
        &self,
        data_type: &CatalogDataType,
        identifiers: Option<&[String]>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<String>> {
        if let Some(type_name) = custom_type_name(data_type) {
            let mut all_files = Vec::new();
            for prefix in custom_data_read_prefixes(type_name) {
                all_files.extend(self.list_prefix_files_with_criteria(
                    prefix.as_ref(),
                    identifiers,
                    start,
                    end,
                )?);
            }

            all_files.sort();
            all_files.dedup();
            return Ok(all_files);
        }

        let mut all_files = Vec::new();
        for data_cls in parquet_catalog_data_type_path_prefixes(data_type) {
            all_files.extend(self.list_prefix_files_with_criteria(
                data_cls.as_ref(),
                identifiers,
                start,
                end,
            )?);
        }

        Ok(all_files)
    }

    fn list_prefix_files_with_criteria(
        &self,
        data_cls: &str,
        identifiers: Option<&[String]>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<String>> {
        let start_u64 = start.map(|s| s.as_u64());
        let end_u64 = end.map(|e| e.as_u64());

        let base_dir = self.make_path(data_cls, None)?;

        // Use recursive listing to match Python's glob behavior
        let mut all_files = self
            .list_objects(&base_dir)?
            .into_iter()
            .map(|object| object.location.to_string())
            .filter(|path| path.ends_with(".parquet"))
            .collect::<Vec<_>>();

        if let Some(identifiers) = identifiers {
            all_files =
                filter_identifier_files(all_files, identifiers, is_parquet_bar_prefix(data_cls));
        }

        all_files.retain(|path| query_intersects_filename(path, start_u64, end_u64));
        Ok(all_files)
    }

    /// Recursively lists all objects under `{dir}/` in the object store.
    pub(super) fn list_objects(&self, dir: &str) -> anyhow::Result<Vec<ObjectMeta>> {
        self.execute_async(|| async {
            let prefix = ObjectPath::from(format!("{dir}/"));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut objects = Vec::new();
            while let Some(object) = stream.next().await {
                objects.push(object?);
            }

            Ok(objects)
        })
    }

    /// Helper method to reconstruct full URI for remote object store paths
    #[must_use]
    pub fn reconstruct_full_uri(&self, path_str: &str) -> String {
        if path_str.contains("://") {
            return path_str.to_string();
        }

        // Check if this is a remote URI scheme that needs reconstruction
        if self.is_remote_uri() {
            let path = self.path_under_base(path_str);
            if let Ok(uri) = remote_full_uri(&self.original_uri, &path) {
                return uri;
            }
        }

        // For local paths, extract the directory from the original URI
        if self.original_uri.starts_with("file://") {
            // Extract the path from the file:// URI
            if let Ok(url) = url::Url::parse(&self.original_uri)
                && let Ok(base_path) = url.to_file_path()
            {
                // Use platform-appropriate path separator for display
                // but object store paths always use forward slashes
                let base_str = base_path.to_string_lossy();
                return make_object_store_path(&base_str, [path_str]);
            }
        }

        // For local paths without file:// prefix, use the original URI as base
        if self.base_path.is_empty() {
            // If base_path is empty and not a file URI, try using original_uri as base
            if self.original_uri.contains("://") {
                // Fallback: return the path as-is
                path_str.to_string()
            } else {
                make_object_store_path(self.original_uri.trim_end_matches('/'), [path_str])
            }
        } else {
            let base = self.base_path.trim_end_matches('/');
            make_object_store_path(base, [path_str])
        }
    }

    /// Resolves a path for use with DataFusion (avoiding Windows path doubling for file://).
    /// Returns the path as-is if it is already a full URI or absolute; otherwise builds
    /// file:// base + path for local catalogs or `reconstruct_full_uri` for remote.
    #[must_use]
    pub(crate) fn resolve_path_for_datafusion(&self, path: &str) -> String {
        if path.contains("://") {
            return path.to_string();
        }

        if path.starts_with('/') {
            return path.to_string();
        }

        if self.original_uri.starts_with("file://") {
            return append_path_to_file_uri(&self.original_uri, path);
        }

        self.reconstruct_full_uri(path)
    }

    /// Like `resolve_path_for_datafusion` but ensures the result ends with a trailing slash.
    #[must_use]
    pub(super) fn resolve_directory_for_datafusion(&self, directory: &str) -> String {
        let mut resolved = self.resolve_path_for_datafusion(directory);
        if !resolved.ends_with('/') {
            resolved.push('/');
        }

        resolved
    }

    /// Returns the path string to push in `query_files` result list: relative for file://,
    /// full URI for remote (so callers can pass to `resolve_path_for_datafusion` later).
    #[must_use]
    pub(super) fn path_for_query_list(&self, path: &str) -> String {
        if self.original_uri.starts_with("file://") {
            path.to_string()
        } else {
            self.reconstruct_full_uri(path)
        }
    }

    /// Returns the native path string for the catalog root (for `std::fs`). Only valid when
    /// !`is_remote_uri()`; uses parquet's `file_uri_to_native_path` for file:// URIs.
    #[must_use]
    pub(crate) fn native_base_path_string(&self) -> String {
        if self.original_uri.starts_with("file://") {
            crate::backend::parquet::io::file_uri_to_native_path(&self.original_uri)
        } else {
            self.original_uri.clone()
        }
    }

    pub(crate) fn register_remote_object_store(&mut self) -> anyhow::Result<()> {
        if self.is_remote_uri() {
            let base_url = remote_store_root_url(&self.original_uri)?;
            self.session
                .register_object_store(&base_url, self.object_store.clone());
        }

        Ok(())
    }

    /// Helper method to check if the original URI uses a remote object store scheme
    #[must_use]
    pub fn is_remote_uri(&self) -> bool {
        self.original_uri
            .split_once("://")
            .is_some_and(|(scheme, _)| is_remote_uri_scheme(scheme))
    }

    /// Constructs a directory path for storing data of a specific type and instrument.
    ///
    /// This method builds the hierarchical directory structure used by the catalog to organize
    /// data by type and instrument. The path follows the pattern: `{base_path}/data/{type_name}/{instrument_id}`.
    /// Instrument IDs are automatically converted to URI-safe format by removing forward slashes.
    ///
    /// # Parameters
    ///
    /// - `type_name`: The data type directory name (e.g., "quotes", "trades", "bars").
    /// - `identifier`: Optional identifier. Can be an `instrument_id` (e.g., "EUR/USD.SIM") or a `bar_type` (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL"). If provided, creates a subdirectory for the identifier. If `None`, returns the path to the data type directory.
    ///
    /// # Returns
    ///
    /// Returns the constructed directory path as a string, or an error if path construction fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument ID contains invalid characters that cannot be made URI-safe.
    /// - Path construction fails due to system limitations.
    ///
    /// # Path Structure
    ///
    /// - Without identifier: `{base_path}/data/{type_name}`.
    /// - With identifier: `{base_path}/data/{type_name}/{safe_identifier}`.
    /// - If `base_path` is empty: `data/{type_name}[/{safe_identifier}]`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    ///
    /// // Path for all quote data
    /// let quotes_path = catalog.make_path("quotes", None)?;
    /// // Returns: "/base/path/data/quotes"
    ///
    /// // Path for specific instrument quotes
    /// let eurusd_quotes = catalog.make_path("quotes", Some("EUR/USD"))?;
    /// // Returns: "/base/path/data/quotes/EURUSD" (slash removed)
    ///
    /// // Path for bar data with complex instrument ID
    /// let bars_path = catalog.make_path("bars", Some("BTC/USD-1H"))?;
    /// // Returns: "/base/path/data/bars/BTCUSD-1H"
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn make_path(&self, type_name: &str, identifier: Option<&str>) -> anyhow::Result<String> {
        let mut components = vec!["data".to_string(), type_name.to_string()];

        if let Some(id) = identifier {
            let safe_id = urisafe_instrument_id(id);
            components.push(safe_id);
        }

        let path = make_object_store_path(&self.base_path, components);
        Ok(path)
    }

    /// Builds the directory path for custom data: `data/custom/{type_name}[/{identifier}]`.
    pub fn make_path_custom_data(
        &self,
        type_name: &str,
        identifier: Option<&str>,
    ) -> anyhow::Result<String> {
        let mut components = vec![
            "data".to_string(),
            "custom".to_string(),
            type_name.to_string(),
        ];

        if let Some(id) = identifier {
            let safe_id = urisafe_instrument_id(id);

            if !safe_id.is_empty() {
                components.push(safe_id);
            }
        }

        let path = make_object_store_path(&self.base_path, components);
        Ok(path)
    }

    /// Converts a catalog path string to an [`ObjectPath`] for object store operations.
    ///
    /// This method handles the conversion between catalog-relative paths and object store paths,
    /// taking into account the catalog's base path configuration. It automatically preserves the
    /// base path prefix for remote catalogs and strips it for local catalog paths.
    ///
    /// # Parameters
    ///
    /// - `path`: The catalog path string to convert. Can be absolute or relative.
    ///
    /// # Returns
    ///
    /// Returns an [`ObjectPath`] suitable for use with object store operations.
    ///
    /// # Path Handling
    ///
    /// - If `base_path` is empty, the path is used as-is.
    /// - If `base_path` is set for a remote catalog, it's preserved or prepended.
    /// - If `base_path` is set for a local catalog, it's stripped from the path if present.
    /// - Trailing slashes and backslashes are automatically handled.
    /// - The resulting path is relative to the object store root.
    /// - All paths are normalized to use forward slashes (object store convention).
    ///
    /// # Errors
    ///
    /// Returns an error for remote catalogs when `path` is a full URI whose scheme/host
    /// does not match the catalog's own root (cross-bucket misuse). Without this guard
    /// the caller could silently write to or read from the wrong bucket.
    ///
    /// # Examples
    ///
    /// Local catalog paths (absolute or relative) strip the catalog's base directory:
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    /// # let catalog: ParquetDataCatalog = unimplemented!();
    /// let object_path = catalog.to_object_path("/base/data/quotes/file.parquet")?;
    /// // ObjectPath("data/quotes/file.parquet")
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    ///
    /// Remote catalog paths (relative or full URI) preserve or prepend the base prefix:
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    /// # let catalog: ParquetDataCatalog = unimplemented!();
    /// let object_path = catalog.to_object_path("data/trades/file.parquet")?;
    /// // ObjectPath("base/data/trades/file.parquet")
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn to_object_path(&self, path: &str) -> anyhow::Result<ObjectPath> {
        Ok(ObjectPath::from(self.object_store_path(path)?))
    }

    /// Converts a path string to [`ObjectPath`] using parse (no percent-encoding).
    ///
    /// Use this for paths that were returned by the object store (e.g. from `list()`),
    /// which may already be percent-encoded. Using [`Self::to_object_path`] (which uses
    /// `Path::from`) on such paths would double-encode (e.g. `%5E` -> `%255E`).
    ///
    /// # Errors
    ///
    /// Returns an error for the same cross-bucket case as [`Self::to_object_path`], or
    /// when the resulting string fails [`ObjectPath::parse`].
    pub fn to_object_path_parsed(&self, path: &str) -> anyhow::Result<ObjectPath> {
        let to_parse = self.object_store_path(path)?;
        ObjectPath::parse(&to_parse).map_err(anyhow::Error::from)
    }

    fn object_store_path(&self, path: &str) -> anyhow::Result<String> {
        let normalized_path = normalize_path_separators(path);

        if self.is_remote_uri() {
            if normalized_path.contains("://") {
                let path_under_root = self.remote_uri_object_path(&normalized_path)?;
                return Ok(self.path_under_base(&path_under_root));
            }

            return Ok(self.path_under_base(&normalized_path));
        }

        Ok(self.path_without_local_base(&normalized_path))
    }

    fn remote_uri_object_path(&self, path: &str) -> anyhow::Result<String> {
        let path_url = url::Url::parse(path)
            .map_err(|e| anyhow::anyhow!("Failed to parse object store URI {path}: {e}"))?;
        if !is_remote_uri_scheme(path_url.scheme()) {
            anyhow::bail!(
                "URI {path} uses non-remote scheme {} for remote catalog at {}",
                path_url.scheme(),
                self.original_uri,
            );
        }

        let catalog_root = remote_store_root_url(&self.original_uri)?;
        let path_root = remote_store_root_url(path)?;
        if catalog_root.as_str().trim_end_matches('/') != path_root.as_str().trim_end_matches('/') {
            anyhow::bail!(
                "Cross-store URI {path} (root {}) does not belong to catalog rooted at {} ({})",
                path_root.as_str().trim_end_matches('/'),
                self.original_uri,
                catalog_root.as_str().trim_end_matches('/'),
            );
        }

        // The URL crate keeps the path component percent-encoded (e.g. `%5E`),
        // so preserve that encoding for `ObjectPath::parse` round-trips through
        // `object_store::list`/`get`.
        Ok(path_url.path().trim_start_matches('/').to_string())
    }

    pub(crate) fn path_without_local_base(&self, path: &str) -> String {
        let base_path = if self.base_path.is_empty() {
            self.native_base_path_string()
        } else {
            self.base_path.clone()
        };

        let normalized_base = normalize_path_separators(&base_path);
        let base = normalized_base.trim_end_matches('/');

        if base.is_empty() {
            path.to_string()
        } else if path == base {
            String::new()
        } else if let Some(without_base) = path.strip_prefix(&format!("{base}/")) {
            without_base.to_string()
        } else {
            path.to_string()
        }
    }

    fn path_under_base(&self, path: &str) -> String {
        let normalized_path = normalize_path_separators(path);
        let path = normalized_path
            .trim_start_matches('/')
            .trim_end_matches('/');

        if self.base_path.is_empty() {
            return path.to_string();
        }

        let normalized_base = normalize_path_separators(&self.base_path);
        let base = normalized_base
            .trim_start_matches('/')
            .trim_end_matches('/');

        if base.is_empty() || path == base || path.starts_with(&format!("{base}/")) {
            path.to_string()
        } else if path.is_empty() {
            base.to_string()
        } else {
            make_object_store_path(base, [path])
        }
    }

    /// Helper method to move a file using object store rename operation
    pub fn move_file(&self, old_path: &ObjectPath, new_path: &ObjectPath) -> anyhow::Result<()> {
        if old_path == new_path {
            return Ok(());
        }

        self.execute_async(|| async {
            self.object_store
                .rename(old_path, new_path)
                .await
                .map_err(anyhow::Error::from)
        })
    }

    /// Helper method to execute async operations with a runtime
    pub fn execute_async<C, F, R>(&self, create_future: C) -> anyhow::Result<R>
    where
        C: FnOnce() -> F + Send,
        F: std::future::Future<Output = anyhow::Result<R>>,
        R: Send,
    {
        block_on_nautilus_with(create_future)
    }

    /// Lists directory stems (directory names without path) in a subdirectory.
    ///
    /// This method scans a subdirectory and returns the names of all immediate
    /// subdirectories. It's used to list data types, backtest runs, and live runs.
    ///
    /// # Parameters
    ///
    /// - `subdirectory`: The subdirectory path to scan (e.g., "data", "backtest", "live").
    ///
    /// # Returns
    ///
    /// Returns a vector of directory names (stems) found in the subdirectory,
    /// or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    ///
    /// // List all data types
    /// let data_types = catalog.list_directory_stems("data")?;
    /// for data_type in data_types {
    ///     println!("Found data type: {}", data_type);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn list_directory_stems(&self, subdirectory: &str) -> anyhow::Result<Vec<String>> {
        // For local filesystem paths, use filesystem operations to detect empty directories
        // For remote object stores, we can only list directories that contain files
        if !self.is_remote_uri() {
            let directory = PathBuf::from(self.native_base_path_string()).join(subdirectory);

            // Check if directory exists
            if !directory.exists() {
                return Ok(Vec::new());
            }

            // List all entries in the directory
            let mut directories = Vec::new();

            if let Ok(entries) = std::fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    if let Ok(file_type) = entry.file_type()
                        && file_type.is_dir()
                    {
                        // Use file_name() to get the directory name (not file_stem which removes extension)
                        if let Some(name) = entry.path().file_name() {
                            directories.push(name.to_string_lossy().to_string());
                        }
                    }
                }
            }

            directories.sort();
            return Ok(directories);
        }

        // For remote URIs, use object store listing (only lists directories with files)
        let directory = make_object_store_path(&self.base_path, [subdirectory]);

        let list_result = self.execute_async(|| async {
            let prefix = ObjectPath::from(format!("{directory}/"));
            let mut stream = self.object_store.list(Some(&prefix));
            let mut directories = Vec::new();
            let mut seen_dirs = std::collections::HashSet::new();

            while let Some(object) = stream.next().await {
                let object = object?;
                let path_str = object.location.to_string();

                // Extract the immediate subdirectory name
                if let Some(relative_path) = path_str.strip_prefix(&format!("{directory}/")) {
                    let parts: Vec<&str> = relative_path.split('/').collect();
                    if let Some(first_part) = parts.first()
                        && !first_part.is_empty()
                        && !seen_dirs.contains(*first_part)
                    {
                        seen_dirs.insert(first_part.to_string());
                        directories.push(first_part.to_string());
                    }
                }
            }

            Ok::<Vec<String>, anyhow::Error>(directories)
        })?;

        Ok(list_result)
    }

    /// Lists all data types available in the catalog.
    ///
    /// This method returns the names of all data type directories in the catalog.
    /// Data types correspond to different kinds of market data (e.g., "quotes", "trades", "bars").
    ///
    /// # Returns
    ///
    /// Returns a vector of data type names, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    ///
    /// // List all data types
    /// let data_types = catalog.list_data_types()?;
    /// for data_type in data_types {
    ///     println!("Available data type: {}", data_type);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn list_data_types(&self) -> anyhow::Result<Vec<String>> {
        self.list_directory_stems("data")
    }

    /// Lists all backtest run IDs available in the catalog.
    ///
    /// This method returns the names of all backtest run directories in the catalog.
    /// Each backtest run corresponds to a specific backtest execution instance.
    ///
    /// # Returns
    ///
    /// Returns a vector of backtest run IDs, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    ///
    /// // List all backtest runs
    /// let runs = catalog.list_backtest_runs()?;
    /// for run_id in runs {
    ///     println!("Backtest run: {}", run_id);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn list_backtest_runs(&self) -> anyhow::Result<Vec<String>> {
        self.list_directory_stems("backtest")
    }

    /// Lists all live run IDs available in the catalog.
    ///
    /// This method returns the names of all live run directories in the catalog.
    /// Each live run corresponds to a specific live trading execution instance.
    ///
    /// # Returns
    ///
    /// Returns a vector of live run IDs, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory access is denied.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
    ///
    /// let mut catalog = ParquetDataCatalog::new(
    ///     std::path::Path::new("/tmp/nautilus_data"),
    ///     None,
    ///     None,
    ///     None,
    ///     None,
    /// );
    ///
    /// // List all live runs
    /// let runs = catalog.list_live_runs()?;
    /// for run_id in runs {
    ///     println!("Live run: {}", run_id);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn list_live_runs(&self) -> anyhow::Result<Vec<String>> {
        self.list_directory_stems("live")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nautilus_model::data::NautilusDataType;
    use object_store::{ObjectStoreExt, PutPayload, memory::InMemory, path::Path as ObjectPath};
    use rstest::rstest;

    use super::ParquetDataCatalog;
    use crate::{catalog::types::CatalogDataType, common::datafusion::DataBackendSession};

    /// Builds a catalog over an in-memory store. Only remote catalogs carry a non-empty
    /// `base_path`, so seeding one here is the sole way to reproduce a bucket sub-prefix.
    fn memory_catalog(base_path: &str) -> ParquetDataCatalog {
        ParquetDataCatalog {
            base_path: base_path.to_string(),
            original_uri: "memory://".to_string(),
            object_store: Arc::new(InMemory::new()),
            session: DataBackendSession::new(5_000),
            batch_size: 5_000,
            compression: parquet::basic::Compression::SNAPPY,
            max_row_group_size: 5_000,
        }
    }

    fn seed(catalog: &ParquetDataCatalog, keys: &[&str]) {
        catalog
            .execute_async(|| async {
                for key in keys {
                    catalog
                        .object_store
                        .put(&ObjectPath::from(*key), PutPayload::from_static(b"x"))
                        .await?;
                }
                Ok(())
            })
            .unwrap();
    }

    #[rstest]
    #[case("nautilus-data", "nautilus-data/")]
    #[case("", "")]
    fn list_instruments_returns_ids_under_base_path(
        #[case] base_path: &str,
        #[case] key_prefix: &str,
    ) {
        let catalog = memory_catalog(base_path);
        let keys = [
            "data/quotes/EURUSD.SIM/0-1.parquet",
            "data/quotes/EURUSD.SIM/2-3.parquet",
            "data/quotes/GBPUSD.SIM/0-1.parquet",
            "data/trades/AUDUSD.SIM/0-1.parquet",
        ]
        .map(|key| format!("{key_prefix}{key}"));
        seed(&catalog, &keys.each_ref().map(String::as_str));

        assert_eq!(
            catalog
                .list_instruments(&NautilusDataType::QuoteTick.into())
                .unwrap(),
            ["EURUSD.SIM", "GBPUSD.SIM"]
        );
    }

    #[rstest]
    fn list_instruments_ignores_unpartitioned_files() {
        // An empty `base_path` keeps this distinct from the remote prefix defect, so an empty
        // result can only come from the layout check.
        let catalog = memory_catalog("");
        seed(
            &catalog,
            &[
                "data/custom/MyType/1-2.parquet",
                "data/custom/MyType/1-2.json",
            ],
        );

        let custom = CatalogDataType::Data(NautilusDataType::Custom {
            type_name: "MyType".to_string(),
        });

        assert!(catalog.list_instruments(&custom).unwrap().is_empty());
    }

    #[rstest]
    fn list_instruments_decodes_percent_encoded_ids() {
        let catalog = memory_catalog("nautilus-data");
        seed(
            &catalog,
            &["nautilus-data/data/quotes/BTC€.SIM/0-1.parquet"],
        );

        assert_eq!(
            catalog
                .list_instruments(&NautilusDataType::QuoteTick.into())
                .unwrap(),
            ["BTC€.SIM"]
        );
    }
}
