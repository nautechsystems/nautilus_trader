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

from abc import ABC
from abc import ABCMeta
from abc import abstractmethod
from typing import Any, Callable, Dict, List, Optional, Union

import pandas as pd
import pyarrow as pa

from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.persistence.base import Singleton
from nautilus_trader.persistence.external.metadata import load_mappings
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.util import GENERIC_DATA_PREFIX
from nautilus_trader.serialization.arrow.util import dict_of_lists_to_list_of_dicts


class _CombinedMeta(Singleton, ABCMeta):  # noqa
    pass


class BaseDataCatalog(ABC, metaclass=_CombinedMeta):
    """
    Provides a abstract base class for a queryable data catalog.
    """

    @abstractmethod
    def from_env(cls):
        raise NotImplementedError

    @abstractmethod
    def from_uri(cls, uri):
        raise NotImplementedError

    # -- QUERIES -----------------------------------------------------------------------------------

    @abstractmethod
    def _query(
        self,
        cls: type,
        filter_expr: Optional[Callable] = None,
        instrument_ids: Optional[List[str]] = None,
        start: Optional[Any] = None,
        end: Optional[Any] = None,
        ts_column: str = "ts_init",
        raise_on_empty: bool = True,
        instrument_id_column="instrument_id",
        table_kwargs: Optional[Dict] = None,
        clean_instrument_keys: bool = True,
        as_dataframe: bool = True,
        projections: Optional[Dict] = None,
        **kwargs,
    ):
        raise NotImplementedError

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

    def query(
        self,
        cls: type,
        filter_expr: Optional[Callable] = None,
        instrument_ids: Optional[List[str]] = None,
        as_nautilus: bool = False,
        sort_columns: Optional[List[str]] = None,
        as_type: Optional[Dict] = None,
        **kwargs,
    ):
        if not is_nautilus_class(cls):
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

    @abstractmethod
    def _query_subclasses(
        self,
        base_cls: type,
        filter_expr: Optional[Callable] = None,
        instrument_ids: Optional[List[str]] = None,
        as_nautilus: bool = False,
        **kwargs,
    ):
        raise NotImplementedError

    def instruments(
        self,
        instrument_type: Optional[type] = None,
        instrument_ids: Optional[List[str]] = None,
        filter_expr: Optional[Callable] = None,
        as_nautilus: bool = False,
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
        self,
        instrument_ids: Optional[List[str]] = None,
        filter_expr: Optional[Callable] = None,
        as_nautilus: bool = False,
        **kwargs,
    ):
        return self.query(
            cls=InstrumentStatusUpdate,
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            as_nautilus=as_nautilus,
            sort_columns=["instrument_id", "ts_init"],
            **kwargs,
        )

    def trade_ticks(
        self,
        instrument_ids: Optional[List[str]] = None,
        filter_expr: Optional[Callable] = None,
        as_nautilus: bool = False,
        **kwargs,
    ):
        return self.query(
            cls=TradeTick,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            as_nautilus=as_nautilus,
            as_type={"price": float, "size": float},
            **kwargs,
        )

    def quote_ticks(
        self,
        instrument_ids: Optional[List[str]] = None,
        filter_expr: Optional[Callable] = None,
        as_nautilus: bool = False,
        **kwargs,
    ):
        return self.query(
            cls=QuoteTick,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            as_nautilus=as_nautilus,
            **kwargs,
        )

    def tickers(
        self,
        instrument_ids: Optional[List[str]] = None,
        filter_expr: Optional[Callable] = None,
        as_nautilus: bool = False,
        **kwargs,
    ):
        return self._query_subclasses(
            base_cls=Ticker,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            as_nautilus=as_nautilus,
            **kwargs,
        )

    def bars(
        self,
        instrument_ids: Optional[List[str]] = None,
        filter_expr: Optional[Callable] = None,
        as_nautilus: bool = False,
        **kwargs,
    ):
        return self._query_subclasses(
            base_cls=Bar,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            as_nautilus=as_nautilus,
            **kwargs,
        )

    def order_book_deltas(
        self,
        instrument_ids: Optional[List[str]] = None,
        filter_expr: Optional[Callable] = None,
        as_nautilus: bool = False,
        **kwargs,
    ):
        return self.query(
            cls=OrderBookData,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            as_nautilus=as_nautilus,
            **kwargs,
        )

    def generic_data(
        self,
        cls: type,
        filter_expr: Optional[Callable] = None,
        as_nautilus: bool = False,
        **kwargs,
    ):
        data = self._query(
            cls=cls,
            filter_expr=filter_expr,
            as_dataframe=not as_nautilus,
            **kwargs,
        )
        if as_nautilus:
            if data is None:
                return []
            return [GenericData(data_type=DataType(cls), data=d) for d in data]
        return data

    @abstractmethod
    def list_data_types(self):
        raise NotImplementedError

    def list_generic_data_types(self):
        data_types = self.list_data_types()
        return [
            n.replace(GENERIC_DATA_PREFIX, "")
            for n in data_types
            if n.startswith(GENERIC_DATA_PREFIX)
        ]

    @abstractmethod
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
