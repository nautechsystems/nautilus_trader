# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.polymarket.common.credentials import PolymarketWebSocketAuth
from nautilus_trader.adapters.polymarket.websocket.client import PolymarketWebSocketChannel
from nautilus_trader.adapters.polymarket.websocket.client import PolymarketWebSocketClient
from nautilus_trader.common.component import LiveClock


class TestPolymarketWebSocketClient:
    @pytest.fixture(autouse=True)
    def setup(self, event_loop):
        self.loop = event_loop
        self.clock = LiveClock()
        self.handler = MagicMock()
        self.handler_reconnect = AsyncMock()

    def create_client(
        self,
        channel: PolymarketWebSocketChannel = PolymarketWebSocketChannel.MARKET,
        auth: PolymarketWebSocketAuth | None = None,
    ) -> PolymarketWebSocketClient:
        return PolymarketWebSocketClient(
            clock=self.clock,
            base_url="wss://test.polymarket.com/ws/",
            channel=channel,
            handler=self.handler,
            handler_reconnect=self.handler_reconnect,
            loop=self.loop,
            auth=auth,
        )

    def test_init_creates_empty_client_tracking(self):
        client = self.create_client()

        assert client._clients == {}
        assert client._client_subscriptions == {}
        assert client._subscriptions == []
        assert client._next_client_id == 0

    def test_url_property_returns_correct_url_for_market_channel(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.MARKET)

        assert client.url == "wss://test.polymarket.com/ws/market"

    def test_url_property_returns_correct_url_for_user_channel(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.USER)

        assert client.url == "wss://test.polymarket.com/ws/user"

    def test_add_subscription_adds_to_subscriptions_list(self):
        client = self.create_client()

        client.add_subscription("token_123")

        assert "token_123" in client.subscriptions
        assert client.has_subscriptions is True

    def test_add_subscription_assigns_to_first_client(self):
        client = self.create_client()

        client.add_subscription("token_123")

        assert 0 in client._client_subscriptions
        assert "token_123" in client._client_subscriptions[0]

    def test_add_subscription_skips_duplicates(self):
        client = self.create_client()

        client.add_subscription("token_123")
        client.add_subscription("token_123")

        assert client.subscriptions.count("token_123") == 1

    def test_add_subscription_respects_max_per_client(self):
        client = self.create_client()

        # Add max subscriptions
        for i in range(client._max_subscriptions_per_connection):
            client.add_subscription(f"token_{i}")

        # This should go to a new client
        client.add_subscription("token_overflow")

        assert len(client._client_subscriptions) == 2
        assert len(client._client_subscriptions[0]) == client._max_subscriptions_per_connection
        assert len(client._client_subscriptions[1]) == 1
        assert "token_overflow" in client._client_subscriptions[1]

    def test_get_client_id_for_new_subscription_uses_existing_client_with_room(self):
        client = self.create_client()

        client.add_subscription("token_1")
        client_id = client._get_client_id_for_new_subscription()

        assert client_id == 0

    def test_get_client_id_for_new_subscription_creates_new_when_full(self):
        client = self.create_client()

        # Fill first client
        for i in range(client._max_subscriptions_per_connection):
            client.add_subscription(f"token_{i}")

        client_id = client._get_client_id_for_new_subscription()

        assert client_id == 1

    def test_get_client_for_subscription_finds_correct_client(self):
        client = self.create_client()

        client.add_subscription("token_1")
        client.add_subscription("token_2")

        assert client._get_client_for_subscription("token_1") == 0
        assert client._get_client_for_subscription("token_2") == 0

    def test_get_client_for_subscription_returns_minus_one_when_not_found(self):
        client = self.create_client()

        assert client._get_client_for_subscription("nonexistent") == -1

    def test_create_subscribe_market_channel_msg(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.MARKET)

        msg = client._create_subscribe_market_channel_msg(["token_1", "token_2"])

        assert msg == {
            "type": "market",
            "assets_ids": ["token_1", "token_2"],
        }

    def test_create_subscribe_user_channel_msg(self):
        auth = PolymarketWebSocketAuth(
            apiKey="test_key",
            secret="test_secret",
            passphrase="test_pass",
        )
        client = self.create_client(channel=PolymarketWebSocketChannel.USER, auth=auth)

        msg = client._create_subscribe_user_channel_msg(["market_1", "market_2"])

        assert msg == {
            "auth": auth,
            "type": "user",
            "markets": ["market_1", "market_2"],
        }

    def test_create_dynamic_subscribe_msg_for_market_channel(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.MARKET)

        msg = client._create_dynamic_subscribe_msg(["token_1"])

        assert msg == {
            "assets_ids": ["token_1"],
            "operation": "subscribe",
        }

    def test_create_dynamic_subscribe_msg_for_user_channel(self):
        auth = PolymarketWebSocketAuth(
            apiKey="test_key",
            secret="test_secret",
            passphrase="test_pass",
        )
        client = self.create_client(channel=PolymarketWebSocketChannel.USER, auth=auth)

        msg = client._create_dynamic_subscribe_msg(["market_1"])

        assert msg == {
            "auth": auth,
            "markets": ["market_1"],
            "operation": "subscribe",
        }

    def test_create_dynamic_unsubscribe_msg_for_market_channel(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.MARKET)

        msg = client._create_dynamic_unsubscribe_msg(["token_1"])

        assert msg == {
            "assets_ids": ["token_1"],
            "operation": "unsubscribe",
        }

    def test_create_dynamic_unsubscribe_msg_for_user_channel(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.USER)

        msg = client._create_dynamic_unsubscribe_msg(["market_1"])

        assert msg == {
            "markets": ["market_1"],
            "operation": "unsubscribe",
        }

    def test_is_connected_returns_false_when_no_clients(self):
        client = self.create_client()

        assert client.is_connected() is False
        assert client.is_disconnected() is True

    def test_is_connected_returns_false_when_clients_are_none(self):
        client = self.create_client()
        client._clients[0] = None

        assert client.is_connected() is False

    def test_is_connected_returns_true_when_client_is_active(self):
        client = self.create_client()
        mock_ws_client = MagicMock()
        mock_ws_client.is_active.return_value = True
        client._clients[0] = mock_ws_client

        assert client.is_connected() is True
        assert client.is_disconnected() is False

    def test_subscribe_market_adds_subscription(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.USER)

        client.subscribe_market("condition_123")

        assert "condition_123" in client.subscriptions

    def test_subscribe_book_adds_subscription(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.MARKET)

        client.subscribe_book("token_123")

        assert "token_123" in client.subscriptions

    def test_market_subscriptions_returns_list_for_user_channel(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.USER)

        client.add_subscription("market_1")
        client.add_subscription("market_2")

        assert client.market_subscriptions() == ["market_1", "market_2"]

    def test_market_subscriptions_returns_empty_for_market_channel(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.MARKET)

        client.add_subscription("token_1")

        assert client.market_subscriptions() == []

    def test_asset_subscriptions_returns_list_for_market_channel(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.MARKET)

        client.add_subscription("token_1")
        client.add_subscription("token_2")

        assert client.asset_subscriptions() == ["token_1", "token_2"]

    def test_asset_subscriptions_returns_empty_for_user_channel(self):
        client = self.create_client(channel=PolymarketWebSocketChannel.USER)

        client.add_subscription("market_1")

        assert client.asset_subscriptions() == []


