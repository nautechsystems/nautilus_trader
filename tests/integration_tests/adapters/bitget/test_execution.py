# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import pytest
from decimal import Decimal
from types import SimpleNamespace
from unittest.mock import patch

from nautilus_trader.adapters.bitget.execution import BitgetExecutionClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money


@pytest.mark.asyncio
async def test_connect_skips_private_websocket_without_credentials() -> None:
    warnings: list[str] = []
    connect_calls: list[object] = []

    class DummyWebSocketClient:
        @staticmethod
        async def connect(*, loop_, config, handler, post_reconnection):
            connect_calls.append(config)
            return object()

    dummy = SimpleNamespace(
        _config=SimpleNamespace(
            api_key=None,
            api_secret=None,
            api_passphrase=None,
            base_url_ws_private="wss://private.example",
            retry_delay_initial_ms=None,
            retry_delay_max_ms=None,
        ),
        _log=SimpleNamespace(
            info=lambda *_args, **_kwargs: None,
            warning=lambda message, *_args, **_kwargs: warnings.append(message),
        ),
    )

    with patch(
        "nautilus_trader.adapters.bitget.execution.nautilus_pyo3.WebSocketClient",
        DummyWebSocketClient,
    ):
        await BitgetExecutionClient._connect(dummy)  # type: ignore[arg-type]

    assert connect_calls == []
    assert warnings == ["Bitget execution client missing private WebSocket credentials; skipping connect"]


@pytest.mark.asyncio
async def test_connect_opens_private_websocket_and_sends_login() -> None:
    captured_configs: list[object] = []
    sent: list[bytes] = []

    class DummyWebSocketClient:
        @staticmethod
        async def connect(*, loop_, config, handler, post_reconnection):
            captured_configs.append(config)
            return object()

    async def send_ws_text(message: str) -> None:
        sent.append(message.encode("utf-8"))

    dummy = SimpleNamespace(
        _config=SimpleNamespace(
            api_key="key",
            api_secret="secret",
            api_passphrase="pass",
            product_types=("SPOT",),
            base_url_ws_private="wss://private.example",
            retry_delay_initial_ms=None,
            retry_delay_max_ms=None,
        ),
        _loop=object(),
        _handle_ws_message=lambda _raw: None,
        _handle_ws_reconnect=lambda: None,
        _send_ws_text=send_ws_text,
        _log=SimpleNamespace(
            info=lambda *_args, **_kwargs: None,
            warning=lambda *_args, **_kwargs: None,
        ),
        _product_types=("SPOT",),
        _ws_client=None,
    )

    async def authenticate_ws() -> None:
        await BitgetExecutionClient._authenticate_ws(dummy)  # type: ignore[arg-type]

    async def subscribe_private_ws() -> None:
        await BitgetExecutionClient._subscribe_private_ws(dummy)  # type: ignore[arg-type]

    dummy._authenticate_ws = authenticate_ws
    dummy._subscribe_private_ws = subscribe_private_ws

    with patch(
        "nautilus_trader.adapters.bitget.execution.nautilus_pyo3.WebSocketConfig",
        lambda **kwargs: SimpleNamespace(**kwargs),
    ), patch(
        "nautilus_trader.adapters.bitget.execution.nautilus_pyo3.WebSocketClient",
        DummyWebSocketClient,
    ), patch(
        "nautilus_trader.adapters.bitget.execution.nautilus_pyo3.BitgetWebSocketClient.ping_message",
        lambda: "rust-ping",
        create=True,
    ), patch(
        "nautilus_trader.adapters.bitget.execution.nautilus_pyo3.BitgetWebSocketClient.login_message",
        lambda api_key, passphrase, secret, timestamp_ms: (
            f"login:{api_key}:{passphrase}:{secret}:{timestamp_ms}"
        ),
        create=True,
    ), patch(
        "nautilus_trader.adapters.bitget.execution.nautilus_pyo3.BitgetWebSocketClient.subscribe_account_message",
        lambda product_type, coin: f"account:{product_type}:{coin}",
        create=True,
    ), patch(
        "nautilus_trader.adapters.bitget.execution.nautilus_pyo3.BitgetWebSocketClient.subscribe_message",
        lambda product_type, channel, inst_id: f"subscribe:{product_type}:{channel}:{inst_id}",
        create=True,
    ), patch(
        "nautilus_trader.adapters.bitget.execution.time.time_ns",
        lambda: 1_708_883_200_123_000_000,
    ):
        await BitgetExecutionClient._connect(dummy)  # type: ignore[arg-type]

    assert captured_configs[0].url == "wss://private.example"
    assert captured_configs[0].heartbeat_msg == "rust-ping"
    assert sent == [b"login:key:pass:secret:1708883200123"]


