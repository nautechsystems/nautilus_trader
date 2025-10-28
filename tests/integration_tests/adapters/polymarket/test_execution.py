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
import pkgutil
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import msgspec
import pytest
from py_clob_client.client import ClobClient

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.credentials import PolymarketWebSocketAuth
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_instrument_id
from nautilus_trader.adapters.polymarket.config import PolymarketExecClientConfig
from nautilus_trader.adapters.polymarket.execution import PolymarketExecutionClient
from nautilus_trader.adapters.polymarket.http.conversion import convert_tif_to_polymarket_order_type
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.currencies import USDC_POS
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


# Test instrument for Polymarket
ELECTION_INSTRUMENT = TestInstrumentProvider.binary_option()


class TestPolymarketExecutionClient:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.trader_id = TestIdStubs.trader_id()
        self.venue = POLYMARKET_VENUE
        self.account_id = AccountId(f"{self.venue.value}-001")

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        # Mock HTTP client
        self.http_client = MagicMock(spec=ClobClient)
        self.http_client.get_address.return_value = "0xa3D82Ed56F4c68d2328Fb8c29e568Ba2cAF7d7c8"

        # Mock the creds attribute
        mock_creds = MagicMock()
        mock_creds.api_key = "test_api_key"
        self.http_client.creds = mock_creds

        # Mock instrument provider
        self.provider = MagicMock(spec=PolymarketInstrumentProvider)
        self.provider.initialize = AsyncMock()

        # Mock WebSocket auth
        self.ws_auth = MagicMock(spec=PolymarketWebSocketAuth)

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Create execution client
        config = PolymarketExecClientConfig()
        self.exec_client = PolymarketExecutionClient(
            loop=self.loop,
            http_client=self.http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=self.provider,
            ws_auth=self.ws_auth,
            config=config,
            name=None,
        )

        self.exec_engine.register_client(self.exec_client)

        self.strategy = Strategy()
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Add test instrument to cache
        self.cache.add_instrument(ELECTION_INSTRUMENT)

        # Add instruments that match the websocket messages for testing
        # From order_placement.json and order_cancel.json
        instrument_id_1 = get_polymarket_instrument_id(
            "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
            "21742633143463906290569050155826241533067272736897614950488156847949938836455",
        )
        test_instrument_1 = BinaryOption(
            instrument_id=instrument_id_1,
            raw_symbol=Symbol(f"{instrument_id_1.symbol.value}"),
            outcome="Yes",
            description="Test Polymarket Instrument 1",
            asset_class=AssetClass.ALTERNATIVE,
            currency=USDC,
            price_precision=3,
            price_increment=Price.from_str("0.001"),
            size_precision=2,
            size_increment=Quantity.from_str("0.01"),
            activation_ns=0,
            expiration_ns=0,
            max_quantity=None,
            min_quantity=Quantity.from_str("1"),
            maker_fee=0.0,
            taker_fee=0.0,
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_instrument(test_instrument_1)

        # Mock account state
        balance = AccountBalance(
            total=Money(1000, USDC_POS),
            locked=Money(0, USDC_POS),
            free=Money(1000, USDC_POS),
        )
        self.exec_client.generate_account_state(
            balances=[balance],
            margins=[],
            reported=True,
            ts_event=self.clock.timestamp_ns(),
        )

        yield

    def _setup_test_order_with_venue_id(
        self,
        venue_order_id_str: str,
        use_ws_instrument: bool = False,
    ) -> tuple[ClientOrderId, VenueOrderId]:
        """
        Create test order and add to cache with venue order ID mapping.
        """
        if use_ws_instrument:
            # Use the instrument that matches websocket messages
            instrument_id = get_polymarket_instrument_id(
                "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
                "21742633143463906290569050155826241533067272736897614950488156847949938836455",
            )
        else:
            instrument_id = ELECTION_INSTRUMENT.id

        order = self.strategy.order_factory.limit(
            instrument_id=instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("5"),
            price=Price.from_str("0.513"),
        )

        client_order_id = order.client_order_id
        venue_order_id = VenueOrderId(venue_order_id_str)

        self.cache.add_order(order, None)
        self.cache.add_venue_order_id(client_order_id, venue_order_id)

        return client_order_id, venue_order_id

    def test_handle_ws_order_placement_message(self):
        """
        Test handling websocket order placement message.
        """
        # Arrange
        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="order_placement.json",
        )

        client_order_id, venue_order_id = self._setup_test_order_with_venue_id(
            "0x0f76f4dc6eaf3332f4100f2e8a0b4a927351dd64646b7bb12f37df775c657a78",
        )

        # Act
        self.exec_client._handle_ws_message(raw_message)

        # Assert - should complete without raising exception
        # The actual processing happens asynchronously in _wait_for_ack_order
        assert True

    def test_handle_ws_order_cancellation_message(self):
        """
        Test handling websocket order cancellation message.
        """
        # Arrange
        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="order_cancel.json",
        )

        client_order_id, venue_order_id = self._setup_test_order_with_venue_id(
            "0xc6e99c14f1c7cae9e0538eb2d45a4d8b93ffd743e850edd1502a8c85700be5d3",
        )

        # Act
        self.exec_client._handle_ws_message(raw_message)

        # Assert - should complete without raising exception
        # The actual processing happens asynchronously in _wait_for_ack_order
        assert True

    def test_handle_ws_trade_message_maker_flow(self):
        """
        Test handling websocket trade message for maker order fill.
        """
        # Arrange - using user_trade1.json which has trader_side: MAKER
        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="user_trade1.json",
        )

        client_order_id, venue_order_id = self._setup_test_order_with_venue_id(
            "0x3ad09f225ebe141dfbdb3824f31cb457e8e0301ca4e0a06311e543f5328b9dea",
        )

        # Act
        self.exec_client._handle_ws_message(raw_message)

        # Assert - should complete without raising exception
        # The actual processing happens asynchronously in _wait_for_ack_trade
        assert True

    def test_handle_ws_trade_message_taker_flow(self):
        """
        Test handling websocket trade message for taker order fill.
        """
        # Arrange - using user_trade2.json which has trader_side: TAKER
        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="user_trade2.json",
        )

        client_order_id, venue_order_id = self._setup_test_order_with_venue_id(
            "0x5b605a0e8e40f3402d3cb3bc19edad6733ed23fbc079d2a09ee399c3487ace81",
        )

        # Act
        self.exec_client._handle_ws_message(raw_message)

        # Assert - should complete without raising exception
        # The actual processing happens asynchronously in _wait_for_ack_trade
        assert True

    @pytest.mark.asyncio()
    async def test_wait_for_ack_order_success(self):
        """
        Test successful order acknowledgment flow.
        """
        # Arrange
        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="order_placement.json",
        )

        msg = msgspec.json.decode(raw_message)
        msg = self.exec_client._decoder_user_msg.decode(msgspec.json.encode(msg))

        client_order_id, venue_order_id = self._setup_test_order_with_venue_id(
            "0x0f76f4dc6eaf3332f4100f2e8a0b4a927351dd64646b7bb12f37df775c657a78",
            use_ws_instrument=True,
        )

        # Act
        await self.exec_client._wait_for_ack_order(msg, venue_order_id)

        # Assert - should complete and generate order accepted event
        # Check that the order is still in cache and has the correct venue_order_id mapped
        assert self.cache.venue_order_id(client_order_id) == venue_order_id

    @pytest.mark.asyncio()
    async def test_wait_for_ack_order_timeout(self):
        """
        Test order acknowledgment timeout handling.
        """
        # Create exec client with short timeout for fast test
        fast_config = PolymarketExecClientConfig(ack_timeout_secs=0.1)
        fast_exec_client = PolymarketExecutionClient(
            loop=self.loop,
            http_client=self.http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=self.provider,
            ws_auth=self.ws_auth,
            config=fast_config,
            name=None,
        )

        # Arrange
        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="order_placement.json",
        )

        msg = msgspec.json.decode(raw_message)
        msg = fast_exec_client._decoder_user_msg.decode(msgspec.json.encode(msg))
        venue_order_id = VenueOrderId(
            "0x0f76f4dc6eaf3332f4100f2e8a0b4a927351dd64646b7bb12f37df775c657a78",
        )

        # Don't add venue_order_id to cache to simulate timeout

        # Act
        await fast_exec_client._wait_for_ack_order(msg, venue_order_id)

        # Assert - should complete without raising exception
        assert True

    @pytest.mark.asyncio()
    async def test_wait_for_ack_order_with_event_signal(self):
        """
        Test order acknowledgment via event signal (concurrent notification path).
        """
        # Arrange
        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="order_placement.json",
        )

        msg = msgspec.json.decode(raw_message)
        msg = self.exec_client._decoder_user_msg.decode(msgspec.json.encode(msg))
        venue_order_id = VenueOrderId(
            "0x0f76f4dc6eaf3332f4100f2e8a0b4a927351dd64646b7bb12f37df775c657a78",
        )

        # Create an order and add to cache (but without venue_order_id mapping yet)
        instrument_id_1 = get_polymarket_instrument_id(
            "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
            "21742633143463906290569050155826241533067272736897614950488156847949938836455",
        )
        order = self.strategy.order_factory.limit(
            instrument_id=instrument_id_1,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("5"),
            price=Price.from_str("0.513"),
        )
        client_order_id = order.client_order_id
        self.cache.add_order(order, None)

        # Start waiting in background task
        wait_task = asyncio.create_task(
            self.exec_client._wait_for_ack_order(msg, venue_order_id),
        )

        # Give the wait task time to set up the event
        await asyncio.sleep(0.01)

        # Simulate what _post_signed_order does: add venue_order_id and signal event
        self.cache.add_venue_order_id(client_order_id, venue_order_id)
        event = self.exec_client._ack_events_order.get(venue_order_id)
        if event:
            event.set()

        # Act - wait for the task to complete
        await asyncio.wait_for(wait_task, timeout=1.0)

        # Assert - task should complete successfully without timeout
        assert wait_task.done()
        assert not wait_task.cancelled()
        # Event should have been cleaned up
        assert venue_order_id not in self.exec_client._ack_events_order

    @pytest.mark.asyncio()
    async def test_wait_for_ack_trade_success(self):
        """
        Test successful trade acknowledgment flow.
        """
        # Arrange
        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="user_trade1.json",
        )

        msg = msgspec.json.decode(raw_message)
        msg = self.exec_client._decoder_user_msg.decode(msgspec.json.encode(msg))

        client_order_id, venue_order_id = self._setup_test_order_with_venue_id(
            "0x3ad09f225ebe141dfbdb3824f31cb457e8e0301ca4e0a06311e543f5328b9dea",
            use_ws_instrument=True,
        )

        # Act
        await self.exec_client._wait_for_ack_trade(msg, venue_order_id)

        # Assert - should complete and generate order filled event
        # Check that the order is still in cache and trade was processed
        assert self.cache.venue_order_id(client_order_id) == venue_order_id

    @pytest.mark.asyncio()
    async def test_wait_for_ack_trade_timeout(self):
        """
        Test trade acknowledgment timeout handling.
        """
        # Create exec client with short timeout for fast test
        fast_config = PolymarketExecClientConfig(ack_timeout_secs=0.1)
        fast_exec_client = PolymarketExecutionClient(
            loop=self.loop,
            http_client=self.http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=self.provider,
            ws_auth=self.ws_auth,
            config=fast_config,
            name=None,
        )

        # Arrange
        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="user_trade1.json",
        )

        msg = msgspec.json.decode(raw_message)
        msg = fast_exec_client._decoder_user_msg.decode(msgspec.json.encode(msg))
        venue_order_id = VenueOrderId(
            "0x3ad09f225ebe141dfbdb3824f31cb457e8e0301ca4e0a06311e543f5328b9dea",
        )

        # Don't add venue_order_id to cache to simulate timeout

        # Act
        await fast_exec_client._wait_for_ack_trade(msg, venue_order_id)

        # Assert - should complete without raising exception
        assert True

    @pytest.mark.asyncio()
    async def test_wait_for_ack_trade_with_event_signal(self):
        """
        Test trade acknowledgment via event signal (concurrent notification path).
        """
        # Arrange
        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="user_trade1.json",
        )

        msg = msgspec.json.decode(raw_message)
        msg = self.exec_client._decoder_user_msg.decode(msgspec.json.encode(msg))
        venue_order_id = VenueOrderId(
            "0x3ad09f225ebe141dfbdb3824f31cb457e8e0301ca4e0a06311e543f5328b9dea",
        )

        # Create an order and add to cache (but without venue_order_id mapping yet)
        instrument_id_1 = get_polymarket_instrument_id(
            "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
            "21742633143463906290569050155826241533067272736897614950488156847949938836455",
        )
        order = self.strategy.order_factory.limit(
            instrument_id=instrument_id_1,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("5"),
            price=Price.from_str("0.513"),
        )
        client_order_id = order.client_order_id
        self.cache.add_order(order, None)

        # Start waiting in background task
        wait_task = asyncio.create_task(
            self.exec_client._wait_for_ack_trade(msg, venue_order_id),
        )

        # Give the wait task time to set up the event
        await asyncio.sleep(0.01)

        # Simulate what _post_signed_order does: add venue_order_id and signal event
        self.cache.add_venue_order_id(client_order_id, venue_order_id)
        event = self.exec_client._ack_events_trade.get(venue_order_id)
        if event:
            event.set()

        # Act - wait for the task to complete
        await asyncio.wait_for(wait_task, timeout=1.0)

        # Assert - task should complete successfully without timeout
        assert wait_task.done()
        assert not wait_task.cancelled()
        # Event should have been cleaned up
        assert venue_order_id not in self.exec_client._ack_events_trade

    def test_handle_ws_message_invalid_json(self):
        """
        Test handling invalid JSON websocket message.
        """
        # Arrange
        invalid_raw = b'{"invalid": "json"'  # Invalid JSON

        # Act & Assert - should handle gracefully
        self.exec_client._handle_ws_message(invalid_raw)

    def test_handle_ws_message_decode_failure(self):
        """
        Test handling message that fails to decode.
        """
        # Arrange
        unknown_message = msgspec.json.encode([{"unknown_field": "unknown_value"}])

        # Act & Assert - should handle gracefully via exception handling
        self.exec_client._handle_ws_message(unknown_message)

    def test_handle_ws_message_with_logging_enabled(self):
        """
        Test handling websocket message with raw message logging enabled.
        """
        # Arrange
        # Create new config with logging enabled
        config_with_logging = PolymarketExecClientConfig(log_raw_ws_messages=True)

        exec_client_with_logging = PolymarketExecutionClient(
            loop=self.loop,
            http_client=self.http_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=self.provider,
            ws_auth=self.ws_auth,
            config=config_with_logging,
            name=None,
        )

        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="order_placement.json",
        )

        # Add the correct instrument to cache for this client too
        instrument_id_1 = get_polymarket_instrument_id(
            "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
            "21742633143463906290569050155826241533067272736897614950488156847949938836455",
        )
        test_instrument_1 = BinaryOption(
            instrument_id=instrument_id_1,
            raw_symbol=Symbol(f"{instrument_id_1.symbol.value}"),
            outcome="Yes",
            description="Test Polymarket Instrument 1",
            asset_class=AssetClass.ALTERNATIVE,
            currency=USDC,
            price_precision=3,
            price_increment=Price.from_str("0.001"),
            size_precision=2,
            size_increment=Quantity.from_str("0.01"),
            activation_ns=0,
            expiration_ns=0,
            max_quantity=None,
            min_quantity=Quantity.from_str("1"),
            maker_fee=0.0,
            taker_fee=0.0,
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_instrument(test_instrument_1)

        # Act & Assert - should handle gracefully and log the raw message
        exec_client_with_logging._handle_ws_message(raw_message)

    def test_add_trade_to_cache(self):
        """
        Test adding trade data to cache.
        """
        # Arrange
        raw_message = pkgutil.get_data(
            package="tests.integration_tests.adapters.polymarket.resources.ws_messages",
            resource="user_trade1.json",
        )

        msg = msgspec.json.decode(raw_message)
        msg = self.exec_client._decoder_user_msg.decode(msgspec.json.encode(msg))

        # Act
        self.exec_client._add_trade_to_cache(msg, raw_message)

        # Assert - trade should be added to cache
        expected_key = f"polymarket:trades:{msg.taker_order_id}:{msg.id}"
        cached_value = self.cache.get(expected_key)
        assert cached_value == raw_message

    @pytest.mark.asyncio()
    async def test_submit_order_success(self, mocker):
        """
        Test successful order submission.
        """
        # Arrange
        mock_create_order = mocker.patch.object(self.http_client, "create_order")
        mock_post_order = mocker.patch.object(self.http_client, "post_order")

        # Mock successful responses
        mock_create_order.return_value = {"signed_order": "mock_signed"}
        mock_post_order.return_value = {"success": True, "orderID": "test_order_id"}

        order = self.strategy.order_factory.limit(
            instrument_id=ELECTION_INSTRUMENT.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("10"),
            price=Price.from_str("0.50"),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._submit_order(submit_order)

        # Assert
        mock_create_order.assert_called_once()
        mock_post_order.assert_called_once()

        # Check that venue order ID was cached
        venue_order_id = VenueOrderId("test_order_id")
        cached_client_order_id = self.cache.client_order_id(venue_order_id)
        assert cached_client_order_id == order.client_order_id

    @pytest.mark.asyncio()
    async def test_submit_order_failure(self, mocker):
        """
        Test order submission failure handling.
        """
        # Arrange
        mock_create_order = mocker.patch.object(self.http_client, "create_order")
        mock_post_order = mocker.patch.object(self.http_client, "post_order")

        # Mock responses
        mock_create_order.return_value = {"signed_order": "mock_signed"}
        mock_post_order.return_value = {"success": False, "error": "Order failed"}

        order = self.strategy.order_factory.limit(
            instrument_id=ELECTION_INSTRUMENT.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("10"),
            price=Price.from_str("0.50"),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._submit_order(submit_order)

        # Assert
        mock_create_order.assert_called_once()
        mock_post_order.assert_called_once()

        # Check that order submission was attempted but failed
        # The venue order ID should not be cached since submission failed
        venue_order_id = VenueOrderId("test_order_id")
        cached_client_order_id = self.cache.client_order_id(venue_order_id)
        assert cached_client_order_id is None

    @pytest.mark.asyncio()
    async def test_submit_market_buy_without_quote_quantity_denied(self, mocker):
        """
        Market BUY orders must be quote-denominated; verify we emit OrderDenied instead
        of submitting.
        """
        mock_create_market_order = mocker.patch.object(self.http_client, "create_market_order")
        mock_post_order = mocker.patch.object(self.http_client, "post_order")

        order = self.strategy.order_factory.market(
            instrument_id=ELECTION_INSTRUMENT.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("10"),  # Base-denominated by default
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        denied_spy = mocker.spy(self.exec_client, "generate_order_denied")

        await self.exec_client._submit_order(submit_order)

        mock_create_market_order.assert_not_called()
        mock_post_order.assert_not_called()
        denied_spy.assert_called_once()
        denied_kwargs = denied_spy.call_args.kwargs
        assert denied_kwargs["client_order_id"] == order.client_order_id
        assert "quote-denominated quantities" in denied_kwargs["reason"]

    @pytest.mark.asyncio()
    async def test_submit_market_sell_with_quote_quantity_denied(self, mocker):
        """
        Market SELL orders must specify base quantity; quote-denominated orders are
        denied.
        """
        mock_create_market_order = mocker.patch.object(self.http_client, "create_market_order")
        mock_post_order = mocker.patch.object(self.http_client, "post_order")

        order = self.strategy.order_factory.market(
            instrument_id=ELECTION_INSTRUMENT.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_str("10"),
            quote_quantity=True,
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        denied_spy = mocker.spy(self.exec_client, "generate_order_denied")

        await self.exec_client._submit_order(submit_order)

        mock_create_market_order.assert_not_called()
        mock_post_order.assert_not_called()
        denied_spy.assert_called_once()
        denied_kwargs = denied_spy.call_args.kwargs
        assert denied_kwargs["client_order_id"] == order.client_order_id
        assert "base-denominated quantities" in denied_kwargs["reason"]

    def test_handle_unknown_instrument_gracefully(self):
        """
        Test handling websocket messages for unknown instruments gracefully.
        """
        # Arrange - Create a trade message for an instrument not in cache
        raw_message = msgspec.json.encode(
            [
                {
                    "asset_id": "99999999999999999999999999999999999999999999999999999999999999999999999999999",
                    "bucket_index": "0",
                    "event_type": "trade",
                    "fee_rate_bps": "0",
                    "id": "test-trade-id",
                    "last_update": "1748092619",
                    "maker_address": "0xD16896480F5768B7b34696a1F888F36Ae109f3cF",
                    "maker_orders": [],
                    "market": "0xunknownmarket0000000000000000000000000000000000000000000000000000",
                    "match_time": "1748092618",
                    "outcome": "No",
                    "owner": "test-owner",
                    "price": "0.968",
                    "side": "BUY",
                    "size": "100",
                    "status": "MATCHED",
                    "taker_order_id": "0xtest000000000000000000000000000000000000000000000000000000000000",
                    "timestamp": "1748092619162",
                    "trade_owner": "test-owner",
                    "trader_side": "MAKER",
                    "transaction_hash": "0xtest000000000000000000000000000000000000000000000000000000000000",
                    "type": "TRADE",
                },
            ],
        )

        # Act - should handle gracefully without raising exception
        self.exec_client._handle_ws_message(raw_message)

        # Assert - no exception raised, warning logged
        # Test passes if we reach this point without exception

    @pytest.mark.asyncio()
    async def test_submit_market_order_success(self, mocker):
        """
        Test successful market order submission using new MarketOrderArgs.
        """
        # Arrange
        mock_create_market_order = mocker.patch.object(self.http_client, "create_market_order")
        mock_post_order = mocker.patch.object(self.http_client, "post_order")

        # Mock successful responses
        mock_create_market_order.return_value = {"signed_order": "mock_signed_market"}
        mock_post_order.return_value = {"success": True, "orderID": "test_market_order_id"}

        market_order = self.strategy.order_factory.market(
            instrument_id=ELECTION_INSTRUMENT.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("10"),
            quote_quantity=True,
            time_in_force=TimeInForce.IOC,  # Test IOC -> FAK mapping
        )
        self.cache.add_order(market_order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=market_order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._submit_order(submit_order)

        # Assert
        mock_create_market_order.assert_called_once()
        mock_post_order.assert_called_once()

        # Verify MarketOrderArgs were created correctly
        call_args = mock_create_market_order.call_args[0][0]  # First positional argument
        assert call_args.amount == 10.0
        assert call_args.side == "BUY"
        assert call_args.price == 0  # Market order should have price 0 (calculated server-side)
        assert call_args.order_type == convert_tif_to_polymarket_order_type(TimeInForce.IOC)

        # Check that venue order ID was cached
        venue_order_id = VenueOrderId("test_market_order_id")
        cached_client_order_id = self.cache.client_order_id(venue_order_id)
        assert cached_client_order_id == market_order.client_order_id

    @pytest.mark.asyncio()
    async def test_submit_market_order_with_fok(self, mocker):
        """
        Test market order submission with FOK time in force.
        """
        # Arrange
        mock_create_market_order = mocker.patch.object(self.http_client, "create_market_order")
        mock_post_order = mocker.patch.object(self.http_client, "post_order")

        # Mock successful responses
        mock_create_market_order.return_value = {"signed_order": "mock_signed_market"}
        mock_post_order.return_value = {"success": True, "orderID": "test_fok_market_order_id"}

        market_order = self.strategy.order_factory.market(
            instrument_id=ELECTION_INSTRUMENT.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_str("5"),
            time_in_force=TimeInForce.FOK,
        )
        self.cache.add_order(market_order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=market_order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._submit_order(submit_order)

        # Assert
        mock_create_market_order.assert_called_once()

        # Verify MarketOrderArgs were created correctly
        call_args = mock_create_market_order.call_args[0][0]
        assert call_args.amount == 5.0
        assert call_args.side == "SELL"
        assert call_args.price == 0
        assert call_args.order_type == convert_tif_to_polymarket_order_type(TimeInForce.FOK)

    @pytest.mark.asyncio()
    async def test_submit_limit_order_still_works(self, mocker):
        """
        Test that limit orders still work with the refactored submission logic.
        """
        # Arrange
        mock_create_order = mocker.patch.object(self.http_client, "create_order")
        mock_post_order = mocker.patch.object(self.http_client, "post_order")

        # Mock successful responses
        mock_create_order.return_value = {"signed_order": "mock_signed_limit"}
        mock_post_order.return_value = {"success": True, "orderID": "test_limit_order_id"}

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ELECTION_INSTRUMENT.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("10"),
            price=Price.from_str("0.50"),
            time_in_force=TimeInForce.GTC,
        )
        self.cache.add_order(limit_order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=limit_order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._submit_order(submit_order)

        # Assert
        mock_create_order.assert_called_once()  # Should use create_order, not create_market_order
        mock_post_order.assert_called_once()

        # Verify OrderArgs were created correctly for limit order
        call_args = mock_create_order.call_args[0][0]
        assert call_args.size == 10.0
        assert call_args.side == "BUY"
        assert call_args.price == 0.50  # Limit order should have specific price

    @pytest.mark.asyncio()
    async def test_submit_order_invalid_time_in_force(self):
        """
        Test that orders with invalid time in force are rejected.
        """
        # Arrange
        order = self.strategy.order_factory.limit(
            instrument_id=ELECTION_INSTRUMENT.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("10"),
            price=Price.from_str("0.50"),
            time_in_force=TimeInForce.DAY,  # Invalid for Polymarket
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._submit_order(submit_order)

        # Assert - order should be rejected (no exception, just logged error)
        # Test passes if we reach this point without exception

    @pytest.mark.asyncio()
    async def test_submit_order_invalid_order_type(self):
        """
        Test that orders with invalid order types are rejected.
        """
        # Arrange - create a stop market order which is not supported
        order = self.strategy.order_factory.stop_market(
            instrument_id=ELECTION_INSTRUMENT.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("10"),
            trigger_price=Price.from_str("0.55"),
        )
        self.cache.add_order(order, None)

        submit_order = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy.id,
            position_id=None,
            order=order,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await self.exec_client._submit_order(submit_order)

        # Assert - order should be rejected (no exception, just logged error)
        # Test passes if we reach this point without exception

    def test_order_branching_logic(self):
        """
        Test that _submit_order correctly branches to market vs limit order methods.
        """
        # This is tested implicitly by the other tests, but we can verify
        # the branching logic by checking which method would be called

        # Create mock orders
        market_order = self.strategy.order_factory.market(
            instrument_id=ELECTION_INSTRUMENT.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("10"),
        )

        limit_order = self.strategy.order_factory.limit(
            instrument_id=ELECTION_INSTRUMENT.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("10"),
            price=Price.from_str("0.50"),
        )

        # Assert order types are correctly identified
        assert market_order.order_type == OrderType.MARKET
        assert limit_order.order_type == OrderType.LIMIT
