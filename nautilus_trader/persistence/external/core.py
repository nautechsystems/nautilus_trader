# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import pathlib
import re
from itertools import groupby
from typing import Dict, List, Optional, Tuple, Union

import dask
import fsspec
import pandas as pd
import pyarrow as pa
import pyarrow.dataset as ds
import pyarrow.parquet as pq
from dask import compute
from dask import delayed
from dask.diagnostics import ProgressBar
from dask.utils import parse_bytes
from fsspec.core import OpenFile
from tqdm import tqdm

from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.metadata import write_partition_column_mappings
from nautilus_trader.persistence.external.readers import Reader
from nautilus_trader.persistence.external.synchronization import named_lock
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.serializer import get_cls_table
from nautilus_trader.serialization.arrow.serializer import get_partition_keys
from nautilus_trader.serialization.arrow.serializer import get_schema
from nautilus_trader.serialization.arrow.util import check_partition_columns
from nautilus_trader.serialization.arrow.util import class_to_filename
from nautilus_trader.serialization.arrow.util import clean_partition_cols
from nautilus_trader.serialization.arrow.util import maybe_list


try:
    import distributed
except ImportError:  # pragma: no cover
    distributed = None


class RawFile:
    """
    Provides a wrapper of fsspec.OpenFile that processes a raw file and writes to parquet.
    """

    def __init__(
        self,
        open_file: OpenFile,
        block_size: Optional[int] = None,
        progress=False,
    ):
        """
        Initialize a new instance of the ``RawFile`` class.

        Parameters
        ----------
        open_file : OpenFile
            The fsspec.OpenFile source of this data.
        block_size: int
            The max block (chunk) size to read from the file.
        progress: bool
            Show a progress bar while processing this individual file.

        """
        self.open_file = open_file
        self.block_size = block_size
        # TODO - waiting for tqdm support in fsspec https://github.com/intake/filesystem_spec/pulls?q=callback
        assert not progress, "Progress not yet available, awaiting fsspec feature"
        self.progress = progress

    def iter(self):
        with self.open_file as f:
            if self.progress:
                f.read = read_progress(  # type: ignore
                    f.read, total=self.open_file.fs.stat(self.open_file.path)["size"]
                )

            while True:
                raw = f.read(self.block_size)
                if not raw:
                    return
                yield raw


def process_raw_file(catalog: DataCatalog, raw_file: RawFile, reader: Reader):
    n_rows = 0
    for block in raw_file.iter():
        objs = [x for x in reader.parse(block) if x is not None]
        dicts = split_and_serialize(objs)
        dataframes = dicts_to_dataframes(dicts)
        n_rows += write_tables(catalog=catalog, tables=dataframes)
    reader.on_file_complete()
    return n_rows


def process_files(
    glob_path,
    reader: Reader,
    catalog: DataCatalog,
    block_size="128mb",
    compression="infer",
    scheduler: Union[str, "distributed.Client"] = "sync",
    **kw,
):
    assert scheduler == "sync" or str(scheduler.__module__) == "distributed.client"
    raw_files = make_raw_files(
        glob_path=glob_path,
        block_size=block_size,
        compression=compression,
        **kw,
    )
    tasks = [
        delayed(process_raw_file)(catalog=catalog, reader=reader, raw_file=rf) for rf in raw_files
    ]
    with ProgressBar():
        with dask.config.set(scheduler=scheduler):
            results = compute(tasks)
    return dict((rf.open_file.path, value) for rf, value in zip(raw_files, results[0]))


def make_raw_files(glob_path, block_size="128mb", compression="infer", **kw) -> List[RawFile]:
    files = scan_files(glob_path, compression=compression, **kw)
    return [RawFile(open_file=f, block_size=parse_bytes(block_size)) for f in files]


def scan_files(glob_path, compression="infer", **kw) -> List[OpenFile]:
    open_files = fsspec.open_files(glob_path, compression=compression, **kw)
    return [of for of in open_files]