class TestPolymarketWebSocketClientAsync:
    @pytest.fixture(autouse=True)
    def setup(self, event_loop):
        self.loop = event_loop
        self.clock = LiveClock()
        self.handler = MagicMock()
        self.handler_reconnect = AsyncMock()

    def create_client(
        self,
        channel: PolymarketWebSocketChannel = PolymarketWebSocketChannel.MARKET,
    ) -> PolymarketWebSocketClient:
        return PolymarketWebSocketClient(
            clock=self.clock,
            base_url="wss://test.polymarket.com/ws/",
            channel=channel,
            handler=self.handler,
            handler_reconnect=self.handler_reconnect,
            loop=self.loop,
        )

    @pytest.mark.asyncio
    async def test_subscribe_connects_new_client_when_none_exists(self):
        client = self.create_client()

        with patch.object(client, "_connect_client", new_callable=AsyncMock) as mock_connect:
            await client.subscribe("token_123")

            mock_connect.assert_called_once_with(0)

    @pytest.mark.asyncio
    async def test_subscribe_sends_dynamic_message_when_client_connected(self):
        client = self.create_client()

        # Setup: pretend client 0 is connected
        mock_ws_client = MagicMock()
        mock_ws_client.is_active.return_value = True
        client._clients[0] = mock_ws_client
        client._client_subscriptions[0] = ["existing_token"]
        client._subscriptions = ["existing_token"]

        with patch.object(client, "_send", new_callable=AsyncMock) as mock_send:
            await client.subscribe("new_token")

            mock_send.assert_called_once()
            call_args = mock_send.call_args
            assert call_args[0][0] == 0  # client_id
            assert call_args[0][1]["operation"] == "subscribe"
            assert "new_token" in call_args[0][1]["assets_ids"]

    @pytest.mark.asyncio
    async def test_subscribe_skips_already_subscribed(self):
        client = self.create_client()
        client._subscriptions = ["token_123"]
        client._subscription_counts["token_123"] = 1

        with patch.object(client, "_connect_client", new_callable=AsyncMock) as mock_connect:
            await client.subscribe("token_123")

            mock_connect.assert_not_called()
        # Count should be incremented
        assert client._subscription_counts["token_123"] == 2

    @pytest.mark.asyncio
    async def test_unsubscribe_removes_subscription(self):
        client = self.create_client()
        client._subscriptions = ["token_123", "token_456"]
        client._subscription_counts = {"token_123": 1, "token_456": 1}
        client._client_subscriptions[0] = ["token_123", "token_456"]

        mock_ws_client = MagicMock()
        mock_ws_client.is_active.return_value = True
        mock_ws_client.is_disconnecting.return_value = False
        mock_ws_client.is_closed.return_value = False
        mock_ws_client.disconnect = AsyncMock()
        client._clients[0] = mock_ws_client

        with patch.object(client, "_send", new_callable=AsyncMock):
            await client.unsubscribe("token_123")

        assert "token_123" not in client._subscriptions
        assert "token_123" not in client._client_subscriptions[0]
        # Client should NOT be disconnected since it still has subscriptions
        mock_ws_client.disconnect.assert_not_called()

    @pytest.mark.asyncio
    async def test_unsubscribe_disconnects_empty_client(self):
        client = self.create_client()
        client._subscriptions = ["token_123"]
        client._subscription_counts = {"token_123": 1}
        client._client_subscriptions[0] = ["token_123"]

        mock_ws_client = MagicMock()
        mock_ws_client.is_active.return_value = True
        mock_ws_client.is_disconnecting.return_value = False
        mock_ws_client.is_closed.return_value = False
        mock_ws_client.disconnect = AsyncMock()
        client._clients[0] = mock_ws_client

        with patch.object(client, "_send", new_callable=AsyncMock):
            await client.unsubscribe("token_123")

        mock_ws_client.disconnect.assert_called_once()

    @pytest.mark.asyncio
    async def test_unsubscribe_does_nothing_when_not_subscribed(self):
        client = self.create_client()

        # Should not raise, just log warning and return
        await client.unsubscribe("nonexistent")

        # Verify no state changes occurred
        assert client._subscriptions == []
        assert client._clients == {}

    @pytest.mark.asyncio
    async def test_connect_returns_early_when_no_subscriptions(self):
        client = self.create_client()

        with patch.object(client, "_connect_client", new_callable=AsyncMock) as mock_connect:
            await client.connect()

            # Should not attempt to connect any clients
            mock_connect.assert_not_called()

    @pytest.mark.asyncio
    async def test_connect_connects_all_clients_with_subscriptions(self):
        client = self.create_client()

        # Add subscriptions to multiple clients
        for i in range(client._max_subscriptions_per_connection + 10):
            client.add_subscription(f"token_{i}")

        with patch.object(client, "_connect_client", new_callable=AsyncMock) as mock_connect:
            await client.connect()

            assert mock_connect.call_count == 2

    @pytest.mark.asyncio
    async def test_disconnect_disconnects_all_clients(self):
        client = self.create_client()

        mock_ws_client_1 = MagicMock()
        mock_ws_client_1.is_disconnecting.return_value = False
        mock_ws_client_1.is_closed.return_value = False
        mock_ws_client_1.disconnect = AsyncMock()

        mock_ws_client_2 = MagicMock()
        mock_ws_client_2.is_disconnecting.return_value = False
        mock_ws_client_2.is_closed.return_value = False
        mock_ws_client_2.disconnect = AsyncMock()

        client._clients[0] = mock_ws_client_1
        client._clients[1] = mock_ws_client_2

        await client.disconnect()

        mock_ws_client_1.disconnect.assert_called_once()
        mock_ws_client_2.disconnect.assert_called_once()

    @pytest.mark.asyncio
    async def test_disconnect_client_skips_already_disconnecting(self):
        client = self.create_client()

        mock_ws_client = MagicMock()
        mock_ws_client.is_disconnecting.return_value = True
        mock_ws_client.disconnect = AsyncMock()
        client._clients[0] = mock_ws_client

        await client._disconnect_client(0)

        mock_ws_client.disconnect.assert_not_called()

    @pytest.mark.asyncio
    async def test_handle_reconnect_resubscribes_client(self):
        client = self.create_client()
        client._client_subscriptions[0] = ["token_1", "token_2"]

        mock_ws_client = MagicMock()
        mock_ws_client.send_text = AsyncMock()
        client._clients[0] = mock_ws_client

        # Call reconnect handler synchronously (it schedules async task)
        client._handle_reconnect(0)

        # Give the event loop a chance to run the scheduled task
        await asyncio.sleep(0.01)

    def test_handle_reconnect_returns_early_when_no_subscriptions(self):
        client = self.create_client()
        client._client_subscriptions[0] = []

        # Should not raise, just return early
        client._handle_reconnect(0)

        # Verify handler_reconnect was NOT called (since we returned early)
        if client._handler_reconnect:
            client._handler_reconnect.assert_not_called()


