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
from typing import Optional
from unittest.mock import MagicMock

import msgspec
import pytest
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming import STREAM_DECODER
from betfair_parser.spec.streaming.ocm import MatchedOrder

from nautilus_trader.adapters.betfair.common import BETFAIR_PRICE_PRECISION
from nautilus_trader.adapters.betfair.common import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.execution import BetfairClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderCancelRejected
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderInitialized
from nautilus_trader.model.events.order import OrderModifyRejected
from nautilus_trader.model.events.order import OrderPendingCancel
from nautilus_trader.model.events.order import OrderPendingUpdate
from nautilus_trader.model.events.order import OrderRejected
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
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


class TestBaseExecutionClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = TestClock()
        self.logger = Logger(clock=self.clock)

        self.trader_id = TestIdStubs.trader_id()
        self.venue = BETFAIR_VENUE
        self.account_id = AccountId(f"{self.venue.value}-001")
        self.venue_order_id = VenueOrderId("240564968665")
        self.client_order_id = ClientOrderId("O-20210327-090738-001-001-2")
        self.instrument = TestInstrumentProvider.betting_instrument()
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

        self.exec_client: BetfairExecutionClient = BetfairExecutionClient(
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

        # Capture events flowing through execution engine
        self.events = []
        self.msgbus.subscribe("events.order*", self.events.append)

        self.logs = []

    async def _setup_state(self, order_change_message: Optional[OCM] = None):
        """
        Ready the engine to test a message from betfair, setting orders into the correct state
        """
        if isinstance(order_change_message, bytes):
            order_change_message = STREAM_DECODER.decode(order_change_message)
        for oc in order_change_message.oc:
            for orc in oc.orc:
                for order_update in orc.uo:
                    order_id = order_update.id
                    venue_order_id = VenueOrderId(order_id)
                    client_order_id = ClientOrderId(order_id)
                    if not self.cache.order(client_order_id):
                        order = TestExecStubs.limit_order(
                            instrument_id=self.instrument.id,
                            price=Price.from_str("0.5"),
                            client_order_id=client_order_id,
                        )
                        self.exec_client.venue_order_id_to_client_order_id[
                            venue_order_id
                        ] = client_order_id
                        await self.accept_order(order, venue_order_id)

    async def submit_order(self, order: Order) -> Order:
        # We don't want the execution client to actually do anything here
        self.exec_client.submit_order = MagicMock()  # type: ignore
        self.strategy.submit_order(order)
        await asyncio.sleep(0)
        assert self.cache.order(order.client_order_id)
        return order

    async def accept_order(self, order, venue_order_id: VenueOrderId) -> Order:
        await self.submit_order(order)
        self.exec_client.generate_order_accepted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id or order.venue_order_id,
            ts_event=0,
        )
        await asyncio.sleep(0)
        return order


