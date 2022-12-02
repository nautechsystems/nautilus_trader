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

import asyncio

import pytest
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming.ocm import MatchedOrder
from betfair_parser.spec.streaming.ocm import OrderAccountChange
from betfair_parser.spec.streaming.ocm import OrderChanges
from betfair_parser.spec.streaming.ocm import UnmatchedOrder

from nautilus_trader.adapters.betfair.common import BETFAIR_PRICE_PRECISION
from nautilus_trader.adapters.betfair.common import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.execution import BetfairClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.parsing.requests import betfair_account_to_account_state
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderCancelRejected
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderModifyRejected
from nautilus_trader.model.events.order import OrderPendingCancel
from nautilus_trader.model.events.order import OrderPendingUpdate
from nautilus_trader.model.events.order import OrderRejected
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.integration_tests.adapters.betfair.test_kit import format_current_orders
from tests.integration_tests.adapters.betfair.test_kit import mock_betfair_request


class TestBetfairExecutionClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.logger = Logger(clock=self.clock)

        self.trader_id = TestIdStubs.trader_id()
        self.venue = BETFAIR_VENUE
        self.account_id = AccountId(f"{self.venue.value}-001")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestComponentStubs.cache()

        self.betfair_client: BetfairClient = BetfairTestStubs.betfair_client(
            loop=self.loop,
            logger=self.logger,
        )
        assert self.betfair_client.session_token

        self.instrument_provider = BetfairTestStubs.instrument_provider(
            betfair_client=self.betfair_client,
        )

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BetfairExecutionClient(
            loop=asyncio.get_event_loop(),
            client=self.betfair_client,
            base_currency=GBP,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            instrument_provider=self.instrument_provider,
            market_filter={},
        )
        self.exec_engine.register_client(self.exec_client)

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.instrument = TestInstrumentProvider.betting_instrument()
        self.instrument_id = self.instrument.id
        self.cache.add_instrument(self.instrument)
        self.cache.add_account(TestExecStubs.betting_account(account_id=self.account_id))
        #
        #
        #
        # # Fixture Setup
        # self.loop = asyncio.get_event_loop()
        # self.loop.set_debug(True)
        #
        # self.clock = LiveClock()
        # self.trader_id = TestIdStubs.trader_id()
        # self.venue = BETFAIR_VENUE
        # self.account_id = AccountId(f"{self.venue.value}-001")
        # # Setup logging
        self.logger = LiveLogger(loop=self.loop, clock=self.clock, level_stdout=LogLevel.DEBUG)
        self._log = LoggerAdapter("TestBetfairExecutionClient", self.logger)
        #
        # self.msgbus = MessageBus(
        #     trader_id=self.trader_id,
        #     clock=self.clock,
        #     logger=self.logger,
        # )
        #
        # self.cache = TestComponentStubs.cache()
        # self.cache.add_instrument(self.instrument)
        # self.cache.add_account(TestExecStubs.betting_account(account_id=self.account_id))
        #
        # self.portfolio = Portfolio(
        #     msgbus=self.msgbus,
        #     cache=self.cache,
        #     clock=self.clock,
        #     logger=self.logger,
        # )
        #
        # config = LiveExecEngineConfig()
        # self.exec_engine = LiveExecutionEngine(
        #     loop=self.loop,
        #     msgbus=self.msgbus,
        #     cache=self.cache,
        #     clock=self.clock,
        #     logger=self.logger,
        #     config=config,
        # )
        #
        # self.betfair_client: BetfairClient = BetfairTestStubs.betfair_client(
        #     loop=self.loop,
        #     logger=self.logger,
        # )
        # assert self.betfair_client.session_token
        # self.instrument_provider = BetfairTestStubs.instrument_provider(
        #     betfair_client=self.betfair_client,
        # )
        # self.instrument_provider.add(self.instrument)
        #
        # self.exec_client = BetfairExecutionClient(
        #     loop=asyncio.get_event_loop(),
        #     client=self.betfair_client,
        #     base_currency=GBP,
        #     msgbus=self.msgbus,
        #     cache=self.cache,
        #     clock=self.clock,
        #     logger=self.logger,
        #     instrument_provider=self.instrument_provider,
        #     market_filter={},
        # )
        #
        # self.exec_engine.register_client(self.exec_client)
        #
        # # Re-route exec engine messages through `handler`
        # self.messages = []
        #
        # def handler(func):
        #     def inner(x):
        #         self.messages.append(x)
        #         return func(x)
        #
        #     return inner

    def _prefill_venue_order_id_to_client_order_id(self, order_change_message: OCM):
        order_ids = [
            unmatched_order.id
            for market in order_change_message.oc
            for order_changes in market.orc
            for unmatched_order in order_changes.uo
        ]
        return {VenueOrderId(oid): ClientOrderId(str(i + 1)) for i, oid in enumerate(order_ids)}

    async def _setup_account(self):
        await self.exec_client.connection_account_state()

    def _setup_exec_client_and_cache(self, order_change_message: OCM):
        """
        Called before processing a test streaming update - ensure all orders are in the cache in `update`.
        """
        venue_order_ids = self._prefill_venue_order_id_to_client_order_id(order_change_message)
        venue_order_id_to_client_order_id = {}
        for c_id, v_id in enumerate(venue_order_ids):
            client_order_id = ClientOrderId(str(c_id))
            venue_order_id = VenueOrderId(str(v_id))
            self._log.debug(f"Adding client_order_id=[{c_id}], venue_order_id=[{v_id}] ")
            order = TestExecStubs.make_accepted_order(
                instrument_id=self.instrument_id,
                venue_order_id=venue_order_id,
                client_order_id=client_order_id,
            )
            self._log.debug(f"created order: {order}")
            venue_order_id_to_client_order_id[v_id] = order.client_order_id
            cache_order = self.cache.order(client_order_id=order.client_order_id)
            self._log.debug(f"Cached order: {order}")
            if cache_order is None:
                self._log.debug("Adding order to cache")
                self.cache.add_order(order, position_id=PositionId(v_id.value))
                assert self.cache.order(client_order_id).venue_order_id == venue_order_id
            self.cache.update_order(order)

        self.exec_client.venue_order_id_to_client_order_id = venue_order_id_to_client_order_id

    async def _account_state(self):
        account_details = await self.betfair_client.get_account_details()
        account_funds = await self.betfair_client.get_account_funds()
        timestamp = self.clock.timestamp_ns()
        account_state = betfair_account_to_account_state(
            account_detail=account_details,
            account_funds=account_funds,
            event_id=UUID4(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        return account_state

    @pytest.mark.asyncio
    async def test_submit_order_success(self):
        # Arrange
        order = TestExecStubs.limit_order(
            instrument_id=self.instrument.id,
            price=Price.from_str("0.5"),
        )
        command = TestCommandStubs.submit_order_command(order=order)
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_place_order_success())

        # Act
        self.exec_client.submit_order(command)
        await asyncio.sleep(0.1)

        # Assert
        submitted, accepted = self.messages
        assert isinstance(submitted, OrderSubmitted)
        assert isinstance(accepted, OrderAccepted)
        assert accepted.venue_order_id == VenueOrderId("228302937743")

    @pytest.mark.asyncio
    async def test_submit_order_error(self):
        # Arrange
        order = TestExecStubs.limit_order(
            instrument_id=self.instrument.id,
            price=Price.from_str("0.5"),
        )
        command = TestCommandStubs.submit_order_command(order=order)
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_place_order_error())

        # Act
        self.exec_client.submit_order(command)
        await asyncio.sleep(0)

        # Assert
        submitted, rejected = self.messages
        assert isinstance(submitted, OrderSubmitted)
        assert isinstance(rejected, OrderRejected)
        assert rejected.reason == "PERMISSION_DENIED: ERROR_IN_ORDER"

    @pytest.mark.asyncio
    async def test_modify_order_success(self):
        # Arrange
        venue_order_id = VenueOrderId("240808576108")
        order = TestExecStubs.make_accepted_order(
            venue_order_id=venue_order_id,
            instrument_id=self.instrument_id,
        )
        command = TestCommandStubs.modify_order_command(
            price=Price.from_str("0.01"),
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_replace_orders_success())

        # Act
        self.cache.add_order(order, PositionId("1"))
        self.exec_client.modify_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_update, updated = self.messages
        assert isinstance(pending_update, OrderPendingUpdate)
        assert isinstance(updated, OrderUpdated)
        assert updated.price == Price.from_str("0.02000")

    @pytest.mark.asyncio
    async def test_modify_order_error_order_doesnt_exist(self):
        # Arrange
        venue_order_id = VenueOrderId("229435133092")
        order = TestExecStubs.make_accepted_order(
            venue_order_id=venue_order_id,
            instrument_id=self.instrument_id,
        )

        command = TestCommandStubs.modify_order_command(
            price=Price.from_str("0.01"),
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_replace_orders_success())

        # Act
        self.exec_client.modify_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_update, rejected = self.messages
        assert isinstance(pending_update, OrderPendingUpdate)
        assert isinstance(rejected, OrderModifyRejected)
        assert rejected.reason == "ORDER NOT IN CACHE"

    @pytest.mark.asyncio
    async def test_modify_order_error_no_venue_id(self):
        # Arrange
        order = TestExecStubs.make_submitted_order(instrument_id=self.instrument_id)
        self.cache.add_order(order, position_id=TestIdStubs.position_id())

        command = TestCommandStubs.modify_order_command(
            price=Price.from_str("0.50"),
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id="",
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_replace_orders_success())

        # Act
        self.exec_client.modify_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_update, rejected = self.messages
        assert isinstance(pending_update, OrderPendingUpdate)
        assert isinstance(rejected, OrderModifyRejected)
        assert rejected.reason == "ORDER MISSING VENUE_ORDER_ID"

    @pytest.mark.asyncio
    async def test_cancel_order_success(self):
        # Arrange
        order = TestExecStubs.make_submitted_order(instrument_id=self.instrument_id)
        self.cache.add_order(order, position_id=TestIdStubs.position_id())

        command = TestCommandStubs.cancel_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("240564968665"),
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_cancel_orders_success())

        # Act
        self.exec_client.cancel_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_cancel, cancelled = self.messages
        assert isinstance(pending_cancel, OrderPendingCancel)
        assert isinstance(cancelled, OrderCanceled)

    @pytest.mark.asyncio
    async def test_cancel_order_fail(self):
        # Arrange
        order = TestExecStubs.make_submitted_order(instrument_id=self.instrument_id)
        self.cache.add_order(order, position_id=TestIdStubs.position_id())

        command = TestCommandStubs.cancel_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("228302937743"),
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_cancel_orders_error())

        # Act
        self.exec_client.cancel_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_cancel, cancelled = self.messages
        assert isinstance(pending_cancel, OrderPendingCancel)
        assert isinstance(cancelled, OrderCancelRejected)

    @pytest.mark.asyncio
    async def test_order_multiple_fills(self):
        # Arrange
        self.exec_engine.start()
        client_order_id = ClientOrderId("1")
        venue_order_id = VenueOrderId("246938411724")
        submitted = TestExecStubs.make_submitted_order(
            client_order_id=client_order_id,
            quantity=Quantity.from_int(20),
            instrument_id=self.instrument_id,
        )
        self.cache.add_order(submitted, position_id=TestIdStubs.position_id())
        self.exec_client.venue_order_id_to_client_order_id[venue_order_id] = client_order_id

        # Act
        for order_change_message in BetfairStreaming.ocm_multiple_fills():
            await self.exec_client._handle_order_stream_update(
                order_change_message=order_change_message,
            )
            await asyncio.sleep(0.01)

        # Assert
        result = [fill.last_qty for fill in self.messages]
        expected = [
            Quantity.from_str("16.1900"),
            Quantity.from_str("0.77"),
            Quantity.from_str("0.77"),
        ]
        assert result == expected

    @pytest.mark.asyncio
    async def test_connection_account_state(self):
        # Arrange, Act, Assert

        await self.exec_client.connection_account_state()

        # Assert
        assert self.cache.account(self.account_id)

    @pytest.mark.asyncio
    async def test_check_account_currency(self):
        # Arrange, Act, Assert
        await self.exec_client.check_account_currency()

    @pytest.mark.asyncio
    async def test_order_stream_full_image(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_FULL_IMAGE()
        await self._setup_account()
        self._setup_exec_client_and_cache(order_change_message=order_change_message)

        # Act
        await self.exec_client._handle_order_stream_update(
            order_change_message=order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 7

    @pytest.mark.asyncio
    async def test_order_stream_empty_image(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_EMPTY_IMAGE()
        await self._setup_account()
        self._setup_exec_client_and_cache(order_change_message=order_change_message)

        # Act
        await self.exec_client._handle_order_stream_update(
            order_change_message=order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 1

    @pytest.mark.asyncio
    async def test_order_stream_new_full_image(self):
        order_change_message = BetfairStreaming.ocm_NEW_FULL_IMAGE()
        await self._setup_account()
        self._setup_exec_client_and_cache(order_change_message=order_change_message)

        await self.exec_client._handle_order_stream_update(
            order_change_message=order_change_message,
        )
        await asyncio.sleep(0)
        assert len(self.messages) == 4

    @pytest.mark.asyncio
    async def test_order_stream_sub_image(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_SUB_IMAGE()
        await self._setup_account()
        self._setup_exec_client_and_cache(order_change_message=order_change_message)

        # Act
        await self.exec_client._handle_order_stream_update(
            order_change_message=order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 1

    @pytest.mark.asyncio
    async def test_order_stream_update(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_UPDATE()
        await self._setup_account()
        self._setup_exec_client_and_cache(order_change_message=order_change_message)

        # Act
        await self.exec_client._handle_order_stream_update(
            order_change_message=order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 2

    @pytest.mark.asyncio
    async def test_order_stream_filled(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_FILLED()
        self._setup_exec_client_and_cache(order_change_message=order_change_message)
        await self._setup_account()

        # Act
        await self.exec_client._handle_order_stream_update(
            order_change_message=order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 2
        assert isinstance(self.messages[1], OrderFilled)
        assert self.messages[1].last_px == Price.from_str("0.9090909")

    @pytest.mark.asyncio
    async def test_order_stream_filled_multiple_prices(self):
        # Arrange
        await self._setup_account()
        order_change_message = BetfairStreaming.generate_order_change_message(
            price=1.50,
            size=20,
            side="B",
            status="E",
            sm=10,
            avp=1.60,
        )
        self._setup_exec_client_and_cache(order_change_message)
        await self.exec_client._handle_order_stream_update(
            order_change_message=order_change_message,
        )
        await asyncio.sleep(0)
        order = self.cache.order(client_order_id=ClientOrderId("0"))
        event = self.messages[-1]
        order.apply(event)

        # Act
        order_change_message = BetfairStreaming.generate_order_change_message(
            price=1.50,
            size=20,
            side="B",
            status="EC",
            sm=20,
            avp=1.55,
        )
        self._setup_exec_client_and_cache(order_change_message)
        await self.exec_client._handle_order_stream_update(
            order_change_message=order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 3
        assert isinstance(self.messages[1], OrderFilled)
        assert isinstance(self.messages[2], OrderFilled)
        assert self.messages[1].last_px == price_to_probability("1.60")
        assert self.messages[2].last_px == price_to_probability("1.50")

    @pytest.mark.asyncio
    async def test_order_stream_mixed(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_MIXED()
        self._setup_exec_client_and_cache(order_change_message=order_change_message)
        await self._setup_account()

        # Act
        await self.exec_client._handle_order_stream_update(
            order_change_message=order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        _, fill1, fill2, cancel = self.messages
        assert isinstance(fill1, OrderFilled) and fill1.venue_order_id.value == "229430281341"
        assert isinstance(fill2, OrderFilled) and fill2.venue_order_id.value == "229430281339"
        assert isinstance(cancel, OrderCanceled) and cancel.venue_order_id.value == "229430281339"

    @pytest.mark.asyncio
    async def test_duplicate_trade_id(self):
        # Arrange
        await self._setup_account()
        for update in BetfairStreaming.ocm_DUPLICATE_EXECUTION():
            self._setup_exec_client_and_cache(update)

        # Act
        for order_change_message in BetfairStreaming.ocm_DUPLICATE_EXECUTION():
            self._setup_exec_client_and_cache(order_change_message=order_change_message)

            await self.exec_client._handle_order_stream_update(
                order_change_message=order_change_message,
            )
            await asyncio.sleep(0)

        # Assert
        _, fill1, cancel, fill2, fill3 = self.messages
        # First order example, partial fill followed by remainder canceled
        assert isinstance(fill1, OrderFilled)
        assert isinstance(cancel, OrderCanceled)
        # Second order example, partial fill followed by remainder filled
        assert (
            isinstance(fill2, OrderFilled)
            and fill2.trade_id.value == "c18af83bb4ca0ac45000fa380a2a5887a1bf3e75"
        )
        assert (
            isinstance(fill3, OrderFilled)
            and fill3.trade_id.value == "561879891c1645e8627cf97ed825d16e43196408"
        )

    @pytest.mark.asyncio
    async def test_betfair_order_reduces_balance(self):
        # Arrange
        balance = self.cache.account_for_venue(self.venue).balances()[GBP]
        order = TestExecStubs.limit_order(
            instrument_id=self.instrument_id,
            price=Price.from_str("0.5"),
            quantity=Quantity.from_int(10),
        )
        command = TestCommandStubs.submit_order_command(order=order)
        self.cache.add_order(order=order, position_id=None)
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_place_order_success())
        self.exec_client.submit_order(command)
        await asyncio.sleep(2)

        # Act
        balance_order = self.cache.account_for_venue(BETFAIR_VENUE).balances()[GBP]

        # Cancel the order, balance should retur`n
        command = TestCommandStubs.cancel_order_command(
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_cancel_orders_success())
        self.exec_client.cancel_order(command)
        await asyncio.sleep(0.1)
        balance_cancel = self.cache.account_for_venue(BETFAIR_VENUE).balances()[GBP]

        # Assert
        assert balance.free == Money(1000.0, GBP)
        assert balance_order.free == Money(990.0, GBP)
        assert balance_cancel.free == Money(1000.0, GBP)

        self.exec_engine.kill()
        await asyncio.sleep(1)

    @pytest.mark.asyncio
    async def test_betfair_order_cancelled_no_timestamp(self):
        update = BetfairStreaming.ocm_error_fill()
        self._setup_exec_client_and_cache(update)
        for unmatched_order in update.oc[0].orc[0].uo:
            self.exec_client._handle_stream_execution_complete_order_update(
                unmatched_order=unmatched_order,
            )
            await asyncio.sleep(0.1)

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        "price,size,side,status,updates",
        [
            (1.50, 50, "B", "EC", [{"sm": 50}]),
            (1.50, 50, "B", "E", [{"sm": 10}, {"sm": 15}]),
        ],
    )
    async def test_various_betfair_order_fill_scenarios(self, price, size, side, status, updates):
        # Arrange
        update = BetfairStreaming.ocm_filled_different_price()
        self._setup_exec_client_and_cache(update)
        await self._setup_account()

        # Act
        for raw in updates:
            order_change_message = BetfairStreaming.generate_order_change_message(
                price=price, size=size, side=side, status=status, **raw
            )
            await self.exec_client._handle_order_stream_update(
                order_change_message=order_change_message,
            )
            await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 1 + len(updates)
        for msg, raw in zip(self.messages[1:], updates):
            assert isinstance(msg, OrderFilled)
            assert msg.last_qty == raw["sm"]

    @pytest.mark.asyncio
    async def test_order_filled_avp_update(self):
        # Arrange
        update = BetfairStreaming.ocm_filled_different_price()
        self._setup_exec_client_and_cache(update)
        await self._setup_account()

        # Act
        order_change_message = BetfairStreaming.generate_order_change_message(
            price=1.50,
            size=20,
            side="B",
            status="E",
            avp=1.50,
            sm=10,
        )
        await self.exec_client._handle_order_stream_update(
            order_change_message=order_change_message,
        )
        await asyncio.sleep(0)

        order_change_message = BetfairStreaming.generate_order_change_message(
            price=1.30,
            size=20,
            side="B",
            status="E",
            avp=1.50,
            sm=10,
        )
        await self.exec_client._handle_order_stream_update(
            order_change_message=order_change_message,
        )
        await asyncio.sleep(0)

    @pytest.mark.asyncio
    async def test_generate_order_status_report_client_id(self, mocker):
        # Arrange
        order_resp = format_current_orders()
        self.instrument_provider.add(
            TestInstrumentProvider.betting_instrument(
                market_id=str(order_resp[0]["marketId"]),
                selection_id=str(order_resp[0]["selectionId"]),
                handicap=str(order_resp[0]["handicap"]),
            ),
        )
        venue_order_id = VenueOrderId("1")

        mocker.patch.object(self.betfair_client, "list_current_orders", return_value=order_resp)

        # Act
        report: OrderStatusReport = await self.exec_client.generate_order_status_report(
            venue_order_id=venue_order_id,
            client_order_id=None,
            instrument_id=None,
        )

        # Assert
        assert report.order_status == OrderStatus.ACCEPTED
        assert report.price == Price(0.2, BETFAIR_PRICE_PRECISION)
        assert report.quantity == Quantity(10.0, BETFAIR_QUANTITY_PRECISION)
        assert report.filled_qty == Quantity(0.0, BETFAIR_QUANTITY_PRECISION)

    @pytest.mark.asyncio
    async def test_check_cache_against_order_image(self):
        # Arrange
        order = TestExecStubs.limit_order(
            instrument_id=self.instrument_id,
            client_order_id=ClientOrderId("O-20210410-022422-001-001-2"),
        )
        active_order = TestExecStubs.make_accepted_order(
            order=order,
            instrument_id=self.instrument_id,
            venue_order_id=VenueOrderId("246938411724"),
        )
        filled_order = TestExecStubs.make_filled_order(
            instrument=self.instrument,
            price=Price(0.5000, BETFAIR_PRICE_PRECISION),
            quantity=Quantity(10, BETFAIR_QUANTITY_PRECISION),
        )
        self.exec_client.venue_order_id_to_client_order_id[
            VenueOrderId("246938411724")
        ] = active_order.client_order_id
        self.cache.add_order(active_order, PositionId("0"))
        self.cache.add_order(filled_order, PositionId("0"))

        # Act
        ocm = OCM(
            id=2,
            clk="AAAAAAAAAAAAAA==",
            pt=1669350204489,
            oc=[
                OrderAccountChange(
                    id="1.179082386",
                    fullImage=True,
                    orc=[
                        OrderChanges(
                            id=50214,
                            fullImage=True,
                            uo=[
                                UnmatchedOrder(
                                    id="246938411724",
                                    p=5.8,
                                    s=20,
                                    side="B",
                                    status="E",
                                    pt="P",
                                    ot="L",
                                    pd=1633905366000,
                                    md=1633905758000,
                                    avp=5.8,
                                    sm=16.19,
                                    sr=3.809999999999999,
                                    sl=0,
                                    sc=0,
                                    sv=0,
                                    rac="",
                                    rc="REG_LGA",
                                    rfo="O-20211010-223605-000",
                                    rfs="TestStrategy-1.",
                                ),
                            ],
                            mb=[MatchedOrder(2.0, 10.0)],
                            ml=[],
                        ),
                    ],
                ),
            ],
        )
        await self.exec_client._handle_order_stream_update(ocm)

        # Assert
        # TODO
