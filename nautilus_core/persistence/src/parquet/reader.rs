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

use std::cmp::Ordering;
use std::collections::HashSet;
use std::io::{Read, Seek};
use std::marker::PhantomData;

use arrow2::array::UInt64Array;
use arrow2::io::parquet::read::{self, RowGroupMetaData};
use arrow2::io::parquet::write::FileMetaData;
use arrow2::{datatypes::Schema, io::parquet::read::FileReader};
use pyo3::types::PyInt;
use pyo3::FromPyObject;

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

impl<'source> FromPyObject<'source> for GroupFilterArg {
    fn extract(ob: &'source pyo3::PyAny) -> pyo3::PyResult<Self> {
        let filter_arg: i64 = ob.downcast::<PyInt>()?.extract()?;
        match filter_arg.cmp(&0) {
            Ordering::Less => Ok(GroupFilterArg::TsInitLt(filter_arg.unsigned_abs())),
            Ordering::Equal => Ok(GroupFilterArg::None),
            Ordering::Greater => Ok(GroupFilterArg::TsInitGt(filter_arg.unsigned_abs())),
        }
    }
}

impl From<i64> for GroupFilterArg {
    fn from(value: i64) -> Self {
        match value.cmp(&0) {
            Ordering::Less => GroupFilterArg::TsInitLt(value.unsigned_abs()),
            Ordering::Equal => GroupFilterArg::None,
            Ordering::Greater => GroupFilterArg::TsInitGt(value.unsigned_abs()),
        }
    }
}

impl GroupFilterArg {
    /// Scan metadata and choose which chunks to filter and returns a HashSet
    /// holding the indexes of the selected chunks.
    fn selected_groups(&self, metadata: FileMetaData, schema: &Schema) -> Vec<RowGroupMetaData> {
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
                    let selected_groups: HashSet<usize> = min_values
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
                        .collect();
                    metadata
                        .row_groups
                        .into_iter()
                        .enumerate()
                        .filter(|(i, _row_group)| selected_groups.contains(i))
                        .map(|(_i, row_group)| row_group)
                        .collect()
                } else {
                    metadata.row_groups
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
                    let selected_groups: HashSet<usize> = max_values
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
                        .collect();
                    metadata
                        .row_groups
                        .into_iter()
                        .enumerate()
                        .filter(|(i, _row_group)| selected_groups.contains(i))
                        .map(|(_i, row_group)| row_group)
                        .collect()
                } else {
                    metadata.row_groups
                }
            }
            GroupFilterArg::None => metadata.row_groups,
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
        let metadata = read::read_metadata(&mut reader).expect("Unable to read metadata");
        let schema = read::infer_schema(&metadata).expect("Unable to infer schema");
        let row_groups = filter_arg.selected_groups(metadata, &schema);
        let fr = FileReader::new(reader, row_groups, schema, Some(chunk_size), None, None);
        ParquetReader {
            file_reader: fr,
            reader_type: PhantomData,
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
