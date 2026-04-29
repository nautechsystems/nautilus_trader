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

from types import SimpleNamespace
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.okx.config import OKXDataClientConfig
from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.data import OKXDataClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import OKXGreeksType
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeOptionGreeks
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeOptionGreeks
from nautilus_trader.model.data import OptionGreeks
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.okx.conftest import _create_ws_mock


@pytest.fixture
def data_client_builder(
    event_loop,
    mock_http_client,
    live_clock,
    mock_instrument_provider,
):
    def builder(monkeypatch, *, config_kwargs: dict | None = None):
        public_ws = _create_ws_mock()
        business_ws = _create_ws_mock()
        ws_iter = iter([public_ws, business_ws])

        monkeypatch.setattr(
            "nautilus_trader.adapters.okx.data.nautilus_pyo3.OKXWebSocketClient",
            lambda *args, **kwargs: next(ws_iter),
        )

        mock_http_client.reset_mock()
        mock_http_client.request_instruments.return_value = []
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()
        mock_instrument_provider.instruments_pyo3.return_value = [MagicMock(name="py_instrument")]

        cache = Cache()
        msgbus = MessageBus(
            trader_id=TestIdStubs.trader_id(),
            clock=live_clock,
        )

        config = OKXDataClientConfig(
            api_key="test_api_key",
            api_secret="test_api_secret",
            api_passphrase="test_passphrase",
            instrument_types=(nautilus_pyo3.OKXInstrumentType.SPOT,),
            update_instruments_interval_mins=1,
            **(config_kwargs or {}),
        )

        client = OKXDataClient(
            loop=event_loop,
            client=mock_http_client,
            msgbus=msgbus,
            cache=cache,
            clock=live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
            name=None,
        )

        return client, public_ws, business_ws, mock_http_client, mock_instrument_provider

    return builder


