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
import os
import pathlib
import platform
from typing import Callable, Dict, List, Optional, Union

import fsspec
import pandas as pd
import pyarrow as pa
import pyarrow.dataset as ds
import pyarrow.parquet as pq
from fsspec.utils import infer_storage_options

from nautilus_trader.persistence.catalog.base import BaseDataCatalog
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.util import class_to_filename
from nautilus_trader.serialization.arrow.util import clean_key
from nautilus_trader.serialization.arrow.util import dict_of_lists_to_list_of_dicts


class ParquetDataCatalog(BaseDataCatalog):
    """
    Provides a queryable data catalog persisted to file in parquet format.

    Parameters
    ----------
    path : str
        The root path for this data catalog. Must exist and must be an absolute path.
    fs_protocol : str, default 'file'
        The fsspec filesystem protocol to use.
    fs_storage_options : Dict, optional
        The fs storage options.
    """

    def __init__(
        self,
        path: str,
        fs_protocol: str = "file",
        fs_storage_options: Optional[Dict] = None,
    ):
        self.fs_protocol = fs_protocol
        self.fs_storage_options = fs_storage_options or {}
        self.fs: fsspec.AbstractFileSystem = fsspec.filesystem(
            self.fs_protocol, **self.fs_storage_options
        )
        self.path: pathlib.Path = pathlib.Path(path)

    @classmethod
    def from_env(cls):
        return cls.from_uri(uri=os.path.join(os.environ["NAUTILUS_PATH"], "catalog"))

    @classmethod
    def from_uri(cls, uri):
        if "://" not in uri:
            # Assume a local path
            uri = "file://" + uri
        parsed = infer_storage_options(uri)
        path = parsed.pop("path")
        protocol = parsed.pop("protocol")
        storage_options = parsed.copy()
        return cls(path=path, fs_protocol=protocol, fs_storage_options=storage_options)

    # -- QUERIES -----------------------------------------------------------------------------------

    @staticmethod
    def _build_filter_expression(
        filter_expr: Optional[Callable] = None,
        instrument_ids: Optional[List[str]] = None,
        start: Optional[Union[pd.Timestamp, str, int]] = None,
        end: Optional[Union[pd.Timestamp, str, int]] = None,
        ts_column="ts_init",
        instrument_id_column="instrument_id",
        clean_instrument_keys: bool = True,
    ):
        filters = [filter_expr] if filter_expr is not None else []
        if instrument_ids is not None:
            if not isinstance(instrument_ids, list):
                instrument_ids = [instrument_ids]
            if clean_instrument_keys:
                instrument_ids = list(set(map(clean_key, instrument_ids)))
            filters.append(ds.field(instrument_id_column).cast("string").isin(instrument_ids))
        if start is not None:
            filters.append(ds.field(ts_column) >= int(pd.Timestamp(start).to_datetime64()))
        if end is not None:
            filters.append(ds.field(ts_column) <= int(pd.Timestamp(end).to_datetime64()))
        return combine_filters(*filters)

    def query(
        self,
        cls: type,
        instrument_ids: Optional[List[str]] = None,
        start: Optional[Union[pd.Timestamp, str, int]] = None,
        end: Optional[Union[pd.Timestamp, str, int]] = None,
        **kwargs,
    ):
        combined_filter = self._build_filter_expression(
            instrument_ids=instrument_ids,
            start=start,
            end=end,
        )

        full_path = str(self._make_path(cls=cls))
        assert self.fs.exists(full_path) or self.fs.isdir(full_path)
        dataset = ds.dataset(full_path, partitioning="hive", filesystem=self.fs)
        # if projections:
        #     projected = {**{c: ds.field(c) for c in dataset.schema.names}, **projections}
        #     table_kwargs.update(columns=projected)
        table = dataset.to_table(filter=combined_filter, **kwargs)

        # TODO: Un-wired rust parquet reader
        # if isinstance(cls, QuoteTick):
        #     reader = ParquetReader(file_path=full_path, parquet_type=QuoteTick)  # noqa
        # elif isinstance(cls, TradeTick):
        #     reader = ParquetReader(file_path=full_path, parquet_type=TradeTick)  # noqa

        return self.parquet_table_to_nautilus_objects(table=table, cls=cls)

    @staticmethod
    def parquet_table_to_nautilus_objects(table: pa.Table, cls: type):
        dicts = dict_of_lists_to_list_of_dicts(table.to_pydict())
        if not dicts:
            return []
        data = ParquetSerializer.deserialize(cls=cls, chunk=dicts)
        return data

    def _make_path(self, cls: type) -> str:
        path: pathlib.Path = self.path / "data" / f"{class_to_filename(cls=cls)}.parquet"
        return str(resolve_path(path=path, fs=self.fs))

    def list_data_types(self):
        glob_path = resolve_path(self.path / "data" / "*.parquet", fs=self.fs)
        return [pathlib.Path(p).stem for p in self.fs.glob(glob_path)]

    def list_partitions(self, cls_type: type):
        assert isinstance(cls_type, type), "`cls_type` should be type, i.e. TradeTick"
        name = class_to_filename(cls_type)
        dataset = pq.ParquetDataset(
            resolve_path(self.path / f"{name}.parquet", fs=self.fs), filesystem=self.fs
        )
        partitions = {}
        for level in dataset.partitions.levels:
            partitions[level.name] = level.keys
        return partitions

    def list_backtests(self) -> List[str]:
        glob = resolve_path(self.path / "backtest" / "*.feather", fs=self.fs)
        return [p.stem for p in map(pathlib.Path, self.fs.glob(glob))]

    def list_live_runs(self) -> List[str]:
        glob = resolve_path(self.path / "live" / "*.feather", fs=self.fs)
        return [p.stem for p in map(pathlib.Path, self.fs.glob(glob))]

    def read_live_run(self, live_run_id: str, **kwargs):
        return self._read_feather_files(kind="live", run_id=live_run_id, **kwargs)

    def read_backtest(self, backtest_run_id: str, **kwargs):
        return self._read_feather_files(kind="backtest", run_id=backtest_run_id, **kwargs)

    def _read_feather_files(self, kind: str, run_id: str):
        raise NotImplementedError("Need to read nautilus objects from feather")
        # class_mapping: Dict[str, type] = {class_to_filename(cls): cls for cls in list_schemas()}
        # data = {}
        # glob_path = resolve_path(self.path / kind / f"{run_id}.feather" / "*.feather", fs=self.fs)
        # for path in [p for p in self.fs.glob(glob_path)]:
        #     cls_name = camel_to_snake_case(pathlib.Path(path).stem).replace("__", "_")
        #     # df = read_feather_file(path=path, fs=self.fs)
        #     # TODO
        #     # objs = read_feather_file()
        #     data[cls_name] = objs
        # return sorted(sum(data.values(), list()), key=lambda x: x.ts_init)


