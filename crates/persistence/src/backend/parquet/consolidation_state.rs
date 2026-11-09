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

//! Publication and cleanup of raw Parquet consolidation with mutations serialized per directory.
//!
//! A marker records source snapshots and the target ID. The target's `created_by` footer field
//! identifies publication; listings then hide matching sources until cleanup removes them and the marker.
//! Key/value footer entries become Arrow schema metadata, where per-file IDs would prevent schema merging.

use std::sync::Arc;

use ahash::AHashSet;
use futures::TryStreamExt;
use nautilus_core::UUID4;
use object_store::{ObjectMeta, ObjectStore, ObjectStoreExt, buffered::BufReader, path::Path};
use parquet::arrow::ParquetRecordBatchStreamBuilder;
use serde::{Deserialize, Serialize};

const MARKER_FILENAME: &str = ".consolidation.json";

pub(super) async fn begin(
    store: &Arc<dyn ObjectStore>,
    target: &Path,
    sources: &[ObjectMeta],
) -> anyhow::Result<String> {
    let pending = PendingConsolidation {
        sources: sources.iter().map(ConsolidationSource::from).collect(),
        target: target.to_string(),
        id: UUID4::new().to_string(),
    };

    let directory = target.parent().unwrap_or_default();
    pending.validate(&directory)?;
    let path = directory.clone().join(MARKER_FILENAME);
    let bytes = serde_json::to_vec(&pending)?;
    store.put(&path, bytes.into()).await?;

    Ok(pending.created_by())
}

pub(super) async fn recover(store: &Arc<dyn ObjectStore>, directory: &Path) -> anyhow::Result<()> {
    let objects = store.list(Some(directory)).try_collect::<Vec<_>>().await?;
    for (path, pending) in read_pending(store, &objects).await? {
        if pending.published(store, &objects).await? {
            for source in &pending.sources {
                if source.path == pending.target {
                    continue;
                }

                if source.matches_stored(store, &objects).await? {
                    store.delete(&Path::parse(&source.path)?).await?;
                }
            }
        }

        store.delete(&path).await?;
    }

    Ok(())
}

pub(super) async fn visible_objects(
    store: &Arc<dyn ObjectStore>,
    mut objects: Vec<ObjectMeta>,
) -> anyhow::Result<Vec<ObjectMeta>> {
    let mut hidden = AHashSet::new();

    for (_, pending) in read_pending(store, &objects).await? {
        if !pending.published(store, &objects).await? {
            continue;
        }

        for source in &pending.sources {
            if source.path != pending.target && source.matches_stored(store, &objects).await? {
                hidden.insert(source.path.clone());
            }
        }
    }

    objects.retain(|meta| {
        meta.location.filename() != Some(MARKER_FILENAME)
            && !hidden.contains(meta.location.as_ref())
    });

    Ok(objects)
}

