from types import SimpleNamespace
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import pytest

from flux.runners.shared.quote_feed_supervisor import QuoteFeedIdentity
from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.data import HyperliquidDataClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from tests.integration_tests.adapters.hyperliquid.conftest import _create_ws_mock


@pytest.fixture
def data_client_builder(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    def builder(monkeypatch, *, config_kwargs: dict | None = None):
        ws_client = _create_ws_mock()
        ws_iter = iter([ws_client])

        monkeypatch.setattr(
            "nautilus_trader.adapters.hyperliquid.data.nautilus_pyo3.HyperliquidWebSocketClient",
            lambda *args, **kwargs: next(ws_iter),
        )

        mock_http_client.reset_mock()
        mock_instrument_provider.initialize.reset_mock()
        mock_instrument_provider.instruments_pyo3.reset_mock()
        mock_instrument_provider.instruments_pyo3.return_value = [
            MagicMock(name="py_instrument"),
        ]

        config = HyperliquidDataClientConfig(
            testnet=False,
            **(config_kwargs or {}),
        )

        client = HyperliquidDataClient(
            loop=event_loop,
            client=mock_http_client,
            msgbus=msgbus,
            cache=cache,
            clock=live_clock,
            instrument_provider=mock_instrument_provider,
            config=config,
            name=None,
        )

        return client, ws_client, mock_http_client, mock_instrument_provider

    return builder


def test_data_client_builder_accepts_trade_xyz_dex(data_client_builder, monkeypatch):
    # Arrange & Act
    client, _, _, _ = data_client_builder(
        monkeypatch,
        config_kwargs={"dex": "xyz"},
    )

    # Assert
    assert client._config.dex == "xyz"


@pytest.mark.asyncio
async def test_connect_and_disconnect_manage_resources(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
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
        ws_client.connect.assert_awaited_once()
    finally:
        await client._disconnect()

    # Assert
    ws_client.close.assert_awaited_once()


@pytest.mark.asyncio
async def test_subscribe_order_book_deltas(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_book.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_order_book_deltas(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_book.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_quote_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_quotes.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_quote_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_quotes.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_recover_quote_subscription_refreshes_cache_before_replay(
    data_client_builder,
    monkeypatch,
):
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    ws_client.recover_quote_subscription = AsyncMock(
        side_effect=[
            ("cache_miss", False, "Instrument not found in cache: BTC-USD-PERP.HYPERLIQUID"),
            ("replayed", True, None),
        ],
    )

    refreshed_instruments = [
        MagicMock(name="refreshed_instrument_1"),
        MagicMock(name="refreshed_instrument_2"),
    ]
    instrument_provider.instruments_pyo3.return_value = refreshed_instruments

    await client._connect()
    try:
        result = await client.recover_quote_subscription(
            InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        assert result["ok"] is True
        assert result["status"] == "replayed"
        assert result["cache_refreshed"] is True
        instrument_provider.initialize.assert_any_await(reload=True)
        ws_client.cache_instruments.assert_called_once_with(refreshed_instruments)
        assert http_client.cache_instrument.call_count >= len(refreshed_instruments)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_recover_quote_subscription_reports_transport_result(
    data_client_builder,
    monkeypatch,
):
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    ws_client.recover_quote_subscription = AsyncMock(
        return_value=("transport_unhealthy", False, "transport inactive"),
    )
    result_ingress = MagicMock()
    client.set_quote_feed_result_ingress(result_ingress)

    await client._connect()
    try:
        result = await client.recover_quote_subscription(
            InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        assert result == {
            "instrument_id": "BTC-USD-PERP.HYPERLIQUID",
            "ok": False,
            "status": "transport_unhealthy",
            "error_summary": "transport inactive",
            "cache_refreshed": False,
        }
        result_ingress.assert_called_once()
        assert result_ingress.call_args.kwargs["result"] == result
        assert result_ingress.call_args.kwargs["instrument_id"] == "BTC-USD-PERP.HYPERLIQUID"
        assert result_ingress.call_args.kwargs["status"] == "transport_unhealthy"
        assert result_ingress.call_args.kwargs["cache_refreshed"] is False
        assert result_ingress.call_args.kwargs["ok"] is False
        assert result_ingress.call_args.kwargs["error_summary"] == "transport inactive"
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_recover_quote_subscription_supports_legacy_result_ingress_signature(
    data_client_builder,
    monkeypatch,
):
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    ws_client.recover_quote_subscription = AsyncMock(
        return_value=("transport_unhealthy", False, "transport inactive"),
    )
    ingress_calls: list[tuple[int, bool, str | None]] = []

    def result_ingress(*, now_ns: int, ok: bool, error_summary: str | None) -> None:
        ingress_calls.append((now_ns, ok, error_summary))

    client.set_quote_feed_result_ingress(result_ingress)

    await client._connect()
    try:
        await client.recover_quote_subscription(
            InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        assert len(ingress_calls) == 1
        _, ok, error_summary = ingress_calls[0]
        assert ok is False
        assert error_summary == "transport inactive"
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_recover_quote_ticks_reports_explicit_feed_identity(
    data_client_builder,
    monkeypatch,
):
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    ws_client.recover_quote_subscription = AsyncMock(
        return_value=("transport_unhealthy", False, "transport inactive"),
    )
    result_ingress = MagicMock()
    client.set_quote_feed_result_ingress(result_ingress)
    feed_identity = QuoteFeedIdentity(
        scope="hyperliquid.xyz.main",
        instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        topic="maker_quote_ticks",
    )

    await client._connect()
    try:
        result = await client.recover_quote_ticks(feed_identity)

        assert result["feed_identity"] == feed_identity
        assert result_ingress.call_args.kwargs["feed_identity"] == feed_identity
        assert result_ingress.call_args.kwargs["instrument_id"] == "BTC-USD-PERP.HYPERLIQUID"
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_trade_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_trades.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_trade_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_trades.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_mark_prices(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_mark_prices.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_mark_prices(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_mark_prices.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_index_prices(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_index_prices.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_index_prices(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_index_prices.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_bars(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_bars.reset_mock()

        bar_type = BarType.from_str("BTC-USD-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL")
        command = SimpleNamespace(bar_type=bar_type)

        # Act
        await client._subscribe_bars(command)

        # Assert
        expected_bar_type = nautilus_pyo3.BarType.from_str(
            "BTC-USD-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL",
        )
        ws_client.subscribe_bars.assert_awaited_once_with(expected_bar_type)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_subscribe_funding_rates(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.subscribe_funding_rates.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._subscribe_funding_rates(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.subscribe_funding_rates.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_order_book_deltas(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_book.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_order_book_deltas(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_book.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_quote_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_quotes.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_quote_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_quotes.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_trade_ticks(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_trades.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_trade_ticks(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_trades.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_mark_prices(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_mark_prices.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_mark_prices(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_mark_prices.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_index_prices(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_index_prices.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_index_prices(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_index_prices.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_bars(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_bars.reset_mock()

        bar_type = BarType.from_str("BTC-USD-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL")
        command = SimpleNamespace(bar_type=bar_type)

        # Act
        await client._unsubscribe_bars(command)

        # Assert
        expected_bar_type = nautilus_pyo3.BarType.from_str(
            "BTC-USD-PERP.HYPERLIQUID-1-MINUTE-LAST-EXTERNAL",
        )
        ws_client.unsubscribe_bars.assert_awaited_once_with(expected_bar_type)
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_unsubscribe_funding_rates(data_client_builder, monkeypatch):
    # Arrange
    client, ws_client, http_client, instrument_provider = data_client_builder(
        monkeypatch,
    )

    await client._connect()
    try:
        ws_client.unsubscribe_funding_rates.reset_mock()

        command = SimpleNamespace(
            instrument_id=InstrumentId(Symbol("BTC-USD-PERP"), HYPERLIQUID_VENUE),
        )

        # Act
        await client._unsubscribe_funding_rates(command)

        # Assert
        expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
        ws_client.unsubscribe_funding_rates.assert_awaited_once_with(expected_id)
    finally:
        await client._disconnect()