class TestBetfairExecutionClient(TestBaseExecutionClient):
    def setup(self):
        super().setup()
        self.instrument = TestInstrumentProvider.betting_instrument()
        self.instrument_id = self.instrument.id
        self.venue_order_id = VenueOrderId("229435133092")
        self.cache.add_instrument(self.instrument)
        self.cache.add_account(TestExecStubs.betting_account(account_id=self.account_id))
        self.test_order = TestExecStubs.limit_order(
            instrument_id=self.instrument.id,
            price=Price.from_str("0.5"),
        )
        self.exec_client.venue_order_id_to_client_order_id[
            self.venue_order_id
        ] = self.client_order_id
        asyncio.run(self._setup_account())

    async def _setup_account(self):
        await self.exec_client.connection_account_state()

    @pytest.mark.asyncio
    async def test_submit_order_success(self):
        # Arrange
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_place_order_success())

        # Act
        self.strategy.submit_order(self.test_order)
        await asyncio.sleep(0)

        # Assert
        _, submitted, accepted = self.test_order.events
        assert isinstance(submitted, OrderSubmitted)
        assert isinstance(accepted, OrderAccepted)
        assert accepted.venue_order_id == VenueOrderId("228302937743")

    @pytest.mark.asyncio
    async def test_submit_order_error(self):
        # Arrange
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_place_order_error())

        # Act
        self.strategy.submit_order(self.test_order)
        await asyncio.sleep(0)

        # Assert
        _, submitted, rejected = self.test_order.events
        assert isinstance(submitted, OrderSubmitted)
        assert isinstance(rejected, OrderRejected)
        assert rejected.reason == "PERMISSION_DENIED: ERROR_IN_ORDER"

    @pytest.mark.asyncio
    async def test_modify_order_success(self):
        # Arrange
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_replace_orders_success())
        await self.accept_order(self.test_order, venue_order_id=self.venue_order_id)

        # Act
        self.strategy.modify_order(self.test_order, price=Price.from_str("0.40"))
        await asyncio.sleep(0)

        # Assert
        pending_update, updated = self.events[-2:]
        assert isinstance(pending_update, OrderPendingUpdate)
        assert isinstance(updated, OrderUpdated)
        assert updated.price == Price.from_str("0.02000")

    @pytest.mark.skip(reason="Log sinks removed")
    @pytest.mark.asyncio
    async def test_modify_order_error_order_doesnt_exist(self):
        # Arrange
        command = TestCommandStubs.modify_order_command(
            order=self.test_order,
            price=Price.from_str("0.01"),
        )
        # Act
        self.exec_client.modify_order(command)
        await asyncio.sleep(0)

        # Assert
        logs = [log["msg"] for log in self.logs[-5:]]
        expected = [
            "Order with ClientOrderId('O-20210410-022422-001-001-1') not found in the cache to apply OrderPendingUpdate(instrument_id=1.179082386|50214|None.BETFAIR, client_order_id=O-20210410-022422-001-001-1, venue_order_id=None, account_id=BETFAIR-001, ts_event=0).",  # noqa
            "Cannot apply event to any order: ClientOrderId('O-20210410-022422-001-001-1') not found in the cache with no `VenueOrderId`.",  # noqa
            "Attempting to update order that does not exist in the cache: ModifyOrder(instrument_id=1.179082386|50214|None.BETFAIR, client_order_id=O-20210410-022422-001-001-1, venue_order_id=None, quantity=None, price=0.01, trigger_price=None)",  # noqa
            "Order with ClientOrderId('O-20210410-022422-001-001-1') not found in the cache to apply OrderModifyRejected(instrument_id=1.179082386|50214|None.BETFAIR, client_order_id=O-20210410-022422-001-001-1, venue_order_id=None, account_id=BETFAIR-001, reason=ORDER NOT IN CACHE, ts_event=0).",  # noqa
            "Cannot apply event to any order: ClientOrderId('O-20210410-022422-001-001-1') not found in the cache with no `VenueOrderId`.",  # noqa
        ]
        assert logs == expected

    @pytest.mark.skip(reason="Log sinks removed")
    @pytest.mark.asyncio
    async def test_modify_order_error_no_venue_id(self):
        # Arrange
        order = await self.submit_order(self.test_order)
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_replace_orders_success())

        # Act
        command = TestCommandStubs.modify_order_command(price=Price.from_str("0.50"), order=order)
        self.exec_client.modify_order(command)
        await asyncio.sleep(0)

        # Assert
        rejected = self.events[-1]
        assert isinstance(rejected, OrderModifyRejected)
        assert rejected.reason == "ORDER MISSING VENUE_ORDER_ID"

    @pytest.mark.asyncio
    async def test_cancel_order_success(self):
        # Arrange
        order = await self.accept_order(order=self.test_order, venue_order_id=self.venue_order_id)
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_cancel_orders_success())

        # Act
        command = TestCommandStubs.cancel_order_command(order=order)
        self.exec_client.cancel_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_cancel, cancelled = self.events[-2:]
        assert isinstance(pending_cancel, OrderPendingCancel)
        assert isinstance(cancelled, OrderCanceled)

    @pytest.mark.asyncio
    async def test_cancel_order_fail(self):
        # Arrange
        order = await self.accept_order(order=self.test_order, venue_order_id=self.venue_order_id)
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_cancel_orders_error())

        # Act
        command = TestCommandStubs.cancel_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=self.venue_order_id,
        )
        self.exec_client.cancel_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_cancel, cancelled = self.events[-2:]
        assert isinstance(pending_cancel, OrderPendingCancel)
        assert isinstance(cancelled, OrderCancelRejected)

    @pytest.mark.asyncio
    async def test_order_multiple_fills(self):
        # Arrange
        for ocm in BetfairStreaming.ocm_multiple_fills():
            await self._setup_state(order_change_message=ocm)

        # Act
        for order_change_message in BetfairStreaming.ocm_multiple_fills():
            self.exec_client.handle_order_stream_update(order_change_message)
            await asyncio.sleep(0.0)

        # Assert
        result = [fill.last_qty for fill in self.events[-3:]]
        expected = [
            Quantity.from_str("16.1900"),
            Quantity.from_str("0.77"),
            Quantity.from_str("0.77"),
        ]
        assert result == expected

    @pytest.mark.asyncio
    async def test_connection_account_state(self):
        # Arrange, Act
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
        for order_id in ("175706685825", "175706685826", "175706685827", "175706685828"):
            venue_order_id = VenueOrderId(order_id)
            client_order_id = ClientOrderId(order_id)
            order = TestExecStubs.limit_order(
                instrument_id=self.instrument.id,
                price=Price.from_str("0.5"),
                client_order_id=client_order_id,
            )
            self.exec_client.venue_order_id_to_client_order_id[venue_order_id] = client_order_id
            await self.accept_order(order, venue_order_id)
        self.exec_client._check_order_update = MagicMock()
        # self.events = []

        # Act
        self.exec_client.handle_order_stream_update(
            BetfairStreaming.ocm_FULL_IMAGE(),
        )
        await asyncio.sleep(0)

        # Assert
        fills = [event for event in self.events if isinstance(event, OrderFilled)]
        assert len(fills) == 5

    @pytest.mark.asyncio
    async def test_order_stream_empty_image(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_EMPTY_IMAGE()

        # Act
        self.exec_client.handle_order_stream_update(
            order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        assert len(self.events) == 0

    @pytest.mark.asyncio
    async def test_order_stream_new_full_image(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_NEW_FULL_IMAGE()
        await self._setup_state(order_change_message)

        # Act
        self.exec_client.handle_order_stream_update(
            BetfairStreaming.ocm_NEW_FULL_IMAGE(),
        )
        await asyncio.sleep(0)
        assert len(self.events) == 6

    @pytest.mark.asyncio
    async def test_order_stream_sub_image(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_SUB_IMAGE()
        await self._setup_state(order_change_message=order_change_message)

        # Act
        self.exec_client.handle_order_stream_update(
            order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        assert len(self.events) == 0

    @pytest.mark.asyncio
    async def test_order_stream_update(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_UPDATE()
        await self._setup_state(order_change_message=order_change_message)

        # Act
        self.exec_client.handle_order_stream_update(
            order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        assert len(self.events) == 3

    @pytest.mark.asyncio
    async def test_order_stream_filled(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_FILLED()
        await self._setup_state(order_change_message=order_change_message)

        # Act
        self.exec_client.handle_order_stream_update(
            order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        assert len(self.events) == 3
        assert isinstance(self.events[2], OrderFilled)
        assert self.events[2].last_px == Price.from_str("0.9090909")

    @pytest.mark.asyncio
    async def test_order_stream_filled_multiple_prices(self):
        # Arrange
        order_change_message = BetfairStreaming.generate_order_change_message(
            price=1.50,
            size=20,
            side="B",
            status="E",
            sm=10,
            avp=1.60,
            order_id=self.venue_order_id.value,
        )
        await self._setup_state(order_change_message)
        self.exec_client.handle_order_stream_update(msgspec.json.encode(order_change_message))
        await asyncio.sleep(0)
        order = self.cache.order(client_order_id=ClientOrderId("229435133092"))
        assert order

        # Act
        order_change_message = BetfairStreaming.generate_order_change_message(
            price=1.50,
            size=20,
            side="B",
            status="EC",
            sm=20,
            avp=1.50,
        )
        await self._setup_state(order_change_message)
        self.exec_client.handle_order_stream_update(msgspec.json.encode(order_change_message))
        await asyncio.sleep(0)

        # Assert
        assert len(self.events) == 6
        assert isinstance(self.events[2], OrderFilled)
        assert isinstance(self.events[5], OrderFilled)
        assert self.events[2].last_px == price_to_probability("1.60")
        assert self.events[5].last_px == price_to_probability("1.50")

    @pytest.mark.asyncio
    async def test_order_stream_mixed(self):
        # Arrange
        order_change_message = BetfairStreaming.ocm_MIXED()
        await self._setup_state(order_change_message=order_change_message)

        # Act
        self.exec_client.handle_order_stream_update(
            order_change_message,
        )
        await asyncio.sleep(0)

        # Assert
        fill1, fill2, cancel = self.events[-3:]
        assert isinstance(fill1, OrderFilled) and fill1.venue_order_id.value == "229430281341"
        assert isinstance(fill2, OrderFilled) and fill2.venue_order_id.value == "229430281339"
        assert isinstance(cancel, OrderCanceled) and cancel.venue_order_id.value == "229430281339"

    @pytest.mark.asyncio
    async def test_duplicate_trade_id(self):
        # Arrange
        for update in BetfairStreaming.ocm_DUPLICATE_EXECUTION():
            await self._setup_state(update)

        # Act
        for order_change_message in BetfairStreaming.ocm_DUPLICATE_EXECUTION():
            self.exec_client.handle_order_stream_update(order_change_message)
            await asyncio.sleep(0)

        # Assert
        fill1, cancel, fill2, fill3 = self.events[-4:]
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
        order = await self.accept_order(self.test_order, self.venue_order_id)

        # Act
        balance_order = self.cache.account_for_venue(BETFAIR_VENUE).balances()[GBP]

        # Cancel the order, balance should return
        command = TestCommandStubs.cancel_order_command(
            instrument_id=self.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_cancel_orders_success())
        self.exec_client.cancel_order(command)
        await asyncio.sleep(0)
        balance_cancel = self.cache.account_for_venue(BETFAIR_VENUE).balances()[GBP]

        # Assert
        assert balance.free == Money(1000.0, GBP)
        assert balance_order.free == Money(900.0, GBP)
        assert balance_cancel.free == Money(1000.0, GBP)

    @pytest.mark.asyncio
    async def test_betfair_order_cancelled_no_timestamp(self):
        # Arrange
        update = STREAM_DECODER.decode(BetfairStreaming.ocm_error_fill())
        await self._setup_state(update)
        self.clock.set_time(1)

        # Act
        for unmatched_order in update.oc[0].orc[0].uo:
            self.exec_client._handle_stream_execution_complete_order_update(
                unmatched_order=unmatched_order,
            )
            await asyncio.sleep(0)

        # Assert
        cancel1, cancel2 = self.events[-2:]
        assert isinstance(cancel1, OrderCanceled)
        assert isinstance(cancel2, OrderCanceled)
        assert cancel1.ts_init == 1
        assert cancel2.ts_init == 1

    @pytest.mark.asyncio
    @pytest.mark.parametrize(
        "price,size,side,status,updates, last_qtys",
        [
            (1.50, 50, "B", "EC", [{"sm": 50}], (50,)),
            (1.50, 50, "B", "E", [{"sm": 10}, {"sm": 15}], (10, 5)),
        ],
    )
    async def test_various_betfair_order_fill_scenarios(
        self,
        price,
        size,
        side,
        status,
        updates,
        last_qtys,
    ):
        # Arrange
        update = BetfairStreaming.ocm_filled_different_price()
        await self._setup_state(update)

        # Act
        for raw in updates:
            order_change_message = BetfairStreaming.generate_order_change_message(
                price=price, size=size, side=side, status=status, **raw
            )
            self.exec_client.handle_order_stream_update(msgspec.json.encode(order_change_message))
            await asyncio.sleep(0)

        # Assert
        events = [
            event
            for event in self.events
            if not isinstance(event, (OrderInitialized, OrderAccepted))
        ]
        for msg, raw, last_qty in zip(events, updates, last_qtys):
            assert isinstance(msg, OrderFilled)
            assert msg.last_qty == last_qty

    @pytest.mark.asyncio
    async def test_order_filled_avp_update(self):
        # Arrange
        update = BetfairStreaming.ocm_filled_different_price()
        await self._setup_state(update)

        # Act
        order_change_message = BetfairStreaming.generate_order_change_message(
            price=1.50,
            size=20,
            side="B",
            status="E",
            avp=1.50,
            sm=10,
        )
        self.exec_client.handle_order_stream_update(msgspec.json.encode(order_change_message))
        await asyncio.sleep(0)

        order_change_message = BetfairStreaming.generate_order_change_message(
            price=1.30,
            size=20,
            side="B",
            status="E",
            avp=1.50,
            sm=10,
        )
        self.exec_client.handle_order_stream_update(msgspec.json.encode(order_change_message))
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

    @pytest.mark.skip(reason="Log sinks removed")
    @pytest.mark.asyncio
    async def test_check_cache_against_order_image(self):
        # Arrange
        ocm = BetfairStreaming.generate_order_change_message(
            price=5.8,
            size=20,
            side="B",
            status="E",
            sm=16.19,
            sr=3.809999999999999,
            avp=1.50,
            order_id=self.venue_order_id.value,
            mb=[MatchedOrder(5.0, 100)],
        )

        # Act
        self.exec_client.check_cache_against_order_image(ocm)

        # Assert
        expected = "UNKNOWN FILL: instrument_id=InstrumentId('1|1|None.BETFAIR') MatchedOrder(price=5.0, size=100)"
        log = self.logs[-1]["msg"]
        assert log == expected