class TestPolymarketWebSocketClientMultiClient:
    """
    Tests specifically for multi-client behavior with 200 subscription limit.
    """

    @pytest.fixture(autouse=True)
    def setup(self, event_loop):
        self.loop = event_loop
        self.clock = LiveClock()
        self.handler = MagicMock()

    def create_client(self) -> PolymarketWebSocketClient:
        return PolymarketWebSocketClient(
            clock=self.clock,
            base_url="wss://test.polymarket.com/ws/",
            channel=PolymarketWebSocketChannel.MARKET,
            handler=self.handler,
            handler_reconnect=None,
            loop=self.loop,
        )

    def test_max_subscriptions_per_connection_default_is_200(self):
        client = self.create_client()

        assert client._max_subscriptions_per_connection == 200

    def test_max_subscriptions_per_connection_configurable(self):
        client = PolymarketWebSocketClient(
            clock=self.clock,
            base_url="wss://test.com/ws/",
            channel=PolymarketWebSocketChannel.MARKET,
            handler=lambda x: None,
            handler_reconnect=None,
            loop=self.loop,
            max_subscriptions_per_connection=50,
        )

        assert client._max_subscriptions_per_connection == 50

    def test_subscriptions_distributed_across_clients(self):
        client = self.create_client()

        # Add 450 subscriptions (should span 3 clients)
        for i in range(450):
            client.add_subscription(f"token_{i}")

        assert len(client._client_subscriptions) == 3
        assert len(client._client_subscriptions[0]) == 200
        assert len(client._client_subscriptions[1]) == 200
        assert len(client._client_subscriptions[2]) == 50

    def test_total_subscriptions_tracked_correctly(self):
        client = self.create_client()

        for i in range(250):
            client.add_subscription(f"token_{i}")

        assert len(client.subscriptions) == 250
        assert client.has_subscriptions is True

    def test_client_ids_increment_correctly(self):
        client = self.create_client()

        # Fill 3 clients
        for i in range(500):
            client.add_subscription(f"token_{i}")

        assert 0 in client._client_subscriptions
        assert 1 in client._client_subscriptions
        assert 2 in client._client_subscriptions
        assert client._next_client_id == 3

    def test_subscriptions_property_returns_copy(self):
        client = self.create_client()
        client.add_subscription("token_1")

        subs = client.subscriptions
        subs.append("token_2")

        # Internal state should not be affected
        assert "token_2" not in client._subscriptions
        assert len(client._subscriptions) == 1