def test_handle_ws_reconnect_schedules_reauth_on_event_loop_thread() -> None:
    calls: list[object] = []

    class DummyLoop:
        def call_soon_threadsafe(self, callback):
            calls.append(callback)

    dummy = SimpleNamespace(
        _loop=DummyLoop(),
        _on_ws_reconnect=lambda: None,
    )

    BitgetExecutionClient._handle_ws_reconnect(dummy)  # type: ignore[arg-type]

    assert calls == [dummy._on_ws_reconnect]


def test_on_ws_reconnect_reauthenticates_private_websocket() -> None:
    created: list[tuple[str, object]] = []

    async def authenticate_ws() -> None:
        return None

    def warning(*_args, **_kwargs) -> None:
        return None

    def create_task(coro, log_msg):
        created.append((log_msg, coro))
        coro.close()
        return object()

    dummy = SimpleNamespace(
        _log=SimpleNamespace(warning=warning),
        _authenticate_ws=authenticate_ws,
        create_task=create_task,
        _ws_tasks=set(),
    )

    BitgetExecutionClient._on_ws_reconnect(dummy)  # type: ignore[arg-type]

    assert [log_msg for log_msg, _ in created] == ["bitget:reauth_private_ws"]


def test_handle_ws_message_login_success_schedules_private_subscriptions() -> None:
    calls: list[object] = []

    class DummyLoop:
        def call_soon_threadsafe(self, callback):
            calls.append(callback)

    dummy = SimpleNamespace(
        _loop=DummyLoop(),
        _on_ws_authenticated=lambda: None,
        _log=SimpleNamespace(
            info=lambda *_args, **_kwargs: None,
            warning=lambda *_args, **_kwargs: None,
            debug=lambda *_args, **_kwargs: None,
            error=lambda *_args, **_kwargs: None,
        ),
    )

    BitgetExecutionClient._handle_ws_message(
        dummy,  # type: ignore[arg-type]
        b'{"event":"login","code":"0","msg":""}',
    )

    assert calls == [dummy._on_ws_authenticated]


def test_on_ws_authenticated_subscribes_private_channels() -> None:
    infos: list[str] = []
    created: list[tuple[str, object]] = []

    async def subscribe_private_ws() -> None:
        return None

    def create_task(coro, log_msg):
        created.append((log_msg, coro))
        coro.close()
        return object()

    dummy = SimpleNamespace(
        _log=SimpleNamespace(
            info=lambda message, *_args, **_kwargs: infos.append(message),
        ),
        _subscribe_private_ws=subscribe_private_ws,
        create_task=create_task,
        _ws_tasks=set(),
    )

    BitgetExecutionClient._on_ws_authenticated(dummy)  # type: ignore[arg-type]

    assert infos == ["Bitget private WebSocket authenticated"]
    assert [log_msg for log_msg, _ in created] == ["bitget:subscribe_private_ws"]


