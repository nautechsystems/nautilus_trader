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

import os
import pathlib
from typing import Dict, List, Optional, Union

import fsspec
import pandas as pd
import pyarrow as pa
import pyarrow.dataset as ds
import pyarrow.parquet as pq
from pyarrow import ArrowInvalid

from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.persistence.external.metadata import load_mappings
from nautilus_trader.persistence.streaming import read_feather
from nautilus_trader.persistence.util import Singleton
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.serializer import list_schemas
from nautilus_trader.serialization.arrow.util import GENERIC_DATA_PREFIX
from nautilus_trader.serialization.arrow.util import camel_to_snake_case
from nautilus_trader.serialization.arrow.util import class_to_filename
from nautilus_trader.serialization.arrow.util import clean_key
from nautilus_trader.serialization.arrow.util import dict_of_lists_to_list_of_dicts


class DataCatalog(metaclass=Singleton):
    """
    Provides a queryable data catalogue.
    """

    def __init__(
        self,
        path: str,
        fs_protocol: str = "file",
        fs_storage_options: Optional[Dict] = None,
    ):
        """
        Initialize a new instance of the ``DataCatalog`` class.

        Parameters
        ----------
        path : str
            The root path to the data.
        fs_protocol : str
            The file system protocol to use.
        fs_storage_options : Dict, optional
            The fs storage options.

        """
        self.fs: fsspec.AbstractFileSystem = fsspec.filesystem(
            fs_protocol, **(fs_storage_options or {})
        )
        self.path = pathlib.Path(path)

    @classmethod
    def from_env(cls):
        return cls.from_uri(uri=os.environ["NAUTILUS_CATALOG"])

    @classmethod
    def from_uri(cls, uri):
        if "://" not in uri:
            uri = "file://" + uri
        protocol, path = uri.split("://")
        return cls(path=path, fs_protocol=protocol)

    # ---- QUERIES ---------------------------------------------------------------------------------------- #

    def _query(
        self,
        cls,
        filter_expr=None,
        instrument_ids=None,
        start=None,
        end=None,
        ts_column="ts_event",
        raise_on_empty=True,
        instrument_id_column="instrument_id",
        table_kwargs: Optional[Dict] = None,
        clean_instrument_keys=True,
        as_dataframe=True,
        **kwargs,
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

        full_path = self._make_path(cls=cls)
        if not (self.fs.exists(full_path) or self.fs.isdir(full_path)):
            if raise_on_empty:
                raise FileNotFoundError(f"protocol={self.fs.protocol}, path={full_path}")
            else:
                return pd.DataFrame() if as_dataframe else None

        dataset = ds.dataset(full_path, partitioning="hive", filesystem=self.fs)
        table = dataset.to_table(filter=combine_filters(*filters), **(table_kwargs or {}))
        mappings = self.load_inverse_mappings(path=full_path)
        if as_dataframe:
            return self._handle_table_dataframe(
                table=table, mappings=mappings, raise_on_empty=raise_on_empty, **kwargs
            )
        else:
            return self._handle_table_nautilus(table=table, cls=cls, mappings=mappings)

    def load_inverse_mappings(self, path):
        mappings = load_mappings(fs=self.fs, path=path)
        for key in mappings:
            mappings[key] = {v: k for k, v in mappings[key].items()}
        return mappings

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
            kw = [
                f"{k}={local_vars[k]}"
                for k in ("path", "filter_expr", "instrument_ids", "start", "end")
            ]
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
        return f"{self.path}/data/{class_to_filename(cls=cls)}.parquet"

    def query(
        self,
        cls: type,
        filter_expr=None,
        instrument_ids=None,
        as_nautilus=False,
        sort_columns: Optional[List[str]] = None,
        as_type: Optional[Dict] = None,
        **kwargs,
    ):
        if not is_nautilus_class(cls=cls):
            # Special handling for generic data
            return self.generic_data(
                cls=cls,
                filter_expr=filter_expr,
                instrument_ids=instrument_ids,
                as_nautilus=as_nautilus,
                **kwargs,
            )
        return self._query(
            cls=cls,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            sort_columns=sort_columns,
            as_type=as_type,
            as_dataframe=not as_nautilus,
            **kwargs,
        )
        # if as_nautilus:
        #     return self._make_objects(df=df, cls=cls)
        # else:
        #     return df

    def _query_subclasses(
        self,
        base_cls: type,
        filter_expr=None,
        instrument_ids=None,
        as_nautilus=False,
        **kwargs,
    ):
        subclasses = [base_cls] + base_cls.__subclasses__()

        dfs = []
        for cls in subclasses:
            try:
                df = self._query(
                    cls=cls,
                    filter_expr=filter_expr,
                    instrument_ids=instrument_ids,
                    raise_on_empty=False,
                    as_dataframe=not as_nautilus,
                    **kwargs,
                )
                dfs.append(df)
            except ArrowInvalid as e:
                # If we're using a `filter_expr` here, there's a good chance this error is using a filter that is
                # specific to one set of instruments and not the others, so we ignore it. If not; raise
                if filter_expr is not None:
                    continue
                else:
                    raise e

        if not as_nautilus:
            return pd.concat([df for df in dfs if df is not None])
        else:
            objects = [o for objs in filter(None, dfs) for o in objs]
            return objects

    def instruments(
        self,
        instrument_type=None,
        instrument_ids=None,
        filter_expr=None,
        as_nautilus=False,
        **kwargs,
    ):
        if instrument_type is not None:
            assert isinstance(instrument_type, type)
            base_cls = instrument_type
        else:
            base_cls = Instrument

        return self._query_subclasses(
            base_cls=base_cls,
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            as_nautilus=as_nautilus,
            instrument_id_column="id",
            clean_instrument_keys=False,
            **kwargs,
        )

    def instrument_status_updates(
        self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs
    ):
        return self.query(
            cls=InstrumentStatusUpdate,
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            as_nautilus=as_nautilus,
            sort_columns=["instrument_id", "ts_init"],
            **kwargs,
        )

    def trade_ticks(self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs):
        return self.query(
            cls=TradeTick,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            as_nautilus=as_nautilus,
            as_type={"price": float, "size": float},
            **kwargs,
        )

    def quote_ticks(self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs):
        return self.query(
            cls=QuoteTick,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            as_nautilus=as_nautilus,
            **kwargs,
        )

    def ticker(self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs):
        return self._query_subclasses(
            base_cls=Ticker,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            as_nautilus=as_nautilus,
            **kwargs,
        )

    def order_book_deltas(self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs):
        return self.query(
            cls=OrderBookData,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            as_nautilus=as_nautilus,
            **kwargs,
        )

    def generic_data(self, cls, filter_expr=None, as_nautilus=False, **kwargs):
        data = self._query(cls=cls, filter_expr=filter_expr, as_dataframe=not as_nautilus, **kwargs)
        if as_nautilus:
            return [GenericData(data_type=DataType(cls), data=d) for d in data]
        return data

    def list_data_types(self):
        return [pathlib.Path(p).stem for p in self.fs.glob(f"{self.path}/data/*.parquet")]

    def list_generic_data_types(self):
        data_types = self.list_data_types()
        return [
            n.replace(GENERIC_DATA_PREFIX, "")
            for n in data_types
            if n.startswith(GENERIC_DATA_PREFIX)
        ]

    def list_partitions(self, cls_type: type):
        assert isinstance(cls_type, type), "`cls_type` should be type, i.e. TradeTick"
        name = class_to_filename(cls_type)
        dataset = pq.ParquetDataset(self.path / f"{name}.parquet", filesystem=self.fs)
        partitions = {}
        for level in dataset.partitions.levels:
            partitions[level.name] = level.keys
        return partitions

    def _read_feather(self, kind: str, run_id: str, raise_on_failed_deserialize: bool = False):
        class_mapping: Dict[str, type] = {class_to_filename(cls): cls for cls in list_schemas()}
        data = {}
        for path in [p for p in self.fs.glob(f"{self.path}/{kind}/{run_id}.feather/*.feather")]:
            cls_name = camel_to_snake_case(pathlib.Path(path).stem).replace("__", "_")
            df = read_feather(path=path, fs=self.fs)
            if df is None:
                print(f"No data for {cls_name}")
                continue
            # Apply post read fixes
            try:
                objs = self._handle_table_nautilus(
                    table=df, cls=class_mapping[cls_name], mappings={}
                )
                data[cls_name] = objs
            except Exception as e:
                if raise_on_failed_deserialize:
                    raise
                print(f"Failed to deserialize {cls_name}: {e}")
        return sorted(sum(data.values(), list()), key=lambda x: x.ts_init)

    def read_live_run(self, live_run_id: str, **kwargs):
        return self._read_feather(kind="live", run_id=live_run_id, **kwargs)

    def read_backtest(self, backtest_run_id: str, **kwargs):
        return self._read_feather(kind="backtest", run_id=backtest_run_id, **kwargs)


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
