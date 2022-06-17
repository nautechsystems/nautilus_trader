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

import datetime
from abc import ABC
from abc import ABCMeta
from abc import abstractclassmethod
from abc import abstractmethod
from typing import Callable, Dict, List, Optional, Union

import pandas as pd
import pyarrow as pa

from nautilus_trader.persistence.catalog.base import BaseDataCatalog
from nautilus_trader.persistence.external.metadata import load_mappings
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.serializer import list_schemas
from nautilus_trader.serialization.arrow.util import camel_to_snake_case
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

    @abstractclassmethod
    def from_env(cls):
        raise NotImplementedError

    @abstractclassmethod
    def from_uri(cls, uri):
        raise NotImplementedError

    # -- QUERIES -----------------------------------------------------------------------------------

    @abstractmethod
    def _query(
        self,
        cls: type,
        filter_expr: Optional[Callable] = None,
        instrument_ids=None,
        start=None,
        end=None,
        ts_column="ts_init",
        raise_on_empty: bool = True,
        instrument_id_column="instrument_id",
        table_kwargs: Optional[Dict] = None,
        clean_instrument_keys: bool = True,
        as_dataframe: bool = True,
        projections: Optional[Dict] = None,
        **kwargs,
    ):
        raise NotImplementedError

    @staticmethod
    def _handle_table_dataframe(
        table: pa.Table,
        mappings: Optional[Dict],
        raise_on_empty: bool = True,
        sort_columns: Optional[List] = None,
        as_type: Optional[Dict] = None,
    ):
        df = table.to_pandas().drop_duplicates()
        for col in mappings:
            df.loc[:, col] = df[col].map(mappings[col])

        if df.empty and raise_on_empty:
            local_vars = dict(locals())
            kw = [f"{k}={local_vars[k]}" for k in ("filter_expr", "instrument_ids", "start", "end")]
            raise ValueError(f"Data empty for {kw}")
        if sort_columns:
            df = df.sort_values(sort_columns)
        if as_type:
            df = df.astype(as_type)
        return df

    @staticmethod
    def _handle_table_nautilus(
        table: Union[pa.Table, pd.DataFrame], cls: type, mappings: Optional[Dict]
    ):
        if isinstance(table, pa.Table):
            dicts = dict_of_lists_to_list_of_dicts(table.to_pydict())
        elif isinstance(table, pd.DataFrame):
            dicts = table.to_dict("records")
        else:
            raise TypeError(
                f"`table` was {type(table)}, expected `pyarrow.Table` or `pandas.DataFrame`"
            )
        if not dicts:
            return []
        for key, maps in mappings.items():
            for d in dicts:
                if d[key] in maps:
                    d[key] = maps[d[key]]
        data = ParquetSerializer.deserialize(cls=cls, chunk=dicts)
        return data

    def _make_path(self, cls: type) -> str:
        path: pathlib.Path = self.path / "data" / f"{class_to_filename(cls=cls)}.parquet"
        return str(resolve_path(path=path, fs=self.fs))

    def _query_subclasses(
        self,
        base_cls: type,
        filter_expr: Optional[Callable] = None,
        instrument_ids=None,
        as_nautilus: bool = False,
        **kwargs,
    ):
        raise NotImplementedError

    def list_data_types(self):
        raise NotImplementedError

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
        raise NotImplementedError

    @abstractmethod
    def list_live_runs(self) -> List[str]:
        raise NotImplementedError

    @abstractmethod
    def read_live_run(self, live_run_id: str, **kwargs):
        raise NotImplementedError

    @abstractmethod
    def read_backtest(self, backtest_run_id: str, **kwargs):
        raise NotImplementedError

    @abstractmethod
    def exists(self, instrument_id: InstrumentId, kind: str, date: datetime.date) -> bool:
        raise NotImplementedError