def test_handle_ws_message_logs_login_failure() -> None:
    infos: list[str] = []
    warnings: list[str] = []
    debugs: list[str] = []
    dummy = SimpleNamespace(
        _log=SimpleNamespace(
            info=lambda message, *_args, **_kwargs: infos.append(message),
            warning=lambda message, *_args, **_kwargs: warnings.append(message),
            debug=lambda message, *_args, **_kwargs: debugs.append(message),
            error=lambda *_args, **_kwargs: None,
        ),
    )

    BitgetExecutionClient._handle_ws_message(
        dummy,  # type: ignore[arg-type]
        b'{"event":"error","code":"30005","msg":"login fail"}',
    )

    assert infos == []
    assert warnings == ["Bitget private WebSocket login failed: code=30005 msg=login fail"]
    assert debugs == []


def test_handle_ws_message_logs_subscription_success() -> None:
    infos: list[str] = []
    warnings: list[str] = []
    debugs: list[str] = []
    dummy = SimpleNamespace(
        _log=SimpleNamespace(
            info=lambda message, *_args, **_kwargs: infos.append(message),
            warning=lambda message, *_args, **_kwargs: warnings.append(message),
            debug=lambda message, *_args, **_kwargs: debugs.append(message),
            error=lambda *_args, **_kwargs: None,
        ),
    )

    BitgetExecutionClient._handle_ws_message(
        dummy,  # type: ignore[arg-type]
        b'{"event":"subscribe","arg":{"instType":"SPOT","channel":"account","coin":"default"}}',
    )

    assert infos == ["Bitget private WebSocket subscribed: channel=account instType=SPOT"]
    assert warnings == []
    assert debugs == []


def test_handle_ws_message_logs_non_login_private_error() -> None:
    infos: list[str] = []
    warnings: list[str] = []
    debugs: list[str] = []
    dummy = SimpleNamespace(
        _log=SimpleNamespace(
            info=lambda message, *_args, **_kwargs: infos.append(message),
            warning=lambda message, *_args, **_kwargs: warnings.append(message),
            debug=lambda message, *_args, **_kwargs: debugs.append(message),
            error=lambda *_args, **_kwargs: None,
        ),
    )

    BitgetExecutionClient._handle_ws_message(
        dummy,  # type: ignore[arg-type]
        b'{"event":"error","code":"30016","msg":"channel not exist"}',
    )

    assert infos == []
    assert warnings == ["Bitget private WebSocket error: code=30016 msg=channel not exist"]
    assert debugs == []


def test_handle_ws_message_routes_account_channel_payload() -> None:
    handled: list[dict] = []
    dummy = SimpleNamespace(
        _handle_account_channel=lambda payload: handled.append(payload),
        _log=SimpleNamespace(
            info=lambda *_args, **_kwargs: None,
            warning=lambda *_args, **_kwargs: None,
            debug=lambda *_args, **_kwargs: None,
            error=lambda *_args, **_kwargs: None,
        ),
    )

    BitgetExecutionClient._handle_ws_message(
        dummy,  # type: ignore[arg-type]
        b'{"action":"snapshot","arg":{"instType":"SPOT","channel":"account","coin":"default"},"data":[{"coin":"USDT"}],"ts":1695713887792}',
    )

    assert handled[0]["arg"]["channel"] == "account"
    assert handled[0]["data"] == [{"coin": "USDT"}]