def split_and_serialize(objs: List) -> Dict[type, Dict[Optional[str], List]]:
    """
    Given a list of Nautilus `objs`; serialize and split into dictionaries per type / instrument ID.
    """
    # Split objects into their respective tables
    values: Dict[type, Dict[str, List]] = {}
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


def dicts_to_dataframes(dicts) -> Dict[type, Dict[str, pd.DataFrame]]:
    """
    Convert dicts from `split_and_serialize` into sorted dataframes.
    """
    # Turn dict of tables into dataframes
    tables: Dict[type, Dict[str, pd.DataFrame]] = {}
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


def determine_partition_cols(cls: type, instrument_id: str = None) -> Union[List, None]:
    """
    Determine partition columns (if any) for this type `cls`.
    """
    partition_keys = get_partition_keys(cls)
    if partition_keys:
        return list(partition_keys)
    elif instrument_id is not None:
        return ["instrument_id"]
    return None


def merge_existing_data(catalog: DataCatalog, cls: type, df: pd.DataFrame) -> pd.DataFrame:
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
            return existing.append(df.drop(["type"], axis=1)).drop_duplicates()
        except pa.lib.ArrowInvalid:
            return df


def write_tables(catalog: DataCatalog, tables: Dict[type, Dict[str, pd.DataFrame]], **kwargs):
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
        name = f"{class_to_filename(cls)}.parquet"
        path = f"{catalog.path}/data/{name}"
        merged = merge_existing_data(catalog=catalog, cls=cls, df=df)
        with named_lock(name):
            write_parquet(
                fs=catalog.fs,
                path=path,
                df=merged,
                partition_cols=partition_cols,
                schema=schema,
                **kwargs,
            )
        rows_written += len(df)

    return rows_written


def write_parquet(
    fs: fsspec.AbstractFileSystem,
    path: str,
    df: pd.DataFrame,
    partition_cols: Optional[List[str]],
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
    if pa.__version__ >= "6.0.0":
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
        ndf = pd.read_parquet(fs.open(fn))
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
        write_partition_column_mappings(fs=fs, path=path, mappings=mappings)


def write_objects(catalog: DataCatalog, chunk: List, **kwargs):
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


def _parse_file_start_by_filename(fn: str):
    """
    Parse start time by filename.

    >>> _parse_file_start_by_filename('/data/test/sample.parquet/instrument_id=a/1577836800000000000-1578182400000000000-0.parquet')
    '1577836800000000000'

    >>> _parse_file_start_by_filename('/data/test/sample.parquet/instrument_id=a/0648140b1fd7491a97983c0c6ece8d57.parquet')

    """
    match = re.match(r"(?P<start>\d{19})\-\d{19}\-\d", pathlib.Path(fn).stem)
    if match:
        return int(match.groups()[0])


def _parse_file_start(fn: str) -> Optional[Tuple[str, pd.Timestamp]]:
    instrument_id = re.findall(r"instrument_id\=(.*)\/", fn)[0] if "instrument_id" in fn else None
    start = _parse_file_start_by_filename(fn=fn)
    if start is not None:
        start = pd.Timestamp(start)
        return instrument_id, start
    return None


def _validate_dataset(catalog: DataCatalog, path: str, new_partition_format="%Y%m%d"):
    """
    Repartition dataset into sorted time chunks (default dates) and drop duplicates.
    """
    fs = catalog.fs
    dataset = ds.dataset(path, filesystem=fs)
    fn_to_start = [
        (fn, _parse_file_start(fn=fn)) for fn in dataset.files if _parse_file_start(fn=fn)
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


def validate_data_catalog(catalog: DataCatalog, **kwargs):
    for cls in catalog.list_data_types():
        path = f"{catalog.path}/data/{cls}.parquet"
        _validate_dataset(catalog=catalog, path=path, **kwargs)