@pytest.mark.asyncio
async def test_connect_and_disconnect_manage_resources(data_client_builder, monkeypatch):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    # Act
    await client._connect()

    try:
        # Assert
        instrument_provider.initialize.assert_awaited_once()
        http_client.cache_instrument.assert_called_once_with(
            instrument_provider.instruments_pyo3.return_value[0],
        )
        public_ws.connect.assert_awaited_once()
        assert public_ws.wait_until_active.await_count >= 1
        business_ws.connect.assert_awaited_once()
        public_ws.subscribe_instruments.assert_awaited_once_with(
            nautilus_pyo3.OKXInstrumentType.SPOT,
        )
    finally:
        await client._disconnect()

    # Assert
    http_client.cancel_all_requests.assert_called_once()
    public_ws.close.assert_awaited_once()
    business_ws.close.assert_awaited_once()


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas_depth_default_uses_standard_channel(
    data_client_builder,
    monkeypatch,
):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        public_ws.subscribe_book_with_depth.reset_mock()
        public_ws.vip_level = nautilus_pyo3.OKXVipLevel.VIP0

        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=0,
            instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert
        public_ws.subscribe_book_with_depth.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas_depth_50_requires_vip(data_client_builder, monkeypatch):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        public_ws.subscribe_book_with_depth.reset_mock()
        public_ws.vip_level = nautilus_pyo3.OKXVipLevel.VIP3

        # Configure mock to raise for insufficient VIP level
        async def mock_subscribe_with_vip_check(instrument_id, depth):
            if depth == 50 and public_ws.vip_level.value < 4:
                raise ValueError(
                    f"VIP level {public_ws.vip_level} insufficient for 50 depth subscription (requires VIP4)",
                )

        public_ws.subscribe_book_with_depth.side_effect = mock_subscribe_with_vip_check

        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=50,
            instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
        )

        # Act & Assert
        with pytest.raises(ValueError, match="insufficient for 50 depth"):
            await client._subscribe_order_book_deltas(command)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas_depth_50_with_vip_calls_compact(
    data_client_builder,
    monkeypatch,
):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        public_ws.subscribe_book_with_depth.reset_mock()
        public_ws.vip_level = nautilus_pyo3.OKXVipLevel.VIP4

        command = SimpleNamespace(
            book_type=BookType.L2_MBP,
            depth=50,
            instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert
        public_ws.subscribe_book_with_depth.assert_awaited_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_bars_uses_business_websocket(data_client_builder, monkeypatch):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        captured_args = {}

        def fake_from_str(value):
            captured_args["value"] = value
            return MagicMock(name="bar_type")

        monkeypatch.setattr(
            "nautilus_trader.adapters.okx.data.nautilus_pyo3.BarType.from_str",
            fake_from_str,
        )

        bar_command = SimpleNamespace(bar_type="BAR-TYPE")

        # Act
        await client._subscribe_bars(bar_command)

        # Assert
        business_ws.subscribe_bars.assert_awaited_once()
        assert captured_args["value"] == str(bar_command.bar_type)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_instruments_is_noop(data_client_builder, monkeypatch):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        command = SubscribeInstruments(
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await client._subscribe_instruments(command)

        # Assert - should not make any WebSocket calls beyond the initial connection
        # Instruments are already subscribed during connect for all configured types
        assert public_ws.subscribe_instruments.call_count == 1  # Only from connect
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_instrument_calls_websocket(data_client_builder, monkeypatch):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        command = SubscribeInstrument(
            instrument_id=InstrumentId(Symbol("BTC-USD"), OKX_VENUE),
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act
        await client._subscribe_instrument(command)

        # Assert - should call subscribe_instrument on the WebSocket client
        # The WebSocket client will determine if the type needs to be subscribed
        public_ws.subscribe_instrument.assert_called_once()
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_instruments_is_noop(data_client_builder, monkeypatch):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        command = UnsubscribeInstruments(
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act & Assert - should complete without error (no-op)
        # OKX instruments channel is subscribed at type level, cannot unsubscribe
        await client._unsubscribe_instruments(command)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_instrument_is_noop(
    data_client_builder,
    monkeypatch,
):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        command = UnsubscribeInstrument(
            instrument_id=InstrumentId(Symbol("ETH-USDT"), OKX_VENUE),
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act & Assert - should complete without error (no-op)
        # OKX instruments channel is subscribed at type level, cannot unsubscribe individual instruments
        await client._unsubscribe_instrument(command)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_request_instrument_receives_single_instrument(
    data_client_builder,
    instrument,
    live_clock,
    monkeypatch,
) -> None:
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )
    instrument_provider.get_all.return_value = {instrument.id: instrument}

    # Use the client's internal msgbus for subscription
    msgbus = client._msgbus

    # Create DataEngine to process messages and publish to topics
    data_engine = DataEngine(msgbus, client._cache, live_clock)
    data_engine.register_client(client)
    data_engine.start()

    # Track received instruments via msgbus subscription
    received_instruments: list = []
    topic = f"data.instrument.{instrument.id.venue}.{instrument.id.symbol}"
    msgbus.subscribe(topic=topic, handler=received_instruments.append)

    # Mock HTTP client to return a pyo3 instrument (async method)
    mock_pyo3_instrument = MagicMock()
    http_client.request_instrument = AsyncMock(return_value=mock_pyo3_instrument)

    try:
        # Patch transform_instrument_from_pyo3 to return our fixture instrument
        with patch(
            "nautilus_trader.adapters.okx.data.transform_instrument_from_pyo3",
            return_value=instrument,
        ):
            # Create request
            request = RequestInstrument(
                instrument_id=instrument.id,
                start=None,
                end=None,
                client_id=ClientId(OKX_VENUE.value),
                venue=OKX_VENUE,
                callback=lambda x: None,
                request_id=UUID4(),
                ts_init=live_clock.timestamp_ns(),
                params=None,
            )

            # Act - Request the instrument
            await client._request_instrument(request)

        # Assert - Should receive exactly ONE instrument (not 2!)
        assert len(received_instruments) == 1, (
            f"Expected 1 instrument publication, was {len(received_instruments)}. "
            f"This indicates duplicate publication bug! "
            f"Received: {received_instruments}"
        )
        assert received_instruments[0].id == instrument.id
    finally:
        data_engine.stop()
        await client._disconnect()


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("command_params", "expected_conventions"),
    [
        (None, [OKXGreeksType.BS, OKXGreeksType.PA]),
        ({"greeks_convention": "BLACK_SCHOLES"}, [OKXGreeksType.BS]),
        ({"greeks_convention": "PRICE_ADJUSTED"}, [OKXGreeksType.PA]),
        (
            {"greeks_convention": ["BLACK_SCHOLES", "PRICE_ADJUSTED"]},
            [OKXGreeksType.BS, OKXGreeksType.PA],
        ),
        ({"greeks_convention": ["PRICE_ADJUSTED"]}, [OKXGreeksType.PA]),
        ({"greeks_convention": "BOGUS"}, [OKXGreeksType.BS, OKXGreeksType.PA]),
    ],
)
async def test_subscribe_option_greeks_propagates_requested_conventions(
    data_client_builder,
    monkeypatch,
    command_params,
    expected_conventions,
):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        instrument_id = InstrumentId(Symbol("BTC-USD-260410-70000-C"), OKX_VENUE)

        command = SubscribeOptionGreeks(
            instrument_id=instrument_id,
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
            params=command_params,
        )

        # Act
        await client._subscribe_option_greeks(command)

        # Assert
        public_ws.add_option_greeks_sub_with_conventions.assert_called_once()
        call_args = public_ws.add_option_greeks_sub_with_conventions.call_args
        actual_names = sorted(str(c) for c in call_args.args[1])
        expected_names = sorted(str(c) for c in expected_conventions)
        assert actual_names == expected_names
        public_ws.subscribe_option_summary.assert_awaited_once_with("BTC-USD")
    finally:
        await client._disconnect()


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "convention",
    [
        nautilus_pyo3.GreeksConvention.BLACK_SCHOLES,
        nautilus_pyo3.GreeksConvention.PRICE_ADJUSTED,
    ],
)
async def test_handle_msg_option_greeks_forwarded_when_subscribed(
    data_client_builder,
    monkeypatch,
    convention,
):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        instrument_id = InstrumentId(Symbol("BTC-USD-260410-70000-C"), OKX_VENUE)

        command = SubscribeOptionGreeks(
            instrument_id=instrument_id,
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )
        await client._subscribe_option_greeks(command)

        handled_data = []
        client._handle_data = handled_data.append

        pyo3_greeks = MagicMock(spec=nautilus_pyo3.OptionGreeks)
        pyo3_greeks.instrument_id = MagicMock()
        pyo3_greeks.instrument_id.value = instrument_id.value

        pyo3_greeks.delta = 0.5
        pyo3_greeks.gamma = 0.01
        pyo3_greeks.vega = 10.0
        pyo3_greeks.theta = -1.0
        pyo3_greeks.rho = 0.0
        pyo3_greeks.mark_iv = 0.5
        pyo3_greeks.bid_iv = 0.49
        pyo3_greeks.ask_iv = 0.51
        pyo3_greeks.underlying_price = 70000.0
        pyo3_greeks.open_interest = 100.0
        pyo3_greeks.ts_event = 1_000_000_000
        pyo3_greeks.ts_init = 1_000_000_000
        pyo3_greeks.convention = convention

        # Act
        client._handle_msg(pyo3_greeks)

        # Assert
        assert len(handled_data) == 1
        assert isinstance(handled_data[0], OptionGreeks)
        assert handled_data[0].instrument_id == instrument_id
        assert handled_data[0].convention == convention
    finally:
        await client._disconnect()


BOTH_CONVENTIONS = [OKXGreeksType.BS, OKXGreeksType.PA]


@pytest.mark.parametrize(
    ("params", "expected"),
    [
        (None, BOTH_CONVENTIONS),
        ({}, BOTH_CONVENTIONS),
        ({"other_key": "value"}, BOTH_CONVENTIONS),
        ({"greeks_convention": "BLACK_SCHOLES"}, [OKXGreeksType.BS]),
        ({"greeks_convention": "PRICE_ADJUSTED"}, [OKXGreeksType.PA]),
        ({"greeks_convention": "black_scholes"}, [OKXGreeksType.BS]),
        ({"greeks_convention": "price_adjusted"}, [OKXGreeksType.PA]),
        (
            {"greeks_convention": ["BLACK_SCHOLES", "PRICE_ADJUSTED"]},
            BOTH_CONVENTIONS,
        ),
        ({"greeks_convention": ["PRICE_ADJUSTED"]}, [OKXGreeksType.PA]),
        (
            {"greeks_convention": ["BLACK_SCHOLES", "black_scholes"]},
            [OKXGreeksType.BS],
        ),
        (
            {"greeks_convention": ["BOGUS", "PRICE_ADJUSTED"]},
            [OKXGreeksType.PA],
        ),
        ({"greeks_convention": ["BOGUS"]}, BOTH_CONVENTIONS),
        ({"greeks_convention": "BOGUS"}, BOTH_CONVENTIONS),
        ({"greeks_convention": 42}, BOTH_CONVENTIONS),
        ({"greeks_convention": [42, None]}, BOTH_CONVENTIONS),
    ],
)
def test_resolve_greeks_conventions(
    data_client_builder,
    monkeypatch,
    params,
    expected,
):
    # Arrange
    client, _, _, _, _ = data_client_builder(monkeypatch)

    # Act
    actual = client._resolve_greeks_conventions(params)

    # Assert
    assert sorted(str(c) for c in actual) == sorted(str(c) for c in expected)


@pytest.mark.asyncio
async def test_handle_msg_option_greeks_dropped_when_not_subscribed(
    data_client_builder,
    monkeypatch,
):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        handled_data = []
        client._handle_data = handled_data.append

        instrument_id = InstrumentId(Symbol("BTC-USD-260410-70000-C"), OKX_VENUE)
        pyo3_greeks = MagicMock(spec=nautilus_pyo3.OptionGreeks)
        pyo3_greeks.instrument_id = MagicMock()
        pyo3_greeks.instrument_id.value = instrument_id.value

        pyo3_greeks.delta = 0.5
        pyo3_greeks.gamma = 0.01
        pyo3_greeks.vega = 10.0
        pyo3_greeks.theta = -1.0
        pyo3_greeks.rho = 0.0
        pyo3_greeks.mark_iv = 0.5
        pyo3_greeks.bid_iv = 0.49
        pyo3_greeks.ask_iv = 0.51
        pyo3_greeks.underlying_price = 70000.0
        pyo3_greeks.open_interest = 100.0
        pyo3_greeks.ts_event = 1_000_000_000
        pyo3_greeks.ts_init = 1_000_000_000

        # Act
        client._handle_msg(pyo3_greeks)

        # Assert
        assert len(handled_data) == 0
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_option_greeks_rolls_back_on_ws_error(
    data_client_builder,
    monkeypatch,
):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        public_ws.subscribe_option_summary.side_effect = RuntimeError("ws fail")
        instrument_id = InstrumentId(Symbol("BTC-USD-260410-70000-C"), OKX_VENUE)

        command = SubscribeOptionGreeks(
            instrument_id=instrument_id,
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )

        # Act & Assert
        with pytest.raises(RuntimeError, match="ws fail"):
            await client._subscribe_option_greeks(command)

        assert instrument_id not in client._option_greeks_instrument_ids
        assert "BTC-USD" not in client._option_summary_family_subs
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_option_greeks_family_ref_counting(
    data_client_builder,
    monkeypatch,
):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        call_id = InstrumentId(Symbol("BTC-USD-260410-70000-C"), OKX_VENUE)
        put_id = InstrumentId(Symbol("BTC-USD-260410-70000-P"), OKX_VENUE)

        call_cmd = SubscribeOptionGreeks(
            instrument_id=call_id,
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )
        put_cmd = SubscribeOptionGreeks(
            instrument_id=put_id,
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )

        import asyncio

        # Act: subscribe two instruments in the same BTC-USD family
        await client._subscribe_option_greeks(call_cmd)
        await client._subscribe_option_greeks(put_cmd)

        # Assert: WS subscribe called once for the family
        assert public_ws.subscribe_option_summary.await_count == 1
        assert client._option_summary_family_subs["BTC-USD"] == 2
        assert call_id in client._option_greeks_instrument_ids
        assert put_id in client._option_greeks_instrument_ids

        # Act: unsubscribe the first, family should stay active
        unsub_call = UnsubscribeOptionGreeks(
            instrument_id=call_id,
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )
        client.unsubscribe_option_greeks(unsub_call)
        await asyncio.sleep(0)
        await asyncio.sleep(0)

        assert client._option_summary_family_subs["BTC-USD"] == 1
        assert public_ws.unsubscribe_option_summary.await_count == 0

        # Act: unsubscribe the second, family should be cleaned up
        unsub_put = UnsubscribeOptionGreeks(
            instrument_id=put_id,
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )
        client.unsubscribe_option_greeks(unsub_put)
        await asyncio.sleep(0)
        await asyncio.sleep(0)

        assert "BTC-USD" not in client._option_summary_family_subs
        assert public_ws.unsubscribe_option_summary.await_count == 1
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_handle_msg_option_greeks_dropped_after_unsubscribe(
    data_client_builder,
    monkeypatch,
):
    # Arrange
    client, public_ws, business_ws, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        instrument_id = InstrumentId(Symbol("BTC-USD-260410-70000-C"), OKX_VENUE)

        sub_command = SubscribeOptionGreeks(
            instrument_id=instrument_id,
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )
        await client._subscribe_option_greeks(sub_command)

        unsub_command = UnsubscribeOptionGreeks(
            instrument_id=instrument_id,
            client_id=None,
            venue=OKX_VENUE,
            command_id=UUID4(),
            ts_init=0,
        )
        client.unsubscribe_option_greeks(unsub_command)

        handled_data = []
        client._handle_data = handled_data.append

        pyo3_greeks = MagicMock(spec=nautilus_pyo3.OptionGreeks)
        pyo3_greeks.instrument_id = MagicMock()
        pyo3_greeks.instrument_id.value = instrument_id.value

        pyo3_greeks.delta = 0.5
        pyo3_greeks.gamma = 0.01
        pyo3_greeks.vega = 10.0
        pyo3_greeks.theta = -1.0
        pyo3_greeks.rho = 0.0
        pyo3_greeks.mark_iv = 0.5
        pyo3_greeks.bid_iv = 0.49
        pyo3_greeks.ask_iv = 0.51
        pyo3_greeks.underlying_price = 70000.0
        pyo3_greeks.open_interest = 100.0
        pyo3_greeks.ts_event = 1_000_000_000
        pyo3_greeks.ts_init = 1_000_000_000

        # Act
        client._handle_msg(pyo3_greeks)

        # Assert
        assert len(handled_data) == 0
    finally:
        await client._disconnect()
