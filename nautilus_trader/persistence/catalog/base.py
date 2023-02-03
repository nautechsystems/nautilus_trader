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

from abc import ABC
from abc import ABCMeta
from abc import abstractmethod
from typing import Optional

from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.persistence.external.util import Singleton
from nautilus_trader.serialization.arrow.util import GENERIC_DATA_PREFIX


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

    def query(
        self,
        cls: type,
        instrument_ids: Optional[list[str]] = None,
        **kwargs,
    ):
        raise NotImplementedError

    def _query_subclasses(
        self,
        base_cls: type,
        instrument_ids: Optional[list[str]] = None,
        **kwargs,
    ):
        objects = []
        for cls in base_cls.__subclasses__():
            try:
                objs = self.query(cls=cls, instrument_ids=instrument_ids, **kwargs)
                objects.extend(objs)
            except AssertionError:
                continue
        return objects

    def instruments(
        self,
        instrument_type: Optional[type] = None,
        instrument_ids: Optional[list[str]] = None,
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
            instrument_id_column="id",
            **kwargs,
        )

    def instrument_status_updates(
        self,
        instrument_ids: Optional[list[str]] = None,
        **kwargs,
    ):
        return self.query(
            cls=InstrumentStatusUpdate,
            instrument_ids=instrument_ids,
            **kwargs,
        )

    def trade_ticks(
        self,
        instrument_ids: Optional[list[str]] = None,
        **kwargs,
    ):
        return self.query(
            cls=TradeTick,
            instrument_ids=instrument_ids,
            **kwargs,
        )

    def quote_ticks(
        self,
        instrument_ids: Optional[list[str]] = None,
        **kwargs,
    ):
        return self.query(
            cls=QuoteTick,
            instrument_ids=instrument_ids,
            **kwargs,
        )

    def tickers(
        self,
        instrument_ids: Optional[list[str]] = None,
        **kwargs,
    ):
        return self._query_subclasses(
            base_cls=Ticker,
            instrument_ids=instrument_ids,
            **kwargs,
        )

    def bars(
        self,
        instrument_ids: Optional[list[str]] = None,
        **kwargs,
    ):
        return self.query(
            cls=Bar,
            instrument_ids=instrument_ids,
            **kwargs,
        )

    def order_book_deltas(
        self,
        instrument_ids: Optional[list[str]] = None,
        **kwargs,
    ):
        return self.query(
            cls=OrderBookData,
            instrument_ids=instrument_ids,
            **kwargs,
        )

    def generic_data(
        self,
        cls: type,
        as_nautilus: bool = False,
        metadata: Optional[dict] = None,
        **kwargs,
    ):
        data = self.query(cls=cls, **kwargs)
        if as_nautilus:
            if data is None:
                return []
            return [GenericData(data_type=DataType(cls, metadata=metadata), data=d) for d in data]
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
    def list_backtests(self) -> list[str]:
        raise NotImplementedError

    @abstractmethod
    def list_live_runs(self) -> list[str]:
        raise NotImplementedError

    @abstractmethod
    def read_live_run(self, live_run_id: str, **kwargs):
        raise NotImplementedError

    @abstractmethod
    def read_backtest(self, backtest_run_id: str, **kwargs):
        raise NotImplementedError
