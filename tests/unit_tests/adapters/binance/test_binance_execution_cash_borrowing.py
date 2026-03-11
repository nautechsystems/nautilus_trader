import asyncio
from types import SimpleNamespace
from unittest.mock import AsyncMock

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.execution import BinanceCommonExecutionClient
from nautilus_trader.adapters.binance.http.account import BinanceOrderHttp
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import StopLimitOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


def _spot_instrument() -> CurrencyPair:
    return CurrencyPair(
        instrument_id=InstrumentId.from_str("BTCUSDT.BINANCE_SPOT"),
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
    )


def _limit_order() -> LimitOrder:
    instrument = _spot_instrument()
    return LimitOrder(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-123456"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        time_in_force=TimeInForce.GTC,
        init_id=TestIdStubs.uuid(),
        ts_init=0,
    )


def _order_factory() -> OrderFactory:
    return OrderFactory(
        trader_id=TestIdStubs.trader_id(),
        strategy_id=TestIdStubs.strategy_id(),
        clock=TestClock(),
    )


def _market_order() -> MarketOrder:
    return _order_factory().market(
        _spot_instrument().id,
        OrderSide.BUY,
        Quantity.from_str("0.100"),
        time_in_force=TimeInForce.IOC,
    )


def _stop_market_order() -> StopMarketOrder:
    return _order_factory().stop_market(
        _spot_instrument().id,
        OrderSide.BUY,
        Quantity.from_str("0.100"),
        Price.from_str("51000.00"),
    )


def _stop_limit_order() -> StopLimitOrder:
    return _order_factory().stop_limit(
        _spot_instrument().id,
        OrderSide.BUY,
        Quantity.from_str("0.100"),
        price=Price.from_str("50000.00"),
        trigger_price=Price.from_str("51000.00"),
    )


def _submit_command(*, allow_cash_borrowing: bool = False) -> SimpleNamespace:
    order = _limit_order()
    return SimpleNamespace(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        order=order,
        params=None,
        position_id=None,
        allow_cash_borrowing=allow_cash_borrowing,
    )


def _submit_list_command(*, allow_cash_borrowing: bool = False) -> SimpleNamespace:
    return SimpleNamespace(
        order_list=SimpleNamespace(orders=[_limit_order()]),
        params=None,
        position_id=None,
        allow_cash_borrowing=allow_cash_borrowing,
    )


def _dummy_submitter(
    *,
    account_type: BinanceAccountType,
    allow_cash_borrowing: bool = True,
) -> SimpleNamespace:
    dummy = SimpleNamespace(
        _binance_account_type=account_type,
        _allow_cash_borrowing=allow_cash_borrowing,
        _http_account=SimpleNamespace(new_order=AsyncMock()),
        _enum_parser=SimpleNamespace(
            parse_internal_order_side=lambda _side: BinanceOrderSide.BUY,
            parse_internal_order_type=lambda _order: BinanceOrderType.LIMIT,
        ),
        _determine_time_in_force=lambda _order: BinanceTimeInForce.GTC,
        _determine_good_till_date=lambda _order, _time_in_force: None,
        _determine_reduce_only_str=lambda _order: None,
        _recv_window=5_000,
    )
    dummy._spot_margin_side_effect_fields = (
        BinanceCommonExecutionClient._spot_margin_side_effect_fields.__get__(dummy, SimpleNamespace)
    )
    return dummy


def test_submit_order_forwards_allow_cash_borrowing_to_submit_order_inner() -> None:
    async def _run() -> None:
        command = _submit_command(allow_cash_borrowing=True)
        dummy = SimpleNamespace(
            _get_position_side_from_position_id=lambda **_kwargs: None,
            _submit_order_inner=AsyncMock(),
        )

        await BinanceCommonExecutionClient._submit_order(dummy, command)  # type: ignore[arg-type]

        dummy._submit_order_inner.assert_awaited_once_with(
            command.order,
            None,
            command.params,
            True,
        )

    asyncio.run(_run())


def test_submit_order_list_forwards_allow_cash_borrowing_to_submit_order_inner() -> None:
    async def _run() -> None:
        command = _submit_list_command(allow_cash_borrowing=True)
        dummy = SimpleNamespace(
            _get_position_side_from_position_id=lambda **_kwargs: None,
            _submit_order_inner=AsyncMock(),
        )

        await BinanceCommonExecutionClient._submit_order_list(dummy, command)  # type: ignore[arg-type]

        dummy._submit_order_inner.assert_awaited_once_with(
            command.order_list.orders[0],
            None,
            command.params,
            True,
        )

    asyncio.run(_run())


def test_binance_order_post_parameters_accept_margin_side_effect_fields() -> None:
    params = BinanceOrderHttp.PostParameters(
        symbol=BinanceSymbol("BTCUSDT"),
        timestamp="1700000000000",
        side=BinanceOrderSide.BUY,
        type=BinanceOrderType.LIMIT,
        timeInForce=BinanceTimeInForce.GTC,
        quantity="1",
        price="50000",
        sideEffectType="AUTO_BORROW_REPAY",
        autoRepayAtCancel="FALSE",
    )

    assert params.sideEffectType == "AUTO_BORROW_REPAY"
    assert params.autoRepayAtCancel == "FALSE"