def test_handle_ws_message_routes_order_fill_and_position_channels() -> None:
    order_payloads: list[dict] = []
    fill_payloads: list[dict] = []
    position_payloads: list[dict] = []
    dummy = SimpleNamespace(
        _handle_orders_channel=lambda payload: order_payloads.append(payload),
        _handle_fill_channel=lambda payload: fill_payloads.append(payload),
        _handle_positions_channel=lambda payload: position_payloads.append(payload),
        _log=SimpleNamespace(
            info=lambda *_args, **_kwargs: None,
            warning=lambda *_args, **_kwargs: None,
            debug=lambda *_args, **_kwargs: None,
            error=lambda *_args, **_kwargs: None,
        ),
    )

    BitgetExecutionClient._handle_ws_message(
        dummy,  # type: ignore[arg-type]
        b'{"action":"snapshot","arg":{"instType":"SPOT","channel":"orders","instId":"default"},"data":[{"orderId":"1"}]}',
    )
    BitgetExecutionClient._handle_ws_message(
        dummy,  # type: ignore[arg-type]
        b'{"action":"snapshot","arg":{"instType":"SPOT","channel":"fill","instId":"default"},"data":[{"tradeId":"1"}]}',
    )
    BitgetExecutionClient._handle_ws_message(
        dummy,  # type: ignore[arg-type]
        b'{"action":"snapshot","arg":{"instType":"USDT-FUTURES","channel":"positions","instId":"default"},"data":[{"posId":"1"}]}',
    )

    assert order_payloads[0]["arg"]["channel"] == "orders"
    assert fill_payloads[0]["arg"]["channel"] == "fill"
    assert position_payloads[0]["arg"]["channel"] == "positions"


def test_handle_account_channel_generates_account_state() -> None:
    generated: list[dict] = []
    payload = {
        "action": "snapshot",
        "arg": {"instType": "SPOT", "channel": "account", "coin": "default"},
        "data": [
            {
                "coin": "USDT",
                "available": "100.5",
                "frozen": "2.0",
                "locked": "1.0",
                "limitAvailable": "97.5",
                "uTime": "1708883200123",
            },
        ],
    }
    dummy = SimpleNamespace(
        generate_account_state=lambda **kwargs: generated.append(kwargs),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
        ),
    )

    BitgetExecutionClient._handle_account_channel(dummy, payload)  # type: ignore[arg-type]

    assert len(generated) == 1
    balance = generated[0]["balances"][0]
    assert str(balance.free) == "100.50000000 USDT"
    assert str(balance.locked) == "3.00000000 USDT"
    assert str(balance.total) == "103.50000000 USDT"
    assert generated[0]["margins"] == []
    assert generated[0]["reported"] is True
    assert generated[0]["ts_event"] == millis_to_nanos(1708883200123)


def test_handle_orders_channel_generates_order_accepted() -> None:
    accepted: list[dict] = []
    warnings: list[str] = []
    client_order_id = ClientOrderId("client-1")
    venue_order_id = VenueOrderId("12345")
    order = SimpleNamespace(
        strategy_id="S-001",
        instrument_id="BTCUSDT.BITGET",
        client_order_id=client_order_id,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("45000"),
        has_price=True,
        trigger_price=None,
        status=OrderStatus.SUBMITTED,
    )
    dummy = SimpleNamespace(
        _cache=SimpleNamespace(
            order=lambda cid: order if cid == client_order_id else None,
            client_order_id=lambda vid: client_order_id if vid == venue_order_id else None,
            venue_order_id=lambda cid: venue_order_id if cid == client_order_id else None,
        ),
        generate_order_accepted=lambda **kwargs: accepted.append(kwargs),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            warning=lambda message, *_args, **_kwargs: warnings.append(message),
        ),
    )

    BitgetExecutionClient._handle_orders_channel(  # type: ignore[arg-type]
        dummy,
        {
            "action": "snapshot",
            "arg": {"instType": "SPOT", "channel": "orders", "instId": "default"},
            "data": [
                {
                    "clientOid": "client-1",
                    "orderId": "12345",
                    "price": "45000",
                    "size": "0.01",
                    "status": "new",
                    "uTime": "1708883200123",
                },
            ],
        },
    )

    assert warnings == []
    assert len(accepted) == 1
    assert accepted[0]["client_order_id"] == client_order_id
    assert accepted[0]["venue_order_id"] == venue_order_id
    assert accepted[0]["ts_event"] == millis_to_nanos(1708883200123)


