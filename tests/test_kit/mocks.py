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

import inspect
import os
from datetime import datetime
from functools import partial
from typing import Dict, Generator, List, Optional

import pandas as pd
from fsspec.implementations.memory import MemoryFileSystem

from nautilus_trader.accounting.accounts.base import Account
from nautilus_trader.cache.database import CacheDatabase
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.execution.client import ExecutionClient
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.position import Position
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.persistence.external.readers import Reader
from nautilus_trader.persistence.util import clear_singleton_instances
from nautilus_trader.trading.filters import NewsEvent
from nautilus_trader.trading.strategy import TradingStrategy


class ObjectStorer:
    """
    A test class which stores objects to assist with test assertions.
    """

    def __init__(self):
        self.count = 0
        self._store = []

    def get_store(self) -> list:
        """
        Return the list or stored objects.

        Returns
        -------
        list[Object]

        """
        return self._store

    def store(self, obj) -> None:
        """Store the given object.

        Parameters
        ----------
        obj : object
            The object to store.

        """
        self.count += 1
        self._store.append(obj)

    def store_2(self, obj1, obj2) -> None:
        """Store the given objects as a tuple.

        Parameters
        ----------
        obj1 : object
            The first object to store.
        obj2 : object
            The second object to store.

        """
        self.store((obj1, obj2))


