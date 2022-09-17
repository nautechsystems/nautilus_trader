use std::collections::HashSet;

use std::sync::Arc;
use std::{fs::File, marker::PhantomData};

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

impl GroupFilterArg {
    /// Scan metadata and choose which chunks to filter and returns a HashSet
    /// holding the indexes of the selected chunks.
    fn filter_groups(&self, metadata: &FileMetaData, schema: &Schema) -> HashSet<usize> {
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

pub struct ParquetReader<A> {
    file_reader: FileReader<File>,
    reader_type: PhantomData<*const A>,
}

impl<A> ParquetReader<A> {
    pub fn new(file_path: &str, chunk_size: usize, filter_arg: GroupFilterArg) -> Self {
        let mut file = File::open(file_path)
            .unwrap_or_else(|_| panic!("Unable to open parquet file {file_path}"));

        // TODO: duplicate type definition from arrow2 parquet file reader
        // because it does not expose it
        type GroupFilter = Arc<dyn Fn(usize, &RowGroupMetaData) -> bool + Send + Sync>;
        let group_filter = match filter_arg {
            GroupFilterArg::None => None,
            // a closure that captures the HashSet of indexes of selected chunks
            // and uses this to check if a chunk is selected based on it's index
            _ => {
                let metadata = read::read_metadata(&mut file).expect("unable to read metadata");
                let schema = read::infer_schema(&metadata).expect("unable to infer schema");
                let select_groups = filter_arg.filter_groups(&metadata, &schema);
                let filter_closure: GroupFilter = Arc::new(
                    move |group_index: usize, _metadata: &RowGroupMetaData| -> bool {
                        select_groups.contains(&group_index)
                    },
                );
                Some(filter_closure)
            }
        };

        let fr = FileReader::try_new(file, None, Some(chunk_size), None, group_filter)
            .expect("Unable to create reader from file");
        ParquetReader {
            file_reader: fr,
            reader_type: PhantomData,
        }
    }
}

impl<A> Iterator for ParquetReader<A>
where
    A: DecodeFromChunk,
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