def test_submit_limit_order_sets_margin_side_effect_fields_when_cash_borrowing_allowed() -> None:
    async def _run() -> None:
        dummy = _dummy_submitter(account_type=BinanceAccountType.MARGIN)

        await BinanceCommonExecutionClient._submit_limit_order(  # type: ignore[arg-type]
            dummy,
            _limit_order(),
            None,
            None,
            True,
        )

        kwargs = dummy._http_account.new_order.call_args.kwargs
        assert kwargs["side_effect_type"] == "AUTO_BORROW_REPAY"
        assert kwargs["auto_repay_at_cancel"] == "FALSE"

    asyncio.run(_run())


def test_submit_limit_order_omits_margin_side_effect_fields_for_futures() -> None:
    async def _run() -> None:
        dummy = _dummy_submitter(account_type=BinanceAccountType.USDT_FUTURES)

        await BinanceCommonExecutionClient._submit_limit_order(  # type: ignore[arg-type]
            dummy,
            _limit_order(),
            None,
            None,
            True,
        )

        kwargs = dummy._http_account.new_order.call_args.kwargs
        assert "side_effect_type" not in kwargs or kwargs["side_effect_type"] is None
        assert "auto_repay_at_cancel" not in kwargs or kwargs["auto_repay_at_cancel"] is None

    asyncio.run(_run())


def test_submit_limit_order_omits_margin_side_effect_fields_when_client_cash_borrowing_disabled() -> None:
    async def _run() -> None:
        dummy = _dummy_submitter(
            account_type=BinanceAccountType.MARGIN,
            allow_cash_borrowing=False,
        )

        await BinanceCommonExecutionClient._submit_limit_order(  # type: ignore[arg-type]
            dummy,
            _limit_order(),
            None,
            None,
            True,
        )

        kwargs = dummy._http_account.new_order.call_args.kwargs
        assert kwargs["side_effect_type"] is None
        assert kwargs["auto_repay_at_cancel"] is None

    asyncio.run(_run())


def test_submit_market_order_sets_margin_side_effect_fields_when_cash_borrowing_allowed() -> None:
    async def _run() -> None:
        dummy = _dummy_submitter(account_type=BinanceAccountType.MARGIN)

        await BinanceCommonExecutionClient._submit_market_order(  # type: ignore[arg-type]
            dummy,
            _market_order(),
            None,
            None,
            True,
        )

        kwargs = dummy._http_account.new_order.call_args.kwargs
        assert kwargs["side_effect_type"] == "AUTO_BORROW_REPAY"
        assert kwargs["auto_repay_at_cancel"] == "FALSE"

    asyncio.run(_run())


def test_submit_stop_market_order_sets_margin_side_effect_fields_when_cash_borrowing_allowed() -> None:
    async def _run() -> None:
        dummy = _dummy_submitter(account_type=BinanceAccountType.MARGIN)

        await BinanceCommonExecutionClient._submit_stop_market_order(  # type: ignore[arg-type]
            dummy,
            _stop_market_order(),
            None,
            None,
            True,
        )

        kwargs = dummy._http_account.new_order.call_args.kwargs
        assert kwargs["side_effect_type"] == "AUTO_BORROW_REPAY"
        assert kwargs["auto_repay_at_cancel"] == "FALSE"

    asyncio.run(_run())


def test_submit_stop_limit_order_sets_margin_side_effect_fields_when_cash_borrowing_allowed() -> None:
    async def _run() -> None:
        dummy = _dummy_submitter(account_type=BinanceAccountType.MARGIN)

        await BinanceCommonExecutionClient._submit_stop_limit_order(  # type: ignore[arg-type]
            dummy,
            _stop_limit_order(),
            None,
            None,
            True,
        )

        kwargs = dummy._http_account.new_order.call_args.kwargs
        assert kwargs["side_effect_type"] == "AUTO_BORROW_REPAY"
        assert kwargs["auto_repay_at_cancel"] == "FALSE"

    asyncio.run(_run())


def test_submit_order_inner_denies_cash_borrowing_when_client_capability_disabled() -> None:
    async def _run() -> None:
        denied: list[str] = []
        dummy = SimpleNamespace(
            _binance_account_type=BinanceAccountType.MARGIN,
            _allow_cash_borrowing=False,
            _extract_price_match=lambda _order, _params: None,
            _validate_order_pre_submit=lambda _order: None,
            _validate_cash_borrowing_request=BinanceCommonExecutionClient._validate_cash_borrowing_request.__get__(
                SimpleNamespace(
                    _binance_account_type=BinanceAccountType.MARGIN,
                    _allow_cash_borrowing=False,
                ),
                SimpleNamespace,
            ),
            _deny_order_pre_submit=lambda _order, reason: denied.append(reason),
            _log=SimpleNamespace(debug=lambda *_args, **_kwargs: None),
        )

        await BinanceCommonExecutionClient._submit_order_inner(  # type: ignore[arg-type]
            dummy,
            _limit_order(),
            None,
            None,
            True,
        )

        assert denied == ["CASH_BORROWING_NOT_ENABLED"]

    asyncio.run(_run())
