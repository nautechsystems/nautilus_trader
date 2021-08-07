# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
from collections import defaultdict
from concurrent.futures import Executor
from concurrent.futures import ThreadPoolExecutor
from functools import partial
from typing import Callable, Dict, List, Optional

import fsspec
import pandas as pd
import pyarrow as pa
import pyarrow.parquet as pq

from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.persistence.catalog.core import DataCatalog
from nautilus_trader.persistence.catalog.metadata import _write_mappings
from nautilus_trader.persistence.catalog.metadata import save_processed_raw_files
from nautilus_trader.persistence.catalog.parsers import ByteReader
from nautilus_trader.persistence.catalog.parsers import Reader
from nautilus_trader.persistence.catalog.scanner import RawFile
from nautilus_trader.persistence.catalog.scanner import scan
from nautilus_trader.persistence.util import executor_queue_process
from nautilus_trader.persistence.util import get_catalog_fs
from nautilus_trader.persistence.util import get_catalog_root
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.serializer import get_cls_table
from nautilus_trader.serialization.arrow.serializer import get_partition_keys
from nautilus_trader.serialization.arrow.serializer import get_schema
from nautilus_trader.serialization.arrow.util import check_partition_columns
from nautilus_trader.serialization.arrow.util import class_to_filename
from nautilus_trader.serialization.arrow.util import clean_key
from nautilus_trader.serialization.arrow.util import clean_partition_cols
from nautilus_trader.serialization.arrow.util import maybe_list


# TODO - Add callable for writing chunk filename

TIMESTAMP_COLUMN = "ts_init"


def _parse(f: RawFile, reader: ByteReader, instrument_provider=None):
    f.reader = reader
    if instrument_provider:
        f.instrument_provider = instrument_provider
    for chunk in f.iter_parsed():
        if chunk:
            yield {"raw_file": f, "chunk": chunk}
    yield {"raw_file": f, "chunk": None}


def preprocess_instrument_provider(chunk=None, instrument_provider=None):
    if instrument_provider is not None:
        # Find any instrument status updates, if we have some, emit instruments first
        instruments = [
            instrument_provider.find(s.instrument_id)
            for s in chunk
            if isinstance(s, InstrumentStatusUpdate)
        ]
        chunk = instruments + chunk
    return chunk


def nautilus_chunk_to_dataframes(
    chunk: Optional[List[object]],
) -> Dict[type, dict[str, pd.DataFrame]]:
    """
    Split a chunk (list of nautilus objects) into a dict of their respective tables
    """
    if chunk is None:
        return {}
    # Split objects into their respective tables
    tables: Dict[type, Dict[str, List[pd.DataFrame]]] = {}
    for obj in chunk:
        cls = get_cls_table(type(obj))
        if cls not in tables:
            tables[cls] = {}
        if isinstance(obj, GenericData):
            cls = obj.data_type.type
        for data in maybe_list(ParquetSerializer.serialize(obj)):
            instrument_id = data.get("instrument_id", None)
            if instrument_id not in tables[cls]:
                tables[cls][instrument_id] = []
            tables[cls][instrument_id].append(data)

    # Turn dict of tables into dataframes
    for cls in tables:
        for ins_id in tuple(tables[cls]):
            data = tables[cls].pop(ins_id)
            if not data:
                continue
            df = pd.DataFrame(data)
            df = df.sort_values("ts_init")
            tables[cls][ins_id] = df
    return tables


def _write_single(
    cls: type, df: pd.DataFrame, instrument_id: str, append: bool, **parquet_dataset_kwargs
):
    """
    Write a single dataframe to parquet.
    """
    fs = get_catalog_fs()
    root = get_catalog_root().joinpath("data")

    name = f"{class_to_filename(cls)}.parquet"
    fn = f"{root}/{name}"
    partition_cols = get_partition_keys(cls=cls)

    if not append:
        existing = read_and_clear_existing_data(
            fs=fs,
            root=str(root),
            cls=cls,
            path=fn,
            instrument_id=instrument_id,
            partition_cols=partition_cols,
        )
        if existing is not None:
            assert isinstance(existing, pd.DataFrame)
            df = existing.append(df).drop_duplicates()

    # Check column values before writing to parquet
    mappings = check_partition_columns(df=df, partition_columns=partition_cols, cls=cls)
    df = clean_partition_cols(df=df, mappings=mappings)

    # Dataframe -> pyarrow Table
    schema = get_schema(cls)
    table = pa.Table.from_pandas(df, schema=schema)

    # Write the actual file
    metadata_collector: List[pq.FileMetaData] = []
    pq.write_to_dataset(
        table=table,
        root_path=str(fn),
        filesystem=fs,
        partition_cols=partition_cols,
        # use_legacy_dataset=True,
        version="2.0",
        metadata_collector=metadata_collector,
        **parquet_dataset_kwargs,
    )

    # Write the ``_common_metadata`` parquet file without row groups statistics
    pq.write_metadata(table.schema, f"{fn}/_common_metadata", version="2.0", filesystem=fs)

    # Write the ``_metadata`` parquet file with row groups statistics of all files
    pq.write_metadata(table.schema, f"{fn}/_metadata", version="2.0", filesystem=fs)

    # Write out any partition columns we had to modify due to filesystem requirements
    if mappings:
        _write_mappings(fs=fs, fn=fn, mappings=mappings)


