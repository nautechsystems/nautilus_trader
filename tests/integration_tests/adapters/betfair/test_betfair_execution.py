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
from unittest.mock import MagicMock
from unittest.mock import patch

import orjson
import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.execution import BetfairClient
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
from tests.integration_tests.adapters.betfair.test_kit import TEST_PATH
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.integration_tests.adapters.betfair.test_kit import mock_betfair_request
from tests.test_kit.stubs import TestStubs


class TestBetfairExecutionClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()

        self.trader_id = TestStubs.trader_id()
        self.venue = BETFAIR_VENUE
        self.account_id = AccountId(self.venue.value, "001")

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

        self.betfair_client: BetfairClient = BetfairTestStubs.betfair_client(
            loop=self.loop, logger=self.logger
        )
        assert self.betfair_client.session_token
        self.instrument_provider = BetfairTestStubs.instrument_provider(
            betfair_client=self.betfair_client
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
            instrument_provider=self.instrument_provider,
            market_filter={},
        )

        self.exec_engine.register_client(self.client)

        # Re-route exec engine messages through `handler`
        self.messages = []

        def handler(func):
            def inner(x):
                self.messages.append(x)
                return func(x)

            return inner

        def listener(x):
            print(x)

        self.msgbus.subscribe("*", listener)

        self.msgbus.deregister(endpoint="ExecEngine.execute", handler=self.exec_engine.execute)
        self.msgbus.register(
            endpoint="ExecEngine.execute", handler=handler(self.exec_engine.execute)
        )

        self.msgbus.deregister(endpoint="ExecEngine.process", handler=self.exec_engine.process)
        self.msgbus.register(
            endpoint="ExecEngine.process", handler=handler(self.exec_engine.process)
        )

        self.msgbus.deregister(
            endpoint="Portfolio.update_account", handler=self.portfolio.update_account
        )
        self.msgbus.register(
            endpoint="Portfolio.update_account", handler=handler(self.portfolio.update_account)
        )

    def _prefill_venue_order_id_to_client_order_id(self, update):
        order_ids = [
            update["id"]
            for market in update.get("oc", [])
            for order in market.get("orc", [])
            for update in order.get("uo", [])
        ]
        return {VenueOrderId(oid): ClientOrderId(str(i + 1)) for i, oid in enumerate(order_ids)}

    async def _setup_account(self):
        await self.client.connection_account_state()

    def _setup_exec_client_and_cache(self, update):
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
    async def test_submit_order_success(self):
        # Arrange
        command = BetfairTestStubs.submit_order_command()
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_place_order_success())

        # Act
        self.client.submit_order(command)
        await asyncio.sleep(0)

        # Assert
        submitted, accepted = self.messages
        assert isinstance(submitted, OrderSubmitted)
        assert isinstance(accepted, OrderAccepted)
        assert accepted.venue_order_id == VenueOrderId("228302937743")

    @pytest.mark.asyncio
    async def test_submit_order_error(self):
        # Arrange
        command = BetfairTestStubs.submit_order_command()
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_place_order_error())

        # Act
        self.client.submit_order(command)
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
        order = BetfairTestStubs.make_accepted_order(venue_order_id=venue_order_id)
        command = BetfairTestStubs.modify_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_replace_orders_success())

        # Act
        self.cache.add_order(order, PositionId("1"))
        self.client.modify_order(command)
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
        order = BetfairTestStubs.make_accepted_order(venue_order_id=venue_order_id)

        command = BetfairTestStubs.modify_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_replace_orders_success())

        # Act
        self.client.modify_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_update, rejected = self.messages
        assert isinstance(pending_update, OrderPendingUpdate)
        assert isinstance(rejected, OrderModifyRejected)
        assert rejected.reason == "ORDER NOT IN CACHE"

    @pytest.mark.asyncio
    async def test_modify_order_error_no_venue_id(self):
        # Arrange
        order = BetfairTestStubs.make_submitted_order()
        self.cache.add_order(order, position_id=BetfairTestStubs.position_id())

        command = BetfairTestStubs.modify_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id="",
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_replace_orders_success())

        # Act
        self.client.modify_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_update, rejected = self.messages
        assert isinstance(pending_update, OrderPendingUpdate)
        assert isinstance(rejected, OrderModifyRejected)
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
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_cancel_orders_success())

        # Act
        self.client.cancel_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_cancel, cancelled = self.messages
        assert isinstance(pending_cancel, OrderPendingCancel)
        assert isinstance(cancelled, OrderCanceled)

    @pytest.mark.asyncio
    async def test_cancel_order_fail(self):
        # Arrange
        order = BetfairTestStubs.make_submitted_order()
        self.cache.add_order(order, position_id=BetfairTestStubs.position_id())

        command = BetfairTestStubs.cancel_order_command(
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=VenueOrderId("228302937743"),
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_cancel_orders_error())

        # Act
        self.client.cancel_order(command)
        await asyncio.sleep(0)

        # Assert
        pending_cancel, cancelled = self.messages
        assert isinstance(pending_cancel, OrderPendingCancel)
        assert isinstance(cancelled, OrderCancelRejected)

    @pytest.mark.asyncio
    @pytest.mark.skip(reason="Not implemented")
    async def test_streaming_orders_full_image_strategy(self):
        pass

    @pytest.mark.asyncio
    async def test_connection_account_state(self):
        # Arrange, Act, Assert

        await self.client.connection_account_state()

        # Assert
        assert self.cache.account(self.account_id)

    @pytest.mark.asyncio
    async def test_check_account_currency(self):
        # Arrange, Act, Assert
        await self.client.check_account_currency()

    @pytest.mark.asyncio
    async def test_order_stream_full_image(self):
        # Arrange
        update = BetfairStreaming.ocm_FULL_IMAGE()
        await self._setup_account()
        self._setup_exec_client_and_cache(update=update)

        # Act
        await self.client._handle_order_stream_update(update=update)
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 7

    @pytest.mark.asyncio
    async def test_order_stream_empty_image(self):
        # Arrange
        update = BetfairStreaming.ocm_EMPTY_IMAGE()
        await self._setup_account()
        self._setup_exec_client_and_cache(update=update)

        # Act
        await self.client._handle_order_stream_update(update=update)
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 1

    @pytest.mark.asyncio
    async def test_order_stream_new_full_image(self):
        update = BetfairStreaming.ocm_NEW_FULL_IMAGE()
        await self._setup_account()
        self._setup_exec_client_and_cache(update=update)

        await self.client._handle_order_stream_update(update=update)
        await asyncio.sleep(0)
        assert len(self.messages) == 4

    @pytest.mark.asyncio
    async def test_order_stream_sub_image(self):
        # Arrange
        update = BetfairStreaming.ocm_SUB_IMAGE()
        await self._setup_account()
        self._setup_exec_client_and_cache(update=update)

        # Act
        await self.client._handle_order_stream_update(update=update)
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 1

    @pytest.mark.asyncio
    async def test_order_stream_update(self):
        # Arrange
        update = BetfairStreaming.ocm_UPDATE()
        await self._setup_account()
        self._setup_exec_client_and_cache(update=update)

        # Act
        await self.client._handle_order_stream_update(update=update)
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 2

    @pytest.mark.asyncio
    async def test_order_stream_filled(self):
        # Arrange
        update = BetfairStreaming.ocm_FILLED()
        self._setup_exec_client_and_cache(update)
        await self._setup_account()

        # Act
        await self.client._handle_order_stream_update(update=update)
        await asyncio.sleep(0)

        # Assert
        assert len(self.messages) == 2
        assert isinstance(self.messages[1], OrderFilled)
        assert self.messages[1].last_px == Price.from_str("0.9090909")

    @pytest.mark.asyncio
    async def test_order_stream_mixed(self):
        # Arrange
        update = BetfairStreaming.ocm_MIXED()
        self._setup_exec_client_and_cache(update)
        await self._setup_account()

        # Act
        await self.client._handle_order_stream_update(update=update)
        await asyncio.sleep(0)

        # Assert
        _, fill1, fill2, cancel = self.messages
        assert isinstance(fill1, OrderFilled) and fill1.venue_order_id.value == "229430281341"
        assert isinstance(fill2, OrderFilled) and fill2.venue_order_id.value == "229430281339"
        assert isinstance(cancel, OrderCanceled) and cancel.venue_order_id.value == "229430281339"

    @pytest.mark.asyncio
    @pytest.mark.skip(reason="Not implemented")
    async def test_generate_order_status_report(self):
        # Betfair client login
        orders = await self.betfair_client.list_current_orders()
        for order in orders:
            result = await self.client.generate_order_status_report(order=order)
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
    async def test_duplicate_execution_id(self):
        # Arrange
        await self._setup_account()
        for update in BetfairStreaming.ocm_DUPLICATE_EXECUTION():
            self._setup_exec_client_and_cache(update)

        # # Load submitted orders
        # for client_order_id in (ClientOrderId('0'), ClientOrderId('1')):
        #     order = BetfairTestStubs.make_order(
        #         price=Price.from_str("0.5"), quantity=Quantity.from_int(10), client_order_id=client_order_id
        #     )
        #     command = BetfairTestStubs.submit_order_command(order=order)
        #     self.client.submit_order(command)
        # await asyncio.sleep(0)

        # Act
        for update in BetfairStreaming.ocm_DUPLICATE_EXECUTION():
            self._setup_exec_client_and_cache(update=update)
            await self.client._handle_order_stream_update(update=update)
            await asyncio.sleep(0)

        # Assert
        _, fill1, cancel, fill2, fill3 = self.messages
        # First order example, partial fill followed by remainder canceled
        assert isinstance(fill1, OrderFilled)
        assert isinstance(cancel, OrderCanceled)
        # Second order example, partial fill followed by remainder filled
        assert (
            isinstance(fill2, OrderFilled)
            and fill2.execution_id.value == "4721ad7594e7a4a4dffb1bacb0cb45ccdec0747a"
        )
        assert (
            isinstance(fill3, OrderFilled)
            and fill3.execution_id.value == "8b3e65be779968a3fdf2d72731c848c5153e88cd"
        )

    @pytest.mark.asyncio
    async def test_betfair_order_reduces_balance(self):
        # Arrange
        self.client.stream = MagicMock()
        self.exec_engine.start()
        self.exec_engine.connect()
        await asyncio.sleep(1)

        balance = self.cache.account_for_venue(self.venue).balances()[GBP]
        order = BetfairTestStubs.make_order(
            price=Price.from_str("0.5"), quantity=Quantity.from_int(10)
        )
        self.cache.add_order(order=order, position_id=None)
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_place_order_success())
        command = BetfairTestStubs.submit_order_command(order=order)
        self.client.submit_order(command)
        await asyncio.sleep(0.01)

        # Act
        balance_order = self.cache.account_for_venue(BETFAIR_VENUE).balances()[GBP]

        # Cancel the order, balance should return
        command = BetfairTestStubs.cancel_order_command(
            client_order_id=order.client_order_id, venue_order_id=order.venue_order_id
        )
        mock_betfair_request(self.betfair_client, BetfairResponses.betting_cancel_orders_success())
        self.client.cancel_order(command)
        await asyncio.sleep(0.1)
        balance_cancel = self.cache.account_for_venue(BETFAIR_VENUE).balances()[GBP]

        # Assert
        assert balance.free == Money(1000.0, GBP)
        assert balance_order.free == Money(990.0, GBP)
        assert balance_cancel.free == Money(1000.0, GBP)

        self.exec_engine.kill()
        await asyncio.sleep(1)

    @pytest.mark.skip(reason="not implemented")
    @pytest.mark.asyncio
    async def test_replay(self):
        # Arrange
        self.client.stream = MagicMock()
        self.exec_engine.start()
        self.exec_engine.connect()
        await asyncio.sleep(1)

        fn = TEST_PATH.joinpath("streaming/streaming_order_stream.json")

        for update in orjson.loads(fn.read_bytes()):
            await self.client._handle_order_stream_update(update=update)
            await asyncio.sleep(0.1)
        raise ZeroDivisionError
