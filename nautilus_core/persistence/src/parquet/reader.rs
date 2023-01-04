// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::collections::HashSet;
use std::io::{Read, Seek};
use std::marker::PhantomData;
use std::sync::Arc;

use arrow2::array::UInt64Array;
use arrow2::io::parquet::read::{self, RowGroupMetaData};
use arrow2::io::parquet::write::FileMetaData;
use arrow2::{datatypes::Schema, io::parquet::read::FileReader};

use super::DecodeFromChunk;

#[repr(C)]
/// Filter groups based on a field's metadata values.
pub enum GroupFilterArg {
    /// Select groups that have minimum ts_init less than limit.
    TsInitLt(u64),
    /// Select groups that have maximum ts_init greater than limit.
    TsInitGt(u64),
    /// No group filtering applied (to avoid `Option).
    None,
}

// duplicate private type definition from arrow2 parquet file reader
type GroupFilterPredicate = Arc<dyn Fn(usize, &RowGroupMetaData) -> bool + Send + Sync>;

impl GroupFilterArg {
    /// Scan metadata and choose which chunks to filter and returns a HashSet
    /// holding the indexes of the selected chunks.
    fn selected_groups(&self, metadata: &FileMetaData, schema: &Schema) -> HashSet<usize> {
        match self {
            // select groups that have minimum ts_init less than limit
            GroupFilterArg::TsInitLt(limit) => {
                if let Some(ts_init_field) =
                    schema.fields.iter().find(|field| field.name.eq("ts_init"))
                {
                    let statistics =
                        read::statistics::deserialize(ts_init_field, &metadata.row_groups)
                            .expect("Cannot extract ts_init statistics");
                    let min_values = statistics
                        .min_value
                        .as_any()
                        .downcast_ref::<UInt64Array>()
                        .expect("Unable to unwrap minimum value metadata for ts_init statistics");
                    min_values
                        .iter()
                        .enumerate()
                        .filter_map(|(i, ts_group_min)| {
                            let min = ts_group_min.unwrap_or(&u64::MAX);
                            if min < limit {
                                Some(i)
                            } else {
                                None
                            }
                        })
                        .collect()
                } else {
                    HashSet::new()
                }
            }
            // select groups that have maximum ts_init time greater than limit
            GroupFilterArg::TsInitGt(limit) => {
                if let Some(ts_init_field) =
                    schema.fields.iter().find(|field| field.name.eq("ts_init"))
                {
                    let statistics =
                        read::statistics::deserialize(ts_init_field, &metadata.row_groups)
                            .expect("Cannot extract ts_init statistics");
                    let max_values = statistics
                        .max_value
                        .as_any()
                        .downcast_ref::<UInt64Array>()
                        .expect("Unable to unwrap maximum value metadata for ts_init statistics");
                    max_values
                        .iter()
                        .enumerate()
                        .filter_map(|(i, ts_group_max)| {
                            let max = ts_group_max.unwrap_or(&u64::MAX);
                            if max > limit {
                                Some(i)
                            } else {
                                None
                            }
                        })
                        .collect()
                } else {
                    HashSet::new()
                }
            }
            GroupFilterArg::None => {
                unreachable!("filter_groups should not be called with None filter")
            }
        }
    }
}

pub struct ParquetReader<A, R>
where
    R: Read + Seek,
{
    file_reader: FileReader<R>,
    reader_type: PhantomData<*const A>,
}

impl<A, R> ParquetReader<A, R>
where
    R: Read + Seek,
{
    pub fn new(mut reader: R, chunk_size: usize, filter_arg: GroupFilterArg) -> Self {
        // let mut file = File::open(file_path)
        //     .unwrap_or_else(|_| panic!("Unable to open parquet file {file_path}"));
        let group_filter_predicate = ParquetReader::<A, R>::new_predicate(&filter_arg, &mut reader);

        let fr = FileReader::try_new(reader, None, Some(chunk_size), None, group_filter_predicate)
            .expect("Unable to create reader from reader");
        ParquetReader {
            file_reader: fr,
            reader_type: PhantomData,
        }
    }

    /// create new predicate from a given argument and metadata
    fn new_predicate(filter_arg: &GroupFilterArg, reader: &mut R) -> Option<GroupFilterPredicate> {
        match filter_arg {
            GroupFilterArg::None => None,
            // a closure that captures the HashSet of indexes of selected chunks
            // and uses this to check if a chunk is selected based on it's index
            _ => {
                let metadata = read::read_metadata(reader).expect("unable to read metadata");
                let schema = read::infer_schema(&metadata).expect("unable to infer schema");
                let selected_groups = filter_arg.selected_groups(&metadata, &schema);
                let filter_closure: GroupFilterPredicate = Arc::new(
                    move |group_index: usize, _metadata: &RowGroupMetaData| -> bool {
                        selected_groups.contains(&group_index)
                    },
                );
                Some(filter_closure)
            }
        }
    }
}

impl<A, R> Iterator for ParquetReader<A, R>
where
    A: DecodeFromChunk,
    R: Read + Seek,
{
    type Item = Vec<A>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(Ok(chunk)) = self.file_reader.next() {
            Some(A::decode(self.file_reader.schema(), chunk))
        } else {
            None
        }
    }
}
