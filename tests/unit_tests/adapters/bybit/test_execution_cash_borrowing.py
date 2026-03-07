import asyncio
from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.execution import BybitExecutionClient
from nautilus_trader.adapters.bybit.providers import BybitInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


def _create_ws_mock() -> MagicMock:
    mock = MagicMock(spec=nautilus_pyo3.BybitWebSocketClient)
    mock.is_closed = MagicMock(return_value=False)
    mock.connect = AsyncMock()
    mock.wait_until_active = AsyncMock()
    mock.close = AsyncMock()
    mock.subscribe_orders = AsyncMock()
    mock.subscribe_executions = AsyncMock()
    mock.subscribe_positions = AsyncMock()
    mock.subscribe_wallet = AsyncMock()
    mock.cache_instrument = MagicMock()
    mock.set_account_id = MagicMock()
    mock.set_mm_level = MagicMock()
    mock.submit_order = AsyncMock()
    return mock


def _mock_http_client() -> MagicMock:
    mock = MagicMock(spec=nautilus_pyo3.BybitHttpClient)
    mock.api_key_masked = "test_api_key"
    mock.cancel_all_requests = MagicMock()
    mock.set_use_spot_position_reports = MagicMock()

    mock_account_state = MagicMock()
    mock_account_state.to_dict = MagicMock(
        return_value={
            "account_id": "BYBIT-123",
            "account_type": "CASH",
            "base_currency": "USDT",
            "reported": True,
            "balances": [
                {
                    "currency": "USDT",
                    "total": "100000.00000000",
                    "locked": "0.00000000",
                    "free": "100000.00000000",
                },
            ],
            "margins": [],
            "info": {},
            "event_id": str(TestIdStubs.uuid()),
            "ts_event": 0,
            "ts_init": 0,
        },
    )
    mock.request_account_state = AsyncMock(return_value=mock_account_state)

    mock_account_details = MagicMock()
    mock_account_details.mkt_maker_level = 0
    mock.get_account_details = AsyncMock(return_value=mock_account_details)
    mock.submit_order = AsyncMock()
    return mock


def _mock_instrument_provider() -> MagicMock:
    provider = MagicMock(spec=BybitInstrumentProvider)
    provider.initialize = AsyncMock()
    provider.instruments_pyo3 = MagicMock(return_value=[])
    provider.product_types = (nautilus_pyo3.BybitProductType.SPOT,)
    provider._product_types = (nautilus_pyo3.BybitProductType.SPOT,)
    return provider


def _spot_instrument() -> CurrencyPair:
    return CurrencyPair(
        instrument_id=InstrumentId.from_str("BTCUSDT-SPOT.BYBIT"),
        raw_symbol=Symbol("BTCUSDT"),
        base_currency=BTC,
        quote_currency=USDT,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        max_quantity=Quantity.from_str("1000"),
        min_quantity=Quantity.from_str("0.000001"),
        max_notional=None,
        min_notional=Money(1.00, USDT),
        ts_event=0,
        ts_init=0,
        maker_fee=Decimal("0.0001"),
        taker_fee=Decimal("0.0006"),
    )


def _build_client(
    monkeypatch,
    loop,
    *,
    reset_cash_borrowing_registry: bool = True,
    **config_kwargs,
) -> tuple[BybitExecutionClient, MagicMock]:
    if reset_cash_borrowing_registry and config_kwargs.get("allow_cash_borrowing"):
        AccountFactory.deregister_cash_borrowing(BYBIT_VENUE.value)

    ws_private_client = _create_ws_mock()
    ws_trade_client = _create_ws_mock()
    ws_iter = iter([ws_private_client, ws_trade_client])

    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.execution.nautilus_pyo3.BybitWebSocketClient.new_private",
        lambda *args, **kwargs: next(ws_iter),
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.execution.nautilus_pyo3.BybitWebSocketClient.new_trade",
        lambda *args, **kwargs: next(ws_iter),
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.bybit.execution.BybitExecutionClient._await_account_registered",
        AsyncMock(),
    )

    http_client = _mock_http_client()
    instrument_provider = _mock_instrument_provider()
    clock = LiveClock()
    msgbus = MessageBus(trader_id=TraderId("TRADER-001"), clock=clock)
    cache = TestComponentStubs.cache()

    config = BybitExecClientConfig(
        api_key="test_api_key",
        api_secret="test_api_secret",
        product_types=(nautilus_pyo3.BybitProductType.SPOT,),
        **config_kwargs,
    )

    client = BybitExecutionClient(
        loop=loop,
        client=http_client,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=instrument_provider,
        config=config,
        name=None,
    )
    return client, ws_trade_client


def _submit_command(*, allow_cash_borrowing: bool = False) -> SubmitOrder:
    instrument = _spot_instrument()
    order = MarketOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("0.100"),
        time_in_force=TimeInForce.IOC,
        reduce_only=False,
        quote_quantity=False,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )
    return SubmitOrder(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        command_id=TestIdStubs.uuid(),
        ts_init=0,
        position_id=None,
        client_id=None,
        allow_cash_borrowing=allow_cash_borrowing,
    )


def test_submit_order_sets_is_leverage_when_cash_borrowing_is_allowed(monkeypatch) -> None:
    async def _run() -> None:
        client, ws_trade_client = _build_client(
            monkeypatch,
            asyncio.get_running_loop(),
            allow_cash_borrowing=True,
        )
        await client._connect()
        try:
            await client._submit_order(_submit_command(allow_cash_borrowing=True))
            ws_trade_client.submit_order.assert_awaited_once()
            assert ws_trade_client.submit_order.call_args.kwargs["is_leverage"] is True
        finally:
            await client._disconnect()
            AccountFactory.deregister_cash_borrowing(BYBIT_VENUE.value)

    asyncio.run(_run())


def test_submit_order_keeps_cash_path_by_default(monkeypatch) -> None:
    async def _run() -> None:
        client, ws_trade_client = _build_client(
            monkeypatch,
            asyncio.get_running_loop(),
            allow_cash_borrowing=True,
        )
        await client._connect()
        try:
            await client._submit_order(_submit_command())
            ws_trade_client.submit_order.assert_awaited_once()
            assert ws_trade_client.submit_order.call_args.kwargs["is_leverage"] is False
        finally:
            await client._disconnect()
            AccountFactory.deregister_cash_borrowing(BYBIT_VENUE.value)

    asyncio.run(_run())


def test_submit_order_denied_when_cash_borrowing_requested_but_client_disabled(monkeypatch) -> None:
    async def _run() -> None:
        client, ws_trade_client = _build_client(monkeypatch, asyncio.get_running_loop())
        await client._connect()
        try:
            await client._submit_order(_submit_command(allow_cash_borrowing=True))
            ws_trade_client.submit_order.assert_not_called()
        finally:
            await client._disconnect()

    asyncio.run(_run())


def test_client_init_tolerates_pre_registered_cash_borrowing(monkeypatch) -> None:
    async def _run() -> None:
        AccountFactory.deregister_cash_borrowing(BYBIT_VENUE.value)
        AccountFactory.register_cash_borrowing(BYBIT_VENUE.value)

        try:
            client, _ = _build_client(
                monkeypatch,
                asyncio.get_running_loop(),
                allow_cash_borrowing=True,
                reset_cash_borrowing_registry=False,
            )
            await client._disconnect()
        finally:
            AccountFactory.deregister_cash_borrowing(BYBIT_VENUE.value)

    asyncio.run(_run())