def test_handle_orders_channel_generates_order_updated_for_changed_live_order() -> None:
    updated: list[dict] = []
    client_order_id = ClientOrderId("client-1")
    venue_order_id = VenueOrderId("12345")
    order = SimpleNamespace(
        strategy_id="S-001",
        instrument_id="BTCUSDT.BITGET",
        client_order_id=client_order_id,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("45000"),
        has_price=True,
        trigger_price=None,
        status=OrderStatus.ACCEPTED,
    )
    dummy = SimpleNamespace(
        _cache=SimpleNamespace(
            order=lambda cid: order if cid == client_order_id else None,
            client_order_id=lambda vid: client_order_id if vid == venue_order_id else None,
            venue_order_id=lambda cid: venue_order_id if cid == client_order_id else None,
        ),
        generate_order_updated=lambda **kwargs: updated.append(kwargs),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            warning=lambda *_args, **_kwargs: None,
        ),
    )

    BitgetExecutionClient._handle_orders_channel(  # type: ignore[arg-type]
        dummy,
        {
            "action": "snapshot",
            "arg": {"instType": "SPOT", "channel": "orders", "instId": "default"},
            "data": [
                {
                    "clientOid": "client-1",
                    "orderId": "12345",
                    "price": "45100",
                    "size": "0.02",
                    "status": "partially_filled",
                    "uTime": "1708883200999",
                },
            ],
        },
    )

    assert len(updated) == 1
    assert updated[0]["client_order_id"] == client_order_id
    assert updated[0]["venue_order_id"] == venue_order_id
    assert updated[0]["quantity"] == Quantity.from_str("0.02")
    assert updated[0]["price"] == Price.from_str("45100")
    assert updated[0]["ts_event"] == millis_to_nanos(1708883200999)


def test_handle_orders_channel_generates_order_canceled_from_venue_lookup() -> None:
    canceled: list[dict] = []
    warnings: list[str] = []
    client_order_id = ClientOrderId("client-1")
    venue_order_id = VenueOrderId("12345")
    order = SimpleNamespace(
        strategy_id="S-001",
        instrument_id="BTCUSDT.BITGET",
        client_order_id=client_order_id,
        quantity=Quantity.from_str("0.01"),
        price=Price.from_str("45000"),
        has_price=True,
        trigger_price=None,
        status=OrderStatus.ACCEPTED,
    )
    dummy = SimpleNamespace(
        _cache=SimpleNamespace(
            order=lambda cid: order if cid == client_order_id else None,
            client_order_id=lambda vid: client_order_id if vid == venue_order_id else None,
            venue_order_id=lambda cid: venue_order_id if cid == client_order_id else None,
        ),
        generate_order_canceled=lambda **kwargs: canceled.append(kwargs),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            warning=lambda message, *_args, **_kwargs: warnings.append(message),
        ),
    )

    BitgetExecutionClient._handle_orders_channel(  # type: ignore[arg-type]
        dummy,
        {
            "action": "snapshot",
            "arg": {"instType": "SPOT", "channel": "orders", "instId": "default"},
            "data": [
                {
                    "clientOid": "",
                    "orderId": "12345",
                    "price": "45000",
                    "size": "0.01",
                    "status": "cancelled",
                    "uTime": "1708883200123",
                },
            ],
        },
    )

    assert warnings == []
    assert len(canceled) == 1
    assert canceled[0]["client_order_id"] == client_order_id
    assert canceled[0]["venue_order_id"] == venue_order_id
    assert canceled[0]["ts_event"] == millis_to_nanos(1708883200123)


