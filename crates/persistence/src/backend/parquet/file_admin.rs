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

//! File-level admin operations: existence checks, deletion, name resets, leaf-directory walk.

use ahash::AHashSet;
use futures::StreamExt;
use nautilus_core::UnixNanos;
use object_store::{ObjectStoreExt, path::Path as ObjectPath};

use crate::{
    backend::parquet::{
        catalog::ParquetDataCatalog,
        intervals::are_intervals_disjoint,
        io::{decode_object_store_segment, min_max_from_parquet_metadata_object_store},
        paths::{make_object_store_path, timestamps_to_filename},
    },
    catalog::types::{CatalogDataType, parquet_catalog_data_type_path_prefixes},
};

impl ParquetDataCatalog {
    /// Checks if a file exists in the object store.
    ///
    /// This method performs a HEAD operation on the object store to determine if a file
    /// exists without downloading its content. It works with both local and remote object stores.
    ///
    /// # Parameters
    ///
    /// - `path`: The file path to check, relative to the catalog structure.
    ///
    /// # Returns
    ///
    /// Returns `true` if the file exists, `false` if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the object store operation fails due to network issues,
    /// authentication problems, or other I/O errors.
    pub(crate) fn file_exists(&self, path: &str) -> anyhow::Result<bool> {
        let object_path = self.to_object_path(path)?;

        self.execute_async(|| async {
            match self.object_store.head(&object_path).await {
                Ok(_) => Ok(true),
                Err(object_store::Error::NotFound { .. }) => Ok(false),
                Err(e) => Err(e.into()),
            }
        })
    }

    /// Deletes a file from the object store.
    ///
    /// This method removes a file from the object store. The operation is permanent
    /// and cannot be undone. It works with both local filesystems and remote object stores.
    ///
    /// # Parameters
    ///
    /// - `path`: The file path to delete, as an object-store listing returns it.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on successful deletion.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file doesn't exist.
    /// - Permission is denied.
    /// - Network issues occur (for remote stores).
    /// - The object store operation fails.
    ///
    /// # Safety
    ///
    /// This operation is irreversible. Ensure the file is no longer needed before deletion.
    pub(crate) fn delete_file(&self, path: &str) -> anyhow::Result<()> {
        let object_path = self.to_object_path_parsed(path)?;
        self.execute_async(|| async {
            self.object_store
                .delete(&object_path)
                .await
                .map_err(anyhow::Error::from)
        })
    }

    /// Resets the filenames of all Parquet files in the catalog to match their actual content timestamps.
    ///
    /// This method scans all leaf data directories in the catalog and renames files based on
    /// the actual timestamp range of their content. This is useful when files have been
    /// modified or when filename conventions have changed.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Directory listing fails.
    /// - File metadata reading fails.
    /// - File rename operations fail.
    /// - The content intervals in a directory are not disjoint, or a new name matches another
    ///   file's current name. Both are checked before any file in that directory is renamed.
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
    /// // Reset all filenames in the catalog
    /// catalog.reset_all_file_names()?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn reset_all_file_names(&self) -> anyhow::Result<()> {
        let leaf_directories = self.find_leaf_data_directories()?;

        for directory in leaf_directories {
            self.reset_file_names(&directory)?;
        }