class TestPolymarketWebSocketClientStateConsistency:
    """
    Tests verifying internal state consistency after various operations.
    """

    @pytest.fixture(autouse=True)
    def setup(self, event_loop):
        self.loop = event_loop
        self.clock = LiveClock()
        self.handler = MagicMock()

    def create_client(self) -> PolymarketWebSocketClient:
        return PolymarketWebSocketClient(
            clock=self.clock,
            base_url="wss://test.polymarket.com/ws/",
            channel=PolymarketWebSocketChannel.MARKET,
            handler=self.handler,
            handler_reconnect=None,
            loop=self.loop,
        )

    def assert_state_consistent(self, client: PolymarketWebSocketClient) -> None:
        """
        Verify internal state invariants hold.
        """
        # All subscriptions in _subscriptions should exist in exactly one client
        for sub in client._subscriptions:
            count = sum(1 for subs in client._client_subscriptions.values() if sub in subs)
            assert count == 1, f"Subscription {sub} found in {count} clients"

        # All client subscriptions should be in the global list
        for client_id, subs in client._client_subscriptions.items():
            for sub in subs:
                assert sub in client._subscriptions, (
                    f"Client {client_id} has subscription {sub} not in global list"
                )

        # Total subscriptions should match
        total_in_clients = sum(len(s) for s in client._client_subscriptions.values())
        assert total_in_clients == len(client._subscriptions)

    def test_state_consistent_after_add_subscriptions(self):
        client = self.create_client()

        for i in range(50):
            client.add_subscription(f"token_{i}")

        self.assert_state_consistent(client)

    def test_state_consistent_after_multi_client_add(self):
        client = self.create_client()

        for i in range(450):
            client.add_subscription(f"token_{i}")

        self.assert_state_consistent(client)

    @pytest.mark.asyncio
    async def test_state_consistent_after_unsubscribe(self):
        client = self.create_client()

        for i in range(10):
            client.add_subscription(f"token_{i}")

        mock_ws = MagicMock()
        mock_ws.is_active.return_value = True
        mock_ws.is_disconnecting.return_value = False
        mock_ws.is_closed.return_value = False
        mock_ws.disconnect = AsyncMock()
        client._clients[0] = mock_ws

        with patch.object(client, "_send", new_callable=AsyncMock):
            await client.unsubscribe("token_5")

        self.assert_state_consistent(client)
        assert "token_5" not in client._subscriptions

    @pytest.mark.asyncio
    async def test_state_consistent_after_unsubscribe_all_from_client(self):
        client = self.create_client()

        client.add_subscription("token_1")
        client.add_subscription("token_2")

        mock_ws = MagicMock()
        mock_ws.is_active.return_value = True
        mock_ws.is_disconnecting.return_value = False
        mock_ws.is_closed.return_value = False
        mock_ws.disconnect = AsyncMock()
        client._clients[0] = mock_ws

        with patch.object(client, "_send", new_callable=AsyncMock):
            await client.unsubscribe("token_1")
            await client.unsubscribe("token_2")

        # Client should be disconnected and state should be clean
        assert len(client._subscriptions) == 0
        assert len(client._client_subscriptions[0]) == 0

    @pytest.mark.asyncio
    async def test_resubscribe_after_unsubscribe(self):
        client = self.create_client()

        client.add_subscription("token_1")

        mock_ws = MagicMock()
        mock_ws.is_active.return_value = True
        mock_ws.is_disconnecting.return_value = False
        mock_ws.is_closed.return_value = False
        mock_ws.disconnect = AsyncMock()
        client._clients[0] = mock_ws

        with patch.object(client, "_send", new_callable=AsyncMock):
            await client.unsubscribe("token_1")

        # Now resubscribe
        with patch.object(client, "_connect_client", new_callable=AsyncMock):
            await client.subscribe("token_1")

        assert "token_1" in client._subscriptions
        self.assert_state_consistent(client)