def test_handle_fill_channel_generates_order_filled_from_venue_lookup() -> None:
    fills: list[dict] = []
    warnings: list[str] = []
    client_order_id = ClientOrderId("client-1")
    venue_order_id = VenueOrderId("12345")
    usdt = Currency.from_str("USDT")
    order = SimpleNamespace(
        strategy_id="S-001",
        instrument_id="BTCUSDT.BITGET",
        client_order_id=client_order_id,
        side="BUY",
        order_type="LIMIT",
    )
    dummy = SimpleNamespace(
        _cache=SimpleNamespace(
            client_order_id=lambda vid: client_order_id if vid == venue_order_id else None,
            order=lambda cid: order if cid == client_order_id else None,
            instrument=lambda instrument_id: (
                SimpleNamespace(quote_currency=usdt)
                if instrument_id == "BTCUSDT.BITGET"
                else None
            ),
        ),
        generate_order_filled=lambda **kwargs: fills.append(kwargs),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            warning=lambda message, *_args, **_kwargs: warnings.append(message),
        ),
    )

    BitgetExecutionClient._handle_fill_channel(  # type: ignore[arg-type]
        dummy,
        {
            "action": "snapshot",
            "arg": {"instType": "SPOT", "channel": "fill", "instId": "default"},
            "data": [
                {
                    "orderId": "12345",
                    "tradeId": "t-1",
                    "priceAvg": "44995",
                    "size": "0.005",
                    "tradeScope": "taker",
                    "feeDetail": [{"feeCoin": "USDT", "totalFee": "0.1"}],
                    "uTime": "1708883200123",
                },
            ],
        },
    )

    assert warnings == []
    assert len(fills) == 1
    assert fills[0]["client_order_id"] == client_order_id
    assert fills[0]["venue_order_id"] == venue_order_id
    assert fills[0]["trade_id"] == TradeId("t-1")
    assert fills[0]["last_qty"] == Quantity.from_str("0.005")
    assert fills[0]["last_px"] == Price.from_str("44995")
    assert fills[0]["quote_currency"] == usdt
    assert fills[0]["commission"] == Money("0.1", usdt)
    assert fills[0]["liquidity_side"] == LiquiditySide.TAKER
    assert fills[0]["ts_event"] == millis_to_nanos(1708883200123)


def test_handle_fill_channel_maps_marker_liquidity_typo_to_maker() -> None:
    fills: list[dict] = []
    client_order_id = ClientOrderId("client-1")
    venue_order_id = VenueOrderId("12345")
    usdt = Currency.from_str("USDT")
    order = SimpleNamespace(
        strategy_id="S-001",
        instrument_id="BTCUSDT.BITGET",
        client_order_id=client_order_id,
        side="BUY",
        order_type="LIMIT",
    )
    dummy = SimpleNamespace(
        _cache=SimpleNamespace(
            client_order_id=lambda vid: client_order_id if vid == venue_order_id else None,
            order=lambda cid: order if cid == client_order_id else None,
            instrument=lambda _instrument_id: SimpleNamespace(quote_currency=usdt),
        ),
        generate_order_filled=lambda **kwargs: fills.append(kwargs),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            warning=lambda *_args, **_kwargs: None,
        ),
    )

    BitgetExecutionClient._handle_fill_channel(  # type: ignore[arg-type]
        dummy,
        {
            "action": "snapshot",
            "arg": {"instType": "SPOT", "channel": "fill", "instId": "default"},
            "data": [
                {
                    "orderId": "12345",
                    "tradeId": "t-2",
                    "priceAvg": "45010",
                    "size": "0.002",
                    "tradeScope": "marker",
                    "feeDetail": [],
                    "uTime": "1708883200456",
                },
            ],
        },
    )

    assert len(fills) == 1
    assert fills[0]["liquidity_side"] == LiquiditySide.MAKER
    assert fills[0]["commission"] == Money("0", usdt)


