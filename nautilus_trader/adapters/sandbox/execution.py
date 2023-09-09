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

import asyncio
from decimal import Decimal
from typing import Optional

import pandas as pd

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.data import Data
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.msgbus.bus import MessageBus


class SandboxExecutionClient(LiveExecutionClient):
    """
    Provides a sandboxed execution client for testing against.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        venue: str,
        currency: str,
        balance: int,
        account_id: str = "001",
        oms_type: OmsType = OmsType.NETTING,
        account_type: AccountType = AccountType.MARGIN,
    ) -> None:
        self._currency = Currency.from_str(currency)
        money = Money(value=balance, currency=self._currency)
        self.balance = AccountBalance(total=money, locked=Money(0, money.currency), free=money)
        self.test_clock = TestClock()
        self._account_type = account_type
        self.sandbox_venue = Venue(venue)
        self._account_id = AccountId(f"{venue}-{account_id}")
        super().__init__(
            loop=loop,
            client_id=ClientId(venue),
            venue=self.sandbox_venue,
            oms_type=oms_type,
            account_type=account_type,
            base_currency=self._currency,
            instrument_provider=InstrumentProvider(venue=self.sandbox_venue, logger=logger),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=None,
        )
        self.exchange = SimulatedExchange(
            venue=self.sandbox_venue,
            oms_type=oms_type,
            account_type=self._account_type,
            base_currency=self._currency,
            starting_balances=[self.balance.free],
            default_leverage=Decimal(10),
            leverages={},
            instruments=[],
            modules=[],
            msgbus=self._msgbus,
            cache=cache,
            fill_model=FillModel(),
            latency_model=LatencyModel(0),
            clock=self.test_clock,
            logger=logger,
            frozen_account=True,  # <-- Freezing account
        )
        self._client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=msgbus,
            cache=self._cache,
            clock=self.test_clock,
            logger=logger,
        )
        self.exchange.register_client(self._client)

    async def _connect(self) -> None:
        self._log.info("Connecting...")

        # Load instruments into simulated exchange
        self._log.info("Waiting for data client to load instruments..")
        await asyncio.sleep(5)
        instruments = self.exchange.cache.instruments(self.sandbox_venue)
        self._log.info(f"Loading {len(instruments)} instruments into SimulatedExchange")
        for instrument in instruments:
            self.exchange.add_instrument(instrument)

        # Subscribe to all data
        self._msgbus.subscribe("data.*", handler=self.on_data)

        # Send account state
        await self.send_account_state()

        # Connected.
        self._client._set_connected(True)
        self._set_connected(True)
        self._log.info("Connected.")

    async def _disconnect(self) -> None:
        """
        Disconnect the client.
        """
        self._log.info("Disconnecting...")
        self._set_connected(False)
        self._log.info("Disconnected.")

    async def send_account_state(self) -> None:
        timestamp = self._clock.timestamp_ns()
        account_state: AccountState = AccountState(
            account_id=self._account_id,
            account_type=self._account_type,
            balances=[self.balance],
            base_currency=self._currency,
            reported=True,
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        self._log.debug(f"Sending sandbox account state: {account_state}")
        self._send_account_state(account_state)

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
    ) -> Optional[OrderStatusReport]:
        return None

    async def generate_order_status_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        return []

    async def generate_trade_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> list[TradeReport]:
        return []

    async def generate_position_status_reports(
        self,
        instrument_id: Optional[InstrumentId] = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
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
        # Taken from main backtest loop of BacktestEngine.
        if data.__class__ is OrderBookDelta:  # Don't want to process subclasses of OrderBookDelta
            self.exchange.process_order_book_delta(data)
        elif (
            data.__class__ is OrderBookDeltas
        ):  # Don't want to process subclasses of OrderBookDeltas
            self.exchange.process_order_book_deltas(data)
        elif isinstance(data, QuoteTick):
            self.exchange.process_quote_tick(data)
        elif isinstance(data, TradeTick):
            self.exchange.process_trade_tick(data)
        elif isinstance(data, Bar):
            self.exchange.process_bar(data)
        self.exchange.process(data.ts_init)