class TestPolymarketWebSocketClientMultiClientUnsubscribe:
    """
    Tests for unsubscribe behavior across multiple clients.
    """

    @pytest.fixture(autouse=True)
    def setup(self, event_loop):
        self.loop = event_loop
        self.clock = LiveClock()
        self.handler = MagicMock()

    def create_client(self) -> PolymarketWebSocketClient:
        return PolymarketWebSocketClient(
            clock=self.clock,
            base_url="wss://test.polymarket.com/ws/",
            channel=PolymarketWebSocketChannel.MARKET,
            handler=self.handler,
            handler_reconnect=None,
            loop=self.loop,
        )

    @pytest.mark.asyncio
    async def test_unsubscribe_from_second_client(self):
        client = self.create_client()

        # Fill first client and add one to second
        for i in range(201):
            client.add_subscription(f"token_{i}")

        # token_200 should be in client 1
        assert "token_200" in client._client_subscriptions[1]

        mock_ws_0 = MagicMock()
        mock_ws_0.is_active.return_value = True
        mock_ws_0.is_disconnecting.return_value = False
        mock_ws_0.is_closed.return_value = False
        mock_ws_0.disconnect = AsyncMock()

        mock_ws_1 = MagicMock()
        mock_ws_1.is_active.return_value = True
        mock_ws_1.is_disconnecting.return_value = False
        mock_ws_1.is_closed.return_value = False
        mock_ws_1.disconnect = AsyncMock()

        client._clients[0] = mock_ws_0
        client._clients[1] = mock_ws_1

        with patch.object(client, "_send", new_callable=AsyncMock) as mock_send:
            await client.unsubscribe("token_200")

            # Should send to client 1, not client 0
            mock_send.assert_called_once()
            assert mock_send.call_args[0][0] == 1

        # Client 1 should be disconnected (no remaining subscriptions)
        mock_ws_1.disconnect.assert_called_once()
        # Client 0 should NOT be disconnected
        mock_ws_0.disconnect.assert_not_called()

    @pytest.mark.asyncio
    async def test_unsubscribe_from_middle_of_client_list(self):
        client = self.create_client()

        # Fill first client completely
        for i in range(200):
            client.add_subscription(f"token_{i}")

        # Add 50 to second client
        for i in range(200, 250):
            client.add_subscription(f"token_{i}")

        mock_ws_0 = MagicMock()
        mock_ws_0.is_active.return_value = True
        mock_ws_1 = MagicMock()
        mock_ws_1.is_active.return_value = True
        mock_ws_1.is_disconnecting.return_value = False
        mock_ws_1.is_closed.return_value = False
        mock_ws_1.disconnect = AsyncMock()

        client._clients[0] = mock_ws_0
        client._clients[1] = mock_ws_1

        with patch.object(client, "_send", new_callable=AsyncMock):
            # Unsubscribe from the middle of client 1's subscriptions
            await client.unsubscribe("token_225")

        assert "token_225" not in client._subscriptions
        assert len(client._client_subscriptions[1]) == 49
        # Client 1 should NOT be disconnected (still has subscriptions)
        mock_ws_1.disconnect.assert_not_called()