def test_handle_positions_channel_sends_position_status_report() -> None:
    reports: list[object] = []
    warnings: list[str] = []
    spot_instrument = SimpleNamespace(
        id=SimpleNamespace(symbol=SimpleNamespace(value="BTCUSDT.BITGET")),
        raw_symbol=SimpleNamespace(value="BTCUSDT"),
        size_precision=8,
        make_qty=lambda value, round_down=True: Quantity.from_str(value),
    )
    futures_instrument = SimpleNamespace(
        id="BTCUSDT-PERP.BITGET",
        raw_symbol=SimpleNamespace(value="BTCUSDT"),
        size_precision=4,
        make_qty=lambda value, round_down=True: Quantity.from_str(value),
    )
    dummy = SimpleNamespace(
        account_id="ACC-001",
        _cache=SimpleNamespace(
            instrument_ids=lambda venue=None: ["BTCUSDT.BITGET", "BTCUSDT-PERP.BITGET"],
            instrument=lambda instrument_id: {
                "BTCUSDT.BITGET": spot_instrument,
                "BTCUSDT-PERP.BITGET": futures_instrument,
            }.get(instrument_id),
        ),
        _send_position_status_report=lambda report: reports.append(report),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            warning=lambda message, *_args, **_kwargs: warnings.append(message),
        ),
    )

    BitgetExecutionClient._handle_positions_channel(  # type: ignore[arg-type]
        dummy,
        {
            "action": "snapshot",
            "arg": {"instType": "USDT-FUTURES", "channel": "positions", "instId": "default"},
            "data": [
                {
                    "posId": "p-1",
                    "instId": "BTCUSDT",
                    "holdSide": "long",
                    "total": "0.01",
                    "openPriceAvg": "45000",
                    "uTime": "1708883200123",
                },
            ],
        },
    )

    assert warnings == []
    assert len(reports) == 1
    report = reports[0]
    assert report.instrument_id == "BTCUSDT-PERP.BITGET"
    assert report.position_side == PositionSide.LONG
    assert report.quantity == Quantity.from_str("0.01")
    assert report.avg_px_open == Decimal("45000")
    assert report.venue_position_id == PositionId("p-1")
    assert report.ts_last == millis_to_nanos(1708883200123)


def test_handle_positions_channel_sends_flat_position_status_report() -> None:
    reports: list[object] = []
    instrument = SimpleNamespace(
        id="BTCUSDT-PERP.BITGET",
        raw_symbol=SimpleNamespace(value="BTCUSDT"),
        size_precision=4,
        make_qty=lambda value, round_down=True: Quantity.from_str(value),
    )
    dummy = SimpleNamespace(
        account_id="ACC-001",
        _cache=SimpleNamespace(
            instrument_ids=lambda venue=None: ["BTCUSDT-PERP.BITGET"],
            instrument=lambda _instrument_id: instrument,
        ),
        _send_position_status_report=lambda report: reports.append(report),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            warning=lambda *_args, **_kwargs: None,
        ),
    )

    BitgetExecutionClient._handle_positions_channel(  # type: ignore[arg-type]
        dummy,
        {
            "action": "snapshot",
            "arg": {"instType": "USDT-FUTURES", "channel": "positions", "instId": "default"},
            "data": [
                {
                    "posId": "p-2",
                    "instId": "BTCUSDT",
                    "holdSide": "long",
                    "total": "0",
                    "openPriceAvg": "0",
                    "uTime": "1708883200456",
                },
            ],
        },
    )

    assert len(reports) == 1
    report = reports[0]
    assert report.position_side == PositionSide.FLAT
    assert report.quantity == Quantity.zero(4)
    assert report.avg_px_open is None
    assert report.venue_position_id == PositionId("p-2")


@pytest.mark.asyncio
async def test_subscribe_private_ws_uses_expected_channels_for_spot_and_futures() -> None:
    sent: list[str] = []

    async def send_ws_text(message: str) -> None:
        sent.append(message)

    dummy = SimpleNamespace(
        _product_types=("SPOT", "USDT-FUTURES"),
        _send_ws_text=send_ws_text,
    )
    dummy._is_spot_product_type = lambda product_type: BitgetExecutionClient._is_spot_product_type(  # type: ignore[attr-defined]
        dummy,
        product_type,
    )

    with patch(
        "nautilus_trader.adapters.bitget.execution.nautilus_pyo3.BitgetWebSocketClient.subscribe_account_message",
        lambda product_type, coin: f"account:{product_type}:{coin}",
        create=True,
    ), patch(
        "nautilus_trader.adapters.bitget.execution.nautilus_pyo3.BitgetWebSocketClient.subscribe_message",
        lambda product_type, channel, inst_id: f"subscribe:{product_type}:{channel}:{inst_id}",
        create=True,
    ):
        await BitgetExecutionClient._subscribe_private_ws(dummy)  # type: ignore[arg-type]

    assert sent == [
        "account:SPOT:default",
        "subscribe:SPOT:orders:default",
        "subscribe:SPOT:fill:default",
        "account:USDT-FUTURES:default",
        "subscribe:USDT-FUTURES:orders:default",
        "subscribe:USDT-FUTURES:fill:default",
        "subscribe:USDT-FUTURES:positions:default",
    ]


