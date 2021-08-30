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
from typing import Dict, List, Optional

import fsspec
import pandas as pd
import pyarrow.dataset as ds
import pyarrow.parquet as pq
from pyarrow import ArrowInvalid

from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.persistence.external.metadata import load_mappings
from nautilus_trader.persistence.util import Singleton
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.util import GENERIC_DATA_PREFIX
from nautilus_trader.serialization.arrow.util import camel_to_snake_case
from nautilus_trader.serialization.arrow.util import class_to_filename
from nautilus_trader.serialization.arrow.util import clean_key
from nautilus_trader.serialization.arrow.util import is_nautilus_class


class DataCatalog(metaclass=Singleton):
    PROCESSED_FILES_FN = ".processed_raw_files.json"
    PARTITION_MAPPINGS_FN = "_partition_mappings.json"

    def __init__(self, path: str, fs_protocol: str = "file"):
        """
        Provides a queryable data catalogue.

        Parameters
        ----------
        path : str
            The root path to the data.
        fs_protocol : str
            The file system protocol to use.
        """
        self.fs = fsspec.filesystem(fs_protocol)
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
        path,
        filter_expr=None,
        instrument_ids=None,
        start=None,
        end=None,
        ts_column="ts_event",
        raise_on_empty=True,
    ):
        filters = [filter_expr] if filter_expr is not None else []
        if instrument_ids is not None:
            if not isinstance(instrument_ids, list):
                instrument_ids = [instrument_ids]
            filters.append(
                ds.field("instrument_id").isin(list(set(map(clean_key, instrument_ids))))
            )
        if start is not None:
            filters.append(ds.field(ts_column) >= int(pd.Timestamp(start).to_datetime64()))
        if end is not None:
            filters.append(ds.field(ts_column) <= int(pd.Timestamp(end).to_datetime64()))

        full_path = str(self.path.joinpath(path))
        if not (self.fs.exists(full_path) or self.fs.isdir(full_path)):
            if raise_on_empty:
                raise FileNotFoundError(f"protocol={self.fs.protocol}, path={full_path}")
            else:
                return pd.DataFrame()

        dataset = ds.dataset(full_path, partitioning="hive", filesystem=self.fs)
        table = dataset.to_table(filter=combine_filters(*filters))
        df = table.to_pandas().drop_duplicates()
        mappings = load_mappings(fs=self.fs, path=full_path)
        for col in mappings:
            df.loc[:, col] = df[col].map({v: k for k, v in mappings[col].items()})

        if df.empty and raise_on_empty:
            local_vars = dict(locals())
            kw = [
                f"{k}={local_vars[k]}"
                for k in ("path", "filter_expr", "instrument_ids", "start", "end")
            ]
            raise ValueError(f"Data empty for {kw}")
        return df

    @staticmethod
    def _make_objects(df, cls):
        if df is None:
            return []
        return ParquetSerializer.deserialize(cls=cls, chunk=df.to_dict("records"))

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
        path = f"{class_to_filename(cls)}.parquet"
        if path.startswith(GENERIC_DATA_PREFIX):
            # Special handling for generic data
            return self.generic_data(
                cls=cls,
                filter_expr=filter_expr,
                instrument_ids=instrument_ids,
                as_nautilus=as_nautilus,
                **kwargs,
            )
        df = self._query(
            path=f"data/{path}",
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            **kwargs,
        )
        if as_nautilus:
            return self._make_objects(df=df, cls=cls)
        else:
            if sort_columns:
                df = df.sort_values(sort_columns)
            if as_type:
                df = df.astype(as_type)
            return df

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
                    path=f"data/{class_to_filename(cls)}.parquet",
                    filter_expr=filter_expr,
                    instrument_ids=instrument_ids,
                    raise_on_empty=False,
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
            objects = []
            for cls, df in zip(subclasses, dfs):
                if df is None or (isinstance(df, pd.DataFrame) and df.empty):
                    continue
                objects.extend(self._make_objects(df=df, cls=cls))
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
        data = self.query(cls=cls, filter_expr=filter_expr, as_nautilus=as_nautilus, **kwargs)
        if as_nautilus:
            return [GenericData(data_type=DataType(cls), data=d) for d in data]
        return data

    def list_data_types(self):
        return [p.stem for p in self.path.glob("*.parquet")]

    def list_generic_data_types(self):
        data_types = self.list_data_types()
        return [
            n.replace(GENERIC_DATA_PREFIX, "")
            for n in data_types
            if n.startswith(GENERIC_DATA_PREFIX)
        ]

    def list_partitions(self, cls_type):
        assert isinstance(cls_type, type), "`cls_type` should be type, ie TradeTick"
        prefix = GENERIC_DATA_PREFIX if not is_nautilus_class(cls_type) else ""
        name = prefix + camel_to_snake_case(cls_type.__name__)
        dataset = pq.ParquetDataset(self.path / f"{name}.parquet", filesystem=self.fs)
        partitions = {}
        for level in dataset.partitions.levels:
            partitions[level.name] = level.keys
        return partitions


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