class MockActor(Actor):
    """
    Provides a mock actor for testing.
    """

    def __init__(self, config: ActorConfig = None):
        super().__init__(config)

        self.object_storer = ObjectStorer()

        self.calls: List[str] = []

    def on_start(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_stop(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_resume(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_reset(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_dispose(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_degrade(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_fault(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_instrument(self, instrument) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(instrument)

    def on_ticker(self, ticker):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(ticker)

    def on_quote_tick(self, tick):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(tick)

    def on_trade_tick(self, tick) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(tick)

    def on_bar(self, bar) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(bar)

    def on_data(self, data) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(data)

    def on_strategy_data(self, data) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(data)

    def on_event(self, event) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(event)


class MockStrategy(TradingStrategy):
    """
    Provides a mock trading strategy for testing.

    Parameters
    ----------
    bar_type : BarType
        The bar type for the strategy.
    """

    def __init__(self, bar_type: BarType):
        super().__init__()

        self.object_storer = ObjectStorer()
        self.bar_type = bar_type

        self.ema1 = ExponentialMovingAverage(10)
        self.ema2 = ExponentialMovingAverage(20)

        self.position_id: Optional[PositionId] = None

        self.calls: List[str] = []

    def on_start(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.register_indicator_for_bars(self.bar_type, self.ema1)
        self.register_indicator_for_bars(self.bar_type, self.ema2)

    def on_instrument(self, instrument) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(instrument)

    def on_ticker(self, ticker):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(ticker)

    def on_quote_tick(self, tick):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(tick)

    def on_trade_tick(self, tick) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(tick)

    def on_bar(self, bar) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(bar)

        if bar.type != self.bar_type:
            return

        if self.ema1.value > self.ema2.value:
            buy_order = self.order_factory.market(
                self.bar_type.instrument_id,
                OrderSide.BUY,
                100000,
            )

            self.submit_order(buy_order)
            self.position_id = buy_order.client_order_id
        elif self.ema1.value < self.ema2.value:
            sell_order = self.order_factory.market(
                self.bar_type.instrument_id,
                OrderSide.SELL,
                100000,
            )

            self.submit_order(sell_order)
            self.position_id = sell_order.client_order_id

    def on_data(self, data) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(data)

    def on_strategy_data(self, data) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(data)

    def on_event(self, event) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(event)

    def on_stop(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_resume(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_reset(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_save(self) -> Dict[str, bytes]:
        self.calls.append(inspect.currentframe().f_code.co_name)
        return {"UserState": b"1"}

    def on_load(self, state: Dict[str, bytes]) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(state)

    def on_dispose(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)


class KaboomActor(Actor):
    """
    Provides a mock actor where every called method blows up.
    """

    def __init__(self):
        super().__init__()

        self._explode_on_start = True
        self._explode_on_stop = True

    def set_explode_on_start(self, setting) -> None:
        self._explode_on_start = setting

    def set_explode_on_stop(self, setting) -> None:
        self._explode_on_stop = setting

    def on_start(self) -> None:
        if self._explode_on_start:
            raise RuntimeError(f"{self} BOOM!")

    def on_stop(self) -> None:
        if self._explode_on_stop:
            raise RuntimeError(f"{self} BOOM!")

    def on_resume(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_reset(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_dispose(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_degrade(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_fault(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_instrument(self, instrument) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_quote_tick(self, tick) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_trade_tick(self, tick) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_bar(self, bar) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_data(self, data) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_event(self, event) -> None:
        raise RuntimeError(f"{self} BOOM!")


class KaboomStrategy(TradingStrategy):
    """
    Provides a mock trading strategy where every called method blows up.
    """

    def __init__(self):
        super().__init__()

        self._explode_on_start = True
        self._explode_on_stop = True

    def set_explode_on_start(self, setting) -> None:
        self._explode_on_start = setting

    def set_explode_on_stop(self, setting) -> None:
        self._explode_on_stop = setting

    def on_start(self) -> None:
        if self._explode_on_start:
            raise RuntimeError(f"{self} BOOM!")

    def on_stop(self) -> None:
        if self._explode_on_stop:
            raise RuntimeError(f"{self} BOOM!")

    def on_resume(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_reset(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_save(self) -> Dict[str, bytes]:
        raise RuntimeError(f"{self} BOOM!")

    def on_load(self, state: Dict[str, bytes]) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_dispose(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_instrument(self, instrument) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_quote_tick(self, tick) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_trade_tick(self, tick) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_bar(self, bar) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_data(self, data) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_event(self, event) -> None:
        raise RuntimeError(f"{self} BOOM!")


class MockExecutionClient(ExecutionClient):
    """
    Provides a mock execution client for testing.

    The client will append all method calls to the calls list.

    Parameters
    ----------
    client_id : ClientId
        The client ID.
    venue : Venue, optional
        The client venue. If multi-venue then can be ``None``.
    account_type : AccountType
        The account type for the client.
    base_currency : Currency, optional
        The account base currency for the client. Use ``None`` for multi-currency accounts.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client
    clock : Clock
        The clock for the client.
    logger : Logger
        The logger for the client.
    """

    def __init__(
        self,
        client_id,
        venue,
        account_type,
        base_currency,
        msgbus,
        cache,
        clock,
        logger,
        config=None,
    ):
        super().__init__(
            client_id=client_id,
            venue=venue,
            oms_type=OMSType.HEDGING,
            account_type=account_type,
            base_currency=base_currency,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self.calls = []
        self.commands = []

    def _start(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self._set_connected()

    def _stop(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self._set_connected(False)

    def _reset(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def _dispose(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    # -- COMMANDS ----------------------------------------------------------------------------------

    def account_inquiry(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_order_list(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def modify_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def cancel_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)


class MockLiveExecutionClient(LiveExecutionClient):
    """
    Provides a mock execution client for testing.

    The client will append all method calls to the calls list.

    Parameters
    ----------
    client_id : ClientId
        The client ID.
    venue : Venue, optional
        The client venue. If multi-venue then can be ``None``.
    account_type : AccountType
        The account type for the client.
    base_currency : Currency, optional
        The account base currency for the client. Use ``None`` for multi-currency accounts.
    instrument_provider : InstrumentProvider
        The instrument provider for the client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.
    logger : Logger
        The logger for the client.
    """

    def __init__(
        self,
        loop,
        client_id,
        venue,
        account_type,
        base_currency,
        instrument_provider,
        msgbus,
        cache,
        clock,
        logger,
    ):
        super().__init__(
            loop=loop,
            client_id=client_id,
            venue=venue,
            oms_type=OMSType.HEDGING,
            account_type=account_type,
            base_currency=base_currency,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._set_account_id(AccountId(client_id.value, "001"))
        self._order_status_reports: Dict[VenueOrderId, OrderStatusReport] = {}
        self._trades_reports: Dict[VenueOrderId, List[TradeReport]] = {}
        self._position_status_reports: Dict[InstrumentId, List[PositionStatusReport]] = {}

        self.calls = []
        self.commands = []

    def add_order_status_report(self, report: OrderStatusReport) -> None:
        self._order_status_reports[report.venue_order_id] = report

    def add_trade_reports(self, venue_order_id: VenueOrderId, trades: List[TradeReport]) -> None:
        self._trades_reports[venue_order_id] = trades

    def add_position_status_report(self, report: PositionStatusReport) -> None:
        if report.instrument_id not in self._position_status_reports:
            self._position_status_reports[report.instrument_id] = []
        self._position_status_reports[report.instrument_id].append(report)

    def dispose(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def reset(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    # -- COMMANDS ----------------------------------------------------------------------------------

    def account_inquiry(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_order_list(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def modify_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def cancel_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    # -- EXECUTION REPORTS -------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
    ) -> Optional[OrderStatusReport]:
        self.calls.append(inspect.currentframe().f_code.co_name)

        return self._order_status_reports.get(venue_order_id)

    async def generate_order_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
        open_only: bool = False,
    ) -> List[OrderStatusReport]:
        self.calls.append(inspect.currentframe().f_code.co_name)

        reports = []
        for _, report in self._order_status_reports.items():
            reports.append(report)

        if instrument_id is not None:
            reports = [r for r in reports if r.instrument_id == instrument_id]

        if start is not None:
            reports = [r for r in reports if r.ts_accepted >= start]

        if end is not None:
            reports = [r for r in reports if r.ts_accepted <= end]

        return reports

    async def generate_trade_reports(
        self,
        instrument_id: InstrumentId = None,
        venue_order_id: VenueOrderId = None,
        start: datetime = None,
        end: datetime = None,
    ) -> List[TradeReport]:
        self.calls.append(inspect.currentframe().f_code.co_name)

        if venue_order_id is not None:
            trades = self._trades_reports.get(venue_order_id, [])
        else:
            trades = []
            for t_list in self._trades_reports.values():
                trades = [*trades, *t_list]

        if instrument_id is not None:
            trades = [t for t in trades if t.instrument_id == instrument_id]

        if start is not None:
            trades = [t for t in trades if t.ts_event >= start]

        if end is not None:
            trades = [t for t in trades if t.ts_event <= end]

        return trades

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: datetime = None,
        end: datetime = None,
    ) -> List[PositionStatusReport]:
        self.calls.append(inspect.currentframe().f_code.co_name)

        if instrument_id is not None:
            reports = self._position_status_reports.get(instrument_id, [])
        else:
            reports = []
            for p_list in self._position_status_reports.values():
                reports = [*reports, *p_list]

        if start is not None:
            reports = [r for r in reports if r.ts_event >= start]

        if end is not None:
            reports = [r for r in reports if r.ts_event <= end]

        return reports


class MockCacheDatabase(CacheDatabase):
    """
    Provides a mock cache database for testing.

    Parameters
    ----------
    logger : Logger
        The logger for the database.
    """

    def __init__(self, logger: Logger):
        super().__init__(logger)

        self.currencies: Dict[str, Currency] = {}
        self.instruments: Dict[InstrumentId, Instrument] = {}
        self.accounts: Dict[AccountId, Account] = {}
        self.orders: Dict[ClientOrderId, Order] = {}
        self.positions: Dict[PositionId, Position] = {}

    def flush(self) -> None:
        self.accounts.clear()
        self.orders.clear()
        self.positions.clear()

    def load_currencies(self) -> dict:
        return self.currencies.copy()

    def load_instruments(self) -> dict:
        return self.instruments.copy()

    def load_accounts(self) -> dict:
        return self.accounts.copy()

    def load_orders(self) -> dict:
        return self.orders.copy()

    def load_positions(self) -> dict:
        return self.positions.copy()

    def load_currency(self, code: str) -> Currency:
        return self.currencies.get(code)

    def load_instrument(self, instrument_id: InstrumentId) -> InstrumentId:
        return self.instruments.get(instrument_id)

    def load_account(self, account_id: AccountId) -> Account:
        return self.accounts.get(account_id)

    def load_order(self, client_order_id: ClientOrderId) -> Order:
        return self.orders.get(client_order_id)

    def load_position(self, position_id: PositionId) -> Position:
        return self.positions.get(position_id)

    def load_strategy(self, strategy_id: StrategyId) -> dict:
        return {}

    def delete_strategy(self, strategy_id: StrategyId) -> None:
        pass

    def add_currency(self, currency: Currency) -> None:
        self.currencies[currency.code] = currency

    def add_instrument(self, instrument: Instrument) -> None:
        self.instruments[instrument.id] = instrument

    def add_account(self, account: Account) -> None:
        self.accounts[account.id] = account

    def add_order(self, order: Order) -> None:
        self.orders[order.client_order_id] = order

    def add_position(self, position: Position) -> None:
        self.positions[position.id] = position

    def update_account(self, event: Account) -> None:
        pass  # Would persist the event

    def update_order(self, order: Order) -> None:
        pass  # Would persist the event

    def update_position(self, position: Position) -> None:
        pass  # Would persist the event

    def update_strategy(self, strategy: TradingStrategy) -> None:
        pass  # Would persist the user state dict


class MockLiveDataEngine(LiveDataEngine):
    """Provides a mock live data engine for testing."""

    def __init__(
        self,
        loop,
        msgbus,
        cache,
        clock,
        logger,
        config=None,
    ):
        super().__init__(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self.commands = []
        self.events = []
        self.responses = []

    def execute(self, command):
        self.commands.append(command)

    def process(self, event):
        self.events.append(event)

    def receive(self, response):
        self.responses.append(response)


class MockLiveExecutionEngine(LiveExecutionEngine):
    """Provides a mock live execution engine for testing."""

    def __init__(
        self,
        loop,
        msgbus,
        cache,
        clock,
        logger,
        config=None,
    ):
        super().__init__(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self.commands = []
        self.events = []

    def execute(self, command):
        self.commands.append(command)

    def process(self, event):
        self.events.append(event)


class MockLiveRiskEngine(LiveRiskEngine):
    """Provides a mock live risk engine for testing."""

    def __init__(
        self,
        loop,
        portfolio,
        msgbus,
        cache,
        clock,
        logger,
        config=None,
    ):
        super().__init__(
            loop=loop,
            portfolio=portfolio,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self.commands = []
        self.events = []

    def execute(self, command):
        self.commands.append(command)

    def process(self, event):
        self.events.append(event)


class MockReader(Reader):
    def parse(self, block: bytes) -> Generator:
        yield block


class NewsEventData(NewsEvent):
    """Generic data NewsEvent, needs to be defined here due to `inspect.is_nautilus_class`"""

    pass


def data_catalog_setup():
    """
    Reset the filesystem and DataCatalog to a clean state
    """
    clear_singleton_instances(DataCatalog)

    os.environ["NAUTILUS_CATALOG"] = "memory:///root/"
    catalog = DataCatalog.from_env()
    assert isinstance(catalog.fs, MemoryFileSystem)
    try:
        catalog.fs.rm("/", recursive=True)
    except FileNotFoundError:
        pass
    catalog.fs.mkdir("/root/data")
    assert catalog.fs.exists("/root/")
    assert not catalog.fs.ls("/root/data")


def aud_usd_data_loader():
    from nautilus_trader.backtest.data.providers import TestInstrumentProvider
    from tests.test_kit.stubs import TestStubs
    from tests.unit_tests.backtest.test_backtest_config import TEST_DATA_DIR

    venue = Venue("SIM")
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=venue)

    def parse_csv_tick(df, instrument_id):
        yield instrument
        for r in df.values:
            ts = secs_to_nanos(pd.Timestamp(r[0]).timestamp())
            tick = QuoteTick(
                instrument_id=instrument_id,
                bid=Price.from_str(str(r[1])),
                ask=Price.from_str(str(r[2])),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=ts,
                ts_init=ts,
            )
            yield tick

    clock = TestClock()
    logger = Logger(clock)
    catalog = DataCatalog.from_env()
    instrument_provider = InstrumentProvider(
        venue=venue,
        logger=logger,
    )
    instrument_provider.add(instrument)
    process_files(
        glob_path=f"{TEST_DATA_DIR}/truefx-audusd-ticks.csv",
        reader=CSVReader(
            block_parser=partial(parse_csv_tick, instrument_id=TestStubs.audusd_id()),
            as_dataframe=True,
        ),
        instrument_provider=instrument_provider,
        catalog=catalog,
    )