def test_product_type_for_instrument_uses_settlement_currency() -> None:
    coin_perp = SimpleNamespace(
        id=SimpleNamespace(symbol=SimpleNamespace(value="BTCUSD-PERP")),
        base_currency=Currency.from_str("BTC"),
        quote_currency=Currency.from_str("USD"),
        settlement_currency=Currency.from_str("BTC"),
    )
    usdc_perp = SimpleNamespace(
        id=SimpleNamespace(symbol=SimpleNamespace(value="BTCUSDC-PERP")),
        base_currency=Currency.from_str("BTC"),
        quote_currency=Currency.from_str("USDC"),
        settlement_currency=Currency.from_str("USDC"),
    )

    dummy = SimpleNamespace(
        _currency_code=BitgetExecutionClient._currency_code,
        _is_delivery_symbol=BitgetExecutionClient._is_delivery_symbol,
    )

    assert BitgetExecutionClient._product_type_for_instrument(dummy, coin_perp) == nautilus_pyo3.BitgetProductType.COIN_FUTURES  # type: ignore[arg-type]
    assert BitgetExecutionClient._product_type_for_instrument(dummy, usdc_perp) == nautilus_pyo3.BitgetProductType.USDC_FUTURES  # type: ignore[arg-type]


def test_margin_coin_for_instrument_id_uses_settlement_currency() -> None:
    instrument_id = SimpleNamespace(symbol=SimpleNamespace(value="BTCUSDC-PERP"))
    instrument = SimpleNamespace(
        id=instrument_id,
        settlement_currency=Currency.from_str("USDC"),
        quote_currency=Currency.from_str("USDC"),
    )
    dummy = SimpleNamespace(
        _cache=SimpleNamespace(instrument=lambda actual_id: instrument if actual_id is instrument_id else None),
        _instrument_provider=SimpleNamespace(find=lambda actual_id: instrument if actual_id is instrument_id else None),
    )

    assert BitgetExecutionClient._margin_coin_for_instrument_id(dummy, instrument_id) == "USDC"  # type: ignore[arg-type]


def test_product_type_and_margin_coin_infer_from_unresolved_symbol() -> None:
    usdc_id = SimpleNamespace(symbol=SimpleNamespace(value="BTCUSDC-PERP"))
    coin_id = SimpleNamespace(symbol=SimpleNamespace(value="BTCUSD-PERP"))
    dummy = SimpleNamespace(
        _cache=SimpleNamespace(instrument=lambda _instrument_id: None),
        _instrument_provider=SimpleNamespace(find=lambda _instrument_id: None),
    )

    assert BitgetExecutionClient._product_type_for_instrument_id(dummy, usdc_id) == nautilus_pyo3.BitgetProductType.USDC_FUTURES  # type: ignore[arg-type]
    assert BitgetExecutionClient._product_type_for_instrument_id(dummy, coin_id) == nautilus_pyo3.BitgetProductType.COIN_FUTURES  # type: ignore[arg-type]
    assert BitgetExecutionClient._margin_coin_for_instrument_id(dummy, usdc_id) == "USDC"  # type: ignore[arg-type]
    assert BitgetExecutionClient._margin_coin_for_instrument_id(dummy, coin_id) == "BTC"  # type: ignore[arg-type]
