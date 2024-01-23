# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
from decimal import Decimal
from typing import ClassVar

import pandas as pd

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.data import Data
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.portfolio.base import PortfolioFacade


class SandboxExecutionClient(LiveExecutionClient):
    """
    Provides a sandboxed execution client for testing against.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    portfolio : PortfolioFacade
        The read-only portfolio for the client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.

    """

    INSTRUMENTS: ClassVar[list[Instrument]] = []

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        venue: str,
        currency: str,
        balance: int,
        oms_type: OmsType = OmsType.NETTING,
        account_type: AccountType = AccountType.MARGIN,
    ) -> None:
        self._currency = Currency.from_str(currency)
        money = Money(value=balance, currency=self._currency)
        self.balance = AccountBalance(total=money, locked=Money(0, money.currency), free=money)
        self.test_clock = TestClock()
        self._account_type = account_type
        sandbox_venue = Venue(venue)
        super().__init__(
            loop=loop,
            client_id=ClientId(venue),
            venue=sandbox_venue,
            oms_type=oms_type,
            account_type=account_type,
            base_currency=self._currency,
            instrument_provider=InstrumentProvider(),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=None,
        )
        self.exchange = SimulatedExchange(
            venue=sandbox_venue,
            oms_type=oms_type,
            account_type=self._account_type,
            base_currency=self._currency,
            starting_balances=[self.balance.free],
            default_leverage=Decimal(10),
            leverages={},
            instruments=self.INSTRUMENTS,
            modules=[],
            portfolio=portfolio,
            msgbus=self._msgbus,
            cache=cache,
            fill_model=FillModel(),
            latency_model=LatencyModel(0),
            clock=self.test_clock,
            frozen_account=True,  # <-- Freezing account
        )
        self._client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=msgbus,
            cache=self._cache,
            clock=self.test_clock,
        )
        self.exchange.register_client(self._client)

    def connect(self) -> None:
        """
        Connect the client.
        """
        self._log.info("Connecting...")
        self._msgbus.subscribe("data.*", handler=self.on_data)
        self._client._set_connected(True)
        self._set_connected(True)
        self._log.info("Connected.")

    def disconnect(self) -> None:
        """
        Disconnect the client.
        """
        self._log.info("Disconnecting...")
        self._set_connected(False)
        self._log.info("Disconnected.")

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
    ) -> OrderStatusReport | None:
        return None

    async def generate_order_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        return []

    async def generate_fill_reports(
        self,
        instrument_id: InstrumentId | None = None,
        venue_order_id: VenueOrderId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[FillReport]:
        return []

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[PositionStatusReport]:
        return []

    def submit_order(self, command):
        return self._client.submit_order(command)

    def modify_order(self, command):
        return self._client.modify_order(command)

    def cancel_order(self, command):
        return self._client.cancel_order(command)

    def cancel_all_orders(self, command):
        return self._client.cancel_all_orders(command)

    def on_data(self, data: Data) -> None:
        # Taken from main backtest loop of BacktestEngine
        if isinstance(data, (OrderBookDelta)):
            self.exchange.process_order_book_delta(data)
        elif isinstance(data, (OrderBookDeltas)):
            self.exchange.process_order_book_deltas(data)
        elif isinstance(data, QuoteTick):
            self.exchange.process_quote_tick(data)
        elif isinstance(data, TradeTick):
            self.exchange.process_trade_tick(data)
        elif isinstance(data, Bar):
            self.exchange.process_bar(data)
        self.exchange.process(data.ts_init)
