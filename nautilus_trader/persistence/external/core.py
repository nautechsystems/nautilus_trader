# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import logging
import os
import pathlib
from concurrent.futures import Executor
from concurrent.futures import ThreadPoolExecutor
from io import BytesIO
from itertools import groupby
from typing import Optional, Union

import fsspec
import pandas as pd
import pyarrow as pa
from fsspec.core import OpenFile
from pyarrow import ArrowInvalid
from pyarrow import dataset as ds
from pyarrow import parquet as pq
from tqdm import tqdm

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetWriter
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.persistence.catalog.base import BaseDataCatalog
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.external.metadata import load_mappings
from nautilus_trader.persistence.external.metadata import write_partition_column_mappings
from nautilus_trader.persistence.external.readers import Reader
from nautilus_trader.persistence.external.util import parse_filename_start
from nautilus_trader.persistence.external.util import py_type_to_parquet_type
from nautilus_trader.persistence.funcs import parse_bytes
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.serializer import get_cls_table
from nautilus_trader.serialization.arrow.serializer import get_partition_keys
from nautilus_trader.serialization.arrow.serializer import get_schema
from nautilus_trader.serialization.arrow.util import check_partition_columns
from nautilus_trader.serialization.arrow.util import class_to_filename
from nautilus_trader.serialization.arrow.util import clean_partition_cols
from nautilus_trader.serialization.arrow.util import maybe_list


class RawFile:
    """
    Provides a wrapper of `fsspec.OpenFile` that processes a raw file and writes to parquet.

    Parameters
    ----------
    open_file : fsspec.core.OpenFile
        The fsspec.OpenFile source of this data.
    block_size: int
        The max block (chunk) size in bytes to read from the file.
    progress: bool, default False
        If a progress bar should be shown when processing this individual file.
    """

    def __init__(
        self,
        open_file: OpenFile,
        block_size: Optional[int] = None,
        progress: bool = False,
    ):
        self.open_file = open_file
        self.block_size = block_size
        # TODO - waiting for tqdm support in fsspec https://github.com/intake/filesystem_spec/pulls?q=callback
        assert not progress, "Progress not yet available, awaiting fsspec feature"
        self.progress = progress

    def iter(self):
        with self.open_file as f:
            if self.progress:
                f.read = read_progress(
                    f.read,
                    total=self.open_file.fs.stat(self.open_file.path)["size"],
                )

            while True:
                raw = f.read(self.block_size)
                if not raw:
                    return
                yield raw


def process_raw_file(
    catalog: ParquetDataCatalog,
    raw_file: RawFile,
    reader: Reader,
    use_rust=False,
    instrument=None,
):
    n_rows = 0
    for block in raw_file.iter():
        objs = [x for x in reader.parse(block) if x is not None]
        if use_rust:
            write_parquet_rust(catalog, objs, instrument)
            n_rows += len(objs)
        else:
            dicts = split_and_serialize(objs)
            dataframes = dicts_to_dataframes(dicts)
            n_rows += write_tables(catalog=catalog, tables=dataframes)
    reader.on_file_complete()
    return n_rows


def process_files(
    glob_path,
    reader: Reader,
    catalog: ParquetDataCatalog,
    block_size: str = "128mb",
    compression: str = "infer",
    executor: Optional[Executor] = None,
    use_rust=False,
    instrument: Instrument = None,
    **kwargs,
):
    PyCondition.type_or_none(executor, Executor, "executor")
    if use_rust:
        assert instrument, "Instrument needs to be provided when saving rust data."

    executor = executor or ThreadPoolExecutor()

    raw_files = make_raw_files(
        glob_path=glob_path,
        block_size=block_size,
        compression=compression,
        **kwargs,
    )

    futures = {}
    for rf in raw_files:
        futures[rf] = executor.submit(
            process_raw_file,
            catalog=catalog,
            raw_file=rf,
            reader=reader,
            instrument=instrument,
            use_rust=use_rust,
        )

    # Show progress
    for _ in tqdm(list(futures.values())):
        pass

    results = {rf.open_file.path: f.result() for rf, f in futures.items()}
    executor.shutdown()

    return results


def make_raw_files(glob_path, block_size="128mb", compression="infer", **kw) -> list[RawFile]:
    files = scan_files(glob_path, compression=compression, **kw)
    return [RawFile(open_file=f, block_size=parse_bytes(block_size)) for f in files]


def scan_files(glob_path, compression="infer", **kw) -> list[OpenFile]:
    open_files = fsspec.open_files(glob_path, compression=compression, **kw)
    return [of for of in open_files]


