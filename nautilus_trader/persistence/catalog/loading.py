import itertools
from concurrent.futures import Executor
from concurrent.futures import Future
from concurrent.futures import ThreadPoolExecutor
from queue import Queue
from typing import Callable, Dict, List

import pandas as pd
import pyarrow.parquet as pq
from distributed.cfexecutor import ClientExecutor
from tqdm import tqdm

from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.persistence.catalog.core import DataCatalog
from nautilus_trader.persistence.catalog.parsers import ByteParser
from nautilus_trader.persistence.catalog.parsers import NewFile
from nautilus_trader.persistence.catalog.scanner import ChunkedFile
from nautilus_trader.persistence.util import SyncExecutor
from nautilus_trader.persistence.util import executor_queue_process
from nautilus_trader.persistence.util import get_catalog_fs
from nautilus_trader.persistence.util import get_catalog_root
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.serializer import get_cls_table
from nautilus_trader.serialization.arrow.serializer import get_partition_keys
from nautilus_trader.serialization.arrow.serializer import get_schema
from nautilus_trader.serialization.arrow.util import class_to_filename
from nautilus_trader.serialization.arrow.util import clean_key
from nautilus_trader.serialization.arrow.util import clean_partition_cols
from nautilus_trader.serialization.arrow.util import maybe_list


# TODO - Add callable for writing chunk filename

TIMESTAMP_COLUMN = "ts_init"


def _parse(f: ChunkedFile, parser: ByteParser, instrument_provider=None):
    for chunk in parser.read(stream=f.iter_chunks(), instrument_provider=instrument_provider):
        if chunk:
            yield chunk


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


def nautilus_chunk_to_dataframes(chunk: List[object]) -> Dict[type, dict[str, List]]:
    """
    Split a chunk (list of nautilus objects) into a dict of their respective tables
    """
    # Split objects into their respective tables
    tables = {}
    for obj in chunk:
        if isinstance(obj, NewFile):  # Simply ignore
            continue
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


def read_and_clear_existing_data(fs, fn, cls, instrument_id, partition_cols):
    if fs.exists(str(fn)) or fs.isdir(str(fn)):
        catalog = DataCatalog()
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
                fs.rm(str(fn / partition_path), recursive=True)
            else:
                fs.rm(str(fn), recursive=True)

            return existing


def write_chunk(chunk, append=False, **kwargs):
    fs = get_catalog_fs()
    root = get_catalog_root().joinpath("data")

    tables = nautilus_chunk_to_dataframes(chunk)

    # Load any existing data, drop dupes
    for cls, df in tables.items():

        name = f"{class_to_filename(cls)}.parquet"
        fn = root.joinpath(name)

        if not append:
            existing = read_and_clear_existing_data(fs=fs)

        partition_cols = get_partition_keys()

        df, mappings = clean_partition_cols(df, partition_cols, cls)
        schema = get_schema(cls)
        table = pa.Table.from_pandas(df, schema=schema)
        metadata_collector = []
        pq.write_to_dataset(
            table=table,
            root_path=str(fn),
            filesystem=self.fs,
            partition_cols=partition_cols,
            # use_legacy_dataset=True,
            version="2.0",
            metadata_collector=metadata_collector,
            **kwargs,
        )
        # Write the ``_common_metadata`` parquet file without row groups statistics
        pq.write_metadata(
            table.schema, str(fn / "_common_metadata"), version="2.0", filesystem=self.fs
        )

        # Write the ``_metadata`` parquet file with row groups statistics of all files
        pq.write_metadata(table.schema, str(fn / "_metadata"), version="2.0", filesystem=self.fs)

        # Write out any partition columns we had to modify due to filesystem requirements
        if mappings:
            write_mappings(fn=fn, mappings=mappings)


def process_files(
    files: List[ChunkedFile],
    parser: Callable,
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
    executor_queue_process(
        executor=executor,
        inputs=files,
        process_func=_parse,
        output_func=write_chunk,
    )

    # Load files into queues
    queue_iter = itertools.cycle(queues)
    for f in files:
        q = next(queue_iter)
        q.put(f)

    # Gather results and write (single thread)
    if progress:
        futures = tqdm(futures)
    prev_file = None  # Used to determine if we're at the end of a file
    for fut in futures:
        chunk = fut.result()
        chunk = preprocess_instrument_provider(chunk=chunk, instrument_provider=instrument_provider)
        new_file = chunk[0] if isinstance(chunk[0], NewFile) else None
        write_chunk(
            chunk=chunk,
            append=new_file
            is None,  # If we don't have a new file, simply append this chunk to existing
        )
        if new_file and prev_file:
            # We've hit a new file, save prev_file to processed
            save_processed_raw_files(files=processed_raw_files)


def _determine_partition_cols(cls: type, instrument_id: str = None):
    partition_keys = get_partition_keys(cls)
    if partition_keys is not None:
        return list(partition_keys)
    elif instrument_id is not None:
        return ["instrument_id"]
    return
