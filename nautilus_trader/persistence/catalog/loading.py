from collections import defaultdict
from concurrent.futures import Executor
from concurrent.futures import Future
from concurrent.futures import ThreadPoolExecutor
from concurrent.futures import as_completed
from typing import Callable, List

import pandas as pd
from tqdm import tqdm

from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.persistence.catalog.core import DataCatalog
from nautilus_trader.persistence.catalog.parsers import ByteParser
from nautilus_trader.persistence.catalog.parsers import NewFile
from nautilus_trader.persistence.catalog.scanner import ChunkedFile
from nautilus_trader.serialization.arrow.serializer import _CLS_TO_TABLE
from nautilus_trader.serialization.arrow.serializer import _PARTITION_KEYS
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.util import class_to_filename
from nautilus_trader.serialization.arrow.util import clean_key
from nautilus_trader.serialization.arrow.util import maybe_list


# TODO - Add callable for writing chunk filename

TIMESTAMP_COLUMN = "ts_init"


def _parse(f: ChunkedFile, parser: ByteParser, instrument_provider=None):
    data = []
    for chunk in parser.read(stream=f.iter_chunks(), instrument_provider=instrument_provider):
        if chunk:
            data.append(chunk)
    return data


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


def split_chunk_tables(chunk: List[object], processed_files=None, processed_raw_files=None):
    """
    Split a chunk (list of nautilus objects) into a dict of their respective tables
    """
    # Split objects into their respective tables
    tables = defaultdict(dict)  # type: ignore
    skip_file = False
    for obj in chunk:
        if skip_file:
            continue
        if isinstance(obj, NewFile):
            if obj.name in processed_files:
                skip_file = True
            else:
                skip_file = False
                processed_raw_files.append(obj.name)
            continue

        cls = _CLS_TO_TABLE.get(type(obj), type(obj))
        if isinstance(obj, GenericData):
            cls = obj.data_type.type
        for data in maybe_list(ParquetSerializer.serialize(obj)):
            instrument_id = data.get("instrument_id", None)
            if instrument_id not in tables[cls]:
                tables[cls][instrument_id] = []
            tables[cls][instrument_id].append(data)
    return tables


def table_to_dataframes(path, tables):
    for cls in tables:
        for ins_id in tables[cls]:
            df = pd.DataFrame(tables[cls][ins_id])

            if df.empty:
                continue

            # partition_cols = _determine_partition_cols(cls=cls, instrument_id=ins_id)

            df = df.sort_values("ts_init")
            # df = df.astype({k: "category" for k in category_attributes.get(cls.__name__, [])})


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


# def write_chunk(fs, chunk, append_only=False, **kwargs):  # noqa: C901
#     processed_raw_files = _load_processed_raw_files()
#     log_filenames = kwargs.pop("log_filenames", False)
#
#     # Load any existing data, drop dupes
#
#     name = f"{class_to_filename(cls)}.parquet"
#     fn = path.joinpath(name)
#
#     if not append_only:
#         existing = read_and_clear_existing_data(fs=fs)
#
#         df, mappings = clean_partition_cols(df, partition_cols, cls)
#         schema = self._schemas.get(cls)
#         table = pa.Table.from_pandas(df, schema=schema)
#         metadata_collector = []
#         pq.write_to_dataset(
#             table=table,
#             root_path=str(fn),
#             filesystem=self.fs,
#             partition_cols=partition_cols,
#             # use_legacy_dataset=True,
#             version="2.0",
#             metadata_collector=metadata_collector,
#             **kwargs,
#         )
#         # Write the ``_common_metadata`` parquet file without row groups statistics
#         pq.write_metadata(
#             table.schema, str(fn / "_common_metadata"), version="2.0", filesystem=self.fs
#         )
#
#         # Write the ``_metadata`` parquet file with row groups statistics of all files
#         pq.write_metadata(table.schema, str(fn / "_metadata"), version="2.0", filesystem=self.fs)
#
#         # Write out any partition columns we had to modify due to filesystem requirements
#         if mappings:
#             self._write_mappings(fn=fn, mappings=mappings)
#
#         # Save any new processed files
#     self._save_processed_raw_files(files=processed_raw_files)


def read_files(
    files: List[ChunkedFile],
    parser: Callable,
    progress=True,
    executor: Executor = None,
    instrument_provider=None,
):
    executor = executor or ThreadPoolExecutor()

    # Submit files for processing
    futures: List[Future] = []
    with executor as client:
        for f in files:
            futures.append(client.submit(_parse, f=f, parser=parser))

    # Gather results
    if progress:
        futures = tqdm(futures)
    for fut in as_completed(futures):
        chunk = fut.result()
        chunk = preprocess_instrument_provider(chunk=chunk, instrument_provider=instrument_provider)
        # write_chunk(chunk)


def _determine_partition_cols(cls: type, instrument_id: str = None):
    if _PARTITION_KEYS.get(cls) is not None:
        return list(_PARTITION_KEYS[cls])
    elif instrument_id is not None:
        return ["instrument_id"]
    return


# def clear_cache(**kwargs):
#     force = kwargs.get("FORCE", False)
#     if not force:
#         print(
#             "Are you sure you want to clear the WHOLE BACKTEST CACHE?, if so, call clear_cache(FORCE=True)"
#         )
#     else:
#         self.fs.rm(self.path, recursive=True)
