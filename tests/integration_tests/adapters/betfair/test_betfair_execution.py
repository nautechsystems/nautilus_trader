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

import asyncio
from asyncio import Future
from functools import partial
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.betfair.client import BetfairClient
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.parsing import betfair_account_to_account_state
from nautilus_trader.adapters.betfair.parsing import generate_trades_list
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderPendingCancel
from nautilus_trader.model.events.order import OrderPendingUpdate
from nautilus_trader.model.events.order import OrderRejected
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.events.order import OrderUpdateRejected
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.message_bus import MessageBus
from nautilus_trader.trading.portfolio import Portfolio
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit.stubs import TestStubs


# monkey patch MagicMock
async def async_magic():
    pass


MagicMock.__await__ = lambda x: async_magic().__await__()


def mock_async(obj, method, value):
    setattr(obj, method, MagicMock(return_value=Future()))
    getattr(obj, method).return_value.set_result(value)


class TestBetfairExecutionClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()

        self.trader_id = TestStubs.trader_id()
        self.account_id = AccountId(BETFAIR_VENUE.value, "001")

        # Setup logging
        self.logger = LiveLogger(loop=self.loop, clock=self.clock, level_stdout=LogLevel.DEBUG)
        self._log = LoggerAdapter("TestBetfairExecutionClient", self.logger)

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()
        self.cache.add_instrument(BetfairTestStubs.betting_instrument())

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.betfair_client = MagicMock(spec=BetfairClient)
        mock_async(
            self.betfair_client, "get_account_details", BetfairResponses.account_details()["result"]
        )
        mock_async(
            self.betfair_client, "list_navigation", BetfairResponses.navigation_list_navigation()
        )

        self.client = BetfairExecutionClient(
            loop=asyncio.get_event_loop(),
            client=self.betfair_client,
            account_id=self.account_id,
            base_currency=GBP,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            market_filter={},
            load_instruments=False,
        )

        self.exec_engine.register_client(self.client)

        # Re-route exec engine messages through `handler`
        self.messages = []

        def handler(x, endpoint):
            self.messages.append(x)
            if endpoint == "execute":
                self.exec_engine.execute(x)
            elif endpoint == "process":
                self.exec_engine.process(x)

        self.msgbus.deregister(endpoint="ExecEngine.execute", handler=self.exec_engine.execute)  # type: ignore
        self.msgbus.register(
            endpoint="ExecEngine.execute", handler=partial(handler, endpoint="execute")  # type: ignore
        )

        self.msgbus.deregister(endpoint="ExecEngine.process", handler=self.exec_engine.process)  # type: ignore
        self.msgbus.register(
            endpoint="ExecEngine.process", handler=partial(handler, endpoint="process")  # type: ignore
        )

    def _prefill_venue_order_id_to_client_order_id(self, update):
        order_ids = [
            update["id"]
            for market in update.get("oc", [])
            for order in market["orc"]
            for update in order.get("uo", [])
        ]
        return {VenueOrderId(oid): ClientOrderId(str(i + 1)) for i, oid in enumerate(order_ids)}

    async def _setup_exec_client_and_cache(self, update):
        """
        Called before processing a test streaming update - ensure all orders are in the cache in `update`.
        """
        venue_order_ids = self._prefill_venue_order_id_to_client_order_id(update)
        venue_order_id_to_client_order_id = {}
        for c_id, v_id in enumerate(venue_order_ids):
            self._log.debug(f"Adding client_order_id=[{c_id}], venue_order_id=[{v_id}] ")
            order = BetfairTestStubs.make_accepted_order(
                venue_order_id=VenueOrderId(str(v_id)), client_order_id=ClientOrderId(str(c_id))
            )
            self._log.debug(f"created order: {order}")
            venue_order_id_to_client_order_id[v_id] = order.client_order_id
            cache_order = self.cache.order(client_order_id=order.client_order_id)
            self._log.debug(f"Cached order: {order}")
            if cache_order is None:
                self._log.debug("Adding order to cache")
                self.cache.add_order(order, position_id=PositionId(v_id.value))
            self.cache.update_order(order)

        self.client.venue_order_id_to_client_order_id = venue_order_id_to_client_order_id

    async def _account_state(self):
        account_details = await self.betfair_client.get_account_details()
        account_funds = await self.betfair_client.get_account_funds()
        timestamp = self.clock.timestamp_ns()
        account_state = betfair_account_to_account_state(
            account_detail=account_details,
            account_funds=account_funds,
            event_id=self.uuid_factory.generate(),
            ts_event=timestamp,
            ts_init=timestamp,
        )
        return account_state

    @pytest.mark.asyncio
    async def test_submit_order(self):
        # Arrange
        command = BetfairTestStubs.submit_order_command()

        # Act
        mock_async(
            self.betfair_client, "place_orders", BetfairResponses.betting_place_order_success()
        )
        self.client.submit_order(command)
        await asyncio.sleep(0)

        # Assert
        submitted, accepted = self.messages
        assert isinstance(submitted, OrderSubmitted)
        assert isinstance(accepted, OrderAccepted)
        assert accepted.venue_order_id == VenueOrderId("228302937743")

    @pytest.mark.asyncio
    async def test_post_order_submit_error(self):
        # Arrange
        command = BetfairTestStubs.submit_order_command()

        # Act
        mock_async(
            self.betfair_client, "place_orders", BetfairResponses.betting_place_order_error()
        )
        self.client.submit_order(command)
        await asyncio.sleep(0)

        # Assert
        submitted, rejected = self.messages
        assert isinstance(submitted, OrderSubmitted)
        assert isinstance(rejected, OrderRejected)
        assert rejected.reason == "PERMISSION_DENIED: ERROR_IN_ORDER"

    @pytest.mark.asyncio
    async def test_update_order_success(self):
        # Arrange
        venue_order_id = VenueOrderId("240808576108")
        order = BetfairTestStubs.make_accepted_order(venue_order_id=venue_order_id)
        command = BetfairTestStubs.update_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
        )

        # Act
        self.cache.add_order(order, PositionId("1"))
        mock_async(
            self.betfair_client, "replace_orders", BetfairResponses.betting_replace_orders_success()
        )
        self.client.update_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_update, updated = self.messages
        assert isinstance(pending_update, OrderPendingUpdate)
        assert isinstance(updated, OrderUpdated)
        assert updated.price == Price.from_str("0.02000")

    @pytest.mark.asyncio
    async def test_update_order_error_order_doesnt_exist(self):
        # Arrange
        venue_order_id = VenueOrderId("229435133092")
        order = BetfairTestStubs.make_accepted_order(venue_order_id=venue_order_id)

        command = BetfairTestStubs.update_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
        )

        # Act
        mock_async(
            self.betfair_client, "replace_orders", BetfairResponses.betting_replace_orders_success()
        )
        self.client.update_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_update, rejected = self.messages
        assert isinstance(pending_update, OrderPendingUpdate)
        assert isinstance(rejected, OrderUpdateRejected)
        assert rejected.reason == "ORDER NOT IN CACHE"

    @pytest.mark.asyncio
    async def test_update_order_error_no_venue_id(self):
        # Arrange
        order = BetfairTestStubs.make_submitted_order()
        self.cache.add_order(order, position_id=BetfairTestStubs.position_id())

        command = BetfairTestStubs.update_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id="",
        )

        # Act
        mock_async(
            self.betfair_client, "replace_orders", BetfairResponses.betting_replace_orders_success()
        )
        self.client.update_order(command)
        await asyncio.sleep(0.1)

        # Assert
        pending_update, rejected = self.messages
        assert isinstance(pending_update, OrderPendingUpdate)
        assert isinstance(rejected, OrderUpdateRejected)
        assert rejected.reason == "ORDER MISSING VENUE_ORDER_ID"

    @pytest.mark.asyncio
    async def test_cancel_order_success(self):
        # Arrange
        order = BetfairTestStubs.make_submitted_order()
        self.cache.add_order(order, position_id=BetfairTestStubs.position_id())

        command = BetfairTestStubs.cancel_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("240564968665"),
        )

        # Act
        mock_async(
            self.betfair_client, "cancel_orders", BetfairResponses.betting_cancel_orders_success()
        )
        self.client.cancel_order(command)
        await asyncio.sleep(0.1)

        # Assert
        pending_cancel, cancelled = self.messages
        assert isinstance(pending_cancel, OrderPendingCancel)
        assert isinstance(cancelled, OrderCanceled)

    @pytest.mark.asyncio
    @pytest.mark.skip(reason="Needs input data")
    async def test_cancel_order_fail(self):
        pass

    @pytest.mark.asyncio
    async def test_cancel_order_response_and_streaming_update(self):
        # Arrange
        order = BetfairTestStubs.make_accepted_order(venue_order_id=VenueOrderId("240564968665"))
        await asyncio.sleep(0.1)

        order = self.cache.order(ClientOrderId("0"))
        await self._setup_exec_client_and_cache(update=BetfairStreaming.ocm_CANCEL())

        # Act
        # Receive response
        command = BetfairTestStubs.cancel_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("240564968665"),
        )
        mock_async(
            self.betfair_client, "cancel_orders", BetfairResponses.betting_cancel_orders_success()
        )
        self.client.cancel_order(command)
        await asyncio.sleep(0.1)

        # Send socket cancel
        update = BetfairStreaming.ocm_CANCEL()
        await self.client._handle_order_stream_update(update)

        # Assert
        pending_cancel, cancelled = self.messages
        assert isinstance(pending_cancel, OrderPendingCancel)
        assert isinstance(pending_cancel, OrderCanceled)

    @pytest.mark.asyncio
    async def test_streaming_orders_full_image_strategy(self):
        pass

    @pytest.mark.asyncio
    async def test_connection_account_state(self):
        await self.client.connection_account_state()
        assert isinstance(self.messages[0], AccountState)

    @pytest.mark.asyncio
    async def test_check_account_currency(self):
        await self.client.check_account_currency()

    @pytest.mark.asyncio
    async def test_order_stream_full_image(self):
        # Arrange
        # account_state = await self._account_state()

        update = BetfairStreaming.ocm_FULL_IMAGE()
        await self._setup_exec_client_and_cache(update=update)

        # Act
        await self.client._handle_order_stream_update(update=update)
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 12

    @pytest.mark.asyncio
    @pytest.mark.skip
    async def test_order_stream_empty_image(self):
        update = BetfairStreaming.ocm_EMPTY_IMAGE()
        self.client.handle_order_stream_update(update=update)
        await asyncio.sleep(0)
        assert len(self.messages) == 0

    @pytest.mark.asyncio
    @pytest.mark.skip
    async def test_order_stream_new_full_image(self):
        update = BetfairStreaming.ocm_NEW_FULL_IMAGE()
        self._setup_exec_client_and_cache(update)

        self.client.handle_order_stream_update(update=update)
        await asyncio.sleep(0)
        assert len(self.messages) == 6

    @pytest.mark.asyncio
    @pytest.mark.skip
    async def test_order_stream_sub_image(self):
        update = BetfairStreaming.ocm_SUB_IMAGE()
        self._prefill_venue_order_id_to_client_order_id(update)
        self.client.handle_order_stream_update(update=update)
        await asyncio.sleep(0)
        assert len(self.messages) == 0  # We don't do anything with matched bets at this stage

    @pytest.mark.asyncio
    @pytest.mark.skip
    async def test_order_stream_update(self):
        update = BetfairStreaming.ocm_UPDATE()
        self._setup_exec_client_and_cache(update)

        self.client.handle_order_stream_update(update=update)
        await asyncio.sleep(0.1)
        assert len(self.messages) == 1

    @pytest.mark.asyncio
    @pytest.mark.skip
    async def test_order_stream_cancel_after_update_doesnt_emit_event(self):
        update = BetfairStreaming.ocm_order_update()
        self._setup_exec_client_and_cache(update)

        s = set()
        s.add(("O-20210409-070830-001-001-1", "229506163591"))
        patch.object(self.client, "pending_update_order_client_ids", s)
        self.client.handle_order_stream_update(update=update)
        await asyncio.sleep(0.01)
        assert len(self.messages) == 0

    @pytest.mark.asyncio
    @pytest.mark.skip
    async def test_order_stream_filled(self):
        update = BetfairStreaming.ocm_FILLED()
        self._setup_exec_client_and_cache(update)

        self.client.handle_order_stream_update(update=update)
        await asyncio.sleep(0.01)
        assert len(self.messages) == 2
        event = self.messages[0]
        assert isinstance(event, OrderFilled)
        assert event.last_px == Price(0.90909, precision=5)

    @pytest.mark.asyncio
    @pytest.mark.skip
    async def test_order_stream_mixed(self):
        update = BetfairStreaming.ocm_MIXED()
        self._setup_exec_client_and_cache(update)

        self.client.handle_order_stream_update(update=update)
        await asyncio.sleep(0.5)
        events = self.messages
        assert len(events) == 5
        assert (
            isinstance(events[0], OrderFilled) and events[0].venue_order_id.value == "229430281341"
        )
        assert isinstance(events[1], AccountState)
        assert (
            isinstance(events[2], OrderFilled) and events[2].venue_order_id.value == "229430281339"
        )
        assert isinstance(events[3], AccountState)
        assert (
            isinstance(events[4], OrderCanceled)
            and events[4].venue_order_id.value == "229430281339"
        )

    @pytest.mark.asyncio
    @pytest.mark.skip(reason="Not implemented")
    async def test_generate_order_status_report(self):
        # Betfair client login
        patch(
            "betfairlightweight.endpoints.betting.Betting.list_current_orders",
            return_value=BetfairDataProvider.current_orders(),
        )
        patch(
            "betfairlightweight.endpoints.betting.Betting.list_current_orders",
            return_value=BetfairDataProvider.current_orders(),
        )
        result = await self.client.generate_order_status_report()
        assert result
        raise NotImplementedError()

    @pytest.mark.asyncio
    @pytest.mark.skip
    async def test_generate_trades_list(self):
        patch(
            "betfairlightweight.endpoints.betting.Betting.list_cleared_orders",
            return_value=BetfairDataProvider.list_cleared_orders(order_id="226125004209"),
        )
        patch.object(
            self.client,
            "venue_order_id_to_client_order_id",
            {"226125004209": ClientOrderId("1")},
        )

        result = await generate_trades_list(
            self=self.client, venue_order_id="226125004209", symbol=None, since=None
        )
        assert result

    @pytest.mark.asyncio
    @pytest.mark.skip
    async def test_duplicate_execution_id(self):
        patch.object(
            self.client,
            "venue_order_id_to_client_order_id",
            {"230486317487": ClientOrderId("1")},
        )

        # Load submitted orders
        kw = {
            "customer_order_ref": "0",
            "bet_id": "230486317487",
        }
        f = asyncio.Future()
        f.set_result(BetfairTestStubs.make_order_place_response())
        self.client._post_submit_order(
            f,
            BetfairTestStubs.strategy_id(),
            BetfairTestStubs.instrument_id(),
            ClientOrderId(kw["customer_order_ref"]),
        )

        kw = {
            "customer_order_ref": "1",
            "bet_id": "230487922962",
        }
        f = asyncio.Future()
        f.set_result(BetfairTestStubs.make_order_place_response(**kw))
        self.client._post_submit_order(
            f,
            BetfairTestStubs.strategy_id(),
            BetfairTestStubs.instrument_id(),
            ClientOrderId(kw["customer_order_ref"]),
        )

        # Act
        for update in BetfairStreaming.ocm_DUPLICATE_EXECUTION():
            self._setup_exec_client_and_cache(update=update)
            self.client.handle_order_stream_update(update=update)
            await asyncio.sleep(0.3)

        # Assert
        events = self.messages
        assert isinstance(events[0], OrderAccepted)
        assert isinstance(events[1], OrderAccepted)
        # First order example, partial fill followed by remainder canceled
        assert isinstance(events[2], OrderFilled)
        assert isinstance(events[3], AccountState)
        assert isinstance(events[4], OrderCanceled)
        # Second order example, partial fill followed by remainder filled
        assert (
            isinstance(events[5], OrderFilled)
            and events[5].execution_id.value == "4721ad7594e7a4a4dffb1bacb0cb45ccdec0747a"
        )
        assert isinstance(events[6], AccountState)
        assert (
            isinstance(events[7], OrderFilled)
            and events[7].execution_id.value == "8b3e65be779968a3fdf2d72731c848c5153e88cd"
        )
        assert isinstance(events[8], AccountState)

    @pytest.mark.asyncio
    @pytest.mark.skip(reason="Not implemented yet")
    async def test_betfair_account_states(self):
        # Setup
        balance = self.cache.account_for_venue(BETFAIR_VENUE).balances()[GBP]
        expected = {
            "type": "AccountBalance",
            "currency": "GBP",
            "total": "1000.00",
            "locked": "-0.00",
            "free": "1000.00",
        }
        assert balance.to_dict() == expected

        # Create an order to buy at 0.5 ($2.0) for $10 - exposure is $20
        order = BetfairTestStubs.make_order(
            price=Price.from_str("0.5"), quantity=Quantity.from_int(10)
        )

        # Order accepted - expect balance to drop by exposure
        order_accepted = BetfairTestStubs.event_order_accepted(order=order)
        self.msgbus._handle_event(order_accepted)
        await asyncio.sleep(0.1)
        balance = self.cache.account_for_venue(BETFAIR_VENUE).balances()[GBP]
        expected = {
            "type": "AccountBalance",
            "currency": "GBP",
            "total": "1000.00",
            "locked": "20.00",
            "free": "980.00",
        }
        assert balance.to_dict() == expected

        # Cancel the order, balance should return
        cancelled = BetfairTestStubs.event_order_canceled(order=order)
        self.msgbus._handle_event(cancelled)
        await asyncio.sleep(0.1)
        balance = self.client.engine.cache.account_for_venue(BETFAIR_VENUE).balances()[GBP]
        expected = {
            "type": "AccountBalance",
            "currency": "GBP",
            "total": "1000.00",
            "locked": "-0.00",
            "free": "1080.00",
        }
        assert balance.to_dict() == expected