        Ok(())
    }

    /// Resets the filenames of Parquet files for a specific data type and identifier.
    ///
    /// This method renames files in a specific directory based on the actual timestamp
    /// range of their content. This is useful for correcting filenames after data
    /// modifications or when filename conventions have changed.
    ///
    /// # Parameters
    ///
    /// - `data_type`: The stored family to target.
    /// - `identifier`: Optional identifier to target a specific instrument's data. Can be an `instrument_id` (e.g., "EUR/USD.SIM") or a `bar_type` (e.g., "EUR/USD.SIM-1-MINUTE-LAST-EXTERNAL").
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if the operation fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The directory path cannot be constructed.
    /// - File metadata reading fails.
    /// - File rename operations fail.
    /// - The content intervals in a directory are not disjoint, or a new name matches another
    ///   file's current name. Both are checked before any file in that directory is renamed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
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
    /// // Reset filenames for all quote files
    /// catalog.reset_data_file_names(&NautilusDataType::QuoteTick.into(), None)?;
    ///
    /// // Reset filenames for a specific instrument's trade files
    /// catalog.reset_data_file_names(&NautilusDataType::TradeTick.into(), Some("BTCUSD"))?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn reset_data_file_names(
        &self,
        data_type: &CatalogDataType,
        identifier: Option<&str>,
    ) -> anyhow::Result<()> {
        for type_name in parquet_catalog_data_type_path_prefixes(data_type) {
            let directory = self.make_path(type_name.as_ref(), identifier)?;
            self.reset_file_names(&directory)?;
        }

        Ok(())
    }

    /// Resets the filenames of Parquet files in a directory to match their actual content timestamps.
    ///
    /// This internal method scans all Parquet files in a directory, reads their metadata to
    /// determine the actual timestamp range of their content, and renames the files accordingly.
    /// This ensures that filenames accurately reflect the data they contain.
    ///
    /// # Parameters
    ///
    /// - `directory`: The directory path containing Parquet files to rename.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or an error if the operation fails.
    ///
    /// # Process
    ///
    /// 1. Lists all Parquet files in the directory
    /// 2. For each file, reads metadata to extract min/max timestamps
    /// 3. Generates a new filename based on actual timestamp range
    /// 4. Validates the new names before moving any file
    /// 5. Moves each file to its new name using object store operations
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Directory listing fails.
    /// - Metadata reading fails for any file.
    /// - The content intervals are not disjoint.
    /// - A new name matches the current name of another file.
    /// - File move operations fail.
    /// - Object store operations fail.
    ///
    /// # Notes
    ///
    /// - This operation can be time-consuming for directories with many files.
    /// - A validation error leaves every file name unchanged.
    /// - Files are renamed one at a time. Object stores without a native rename copy then
    ///   delete, so a failure can leave a file under both names.
    fn reset_file_names(&self, directory: &str) -> anyhow::Result<()> {
        let parquet_files = self.list_parquet_files(directory)?;
        let mut intervals = Vec::with_capacity(parquet_files.len());
        let mut moves = Vec::with_capacity(parquet_files.len());

        for file in parquet_files {
            let object_path = self.to_object_path_parsed(&file)?;

            let (first_ts, last_ts) = self.execute_async(|| async {
                min_max_from_parquet_metadata_object_store(
                    self.object_store.clone(),
                    &object_path,
                    "ts_init",
                )
                .await
            })?;

            let new_filename =
                timestamps_to_filename(UnixNanos::from(first_ts), UnixNanos::from(last_ts));
            let new_file_path = make_object_store_path(directory, [&new_filename]);
            let new_object_path = ObjectPath::from(new_file_path);

            intervals.push((first_ts, last_ts));
            moves.push((object_path, new_object_path));
        }

        anyhow::ensure!(
            are_intervals_disjoint(&intervals),
            "Cannot reset file names in directory '{directory}': content intervals are not \
             disjoint: {intervals:?}",
        );

        // Moves overwrite, so a new name held by another file would replace that file
        let current_paths = moves
            .iter()
            .map(|(old_path, _)| old_path)
            .collect::<AHashSet<_>>();

        if let Some((old_path, new_path)) = moves
            .iter()
            .find(|(old_path, new_path)| old_path != new_path && current_paths.contains(new_path))
        {
            anyhow::bail!(
                "Cannot reset file names in directory '{directory}': new name {new_path} for \
                 {old_path} is held by another file; rename that file to an unused interval \
                 name and retry"
            );
        }

        for (old_path, new_path) in &moves {
            self.move_file(old_path, new_path)?;
        }

        Ok(())
    }

    /// Finds all leaf data directories in the catalog.
    ///
    /// A leaf directory is one that contains data files but no subdirectories.
    /// This method is used to identify directories that can be processed for
    /// consolidation or other operations.
    ///
    /// # Returns
    ///
    /// Returns a vector of directory path strings representing leaf directories,
    /// or an error if directory traversal fails.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Object store listing operations fail.
    /// - Directory structure cannot be analyzed.
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
    /// let leaf_dirs = catalog.find_leaf_data_directories()?;
    /// for dir in leaf_dirs {
    ///     println!("Found leaf directory: {}", dir);
    /// }
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn find_leaf_data_directories(&self) -> anyhow::Result<Vec<String>> {
        let data_dir = make_object_store_path(&self.base_path, ["data"]);

        self.execute_async(|| async {
            let mut directories = AHashSet::new();

            // List all objects under the data directory
            let prefix = ObjectPath::from(format!("{data_dir}/"));
            let mut stream = self.object_store.list(Some(&prefix));

            while let Some(object) = stream.next().await {
                let object = object?;
                let parts = object.location.parts().collect::<Vec<_>>();

                // Decode to match `make_path` output, which callers encode again
                if let Some((_, parent)) = parts.split_last() {
                    let directory = parent
                        .iter()
                        .map(|part| decode_object_store_segment(part.as_ref()))
                        .collect::<Vec<_>>()
                        .join("/");
                    directories.insert(directory);
                }
            }

            // Find leaf directories (every listed directory contains at least one file,
            // so a leaf is one without subdirectories)
            let mut leaf_dirs = Vec::new();

            for dir in &directories {
                let prefix = format!("{dir}/");
                let has_subdirs = directories.iter().any(|d| d.starts_with(&prefix));

                if !has_subdirs {
                    leaf_dirs.push(dir.clone());
                }
            }

            leaf_dirs.sort();

            Ok::<Vec<String>, anyhow::Error>(leaf_dirs)
        })
    }
}
