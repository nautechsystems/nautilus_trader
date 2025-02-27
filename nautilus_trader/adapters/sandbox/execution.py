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

import asyncio

from nautilus_trader.adapters.sandbox.config import SandboxExecutionClientConfig
from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.data import Data
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import account_type_from_str
from nautilus_trader.model.enums import book_type_from_str
from nautilus_trader.model.enums import oms_type_from_str
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
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

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        config: SandboxExecutionClientConfig,
    ) -> None:
        sandbox_venue = Venue(config.venue)
        oms_type = oms_type_from_str(config.oms_type)
        account_type = account_type_from_str(config.account_type)
        base_currency = Currency.from_str(config.base_currency) if config.base_currency else None

        self.test_clock = TestClock()

        super().__init__(
            loop=loop,
            client_id=ClientId(config.venue),
            venue=sandbox_venue,
            oms_type=oms_type,
            account_type=account_type,
            base_currency=base_currency,
            instrument_provider=InstrumentProvider(),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=None,
        )
        self.exchange = SimulatedExchange(
            venue=sandbox_venue,
            oms_type=oms_type,
            account_type=account_type,
            starting_balances=[Money.from_str(b) for b in config.starting_balances],
            base_currency=base_currency,
            default_leverage=config.default_leverage,
            leverages=config.leverages or {},
            modules=[],
            portfolio=portfolio,
            msgbus=self._msgbus,
            cache=cache,
            clock=self.test_clock,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            latency_model=LatencyModel(0),
            book_type=book_type_from_str(config.book_type),
            frozen_account=config.frozen_account,
            bar_execution=config.bar_execution,
            trade_execution=config.trade_execution,
            reject_stop_orders=config.reject_stop_orders,
            support_gtd_orders=config.support_gtd_orders,
            support_contingent_orders=config.support_contingent_orders,
            use_position_ids=config.use_position_ids,
            use_random_ids=config.use_random_ids,
            use_reduce_only=config.use_reduce_only,
            use_message_queue=False,  # Do not use internal message queue for real-time
        )

        self._client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=msgbus,
            cache=cache,
            clock=self.test_clock,
        )
        self.exchange.register_client(self._client)
        self.exchange.initialize_account()

    def connect(self) -> None:
        """
        Connect the client.
        """
        self._log.info("Connecting...")
        self._msgbus.subscribe("data.*", handler=self.on_data)

        # Load all instruments for venue
        for instrument in self.exchange.cache.instruments(venue=self.venue):
            self.exchange.add_instrument(instrument)

        self._client._set_connected(True)
        self._set_connected(True)
        self._log.info("Connected")

    def disconnect(self) -> None:
        """
        Disconnect the client.
        """
        self._log.info("Disconnecting...")
        self._set_connected(False)
        self._log.info("Disconnected")

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        return None

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        return []

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        return []

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
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