class TestPolymarketWebSocketClientConcurrency:
    """
    Tests for concurrent operations.
    """

    @pytest.fixture(autouse=True)
    def setup(self, event_loop):
        self.loop = event_loop
        self.clock = LiveClock()
        self.handler = MagicMock()

    def create_client(self) -> PolymarketWebSocketClient:
        return PolymarketWebSocketClient(
            clock=self.clock,
            base_url="wss://test.polymarket.com/ws/",
            channel=PolymarketWebSocketChannel.MARKET,
            handler=self.handler,
            handler_reconnect=None,
            loop=self.loop,
        )

    @pytest.mark.asyncio
    async def test_concurrent_subscribes_to_different_tokens(self):
        client = self.create_client()

        with patch.object(client, "_connect_client", new_callable=AsyncMock):
            # Subscribe to multiple tokens concurrently
            await asyncio.gather(
                client.subscribe("token_1"),
                client.subscribe("token_2"),
                client.subscribe("token_3"),
            )

        assert len(client._subscriptions) == 3
        assert "token_1" in client._subscriptions
        assert "token_2" in client._subscriptions
        assert "token_3" in client._subscriptions

    @pytest.mark.asyncio
    async def test_subscribe_waits_for_connecting_client(self):
        client = self.create_client()

        # Simulate client 0 is currently connecting
        client._is_connecting[0] = True
        client._clients[0] = None
        client._client_subscriptions[0] = []

        async def delayed_connect():
            await asyncio.sleep(0.05)
            client._is_connecting[0] = False
            mock_ws = MagicMock()
            mock_ws.is_active.return_value = True
            client._clients[0] = mock_ws

        with patch.object(client, "_send", new_callable=AsyncMock):
            # Start delayed connect and subscribe concurrently
            await asyncio.gather(
                delayed_connect(),
                client.subscribe("token_1"),
            )

        # Subscribe should have waited and used the connected client
        assert "token_1" in client._subscriptions

    @pytest.mark.asyncio
    async def test_concurrent_subscribe_same_token_only_subscribes_once(self):
        client = self.create_client()

        async def mock_connect_sets_client(client_id):
            mock_ws = MagicMock()
            mock_ws.is_active.return_value = True
            client._clients[client_id] = mock_ws

        with patch.object(
            client,
            "_connect_client",
            new_callable=AsyncMock,
            side_effect=mock_connect_sets_client,
        ) as mock_connect:
            # Try to subscribe to the same token concurrently
            await asyncio.gather(
                client.subscribe("token_1"),
                client.subscribe("token_1"),
                client.subscribe("token_1"),
            )

        # Lock should prevent duplicate subscriptions
        assert client._subscriptions.count("token_1") == 1
        # Should only connect once
        assert mock_connect.call_count == 1

    @pytest.mark.asyncio
    async def test_concurrent_subscribe_and_unsubscribe(self):
        client = self.create_client()

        # Pre-populate with some subscriptions
        for i in range(5):
            client.add_subscription(f"token_{i}")

        mock_ws = MagicMock()
        mock_ws.is_active.return_value = True
        mock_ws.is_disconnecting.return_value = False
        mock_ws.is_closed.return_value = False
        mock_ws.disconnect = AsyncMock()
        client._clients[0] = mock_ws

        with patch.object(client, "_send", new_callable=AsyncMock):
            # Concurrently subscribe new tokens and unsubscribe existing ones
            await asyncio.gather(
                client.subscribe("token_new_1"),
                client.unsubscribe("token_0"),
                client.subscribe("token_new_2"),
                client.unsubscribe("token_1"),
            )

        # Verify final state is consistent
        assert "token_new_1" in client._subscriptions
        assert "token_new_2" in client._subscriptions
        assert "token_0" not in client._subscriptions
        assert "token_1" not in client._subscriptions
        assert "token_2" in client._subscriptions