def read_feather_file(path: str, fs: fsspec.AbstractFileSystem = None):
    fs = fs or fsspec.filesystem("file")
    if not fs.exists(path):
        return
    try:
        with fs.open(path) as f:
            reader = pa.ipc.open_stream(f)
            return reader.read_pandas()
    except (pa.ArrowInvalid, FileNotFoundError):
        return


def combine_filters(*filters):
    filters = tuple(x for x in filters if x is not None)
    if len(filters) == 0:
        return
    elif len(filters) == 1:
        return filters[0]
    else:
        expr = filters[0]
        for f in filters[1:]:
            expr = expr & f
        return expr


def _should_use_windows_paths(fs: fsspec.filesystem) -> bool:
    # `Pathlib` will try and use Windows style paths even when an
    # `fsspec.filesystem` does not (memory, s3, etc).

    # We need to determine the case when we should use Windows paths, which is
    # when we are on Windows and using an `fsspec.filesystem` which is local.
    from fsspec.implementations.local import LocalFileSystem

    try:
        from fsspec.implementations.smb import SMBFileSystem
    except ImportError:
        SMBFileSystem = LocalFileSystem

    is_windows: bool = platform.system() == "Windows"
    is_windows_local_fs: bool = isinstance(fs, (LocalFileSystem, SMBFileSystem))
    return is_windows and is_windows_local_fs


def resolve_path(path: pathlib.Path, fs: fsspec.filesystem) -> str:
    if _should_use_windows_paths(fs=fs):
        return str(path)
    else:
        return path.as_posix()