def split_and_serialize(objs: list) -> dict[type, dict[Optional[str], list]]:
    """
    Given a list of Nautilus `objs`; serialize and split into dictionaries per type / instrument ID.
    """
    # Split objects into their respective tables
    values: dict[type, dict[str, list]] = {}
    for obj in objs:
        cls = get_cls_table(type(obj))
        if isinstance(obj, GenericData):
            cls = obj.data_type.type
        if cls not in values:
            values[cls] = {}
        for data in maybe_list(ParquetSerializer.serialize(obj)):
            instrument_id = data.get("instrument_id", None)
            if instrument_id not in values[cls]:
                values[cls][instrument_id] = []
            values[cls][instrument_id].append(data)
    return values


def dicts_to_dataframes(dicts) -> dict[type, dict[str, pd.DataFrame]]:
    """
    Convert dicts from `split_and_serialize` into sorted dataframes.
    """
    # Turn dict of tables into dataframes
    tables: dict[type, dict[str, pd.DataFrame]] = {}
    for cls in dicts:
        tables[cls] = {}
        for ins_id in tuple(dicts[cls]):
            data = dicts[cls].pop(ins_id)
            if not data:
                continue
            df = pd.DataFrame(data)
            df = df.sort_values("ts_init")
            if "instrument_id" in df.columns:
                df = df.astype({"instrument_id": "category"})
            tables[cls][ins_id] = df

    return tables


def determine_partition_cols(cls: type, instrument_id: str = None) -> Union[list, None]:
    """
    Determine partition columns (if any) for this type `cls`.
    """
    partition_keys = get_partition_keys(cls)
    if partition_keys:
        return list(partition_keys)
    elif instrument_id is not None:
        return ["instrument_id"]
    return None


def merge_existing_data(catalog: BaseDataCatalog, cls: type, df: pd.DataFrame) -> pd.DataFrame:
    """
    Handle existing data for instrument subclasses.

    Instruments all live in a single file, so merge with existing data.
    For all other classes, simply return data unchanged.
    """
    if cls not in Instrument.__subclasses__():
        return df
    else:
        try:
            existing = catalog.instruments(instrument_type=cls)
            subset = [c for c in df.columns if c not in ("ts_init", "ts_event", "type")]
            merged = pd.concat([existing, df.drop(["type"], axis=1)])
            return merged.drop_duplicates(subset=subset)
        except pa.lib.ArrowInvalid:
            return df


def write_tables(
    catalog: ParquetDataCatalog, tables: dict[type, dict[str, pd.DataFrame]], **kwargs
):
    """
    Write tables to catalog.
    """
    rows_written = 0

    iterator = [
        (cls, instrument_id, df)
        for cls, instruments in tables.items()
        for instrument_id, df in instruments.items()
    ]

    for cls, instrument_id, df in iterator:
        try:
            schema = get_schema(cls)
        except KeyError:
            print(f"Can't find parquet schema for type: {cls}, skipping!")
            continue
        partition_cols = determine_partition_cols(cls=cls, instrument_id=instrument_id)
        path = f"{catalog.path}/data/{class_to_filename(cls)}.parquet"
        merged = merge_existing_data(catalog=catalog, cls=cls, df=df)

        write_parquet(
            fs=catalog.fs,
            path=path,
            df=merged,
            partition_cols=partition_cols,
            schema=schema,
            **kwargs,
            **({"basename_template": "{i}.parquet"} if cls in Instrument.__subclasses__() else {}),
        )
        rows_written += len(df)

    return rows_written


def write_parquet_rust(catalog: ParquetDataCatalog, objs: list, instrument: Instrument):
    cls = type(objs[0])

    assert cls in (QuoteTick, TradeTick)
    instrument_id = str(instrument.id)

    min_timestamp = str(objs[0].ts_init).rjust(19, "0")
    max_timestamp = str(objs[-1].ts_init).rjust(19, "0")

    parent = catalog.make_path(cls=cls, instrument_id=instrument_id)
    file_path = f"{parent}/{min_timestamp}-{max_timestamp}-0.parquet"

    metadata = {
        "instrument_id": instrument_id,
        "price_precision": str(instrument.price_precision),
        "size_precision": str(instrument.size_precision),
    }
    writer = ParquetWriter(py_type_to_parquet_type(cls), metadata)

    capsule = cls.capsule_from_list(objs)

    writer.write(capsule)

    data: bytes = writer.flush_bytes()

    os.makedirs(os.path.dirname(file_path), exist_ok=True)
    with open(file_path, "wb") as f:
        f.write(data)

    write_objects(catalog, [instrument], existing_data_behavior="overwrite_or_ignore")


