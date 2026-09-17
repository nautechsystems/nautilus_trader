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

//! Shared storage construction for persistence backends.

use std::{
    collections::BTreeSet,
    fmt::Display,
    fs, io,
    path::{Component, PathBuf},
    sync::Arc,
};

use ahash::AHashMap;
use futures::{StreamExt, TryStreamExt, stream::BoxStream};
use nautilus_core::time::nanos_since_unix_epoch;
use object_store::{
    CopyOptions, Error as ObjectStoreError, GetOptions, GetResult, ListResult, MultipartUpload,
    ObjectMeta, ObjectStore, ObjectStoreExt, PutMultipartOptions, PutOptions, PutPayload,
    PutResult, Result as ObjectStoreResult, path::Path as ObjectPath,
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::common::paths::make_object_store_path;

/// File name used to represent run sessions, including runs that wrote no data files.
pub const RUN_MANIFEST_FILENAME: &str = "_nautilus_run_manifest.json";

/// Storage-native run manifest shared by catalog and stream writer session discovery.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunManifest {
    pub schema_version: u32,
    pub kind: String,
    pub instance_id: String,
    pub status: String,
    pub empty: bool,
    pub created_ts: u64,
}

impl RunManifest {
    /// Creates a run manifest for `status`.
    #[must_use]
    pub fn new(kind: &str, instance_id: &str, status: &str, empty: bool) -> Self {
        Self {
            schema_version: 1,
            kind: kind.to_string(),
            instance_id: instance_id.to_string(),
            status: status.to_string(),
            empty,
            created_ts: nanos_since_unix_epoch(),
        }
    }
}

/// Native object-store storage handles shared by catalog, session, and stream writers.
#[derive(Clone)]
pub struct StorageBackend {
    /// `object_store` adapter used by DataFusion, Parquet, and existing persistence code.
    pub object_store: Arc<dyn ObjectStore>,
    /// Path prefix inside the object store for URI schemes that carry bucket/container roots.
    pub base_path: String,
    /// Normalized URI used to create this backend.
    pub original_uri: String,
}

impl StorageBackend {
    /// Returns the root URL DataFusion should associate with this object store.
    ///
    /// Delta and external catalog integrations use object-store-relative paths after the object store has
    /// already rooted the operator at the catalog path. Registering the object store at the URI
    /// authority root keeps those relative table paths stable across local, memory, and cloud
    /// storage.
    ///
    /// # Errors
    ///
    /// Returns an error if the original URI cannot be converted into a DataFusion object-store URL.
    pub fn datafusion_root_url(&self) -> anyhow::Result<Url> {
        datafusion_root_url(&self.original_uri)
    }

    /// Lists immediate child directory stems below a storage-relative subdirectory.
    ///
    /// This is used by catalog data and run-session discovery so local, memory, and cloud
    /// backends share one object-store listing path.
    ///
    /// # Errors
    ///
    /// Returns an error if the object-store listing fails.
    pub async fn list_directory_stems(&self, subdirectory: &str) -> anyhow::Result<Vec<String>> {
        let directory = make_object_store_path(&self.base_path, [subdirectory]);
        let prefix = ObjectPath::from(format!("{}/", directory.trim_end_matches('/')));
        let prefix_str = format!("{}/", directory.trim_matches('/'));
        let mut stream = self.object_store.list(Some(&prefix));
        let mut stems = BTreeSet::new();

        while let Some(object) = stream.next().await {
            let object = object?;
            let path = object.location.to_string();

            if let Some(relative_path) = path.strip_prefix(&prefix_str)
                && let Some(stem) = relative_path.split('/').find(|segment| !segment.is_empty())
            {
                stems.insert(stem.to_string());
            }
        }

        Ok(stems.into_iter().collect())
    }