pub(super) async fn has_pending(
    store: &Arc<dyn ObjectStore>,
    directory: &Path,
) -> anyhow::Result<bool> {
    let mut objects = store.list(Some(directory));
    while let Some(object) = objects.try_next().await? {
        if object.location.filename() == Some(MARKER_FILENAME) {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn read_pending(
    store: &Arc<dyn ObjectStore>,
    objects: &[ObjectMeta],
) -> anyhow::Result<Vec<(Path, PendingConsolidation)>> {
    let mut pending = Vec::new();

    for meta in objects {
        if meta.location.filename() != Some(MARKER_FILENAME) {
            continue;
        }

        let bytes = store.get(&meta.location).await?.bytes().await?;
        let state: PendingConsolidation = serde_json::from_slice(&bytes)?;
        state.validate(&meta.location.parent().unwrap_or_default())?;
        pending.push((meta.location.clone(), state));
    }

    Ok(pending)
}

#[derive(Serialize, Deserialize)]
struct PendingConsolidation {
    sources: Vec<ConsolidationSource>,
    target: String,
    id: String,
}

impl PendingConsolidation {
    fn validate(&self, directory: &Path) -> anyhow::Result<()> {
        let target = Path::parse(&self.target)?;
        anyhow::ensure!(
            target.parent().unwrap_or_default() == *directory,
            "Consolidation target {} is outside marker directory {directory}",
            self.target,
        );

        for source in &self.sources {
            let path = Path::parse(&source.path)?;
            anyhow::ensure!(
                path.parent().unwrap_or_default() == *directory,
                "Consolidation source {} is outside marker directory {directory}",
                source.path,
            );
        }

        Ok(())
    }

    async fn published(
        &self,
        store: &Arc<dyn ObjectStore>,
        objects: &[ObjectMeta],
    ) -> anyhow::Result<bool> {
        let Some(meta) = objects
            .iter()
            .find(|meta| meta.location.as_ref() == self.target)
        else {
            return Ok(false);
        };

        let reader = BufReader::new(store.clone(), meta);
        let builder = ParquetRecordBatchStreamBuilder::new(reader).await?;
        Ok(builder.metadata().file_metadata().created_by() == Some(self.created_by().as_str()))
    }

    fn created_by(&self) -> String {
        format!("nautilus-consolidation/{}", self.id)
    }
}

#[derive(Serialize, Deserialize)]
struct ConsolidationSource {
    path: String,
    e_tag: Option<String>,
    version: Option<String>,
    size: u64,
    last_modified: String,
}

impl From<&ObjectMeta> for ConsolidationSource {
    fn from(meta: &ObjectMeta) -> Self {
        Self {
            path: meta.location.to_string(),
            e_tag: meta.e_tag.clone(),
            version: meta.version.clone(),
            size: meta.size,
            last_modified: meta.last_modified.to_string(),
        }
    }
}

impl ConsolidationSource {
    async fn matches_stored(
        &self,
        store: &Arc<dyn ObjectStore>,
        objects: &[ObjectMeta],
    ) -> anyhow::Result<bool> {
        if !objects
            .iter()
            .any(|meta| meta.location.as_ref() == self.path)
        {
            return Ok(false);
        }

        let meta = store.head(&Path::parse(&self.path)?).await?;
        Ok(self.matches(&meta))
    }

    fn matches(&self, meta: &ObjectMeta) -> bool {
        self.path == meta.location.as_ref()
            && self.e_tag == meta.e_tag
            && self.version == meta.version
            && self.size == meta.size
            && self.last_modified == meta.last_modified.to_string()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::{QuoteTick, stubs::quote_audusd};
    use nautilus_serialization::arrow::EncodeToRecordBatch;
    use object_store::memory::InMemory;
    use parquet::{arrow::ArrowWriter, file::properties::WriterProperties};
    use rstest::rstest;

    use super::*;
    use crate::backend::parquet::io::write_batches_to_object_store;

    #[rstest]
    #[case::target(
        "elsewhere/result.parquet",
        "data/quotes/source.parquet",
        "Consolidation target elsewhere/result.parquet is outside marker directory data/quotes"
    )]
    #[case::source(
        "data/quotes/result.parquet",
        "elsewhere/source.parquet",
        "Consolidation source elsewhere/source.parquet is outside marker directory data/quotes"
    )]
    fn pending_paths_stay_in_the_marker_directory(
        #[case] target: &str,
        #[case] source: &str,
        #[case] expected: &str,
    ) {
        let pending = PendingConsolidation {
            sources: vec![ConsolidationSource {
                path: source.to_string(),
                e_tag: None,
                version: None,
                size: 0,
                last_modified: String::new(),
            }],
            target: target.to_string(),
            id: "synthetic".to_string(),
        };

        let error = pending.validate(&Path::from("data/quotes")).unwrap_err();
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[tokio::test]
    async fn marker_hides_sources_after_publication() {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let source = Path::from("data/quotes/source.parquet");
        let target = Path::from("data/quotes/target.parquet");
        let row = quote_audusd();
        let batch = QuoteTick::encode_batch(&row.metadata(), &[row]).unwrap();
        write_batches_to_object_store(
            std::slice::from_ref(&batch),
            store.clone(),
            &source,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let original = store.head(&source).await.unwrap();
        let created_by = begin(&store, &target, std::slice::from_ref(&original))
            .await
            .unwrap();
        let objects = store.list(None).try_collect::<Vec<_>>().await.unwrap();
        let unpublished = visible_objects(&store, objects).await.unwrap();
        let properties = WriterProperties::builder()
            .set_created_by(created_by)
            .build();
        let mut bytes = Vec::new();
        let mut writer =
            ArrowWriter::try_new(&mut bytes, batch.schema(), Some(properties)).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();
        store.put(&target, bytes.into()).await.unwrap();
        let expected = store.head(&target).await.unwrap();
        let objects = store.list(None).try_collect::<Vec<_>>().await.unwrap();
        let published = visible_objects(&store, objects).await.unwrap();
        recover(&store, &target.parent().unwrap()).await.unwrap();
        let cleaned = store.list(None).try_collect::<Vec<_>>().await.unwrap();

        assert_eq!(unpublished, vec![original]);
        assert_eq!(published, vec![expected.clone()]);
        assert_eq!(cleaned, vec![expected]);
    }
}