class TestPolymarketWebSocketClientReferenceCount:
    """
    Tests for subscription reference counting.
    """

    @pytest.fixture(autouse=True)
    def setup(self, event_loop):
        self.loop = event_loop
        self.clock = LiveClock()
        self.handler = MagicMock()

    def create_client(self) -> PolymarketWebSocketClient:
        return PolymarketWebSocketClient(
            clock=self.clock,
            base_url="wss://test.polymarket.com/ws/",
            channel=PolymarketWebSocketChannel.MARKET,
            handler=self.handler,
            handler_reconnect=None,
            loop=self.loop,
        )

    @pytest.mark.asyncio
    async def test_multiple_subscribes_increment_count(self):
        client = self.create_client()

        async def mock_connect_sets_client(client_id):
            mock_ws = MagicMock()
            mock_ws.is_active.return_value = True
            client._clients[client_id] = mock_ws

        with patch.object(
            client,
            "_connect_client",
            new_callable=AsyncMock,
            side_effect=mock_connect_sets_client,
        ) as mock_connect:
            await client.subscribe("token_1")
            await client.subscribe("token_1")
            await client.subscribe("token_1")

        # Should only connect once
        assert mock_connect.call_count == 1
        # Count should be 3
        assert client._subscription_counts["token_1"] == 3
        # Only one subscription in the list
        assert client._subscriptions.count("token_1") == 1

    @pytest.mark.asyncio
    async def test_unsubscribe_decrements_count(self):
        client = self.create_client()

        mock_ws = MagicMock()
        mock_ws.is_active.return_value = True
        mock_ws.is_disconnecting.return_value = False
        mock_ws.is_closed.return_value = False
        mock_ws.disconnect = AsyncMock()
        client._clients[0] = mock_ws

        with patch.object(client, "_connect_client", new_callable=AsyncMock):
            await client.subscribe("token_1")
            await client.subscribe("token_1")
            await client.subscribe("token_1")

        with patch.object(client, "_send", new_callable=AsyncMock) as mock_send:
            await client.unsubscribe("token_1")

        # Should not send unsubscribe yet (count is 2)
        mock_send.assert_not_called()
        assert client._subscription_counts["token_1"] == 2
        assert "token_1" in client._subscriptions

    @pytest.mark.asyncio
    async def test_final_unsubscribe_sends_message(self):
        client = self.create_client()

        mock_ws = MagicMock()
        mock_ws.is_active.return_value = True
        mock_ws.is_disconnecting.return_value = False
        mock_ws.is_closed.return_value = False
        mock_ws.disconnect = AsyncMock()
        mock_ws.send_text = AsyncMock()
        client._clients[0] = mock_ws
        client._client_subscriptions[0] = ["token_1", "token_2"]

        # First subscribe creates the subscription
        with patch.object(client, "_connect_client", new_callable=AsyncMock):
            await client.subscribe("token_1")
        # Second subscribe just increments count
        await client.subscribe("token_1")

        with patch.object(client, "_send", new_callable=AsyncMock) as mock_send:
            await client.unsubscribe("token_1")
            await client.unsubscribe("token_1")

        # Should send unsubscribe on the second call
        mock_send.assert_called_once()
        assert "token_1" not in client._subscription_counts
        assert "token_1" not in client._subscriptions

    @pytest.mark.asyncio
    async def test_unsubscribe_without_subscribe_warns(self):
        client = self.create_client()

        # Should not raise, just log warning
        await client.unsubscribe("nonexistent")

        assert client._subscription_counts.get("nonexistent") is None