    /// Lists files below a storage-relative subdirectory.
    ///
    /// # Errors
    ///
    /// Returns an error if the object-store listing fails.
    pub async fn list_files(
        &self,
        subdirectory: &str,
        suffix: Option<&str>,
    ) -> anyhow::Result<Vec<String>> {
        let directory = make_object_store_path(&self.base_path, [subdirectory]);
        let prefix = ObjectPath::from(format!("{}/", directory.trim_end_matches('/')));
        let mut stream = self.object_store.list(Some(&prefix));
        let mut files = Vec::new();

        while let Some(object) = stream.next().await {
            let object = object?;
            let path = object.location.to_string();
            if suffix.is_none_or(|suffix| path.ends_with(suffix)) {
                files.push(path);
            }
        }

        files.sort();
        Ok(files)
    }

    /// Writes a manifest for a run under the catalog session root.
    ///
    /// # Errors
    ///
    /// Returns an error if manifest serialization or storage writing fails.
    pub async fn write_run_manifest(
        &self,
        kind: &str,
        instance_id: &str,
        status: &str,
        empty: bool,
    ) -> anyhow::Result<()> {
        let manifest = RunManifest::new(kind, instance_id, status, empty);
        let path = self.run_manifest_path(kind, instance_id);
        let bytes = serde_json::to_vec(&manifest)?;
        self.object_store.put(&path, bytes.into()).await?;
        Ok(())
    }

    /// Writes a manifest at the backend root, for writers rooted directly at one run directory.
    ///
    /// # Errors
    ///
    /// Returns an error if manifest serialization or storage writing fails.
    pub async fn write_current_run_manifest(
        &self,
        kind: &str,
        instance_id: &str,
        status: &str,
        empty: bool,
    ) -> anyhow::Result<()> {
        let manifest = RunManifest::new(kind, instance_id, status, empty);
        let path = ObjectPath::from(make_object_store_path(
            &self.base_path,
            [RUN_MANIFEST_FILENAME],
        ));
        let bytes = serde_json::to_vec(&manifest)?;
        self.object_store.put(&path, bytes.into()).await?;
        Ok(())
    }

