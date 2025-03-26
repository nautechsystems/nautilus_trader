# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from __future__ import annotations

from abc import ABC
from abc import ABCMeta
from abc import abstractmethod
from typing import Any

import pandas as pd

from nautilus_trader.core.data import Data
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import InstrumentClose
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.persistence.catalog.singleton import Singleton
from nautilus_trader.persistence.funcs import CUSTOM_DATA_PREFIX


class _CombinedMeta(Singleton, ABCMeta):
    pass


class BaseDataCatalog(ABC, metaclass=_CombinedMeta):
    """
    Provides a abstract base class for a queryable data catalog.
    """

    @classmethod
    @abstractmethod
    def from_env(cls) -> BaseDataCatalog:
        raise NotImplementedError

    @classmethod
    @abstractmethod
    def from_uri(cls, uri: str) -> BaseDataCatalog:
        raise NotImplementedError

    # -- QUERIES -----------------------------------------------------------------------------------

    @abstractmethod
    def query(
        self,
        data_cls: type,
        instrument_ids: list[str] | None = None,
        bar_types: list[str] | None = None,
        **kwargs: Any,
    ) -> list[Data]:
        raise NotImplementedError

    @abstractmethod
    def query_timestamp_bound(
        self,
        data_cls: type,
        instrument_id: str | None = None,
        bar_type: str | None = None,
        ts_column: str = "ts_init",
        is_last: bool = True,
    ) -> pd.Timestamp | None:
        raise NotImplementedError

    def _query_subclasses(
        self,
        base_cls: type,
        instrument_ids: list[str] | None = None,
        **kwargs: Any,
    ) -> list[Data]:
        objects = []

        for cls in base_cls.__subclasses__():
            try:
                objs = self.query(data_cls=cls, instrument_ids=instrument_ids, **kwargs)
                objects.extend(objs)
            except AssertionError as e:
                print(e)
                continue

        return objects

    def instruments(
        self,
        instrument_type: type | None = None,
        instrument_ids: list[str] | None = None,
        **kwargs: Any,
    ) -> list[Instrument]:
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

    def instrument_status(
        self,
        instrument_ids: list[str] | None = None,
        **kwargs: Any,
    ) -> list[InstrumentStatus]:
        return self.query(data_cls=InstrumentStatus, instrument_ids=instrument_ids, **kwargs)

    def instrument_closes(
        self,
        instrument_ids: list[str] | None = None,
        **kwargs: Any,
    ) -> list[InstrumentClose]:
        return self.query(data_cls=InstrumentClose, instrument_ids=instrument_ids, **kwargs)

    def order_book_deltas(
        self,
        instrument_ids: list[str] | None = None,
        batched: bool = False,
        **kwargs: Any,
    ) -> list[OrderBookDelta] | list[OrderBookDeltas]:
        data_cls = OrderBookDeltas if batched else OrderBookDelta

        return self.query(data_cls=data_cls, instrument_ids=instrument_ids, **kwargs)

    def order_book_depth10(
        self,
        instrument_ids: list[str] | None = None,
        **kwargs: Any,
    ) -> list[OrderBookDepth10]:
        return self.query(data_cls=OrderBookDepth10, instrument_ids=instrument_ids, **kwargs)

    def quote_ticks(
        self,
        instrument_ids: list[str] | None = None,
        **kwargs: Any,
    ) -> list[QuoteTick]:
        return self.query(data_cls=QuoteTick, instrument_ids=instrument_ids, **kwargs)

    def trade_ticks(
        self,
        instrument_ids: list[str] | None = None,
        **kwargs: Any,
    ) -> list[TradeTick]:
        return self.query(data_cls=TradeTick, instrument_ids=instrument_ids, **kwargs)

    def bars(
        self,
        bar_types: list[str] | None = None,
        **kwargs: Any,
    ) -> list[Bar]:
        return self.query(data_cls=Bar, bar_types=bar_types, **kwargs)

    def custom_data(
        self,
        cls: type,
        as_nautilus: bool = False,
        metadata: dict | None = None,
        **kwargs: Any,
    ) -> list[CustomData]:
        data = self.query(data_cls=cls, **kwargs)

        if as_nautilus:
            if data is None:
                return []

            return [CustomData(data_type=DataType(cls, metadata=metadata), data=d) for d in data]

        return data

    @abstractmethod
    def list_data_types(self) -> list[str]:
        raise NotImplementedError

    def list_generic_data_types(self) -> list[str]:
        data_types = self.list_data_types()

        return [
            n.replace(CUSTOM_DATA_PREFIX, "")
            for n in data_types
            if n.startswith(CUSTOM_DATA_PREFIX)
        ]

    @abstractmethod
    def list_backtest_runs(self) -> list[str]:
        raise NotImplementedError

    @abstractmethod
    def list_live_runs(self) -> list[str]:
        raise NotImplementedError

    @abstractmethod
    def read_live_run(self, instance_id: str, **kwargs: Any) -> list[str]:
        raise NotImplementedError

    @abstractmethod
    def read_backtest(self, instance_id: str, **kwargs: Any) -> list[str]:
        raise NotImplementedError