def read_and_clear_existing_data(
    fs: fsspec.AbstractFileSystem,
    root: str,
    path: str,
    cls: type,
    instrument_id: str,
    partition_cols: List[str],
):
    """
    Check if any file exists at `path`, reading if it exists and removing the file. It will be rewritten later.
    """
    if fs.exists(path) or fs.isdir(path):
        catalog = DataCatalog(path=root)
        existing = catalog._query(
            filename=class_to_filename(cls),
            instrument_ids=instrument_id if cls not in Instrument.__subclasses__() else None,
            ts_column=TIMESTAMP_COLUMN,
            raise_on_empty=False,
        )
        if not existing.empty:
            # Remove this file/partition, will be written again
            if partition_cols:
                assert partition_cols == [
                    "instrument_id"
                ], "Only support appending to instrument_id partitions"
                # We only want to remove this partition
                partition_path = f"instrument_id={clean_key(instrument_id)}"
                fs.rm(f"{path}/{partition_path}", recursive=True)
            else:
                fs.rm(path, recursive=True)

            return existing


def write_chunk(raw_file: RawFile, chunk, append=False, **parquet_dataset_kwargs):
    shape = 0
    tables = nautilus_chunk_to_dataframes(chunk)

    # Load any existing data, drop dupes
    for cls, instruments in tables.items():
        for instrument_id, df in instruments.items():
            _write_single(
                cls=cls, df=df, instrument_id=instrument_id, append=append, **parquet_dataset_kwargs
            )
            shape += len(df)

    if chunk is None:  # EOF
        save_processed_raw_files(files=[raw_file.path])
    return raw_file, shape


def process_files(
    files: List[RawFile],
    reader: Reader,
    progress=True,
    executor: Executor = None,
    instrument_provider=None,
):
    """
    Load data in chunks from `files`, parsing with the `parser` function using `executor`.

    Utilises queues to block the executors reading too many chunks (limiting memory use), while also allowing easy
    parallelisation.

    """
    executor = executor or ThreadPoolExecutor()
    return executor_queue_process(
        executor=executor,
        inputs=[{"f": f} for f in files],
        process_func=partial(_parse, reader=reader, instrument_provider=instrument_provider),
        output_func=write_chunk,
        progress=progress,
    )


def load(
    path: str,
    reader: Reader,
    fs_protocol="file",
    glob_pattern="**",
    progress=True,
    chunk_size=-1,
    compression: str = "infer",
    file_filter: Callable = None,
    executor: Executor = None,
    skip_already_processed=True,
    instrument_provider=None,
):
    """
    Scan and process files
    """
    files = scan(
        path=path,
        fs_protocol=fs_protocol,
        glob_pattern=glob_pattern,
        progress=progress,
        chunk_size=chunk_size,
        compression=compression,
        file_filter=file_filter,
        executor=executor,
        skip_already_processed=skip_already_processed,
    )
    results = process_files(
        files=files,
        reader=reader,
        progress=progress,
        executor=executor,
        instrument_provider=instrument_provider,
    )

    # Aggregate results
    file_shapes: Dict[str, int] = defaultdict(lambda: 0)
    for k, shape in results:
        file_shapes[k] += shape
    return file_shapes


def _determine_partition_cols(cls: type, instrument_id: str = None):
    partition_keys = get_partition_keys(cls)
    if partition_keys is not None:
        return list(partition_keys)
    elif instrument_id is not None:
        return ["instrument_id"]
    return