    /// Reads a run manifest from catalog session storage.
    ///
    /// # Errors
    ///
    /// Returns an error if storage reading or manifest deserialization fails.
    pub async fn read_run_manifest(
        &self,
        kind: &str,
        instance_id: &str,
    ) -> anyhow::Result<Option<RunManifest>> {
        let path = self.run_manifest_path(kind, instance_id);
        match self.object_store.get(&path).await {
            Ok(result) => {
                let bytes = result.bytes().await?;
                Ok(Some(serde_json::from_slice(&bytes)?))
            }
            Err(ObjectStoreError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Lists run IDs from directories and manifests for one session kind.
    ///
    /// # Errors
    ///
    /// Returns an error if storage listing or manifest reading fails.
    pub async fn list_run_ids(&self, kind: &str) -> anyhow::Result<Vec<String>> {
        self.list_run_ids_for_kinds(&[kind]).await
    }

    /// Lists run IDs from directories and manifests for multiple session kind aliases.
    ///
    /// # Errors
    ///
    /// Returns an error if storage listing or manifest reading fails.
    pub async fn list_run_ids_for_kinds(&self, kinds: &[&str]) -> anyhow::Result<Vec<String>> {
        let mut run_ids = BTreeSet::new();

        for kind in kinds {
            run_ids.extend(self.list_directory_stems(kind).await?);

            for manifest in self.list_run_manifests(kind).await? {
                run_ids.insert(manifest.instance_id);
            }
        }

        Ok(run_ids.into_iter().collect())
    }

    async fn list_run_manifests(&self, kind: &str) -> anyhow::Result<Vec<RunManifest>> {
        let files = self.list_files(kind, Some(RUN_MANIFEST_FILENAME)).await?;
        let mut manifests = Vec::new();

        for file in files {
            match self.object_store.get(&ObjectPath::from(file)).await {
                Ok(result) => {
                    let bytes = result.bytes().await?;
                    manifests.push(serde_json::from_slice(&bytes)?);
                }
                Err(ObjectStoreError::NotFound { .. }) => {}
                Err(e) => return Err(e.into()),
            }
        }

        Ok(manifests)
    }

    fn run_manifest_path(&self, kind: &str, instance_id: &str) -> ObjectPath {
        ObjectPath::from(make_object_store_path(
            &self.base_path,
            [kind, instance_id, RUN_MANIFEST_FILENAME],
        ))
    }
}

/// Returns the root URL DataFusion should use when registering an `OpenDAL` object store.
///
/// # Errors
///
/// Returns an error if `uri` is not a valid storage URI.
pub fn datafusion_root_url(uri: &str) -> anyhow::Result<Url> {
    if uri.starts_with("memory://") {
        return Url::parse("memory:///")
            .map_err(|e| anyhow::anyhow!("Invalid memory object-store root URL: {e}"));
    }

    let mut url = Url::parse(uri)?;
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

/// Normalizes a local path or storage URI for persistence backends.
///
/// # Errors
///
/// Returns an error if a relative path cannot be resolved against the current directory.
pub fn normalize_path_to_uri(path: &str) -> anyhow::Result<String> {
    if path.contains("://") {
        return Ok(path.to_string());
    }

    if is_absolute_path(path) {
        return Ok(path_to_file_uri(path));
    }

    let current_dir = std::env::current_dir().map_err(|e| {
        anyhow::anyhow!("Failed to resolve current directory for relative path '{path}': {e}")
    })?;
    Ok(path_to_file_uri(&current_dir.join(path).to_string_lossy()))
}

/// Resolves a storage location for overlap checks without creating files.
///
/// Local paths follow existing symlinks and normalize missing suffixes. Remote locations use
/// the same URL parsing as their storage backend.
///
/// # Errors
///
/// Returns an error if the URI is invalid or an existing local prefix cannot be resolved.
pub fn normalize_storage_location(path: &str) -> anyhow::Result<String> {
    let uri = normalize_path_to_uri(path)?;
    if uri.starts_with("file://") {
        let path = std::path::absolute(file_uri_to_native_path(&uri))?;
        let mut resolved = PathBuf::new();

        for component in path.components() {
            match component {
                Component::CurDir => {}
                component @ (Component::Prefix(_) | Component::RootDir) => resolved.push(component),
                Component::ParentDir => {
                    resolved.pop();
                }
                component => {
                    resolved.push(component);
                    match fs::canonicalize(&resolved) {
                        Ok(canonical) => resolved = canonical,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }
        return Ok(path_to_file_uri(&resolved.to_string_lossy())
            .trim_end_matches('/')
            .to_string());
    }
    let mut url = Url::parse(&uri)?;
    if url.scheme() == "gcs" {
        url.set_scheme("gs")
            .map_err(|()| anyhow::anyhow!("invalid storage scheme"))?;
    }
    url.set_fragment(None);
    url.set_query(None);
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn is_absolute_path(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with("\\\\")
        || (path.len() >= 3
            && path.chars().nth(1) == Some(':')
            && matches!(path.chars().nth(2), Some('\\' | '/')))
}

fn path_to_file_uri(path: &str) -> String {
    if path.starts_with('/') {
        format!("file://{path}")
    } else if path.len() >= 3 && path.chars().nth(1) == Some(':') {
        format!("file:///{}", path.replace('\\', "/"))
    } else if let Some(path) = path.strip_prefix("\\\\") {
        format!("file://{}", path.replace('\\', "/"))
    } else {
        format!("file://{path}")
    }
}

#[cfg(windows)]
fn file_uri_to_native_path(uri: &str) -> String {
    uri.strip_prefix("file://")
        .or_else(|| uri.strip_prefix("file:"))
        .unwrap_or(uri)
        .trim_start_matches('/')
        .replace('/', "\\")
}

#[cfg(not(windows))]
fn file_uri_to_native_path(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
}

/// Creates an OpenDAL-backed storage backend from a Nautilus storage URI.
///
/// # Errors
///
/// Returns an error when the URI cannot be parsed or the requested storage service is not enabled.
#[cfg_attr(not(feature = "cloud"), allow(clippy::needless_pass_by_value))]
pub fn create_storage_backend_from_path(
    path: &str,
    storage_options: Option<AHashMap<String, String>>,
) -> anyhow::Result<StorageBackend> {
    let uri = normalize_path_to_uri(path)?;
    if uri.starts_with("memory://") {
        return Ok(storage_backend(
            Arc::new(object_store::memory::InMemory::new()),
            String::new(),
            uri,
        ));
    }

    if uri.starts_with("file://") {
        fs::create_dir_all(file_uri_to_native_path(&uri))?;
    }
    let (object_store, base_path, original_uri) =
        crate::backend::parquet::io::create_object_store_from_path(&uri, storage_options)?;
    Ok(storage_backend(object_store, base_path, original_uri))
}

fn storage_backend(
    inner: Arc<dyn ObjectStore>,
    base_path: String,
    original_uri: String,
) -> StorageBackend {
    StorageBackend {
        object_store: Arc::new(SortedListObjectStore { inner }),
        base_path,
        original_uri,
    }
}

#[derive(Debug)]
struct SortedListObjectStore {
    inner: Arc<dyn ObjectStore>,
}

impl Display for SortedListObjectStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

#[async_trait::async_trait]
impl ObjectStore for SortedListObjectStore {
    async fn put_opts(
        &self,
        location: &ObjectPath,
        payload: PutPayload,
        opts: PutOptions,
    ) -> ObjectStoreResult<PutResult> {
        self.inner.put_opts(location, payload, opts).await
    }

    async fn put_multipart_opts(
        &self,
        location: &ObjectPath,
        opts: PutMultipartOptions,
    ) -> ObjectStoreResult<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, opts).await
    }

    async fn get_opts(
        &self,
        location: &ObjectPath,
        options: GetOptions,
    ) -> ObjectStoreResult<GetResult> {
        self.inner.get_opts(location, options).await
    }

    fn list(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> BoxStream<'static, ObjectStoreResult<ObjectMeta>> {
        let inner = Arc::clone(&self.inner);
        let prefix = prefix.cloned();
        Box::pin(
            futures::stream::once(async move {
                let mut entries = inner.list(prefix.as_ref()).try_collect::<Vec<_>>().await?;
                entries.sort_by(|left, right| left.location.cmp(&right.location));
                Ok::<_, object_store::Error>(futures::stream::iter(entries.into_iter().map(Ok)))
            })
            .try_flatten(),
        )
    }

    async fn list_with_delimiter(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> ObjectStoreResult<ListResult> {
        let mut result = self.inner.list_with_delimiter(prefix).await?;
        result
            .common_prefixes
            .sort_by(|left, right| left.as_ref().cmp(right.as_ref()));
        result
            .objects
            .sort_by(|left, right| left.location.cmp(&right.location));
        Ok(result)
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, ObjectStoreResult<ObjectPath>>,
    ) -> BoxStream<'static, ObjectStoreResult<ObjectPath>> {
        self.inner.delete_stream(locations)
    }

    async fn copy_opts(
        &self,
        from: &ObjectPath,
        to: &ObjectPath,
        opts: CopyOptions,
    ) -> ObjectStoreResult<()> {
        self.inner.copy_opts(from, to, opts).await
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "cloud")]
    use object_store::{ObjectStoreExt, path::Path as ObjectPath};
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    fn storage_location_resolves_relative_file_uris() {
        assert_eq!(
            normalize_storage_location("file://nautilus-stream-location/new-destination").unwrap(),
            normalize_storage_location("nautilus-stream-location/new-destination").unwrap(),
        );
    }

    #[rstest]
    fn datafusion_root_url_handles_memory_storage() {
        let storage = create_storage_backend_from_path("memory://", None).unwrap();

        assert_eq!(
            storage.datafusion_root_url().unwrap().as_str(),
            "memory:///"
        );
    }

    #[rstest]
    fn datafusion_root_url_handles_local_storage_paths() {
        let temp_dir = TempDir::new().unwrap();
        let storage =
            create_storage_backend_from_path(temp_dir.path().to_str().unwrap(), None).unwrap();

        assert_eq!(storage.datafusion_root_url().unwrap().as_str(), "file:///");
    }

    #[rstest]
    fn datafusion_root_url_handles_file_uri_storage() {
        let storage =
            create_storage_backend_from_path("file:///tmp/nautilus-catalog", None).unwrap();

        assert_eq!(storage.datafusion_root_url().unwrap().as_str(), "file:///");
    }

    #[rstest]
    #[case("/tmp/test", "file:///tmp/test")]
    #[case("C:\\tmp\\test", "file:///C:/tmp/test")]
    #[case("C:/tmp/test", "file:///C:/tmp/test")]
    #[case("\\\\server\\share\\file", "file://server/share/file")]
    #[case("s3://bucket/path", "s3://bucket/path")]
    #[case("https://example.com/path", "https://example.com/path")]
    fn normalize_path_to_uri_handles_local_paths_and_storage_uris(
        #[case] path: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(normalize_path_to_uri(path).unwrap(), expected);
    }

    #[rstest]
    fn storage_backend_lists_directory_stems() {
        let storage = create_storage_backend_from_path("memory://", None).unwrap();
        futures::executor::block_on(async {
            storage
                .object_store
                .put(
                    &ObjectPath::from("backtest/run-001/quotes.feather"),
                    b"quotes".to_vec().into(),
                )
                .await
                .unwrap();
            storage
                .object_store
                .put(
                    &ObjectPath::from("backtest/run-002/trades.feather"),
                    b"trades".to_vec().into(),
                )
                .await
                .unwrap();
        });

        let runs = futures::executor::block_on(storage.list_directory_stems("backtest")).unwrap();

        assert_eq!(runs, vec!["run-001".to_string(), "run-002".to_string()]);
    }

    #[rstest]
    fn storage_backend_lists_files_with_suffix() {
        let storage = create_storage_backend_from_path("memory://", None).unwrap();
        futures::executor::block_on(async {
            storage
                .object_store
                .put(
                    &ObjectPath::from("live/run-001/quotes.feather"),
                    b"quotes".to_vec().into(),
                )
                .await
                .unwrap();
            storage
                .object_store
                .put(
                    &ObjectPath::from("live/run-001/manifest.json"),
                    b"manifest".to_vec().into(),
                )
                .await
                .unwrap();
        });

        let files =
            futures::executor::block_on(storage.list_files("live/run-001", Some(".feather")))
                .unwrap();

        assert_eq!(files, vec!["live/run-001/quotes.feather".to_string()]);
    }

    #[rstest]
    fn storage_backend_lists_manifest_only_empty_runs() {
        let storage = create_storage_backend_from_path("memory://", None).unwrap();
        futures::executor::block_on(async {
            storage
                .write_run_manifest("backtest", "empty-run-001", "completed", true)
                .await
                .unwrap();
        });

        let runs = futures::executor::block_on(storage.list_run_ids("backtest")).unwrap();
        let manifest =
            futures::executor::block_on(storage.read_run_manifest("backtest", "empty-run-001"))
                .unwrap()
                .unwrap();

        assert_eq!(runs, vec!["empty-run-001".to_string()]);
        assert_eq!(manifest.kind, "backtest");
        assert_eq!(manifest.instance_id, "empty-run-001");
        assert_eq!(manifest.status, "completed");
        assert!(manifest.empty);
        assert_eq!(manifest.schema_version, 1);
    }

    #[rstest]
    fn storage_backend_unions_directory_and_manifest_runs() {
        let storage = create_storage_backend_from_path("memory://", None).unwrap();
        futures::executor::block_on(async {
            storage
                .object_store
                .put(
                    &ObjectPath::from("live/run-with-data/quotes.feather"),
                    b"quotes".to_vec().into(),
                )
                .await
                .unwrap();
            storage
                .write_run_manifest("live", "empty-run-001", "completed", true)
                .await
                .unwrap();
        });

        let runs = futures::executor::block_on(storage.list_run_ids("live")).unwrap();

        assert_eq!(
            runs,
            vec!["empty-run-001".to_string(), "run-with-data".to_string()],
        );
    }

    #[cfg(feature = "cloud")]
    #[rstest]
    fn datafusion_root_url_handles_cloud_storage_paths() {
        let storage =
            create_storage_backend_from_path("s3://nautilus-test/catalog/data", None).unwrap();

        assert_eq!(storage.base_path, "catalog/data");
        assert_eq!(
            storage.datafusion_root_url().unwrap().as_str(),
            "s3://nautilus-test/"
        );
    }
}
