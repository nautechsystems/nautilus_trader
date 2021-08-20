# # -------------------------------------------------------------------------------------------------
# #  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
# #  https://nautechsystems.io
# #
# #  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
# #  You may not use this file except in compliance with the License.
# #  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# #
# #  Unless required by applicable law or agreed to in writing, software
# #  distributed under the License is distributed on an "AS IS" BASIS,
# #  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# #  See the License for the specific language governing permissions and
# #  limitations under the License.
# # -------------------------------------------------------------------------------------------------
#
# import pathlib
# import pickle
# from collections import defaultdict
# from concurrent.futures import Executor
# from concurrent.futures import ThreadPoolExecutor
# from functools import partial
# from typing import Any, Callable, Dict, List, Optional
#
# import fsspec
# import pandas as pd
# import pyarrow as pa
# import pyarrow.parquet as pq
# from distributed.protocol import Serialize
# from tqdm import tqdm
#
# from nautilus_trader.model.data.base import GenericData
# from nautilus_trader.model.instruments.base import Instrument
# from nautilus_trader.persistence.catalog import DataCatalog
# from nautilus_trader.persistence.external.metadata import save_processed_raw_files
# from nautilus_trader.persistence.external.parsers import ByteReader
# from nautilus_trader.persistence.external.parsers import RawFile
# from nautilus_trader.persistence.external.parsers import Reader
# from nautilus_trader.persistence.external.processing import distributed_executor_cls
# from nautilus_trader.persistence.external.processing import executor_queue_process
# from nautilus_trader.persistence.external.scanner import scan
# from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
# from nautilus_trader.serialization.arrow.serializer import get_cls_table
# from nautilus_trader.serialization.arrow.serializer import get_partition_keys
# from nautilus_trader.serialization.arrow.serializer import get_schema
# from nautilus_trader.serialization.arrow.util import check_partition_columns
# from nautilus_trader.serialization.arrow.util import class_to_filename
# from nautilus_trader.serialization.arrow.util import clean_key
# from nautilus_trader.serialization.arrow.util import clean_partition_cols
# from nautilus_trader.serialization.arrow.util import maybe_list
#
#
# # TODO - Add callable for writing chunk filename
#
# TIMESTAMP_COLUMN = "ts_init"
#
#
# def parse_raw_file(f: RawFile, reader: ByteReader, instrument_provider=None, wrapper=None):
#     f.reader = reader
#     if instrument_provider:
#         f.instrument_provider = instrument_provider
#     for chunk in f.iter_parsed():
#         if chunk:
#             if wrapper:
#                 f = wrapper(f)
#             yield {"raw_file": f, "chunk": pickle.dumps(chunk)}
#     if wrapper:
#         f = wrapper(f)
#     yield {"raw_file": f, "chunk": None}
#
#
# def nautilus_chunk_to_dataframes(  # noqa: C901
#     chunk: Optional[List[Any]],
# ) -> Dict[type, Dict[str, pd.DataFrame]]:
#     """
#     Split a chunk (list of nautilus objects) into a dict of their respective tables
#     """
#     if chunk is None:
#         return {}
#     # Split objects into their respective tables
#     values: Dict[type, Dict[str, List[Any]]] = {}
#     for obj in chunk:
#         cls = get_cls_table(type(obj))
#         if cls not in values:
#             values[cls] = {}
#         if isinstance(obj, GenericData):
#             cls = obj.data_type.type
#         for data in maybe_list(ParquetSerializer.serialize(obj)):
#             instrument_id = data.get("instrument_id", None)
#             if instrument_id not in values[cls]:
#                 values[cls][instrument_id] = []
#             values[cls][instrument_id].append(data)
#
#     # Turn dict of tables into dataframes
#     tables: Dict[type, Dict[str, pd.DataFrame]] = {}
#     for cls in values:
#         tables[cls] = {}
#         for ins_id in tuple(values[cls]):
#             data = values[cls].pop(ins_id)
#             if not data:
#                 continue
#             df = pd.DataFrame(data)
#             df = df.sort_values("ts_init")
#             if "instrument_id" in df.columns:
#                 df = df.astype({"instrument_id": "category"})
#             tables[cls][ins_id] = df
#     return tables
#
#
# def write_parquet(
#     fs: fsspec.AbstractFileSystem,
#     root: pathlib.Path,
#     path: str,
#     df: pd.DataFrame,
#     instrument_id: Optional[str],
#     partition_cols: Optional[List[str]],
#     schema: pa.Schema,
#     append: bool,
#     **parquet_dataset_kwargs,
# ):
#     """
#     Write a single dataframe to parquet.
#     """
#
#     if not append:
#         existing = read_and_clear_existing_data(
#             fs=fs,
#             root=root,
#             path=path,
#             instrument_id=instrument_id,
#             partition_cols=partition_cols,
#         )
#         if existing is not None:
#             assert isinstance(existing, pd.DataFrame)
#             df = existing.append(df).drop_duplicates()
#
#     # Check column values before writing to parquet
#     mappings = check_partition_columns(df=df, partition_columns=partition_cols)
#     df = clean_partition_cols(df=df, mappings=mappings)
#
#     # Dataframe -> pyarrow Table
#     table = pa.Table.from_pandas(df, schema=schema)
#
#     full_path = f"{root}/{path}"
#
#     # Write the actual file
#     metadata_collector: List[pq.FileMetaData] = []
#     pq.write_to_dataset(
#         table=table,
#         root_path=full_path,
#         filesystem=fs,
#         partition_cols=partition_cols,
#         # use_legacy_dataset=True,
#         version="2.0",
#         metadata_collector=metadata_collector,
#         **parquet_dataset_kwargs,
#     )
#
#     # Write the ``_common_metadata`` parquet file without row groups statistics
#     pq.write_metadata(table.schema, f"{full_path}/_common_metadata", version="2.0", filesystem=fs)
#
#     # Write the ``_metadata`` parquet file with row groups statistics of all files
#     pq.write_metadata(table.schema, f"{full_path}/_metadata", version="2.0", filesystem=fs)
#
#     # Write out any partition columns we had to modify due to filesystem requirements
#     if mappings:
#         write_mappings(fs=fs, path=full_path, mappings=mappings)
#
#
# def read_and_clear_existing_data(
#     catalog: DataCatalog,
#     path: str,
#     instrument_id: Optional[str],
#     partition_cols: List[str],
# ):
#     """
#     Check if any file exists at `path`, reading if it exists and removing the file. It will be rewritten later.
#     """
#     fs = catalog.fs
#     if fs.exists(path) or fs.isdir(path):
#         existing = catalog._query(
#             path=path,
#             instrument_ids=instrument_id,
#             ts_column=TIMESTAMP_COLUMN,
#             raise_on_empty=False,
#         )
#         if not existing.empty:
#             # Remove this file/partition, will be written again
#             if partition_cols:
#                 assert partition_cols == [
#                     "instrument_id"
#                 ], "Only support appending to instrument_id partitions"
#                 # We only want to remove this partition
#                 partition_path = f"instrument_id={clean_key(instrument_id)}"
#                 fs.rm(f"{path}/{partition_path}", recursive=True)
#             else:
#                 fs.rm(path, recursive=True)
#
#             return existing
#
#
# def write_chunk(raw_file: RawFile, chunk, catalog=None, append=False, **parquet_dataset_kwargs):
#     catalog = catalog or DataCatalog.from_env()
#
#     if chunk is None:  # EOF
#         if isinstance(raw_file, Serialize):
#             raw_file = raw_file.data
#         save_processed_raw_files(fs=catalog.fs, root=catalog.path, files=[raw_file.path])
#         return
#
#     if isinstance(chunk, bytes):
#         # Return as bytes from dask distributed worker
#         chunk = pickle.loads(chunk)
#
#     fs = catalog.fs
#     root = catalog.path.joinpath("data")
#     shape = 0
#     tables = nautilus_chunk_to_dataframes(chunk)
#
#     # Load any existing data, drop dupes
#     for cls, instruments in tables.items():
#         for instrument_id, df in instruments.items():
#             partition_cols = determine_partition_cols(cls=cls, instrument_id=instrument_id)
#             try:
#                 schema = get_schema(cls)
#             except KeyError:
#                 print(f"Can't find parquet schema for type: {cls}, skipping!")
#                 continue
#
#             path = f"{class_to_filename(cls)}.parquet"
#             write_parquet(
#                 fs=fs,
#                 root=root,
#                 path=path,
#                 df=df,
#                 instrument_id=instrument_id if cls not in Instrument.__subclasses__() else None,
#                 partition_cols=partition_cols,
#                 schema=schema,
#                 append=append,
#                 **parquet_dataset_kwargs,
#             )
#             shape += len(df)
#
#     return raw_file.path, shape
#
#
# def progress_wrapper(f, total):
#     progress = tqdm(total=total)
#
#     def inner(*args, **kwargs):
#         result = f(*args, **kwargs)
#         progress.update()
#         return result
#
#     return inner
#
#
# def process_files(
#     files: List[RawFile],
#     reader: Reader,
#     catalog=None,
#     progress=True,
#     executor: Executor = None,
#     instrument_provider=None,
#     output_func=None,
# ):
#     """
#     Load data in chunks from `files`, parsing with the `parser` function using
#     `executor`.
#
#     Utilises queues to block the executors reading too many chunks (limiting
#     memory use), while also allowing easy parallelization.
#
#     """
#     catalog = catalog or DataCatalog.from_env()
#     executor = executor or ThreadPoolExecutor()
#     raw_file_wrapper = None
#     output_func = output_func or partial(write_chunk, catalog=catalog)
#
#     if progress:
#         output_func = progress_wrapper(output_func, total=sum([f.num_chunks for f in files]))
#     if isinstance(executor, distributed_executor_cls):
#         # If using dask.distributed executor, we need to wrap files in `to_serialize` to tell the client
#         # to serialize before putting into the queue in `executor_queue_process`.
#         from distributed.protocol.serialize import to_serialize
#
#         files = [to_serialize(f) for f in files]
#         raw_file_wrapper = to_serialize
#     return executor_queue_process(
#         executor=executor,
#         inputs=[{"f": f} for f in files],
#         process_func=partial(
#             parse_raw_file,
#             reader=reader,
#             instrument_provider=instrument_provider,
#             wrapper=raw_file_wrapper,
#         ),
#         output_func=output_func,
#     )
#
#
# def load(
#     path: str,
#     reader: Reader,
#     fs_protocol="file",
#     glob_pattern="**",
#     progress=True,
#     chunk_size=-1,
#     compression: str = "infer",
#     file_filter: Callable = None,
#     executor: Executor = None,
#     skip_already_processed=True,
#     instrument_provider=None,
# ):
#     """
#     Scan and process files
#     """
#     files = scan(
#         path=path,
#         fs_protocol=fs_protocol,
#         glob_pattern=glob_pattern,
#         progress=progress,
#         chunk_size=chunk_size,
#         compression=compression,
#         file_filter=file_filter,
#         executor=executor,
#         skip_already_processed=skip_already_processed,
#     )
#     assert files, f"Could not find files for protocol={fs_protocol}, path={path}"
#     results = process_files(
#         files=files,
#         reader=reader,
#         progress=progress,
#         executor=executor,
#         instrument_provider=instrument_provider,
#     )
#
#     # Aggregate results
#     file_shapes: Dict[str, int] = defaultdict(lambda: 0)
#     for k, shape in results:
#         file_shapes[k] += shape
#     return dict(file_shapes)
#
#
# def determine_partition_cols(cls: type, instrument_id: str = None):
#     if cls in Instrument.__subclasses__():
#         # No partitioning for instrument tables
#         return None
#     partition_keys = get_partition_keys(cls)
#     if partition_keys:
#         return list(partition_keys)
#     elif instrument_id is not None:
#         return ["instrument_id"]
#     return