class TestPolymarketWebSocketClientConnectFailure:
    """
    Tests for connection failure handling.
    """

    @pytest.fixture(autouse=True)
    def setup(self, event_loop):
        self.loop = event_loop
        self.clock = LiveClock()
        self.handler = MagicMock()

    def create_client(self) -> PolymarketWebSocketClient:
        return PolymarketWebSocketClient(
            clock=self.clock,
            base_url="wss://test.polymarket.com/ws/",
            channel=PolymarketWebSocketChannel.MARKET,
            handler=self.handler,
            handler_reconnect=None,
            loop=self.loop,
        )

    @pytest.mark.asyncio
    async def test_connect_failure_resets_connecting_flag(self):
        client = self.create_client()
        client.add_subscription("token_1")

        with (
            patch(
                "nautilus_trader.adapters.polymarket.websocket.client.WebSocketClient.connect",
                new_callable=AsyncMock,
                side_effect=Exception("Connection failed"),
            ),
            pytest.raises(Exception, match="Connection failed"),
        ):
            await client.connect()

        # Flag should be reset even after failure
        assert client._is_connecting.get(0) is False

    @pytest.mark.asyncio
    async def test_subscribe_retries_failed_connection(self):
        """
        Calling subscribe() again after a failed connection should retry.
        """
        client = self.create_client()

        call_count = 0

        async def failing_then_succeeding_connect(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            if call_count == 1:
                raise Exception("Connection failed")
            mock_client = MagicMock()
            mock_client.is_active.return_value = True
            mock_client.send_text = AsyncMock()
            return mock_client

        with patch(
            "nautilus_trader.adapters.polymarket.websocket.client.WebSocketClient.connect",
            new_callable=AsyncMock,
            side_effect=failing_then_succeeding_connect,
        ):
            # First subscribe fails
            with pytest.raises(Exception, match="Connection failed"):
                await client.subscribe("token_1")

            # Subscription should be tracked despite failure
            assert "token_1" in client._subscriptions
            assert client._subscription_counts["token_1"] == 1
            assert client._clients.get(0) is None

            # Second subscribe with same token should retry connection
            await client.subscribe("token_1")

            # Now client should be connected
            assert client._clients.get(0) is not None
            assert call_count == 2

    @pytest.mark.asyncio
    async def test_subscribe_does_not_retry_while_connecting(self):
        """
        Subscribe should not trigger retry if connection is already in progress.
        """
        client = self.create_client()

        # Simulate a subscription that's tracked but client is connecting
        client._subscriptions = ["token_1"]
        client._subscription_counts = {"token_1": 1}
        client._client_subscriptions[0] = ["token_1"]
        client._clients[0] = None
        client._is_connecting[0] = True  # Connection in progress

        with patch.object(client, "_connect_client", new_callable=AsyncMock) as mock_connect:
            await client.subscribe("token_1")

            # Should not retry since connection is already in progress
            mock_connect.assert_not_called()

        # Count should still be incremented
        assert client._subscription_counts["token_1"] == 2


class TestPolymarketWebSocketClientConcurrentConnection:
    """
    Tests for concurrent subscriptions during initial connection.
    """

    @pytest.fixture(autouse=True)
    def setup(self, event_loop):
        self.loop = event_loop
        self.clock = LiveClock()
        self.handler = MagicMock()

    def create_client(self) -> PolymarketWebSocketClient:
        return PolymarketWebSocketClient(
            clock=self.clock,
            base_url="wss://test.polymarket.com/ws/",
            channel=PolymarketWebSocketChannel.MARKET,
            handler=self.handler,
            handler_reconnect=None,
            loop=self.loop,
        )

    @pytest.mark.asyncio
    async def test_concurrent_subscribes_during_connection_all_tracked(self):
        """
        Subscriptions added while connecting should all be tracked.
        """
        client = self.create_client()

        connection_started = asyncio.Event()
        connection_complete = asyncio.Event()

        async def slow_connect(*args, **kwargs):
            connection_started.set()
            await connection_complete.wait()
            mock_ws = MagicMock()
            mock_ws.is_active.return_value = True
            mock_ws.send_text = AsyncMock()
            return mock_ws

        with patch(
            "nautilus_trader.adapters.polymarket.websocket.client.WebSocketClient.connect",
            side_effect=slow_connect,
        ):
            # Start first subscription (triggers connection)
            task1 = asyncio.create_task(client.subscribe("token_1"))

            # Wait for connection to start
            await connection_started.wait()

            # Add more subscriptions while connecting
            task2 = asyncio.create_task(client.subscribe("token_2"))
            task3 = asyncio.create_task(client.subscribe("token_3"))

            # Give tasks time to add subscriptions
            await asyncio.sleep(0.02)

            # Complete the connection
            connection_complete.set()

            # Wait for all tasks
            await asyncio.gather(task1, task2, task3)

        # All subscriptions should be tracked
        assert "token_1" in client._subscriptions
        assert "token_2" in client._subscriptions
        assert "token_3" in client._subscriptions
        assert "token_1" in client._client_subscriptions[0]
        assert "token_2" in client._client_subscriptions[0]
        assert "token_3" in client._client_subscriptions[0]

    @pytest.mark.asyncio
    async def test_concurrent_subscribes_no_duplicate_messages(self):
        """
        Subscriptions added while connecting should not send duplicate messages.
        """
        client = self.create_client()

        connection_started = asyncio.Event()
        connection_complete = asyncio.Event()
        send_calls = []

        async def slow_connect(*args, **kwargs):
            connection_started.set()
            await connection_complete.wait()
            mock_ws = MagicMock()
            mock_ws.is_active.return_value = True

            async def track_send(data):
                send_calls.append(data)

            mock_ws.send_text = track_send
            return mock_ws

        with patch(
            "nautilus_trader.adapters.polymarket.websocket.client.WebSocketClient.connect",
            side_effect=slow_connect,
        ):
            task1 = asyncio.create_task(client.subscribe("token_1"))
            await connection_started.wait()

            task2 = asyncio.create_task(client.subscribe("token_2"))
            await asyncio.sleep(0.02)

            connection_complete.set()
            await asyncio.gather(task1, task2)

        # Should only have ONE send call (the initial subscription message)
        assert len(send_calls) == 1