def write_parquet(
    fs: fsspec.AbstractFileSystem,
    path: str,
    df: pd.DataFrame,
    partition_cols: Optional[list[str]],
    schema: pa.Schema,
    **kwargs,
):
    """
    Write a single dataframe to parquet.
    """
    # Check partition values are valid before writing to parquet
    mappings = check_partition_columns(df=df, partition_columns=partition_cols)
    df = clean_partition_cols(df=df, mappings=mappings)

    # Dataframe -> pyarrow Table
    table = pa.Table.from_pandas(df, schema=schema)

    if "basename_template" not in kwargs and "ts_init" in df.columns:
        if "bar_type" in df.columns:
            suffix = df.iloc[0]["bar_type"].split(".")[-1]
            kwargs["basename_template"] = (
                f"{df['ts_init'].min()}-{df['ts_init'].max()}" + "-" + suffix + "-{i}.parquet"
            )
        else:
            kwargs["basename_template"] = (
                f"{df['ts_init'].min()}-{df['ts_init'].max()}" + "-{i}.parquet"
            )

    # Write the actual file
    partitions = (
        ds.partitioning(
            schema=pa.schema(fields=[table.schema.field(c) for c in partition_cols]),
            flavor="hive",
        )
        if partition_cols
        else None
    )
    if int(pa.__version__.split(".")[0]) >= 6:
        kwargs.update(existing_data_behavior="overwrite_or_ignore")

    files = set(fs.glob(f"{path}/**"))

    ds.write_dataset(
        data=table,
        base_dir=path,
        filesystem=fs,
        partitioning=partitions,
        format="parquet",
        **kwargs,
    )

    # Ensure data written by write_dataset is sorted
    new_files = set(fs.glob(f"{path}/**/*.parquet")) - files

    del df
    for fn in new_files:
        try:
            ndf = pd.read_parquet(BytesIO(fs.open(fn).read()))
        except ArrowInvalid:
            logging.error(f"Failed to read {fn}")
            continue
        # assert ndf.shape[0] == shape
        if "ts_init" in ndf.columns:
            ndf = ndf.sort_values("ts_init").reset_index(drop=True)
        pq.write_table(
            table=pa.Table.from_pandas(ndf),
            where=fn,
            filesystem=fs,
        )

    # Write the ``_common_metadata`` parquet file without row groups statistics
    pq.write_metadata(table.schema, f"{path}/_common_metadata", version="2.6", filesystem=fs)

    # Write out any partition columns we had to modify due to filesystem requirements
    if mappings:
        existing = load_mappings(fs=fs, path=path)
        if existing:
            mappings["instrument_id"].update(existing["instrument_id"])
        write_partition_column_mappings(fs=fs, path=path, mappings=mappings)


def write_objects(catalog: ParquetDataCatalog, chunk: list, **kwargs):
    serialized = split_and_serialize(objs=chunk)
    tables = dicts_to_dataframes(serialized)
    write_tables(catalog=catalog, tables=tables, **kwargs)


def read_progress(func, total):
    """
    Wrap a file handle and update progress bar as bytes are read.
    """
    progress = tqdm(total=total)

    def inner(*args, **kwargs):
        for data in func(*args, **kwargs):
            progress.update(n=len(data))
            yield data

    return inner


def _validate_dataset(catalog: ParquetDataCatalog, path: str, new_partition_format="%Y%m%d"):
    """
    Repartition dataset into sorted time chunks (default dates) and drop duplicates.
    """
    fs = catalog.fs
    dataset = ds.dataset(path, filesystem=fs)
    fn_to_start = [
        (fn, parse_filename_start(fn=fn)) for fn in dataset.files if parse_filename_start(fn=fn)
    ]

    sort_key = lambda x: (x[1][0], x[1][1].strftime(new_partition_format))  # noqa: E731

    for part, values_iter in groupby(sorted(fn_to_start, key=sort_key), key=sort_key):
        values = list(values_iter)
        filenames = [v[0] for v in values]

        # Read files, drop duplicates
        df: pd.DataFrame = ds.dataset(filenames, filesystem=fs).to_table().to_pandas()
        df = df.drop_duplicates(ignore_index=True, keep="last")

        # Write new file
        table = pa.Table.from_pandas(df, schema=dataset.schema)
        new_fn = filenames[0].replace(pathlib.Path(filenames[0]).stem, part[1])
        pq.write_table(table=table, where=fs.open(new_fn, "wb"))

        # Remove old files
        for fn in filenames:
            fs.rm(fn)


def validate_data_catalog(catalog: ParquetDataCatalog, **kwargs):
    for cls in catalog.list_data_types():
        path = f"{catalog.path}/data/{cls}.parquet"
        _validate_dataset(catalog=catalog, path=path, **kwargs)
