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

import fsspec
import pandas as pd
import pyarrow.dataset as ds
import pyarrow.parquet as pq
from pyarrow import ArrowInvalid

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.orderbook.data import OrderBookDeltas
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

    # ---- BACKTEST ---------------------------------------------------------------------------------------- #

    def setup_engine(
        self,
        engine: BacktestEngine,
        instruments,
        chunk_size=None,
        **kwargs,
    ) -> BacktestEngine:
        """
        Load data into a backtest engine.

        Parameters
        ----------
        engine : BacktestEngine
            The backtest engine to load data into.
        instruments : list[Instrument]
            The instruments to load data for.
        chunk_size : int
            The chunk size to return (used for streaming backtest).
            Use None for a loading all the data.
        kwargs : dict
            The kwargs passed to `self.load_backtest_data`.

        """
        # TODO(bm): Handle chunk size
        if chunk_size is not None:
            pass

        # Add instruments & data to engine
        for instrument in instruments:
            data = self.load_backtest_data(
                instrument_ids=[instrument.id.value],
                chunk_size=chunk_size,
                **kwargs,
            )
            engine.add_instrument(instrument)
            for name in data:
                if name == "trade_ticks" and data[name]:
                    engine.add_trade_tick_objects(instrument_id=instrument.id, data=data[name])
                elif name == "quote_ticks":
                    engine.add_quote_ticks(instrument_id=instrument.id, data=data[name])
                elif name == "order_book_deltas":
                    engine.add_order_book_data(data=data[name])
                elif name == "instrument_status_update" and data["instrument_status_update"]:
                    venue = data["instrument_status_update"][0].instrument_id.venue
                    engine.add_data(data=data[name], client_id=ClientId(venue.value))

        return engine

    # ---- QUERIES ---------------------------------------------------------------------------------------- #

    # def _load_chunked_backtest_data(self, name, query, instrument_ids, filters, chunk_size):
    #     """
    #     Stream chunked data from parquet dataset
    #
    #     :param name:
    #     :param query:
    #     :param instrument_ids:
    #     :param filters:
    #     :return:
    #     """
    #     # TODO - look at dask.dataframe.aggregate_row_groups for chunking solution
    #     dataset = query(instrument_ids=instrument_ids, filters=filters, return_dataset=True)
    #     ts_column_idx = ds.schema.names.index('ts_init')
    #     for piece in ds.pieces:
    #         meta = piece.get_metadata()
    #         for i in range(meta.num_row_groups):
    #             rg = meta.row_group(i)
    #             rg_size = rg.total_byte_size
    #             ts_stats = rg.column(ts_column_idx).statistics
    #     return

    def load_backtest_data(
        self,
        instrument_ids=None,
        start_timestamp=None,
        end_timestamp=None,
        order_book_deltas=True,
        trade_ticks=True,
        quote_ticks=False,
        instrument_status_events=True,
        chunk_size=None,
    ):
        """
        Load backtest data objects from the catalogue.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instruments to load data for.
        start_timestamp : datetime
            The starting timestamp of the data to load.
        end_timestamp : datetime
            The ending timestamp of the data to load.
        order_book_deltas : bool
            If order book deltas should be loaded.
        trade_ticks : bool
            If trade ticks should be loaded.
        quote_ticks : bool
            If quote ticks should be loaded.
        instrument_status_events : bool
            If instrument status events should be loaded.
        chunk_size : int
            The chunk size to return (used for streaming backtest).
            Use None for a loading all the data.

        """
        assert instrument_ids is None or isinstance(
            instrument_ids, list
        ), "instrument_ids must be list"
        queries = [
            ("order_book_deltas", order_book_deltas, self.order_book_deltas, {}),
            ("trade_ticks", trade_ticks, self.trade_ticks, {}),
            (
                "instrument_status_update",
                instrument_status_events,
                self.instrument_status_updates,
                {},
            ),
            ("quote_ticks", quote_ticks, self.quote_ticks, {}),
        ]
        data = {}

        if chunk_size:
            raise KeyError
            # data[name] = self._load_chunked_backtest_data(
            #     chunk_size=chunk_size, name=name, query=query, instrument_ids=instrument_ids, filters=filters,
            # )

        for name, to_load, query, kw in queries:
            if to_load:
                data[name] = query(
                    instrument_ids=instrument_ids,
                    as_nautilus=True,
                    start=start_timestamp,
                    end=end_timestamp,
                    raise_on_empty=False,
                    **kw,
                )

        return data

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
            instrument_types = (instrument_type,)
        else:
            instrument_types = Instrument.__subclasses__()

        dfs = []
        for ins_type in instrument_types:
            try:
                df = self._query(
                    path=f"data/{camel_to_snake_case(ins_type.__name__)}.parquet",
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
            for ins_type, df in zip(instrument_types, dfs):
                if df is None or (isinstance(df, pd.DataFrame) and df.empty):
                    continue
                objects.extend(self._make_objects(df=df, cls=ins_type))
            return objects

    def instrument_status_updates(
        self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs
    ):
        df = self._query(
            "data/instrument_status_update.parquet",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        df = df.sort_values(["instrument_id", "ts_event"]).drop_duplicates(
            subset=[c for c in df.columns if c not in ("event_id",)], keep="last"
        )
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=InstrumentStatusUpdate)

    def trade_ticks(self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs):
        df = self._query(
            "data/trade_tick.parquet",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df.astype({"price": float, "size": float})
        return self._make_objects(df=df, cls=TradeTick)

    def quote_ticks(self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs):
        df = self._query(
            "data/quote_tick.parquet",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=QuoteTick)

    def order_book_deltas(self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs):
        df = self._query(
            "data/order_book_data.parquet",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=OrderBookDeltas)

    def generic_data(self, cls, filter_expr=None, as_nautilus=False, **kwargs):
        df = self._query(
            path=f"data/{class_to_filename(cls)}",
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return [
            GenericData(data_type=DataType(cls), data=d) for d in self._make_objects(df=df, cls=cls)
        ]

    def query(self, cls, filter_expr=None, instrument_ids=None, as_nautilus=False, **kwargs):
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
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=cls)

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
