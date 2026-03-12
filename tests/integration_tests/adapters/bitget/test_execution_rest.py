# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from datetime import datetime
from datetime import timezone
import pytest
from types import SimpleNamespace

from nautilus_trader.adapters.bitget.execution import BitgetExecutionClient
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def _make_spot_instrument() -> SimpleNamespace:
    instrument_id = InstrumentId.from_str("BTCUSDT.BITGET")
    return SimpleNamespace(
        id=instrument_id,
        raw_symbol=SimpleNamespace(value="BTCUSDT"),
        quote_currency=Currency.from_str("USDT"),
    )


def _make_futures_instrument() -> SimpleNamespace:
    instrument_id = InstrumentId.from_str("BTCUSDT-PERP.BITGET")
    return SimpleNamespace(
        id=instrument_id,
        raw_symbol=SimpleNamespace(value="BTCUSDT"),
        quote_currency=Currency.from_str("USDT"),
        settlement_currency=Currency.from_str("USDT"),
        size_precision=3,
        make_qty=lambda value, round_down=True: Quantity.from_str(value),
    )


def _make_usdc_futures_instrument() -> SimpleNamespace:
    instrument_id = InstrumentId.from_str("BTCUSDC-PERP.BITGET")
    return SimpleNamespace(
        id=instrument_id,
        raw_symbol=SimpleNamespace(value="BTCUSDC"),
        quote_currency=Currency.from_str("USDC"),
        settlement_currency=Currency.from_str("USDC"),
        base_currency=Currency.from_str("BTC"),
        size_precision=3,
        make_qty=lambda value, round_down=True: Quantity.from_str(value),
    )


@pytest.mark.asyncio
async def test_submit_order_routes_to_http_and_indexes_venue_order_id() -> None:
    calls: list[dict] = []
    submitted: list[dict] = []
    denied: list[dict] = []
    rejected: list[dict] = []
    added_ids: list[tuple[ClientOrderId, VenueOrderId]] = []

    instrument = _make_spot_instrument()
    order = SimpleNamespace(
        is_closed=False,
        strategy_id=StrategyId("S-001"),
        trader_id=TraderId("T-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CID-001"),
        side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        quantity=Quantity.from_str("0.010"),
        time_in_force=TimeInForce.GTC,
        has_price=True,
        price=Price.from_str("100000.0"),
        has_trigger_price=False,
        trigger_price=None,
        is_post_only=False,
        is_reduce_only=False,
    )

    async def submit_order(**kwargs):
        calls.append(kwargs)
        return {"orderId": "12345", "clientOid": "CID-001"}

    dummy = SimpleNamespace(
        _http_client=SimpleNamespace(submit_order=submit_order),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            add_venue_order_id=lambda client_order_id, venue_order_id: added_ids.append(
                (client_order_id, venue_order_id),
            ),
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 111),
        _log=SimpleNamespace(
            warning=lambda *_args, **_kwargs: None,
            error=lambda *_args, **_kwargs: None,
        ),
        generate_order_submitted=lambda **kwargs: submitted.append(kwargs),
        generate_order_denied=lambda **kwargs: denied.append(kwargs),
        generate_order_rejected=lambda **kwargs: rejected.append(kwargs),
    )
    command = SimpleNamespace(order=order, params=None)

    await BitgetExecutionClient._submit_order(dummy, command)  # type: ignore[arg-type]

    assert calls[0]["symbol"] == "BTCUSDT"
    assert calls[0]["side"] == "buy"
    assert calls[0]["order_type"] == "limit"
    assert calls[0]["size"] == "0.010"
    assert calls[0]["price"] == "100000.0"
    assert submitted[0]["client_order_id"] == order.client_order_id
    assert added_ids == [(order.client_order_id, VenueOrderId("12345"))]
    assert denied == []
    assert rejected == []


@pytest.mark.asyncio
async def test_submit_order_passes_margin_coin_for_usdc_futures() -> None:
    calls: list[dict] = []
    instrument = _make_usdc_futures_instrument()
    order = SimpleNamespace(
        is_closed=False,
        strategy_id=StrategyId("S-001"),
        trader_id=TraderId("T-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CID-100"),
        side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        quantity=Quantity.from_str("0.010"),
        time_in_force=TimeInForce.GTC,
        has_price=True,
        price=Price.from_str("100000.0"),
        has_trigger_price=False,
        trigger_price=None,
        is_post_only=False,
        is_reduce_only=False,
    )

    async def submit_order(**kwargs):
        calls.append(kwargs)
        return {"orderId": "12345", "clientOid": "CID-100"}

    dummy = SimpleNamespace(
        _http_client=SimpleNamespace(submit_order=submit_order),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            add_venue_order_id=lambda *_args, **_kwargs: None,
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 111),
        _log=SimpleNamespace(
            warning=lambda *_args, **_kwargs: None,
            error=lambda *_args, **_kwargs: None,
        ),
        generate_order_submitted=lambda **_kwargs: None,
        generate_order_denied=lambda **_kwargs: None,
        generate_order_rejected=lambda **_kwargs: None,
    )

    await BitgetExecutionClient._submit_order(dummy, SimpleNamespace(order=order, params=None))  # type: ignore[arg-type]

    assert calls[0]["product_type"] == nautilus_pyo3.BitgetProductType.USDC_FUTURES
    assert calls[0]["margin_coin"] == "USDC"


@pytest.mark.asyncio
async def test_submit_order_passes_uta_margin_fields_for_spot_borrowing() -> None:
    calls: list[dict] = []
    instrument = _make_spot_instrument()
    order = SimpleNamespace(
        is_closed=False,
        strategy_id=StrategyId("S-001"),
        trader_id=TraderId("T-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CID-UTA-SPOT"),
        side=OrderSide.SELL,
        order_type=OrderType.LIMIT,
        quantity=Quantity.from_str("0.010"),
        time_in_force=TimeInForce.GTC,
        has_price=True,
        price=Price.from_str("100000.0"),
        has_trigger_price=False,
        trigger_price=None,
        is_post_only=False,
        is_reduce_only=False,
    )

    async def submit_order(**kwargs):
        calls.append(kwargs)
        return {"orderId": "12345", "clientOid": "CID-UTA-SPOT"}

    dummy = SimpleNamespace(
        _config=SimpleNamespace(
            account_mode="UTA",
            allow_cash_borrowing=True,
            margin_mode="cross",
            position_mode="one_way",
        ),
        _http_client=SimpleNamespace(submit_order=submit_order),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            add_venue_order_id=lambda *_args, **_kwargs: None,
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 111),
        _log=SimpleNamespace(
            warning=lambda *_args, **_kwargs: None,
            error=lambda *_args, **_kwargs: None,
        ),
        generate_order_submitted=lambda **_kwargs: None,
        generate_order_denied=lambda **_kwargs: None,
        generate_order_rejected=lambda **_kwargs: None,
    )

    await BitgetExecutionClient._submit_order(dummy, SimpleNamespace(order=order, params=None))  # type: ignore[arg-type]

    assert calls[0]["account_mode"] == "UTA"
    assert calls[0]["allow_cash_borrowing"] is True
    assert calls[0]["margin_mode"] == "cross"
    assert calls[0]["position_mode"] == "one_way"


@pytest.mark.asyncio
async def test_submit_order_passes_uta_one_way_fields_for_perp() -> None:
    calls: list[dict] = []
    instrument = _make_futures_instrument()
    order = SimpleNamespace(
        is_closed=False,
        strategy_id=StrategyId("S-001"),
        trader_id=TraderId("T-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CID-UTA-PERP"),
        side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        quantity=Quantity.from_str("0.010"),
        time_in_force=TimeInForce.GTC,
        has_price=True,
        price=Price.from_str("100000.0"),
        has_trigger_price=False,
        trigger_price=None,
        is_post_only=False,
        is_reduce_only=False,
    )

    async def submit_order(**kwargs):
        calls.append(kwargs)
        return {"orderId": "12345", "clientOid": "CID-UTA-PERP"}

    dummy = SimpleNamespace(
        _config=SimpleNamespace(
            account_mode="UTA",
            allow_cash_borrowing=False,
            margin_mode="cross",
            position_mode="one_way",
        ),
        _http_client=SimpleNamespace(submit_order=submit_order),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            add_venue_order_id=lambda *_args, **_kwargs: None,
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 111),
        _log=SimpleNamespace(
            warning=lambda *_args, **_kwargs: None,
            error=lambda *_args, **_kwargs: None,
        ),
        generate_order_submitted=lambda **_kwargs: None,
        generate_order_denied=lambda **_kwargs: None,
        generate_order_rejected=lambda **_kwargs: None,
    )

    await BitgetExecutionClient._submit_order(dummy, SimpleNamespace(order=order, params=None))  # type: ignore[arg-type]

    assert calls[0]["account_mode"] == "UTA"
    assert calls[0]["allow_cash_borrowing"] is False
    assert calls[0]["margin_mode"] == "cross"
    assert calls[0]["position_mode"] == "one_way"


@pytest.mark.asyncio
async def test_submit_order_failure_normalizes_bitget_http_error_reason() -> None:
    rejected: list[dict] = []
    instrument = _make_spot_instrument()
    order = SimpleNamespace(
        is_closed=False,
        strategy_id=StrategyId("S-001"),
        trader_id=TraderId("T-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CID-ERR"),
        side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        quantity=Quantity.from_str("0.010"),
        time_in_force=TimeInForce.GTC,
        has_price=True,
        price=Price.from_str("100000.0"),
        has_trigger_price=False,
        trigger_price=None,
        is_post_only=False,
        is_reduce_only=False,
    )

    async def submit_order(**_kwargs):
        raise RuntimeError(
            'HTTP request failed with status 400 body={"code":"22001","msg":"insufficient balance"}',
        )

    dummy = SimpleNamespace(
        _config=SimpleNamespace(
            account_mode="UTA",
            allow_cash_borrowing=True,
            margin_mode="cross",
            position_mode="one_way",
        ),
        _http_client=SimpleNamespace(submit_order=submit_order),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 111),
        _log=SimpleNamespace(
            warning=lambda *_args, **_kwargs: None,
            error=lambda *_args, **_kwargs: None,
        ),
        generate_order_submitted=lambda **_kwargs: None,
        generate_order_denied=lambda **_kwargs: None,
        generate_order_rejected=lambda **kwargs: rejected.append(kwargs),
    )

    await BitgetExecutionClient._submit_order(dummy, SimpleNamespace(order=order, params=None))  # type: ignore[arg-type]

    assert rejected[0]["reason"] == (
        "bitget_http_error: status=400 code=22001 msg=insufficient balance"
    )


@pytest.mark.asyncio
async def test_generate_order_status_report_maps_spot_payload() -> None:
    instrument = _make_spot_instrument()
    command = SimpleNamespace(
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CID-001"),
        venue_order_id=None,
    )

    async def request_order_status_report(**kwargs):
        assert kwargs["symbol"] == "BTCUSDT"
        return {
            "orderId": "12345",
            "clientOid": "CID-001",
            "price": "100000.0",
            "priceAvg": "99950.0",
            "size": "0.010",
            "baseVolume": "0.005",
            "side": "buy",
            "orderType": "limit",
            "status": "live",
            "force": "gtc",
            "cTime": "1700000000000",
            "uTime": "1700000001000",
        }

    dummy = SimpleNamespace(
        account_id=AccountId("BITGET-001"),
        _http_client=SimpleNamespace(request_order_status_report=request_order_status_report),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            instrument_ids=lambda venue=None: [instrument.id],
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 999),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            warning=lambda *_args, **_kwargs: None,
            exception=lambda *_args, **_kwargs: None,
        ),
    )

    report = await BitgetExecutionClient.generate_order_status_report(
        dummy,  # type: ignore[arg-type]
        command,
    )

    assert isinstance(report, OrderStatusReport)
    assert report.client_order_id == ClientOrderId("CID-001")
    assert report.venue_order_id == VenueOrderId("12345")
    assert report.order_status == OrderStatus.PARTIALLY_FILLED
    assert report.quantity == Quantity.from_str("0.010")
    assert report.filled_qty == Quantity.from_str("0.005")
    assert report.avg_px == Price.from_str("99950.0").as_decimal()


@pytest.mark.asyncio
async def test_generate_order_status_report_maps_uta_spot_payload() -> None:
    instrument = _make_spot_instrument()
    command = SimpleNamespace(
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CID-UTA-001"),
        venue_order_id=None,
    )

    async def request_order_status_report(**kwargs):
        assert kwargs["symbol"] == "BTCUSDT"
        assert kwargs["account_mode"] == "UTA"
        assert kwargs["allow_cash_borrowing"] is True
        assert kwargs["margin_mode"] == "cross"
        assert kwargs["position_mode"] == "one_way"
        return {
            "orderId": "12345",
            "clientOid": "CID-UTA-001",
            "price": "100000.0",
            "avgPrice": "99950.0",
            "qty": "0.010",
            "cumExecQty": "0.005",
            "side": "buy",
            "orderType": "limit",
            "orderStatus": "partially_filled",
            "timeInForce": "gtc",
            "createdTime": "1700000000000",
            "updatedTime": "1700000001000",
        }

    dummy = SimpleNamespace(
        account_id=AccountId("BITGET-001"),
        _config=SimpleNamespace(
            account_mode="UTA",
            allow_cash_borrowing=True,
            margin_mode="cross",
            position_mode="one_way",
        ),
        _http_client=SimpleNamespace(request_order_status_report=request_order_status_report),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            instrument_ids=lambda venue=None: [instrument.id],
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 999),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            warning=lambda *_args, **_kwargs: None,
            exception=lambda *_args, **_kwargs: None,
        ),
    )

    report = await BitgetExecutionClient.generate_order_status_report(
        dummy,  # type: ignore[arg-type]
        command,
    )

    assert isinstance(report, OrderStatusReport)
    assert report.client_order_id == ClientOrderId("CID-UTA-001")
    assert report.venue_order_id == VenueOrderId("12345")
    assert report.order_status == OrderStatus.PARTIALLY_FILLED
    assert report.quantity == Quantity.from_str("0.010")
    assert report.filled_qty == Quantity.from_str("0.005")
    assert report.avg_px == Price.from_str("99950.0").as_decimal()


@pytest.mark.asyncio
async def test_generate_order_status_reports_maps_uta_history_payload() -> None:
    instrument = _make_spot_instrument()
    command = SimpleNamespace(
        instrument_id=instrument.id,
        open_only=False,
        start=None,
        end=None,
    )

    async def request_order_status_reports(**kwargs):
        assert kwargs["symbol"] == "BTCUSDT"
        assert kwargs["account_mode"] == "UTA"
        assert kwargs["allow_cash_borrowing"] is True
        assert kwargs["margin_mode"] == "cross"
        assert kwargs["position_mode"] == "one_way"
        return [
            {
                "orderId": "12345",
                "clientOid": "CID-UTA-002",
                "price": "100000.0",
                "avgPrice": "99950.0",
                "qty": "0.010",
                "cumExecQty": "0.005",
                "side": "buy",
                "orderType": "limit",
                "orderStatus": "partially_filled",
                "timeInForce": "gtc",
                "createdTime": "1700000000000",
                "updatedTime": "1700000001000",
            },
        ]

    dummy = SimpleNamespace(
        account_id=AccountId("BITGET-001"),
        _config=SimpleNamespace(
            account_mode="UTA",
            allow_cash_borrowing=True,
            margin_mode="cross",
            position_mode="one_way",
        ),
        _product_types=(nautilus_pyo3.BitgetProductType.SPOT,),
        _http_client=SimpleNamespace(request_order_status_reports=request_order_status_reports),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            instrument_ids=lambda venue=None: [instrument.id],
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 999),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            warning=lambda *_args, **_kwargs: None,
            exception=lambda *_args, **_kwargs: None,
        ),
    )

    reports = await BitgetExecutionClient.generate_order_status_reports(
        dummy,  # type: ignore[arg-type]
        command,
    )

    assert len(reports) == 1
    assert reports[0].client_order_id == ClientOrderId("CID-UTA-002")
    assert reports[0].order_status == OrderStatus.PARTIALLY_FILLED


@pytest.mark.asyncio
async def test_generate_fill_reports_maps_payload_and_filters_order_id() -> None:
    instrument = _make_spot_instrument()

    async def request_fill_reports(**kwargs):
        assert kwargs["symbol"] == "BTCUSDT"
        return [
            {
                "orderId": "12345",
                "tradeId": "54321",
                "side": "buy",
                "priceAvg": "100001.0",
                "size": "0.010",
                "feeDetail": {"feeCoin": "USDT", "totalFee": "0.10"},
                "tradeScope": "taker",
                "cTime": "1700000002000",
            },
            {
                "orderId": "99999",
                "tradeId": "11111",
                "side": "buy",
                "priceAvg": "100002.0",
                "size": "0.020",
                "feeDetail": {"feeCoin": "USDT", "totalFee": "0.20"},
                "tradeScope": "maker",
                "cTime": "1700000003000",
            },
        ]

    command = SimpleNamespace(
        instrument_id=instrument.id,
        venue_order_id=VenueOrderId("12345"),
        start=None,
        end=None,
    )

    dummy = SimpleNamespace(
        account_id=AccountId("BITGET-001"),
        _http_client=SimpleNamespace(request_fill_reports=request_fill_reports),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            instrument_ids=lambda venue=None: [instrument.id],
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 999),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            info=lambda *_args, **_kwargs: None,
            exception=lambda *_args, **_kwargs: None,
        ),
    )

    reports = await BitgetExecutionClient.generate_fill_reports(
        dummy,  # type: ignore[arg-type]
        command,
    )

    assert len(reports) == 1
    report = reports[0]
    assert isinstance(report, FillReport)
    assert report.venue_order_id == VenueOrderId("12345")
    assert report.trade_id == TradeId("54321")
    assert report.liquidity_side == LiquiditySide.TAKER


@pytest.mark.asyncio
async def test_generate_fill_reports_maps_uta_payload_and_filters_order_id() -> None:
    instrument = _make_spot_instrument()

    async def request_fill_reports(**kwargs):
        assert kwargs["symbol"] == "BTCUSDT"
        assert kwargs["account_mode"] == "UTA"
        assert kwargs["allow_cash_borrowing"] is True
        assert kwargs["margin_mode"] == "cross"
        assert kwargs["position_mode"] == "one_way"
        return [
            {
                "orderId": "12345",
                "execId": "54321",
                "side": "buy",
                "execPrice": "100001.0",
                "execQty": "0.010",
                "feeDetail": [{"feeCoin": "USDT", "fee": "0.10"}],
                "tradeScope": "T",
                "createdTime": "1700000002000",
            },
            {
                "orderId": "99999",
                "execId": "11111",
                "side": "buy",
                "execPrice": "100002.0",
                "execQty": "0.020",
                "feeDetail": [{"feeCoin": "USDT", "fee": "0.20"}],
                "tradeScope": "M",
                "createdTime": "1700000003000",
            },
        ]

    command = SimpleNamespace(
        instrument_id=instrument.id,
        venue_order_id=VenueOrderId("12345"),
        start=None,
        end=None,
    )

    dummy = SimpleNamespace(
        account_id=AccountId("BITGET-001"),
        _config=SimpleNamespace(
            account_mode="UTA",
            allow_cash_borrowing=True,
            margin_mode="cross",
            position_mode="one_way",
        ),
        _http_client=SimpleNamespace(request_fill_reports=request_fill_reports),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            instrument_ids=lambda venue=None: [instrument.id],
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 999),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            info=lambda *_args, **_kwargs: None,
            exception=lambda *_args, **_kwargs: None,
        ),
    )

    reports = await BitgetExecutionClient.generate_fill_reports(
        dummy,  # type: ignore[arg-type]
        command,
    )

    assert len(reports) == 1
    report = reports[0]
    assert isinstance(report, FillReport)
    assert report.venue_order_id == VenueOrderId("12345")
    assert report.trade_id == TradeId("54321")
    assert report.liquidity_side == LiquiditySide.TAKER
    assert report.commission == Money("0.10", Currency.from_str("USDT"))


@pytest.mark.asyncio
async def test_generate_fill_reports_pages_uta_history_until_start_boundary_and_dedupes_overlap() -> None:
    instrument = _make_futures_instrument()
    calls: list[dict] = []
    start = datetime.fromtimestamp(1700000002, tz=timezone.utc)
    page_one_fillers = [
        {
            "symbol": "ETHUSDT",
            "orderId": "",
            "execId": "",
            "side": "buy",
            "execPrice": "2000.0",
            "execQty": "0.010",
            "feeDetail": [{"feeCoin": "USDT", "fee": "0.10"}],
            "tradeScope": "T",
            "createdTime": str(1700000004098 - i),
        }
        for i in range(98)
    ]

    async def request_fill_reports(**kwargs):
        calls.append(kwargs)
        if kwargs["end"] is None:
            return [
                *page_one_fillers,
                {
                    "orderId": "12345",
                    "execId": "t-1",
                    "side": "buy",
                    "execPrice": "100001.0",
                    "execQty": "0.010",
                    "feeDetail": [{"feeCoin": "USDT", "fee": "0.10"}],
                    "tradeScope": "T",
                    "createdTime": "1700000004000",
                },
                {
                    "orderId": "12346",
                    "execId": "t-2",
                    "side": "buy",
                    "execPrice": "100002.0",
                    "execQty": "0.020",
                    "feeDetail": [{"feeCoin": "USDT", "fee": "0.20"}],
                    "tradeScope": "M",
                    "createdTime": "1700000003000",
                },
            ]
        assert kwargs["end"] == 1700000002999
        return [
            {
                "orderId": "12346",
                "execId": "t-2",
                "side": "buy",
                "execPrice": "100002.0",
                "execQty": "0.020",
                "feeDetail": [{"feeCoin": "USDT", "fee": "0.20"}],
                "tradeScope": "M",
                "createdTime": "1700000003000",
            },
            {
                "orderId": "12347",
                "execId": "t-3",
                "side": "sell",
                "execPrice": "100003.0",
                "execQty": "0.030",
                "feeDetail": [{"feeCoin": "USDT", "fee": "0.30"}],
                "tradeScope": "T",
                "createdTime": "1700000002000",
            },
            {
                "orderId": "12348",
                "execId": "t-4",
                "side": "buy",
                "execPrice": "100004.0",
                "execQty": "0.040",
                "feeDetail": [{"feeCoin": "USDT", "fee": "0.40"}],
                "tradeScope": "M",
                "createdTime": "1700000001000",
            },
        ]

    command = SimpleNamespace(
        instrument_id=instrument.id,
        venue_order_id=None,
        start=start,
        end=None,
    )

    dummy = SimpleNamespace(
        account_id=AccountId("BITGET-001"),
        _config=SimpleNamespace(
            account_mode="UTA",
            allow_cash_borrowing=False,
            margin_mode="cross",
            position_mode="one_way",
        ),
        _http_client=SimpleNamespace(request_fill_reports=request_fill_reports),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            instrument_ids=lambda venue=None: [instrument.id],
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 999),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            info=lambda *_args, **_kwargs: None,
            exception=lambda *_args, **_kwargs: None,
        ),
    )

    reports = await BitgetExecutionClient.generate_fill_reports(
        dummy,  # type: ignore[arg-type]
        command,
    )

    assert len(calls) == 2
    assert [call["limit"] for call in calls] == [100, 100]
    assert [report.trade_id for report in reports] == [
        TradeId("t-1"),
        TradeId("t-2"),
        TradeId("t-3"),
    ]
    assert all(report.ts_event >= 1700000002000 * 1_000_000 for report in reports)


@pytest.mark.asyncio
async def test_generate_position_status_reports_maps_futures_payload() -> None:
    instrument = _make_futures_instrument()

    async def request_position_status_reports(**kwargs):
        assert kwargs["symbol"] is None
        return [
            {
                "symbol": "BTCUSDT",
                "total": "0.500",
                "holdSide": "long",
                "openPriceAvg": "100000.0",
                "posId": "POS-001",
                "uTime": "1700000004000",
            },
        ]

    command = SimpleNamespace(instrument_id=None, start=None, end=None)

    dummy = SimpleNamespace(
        account_id=AccountId("BITGET-001"),
        _product_types=(nautilus_pyo3.BitgetProductType.USDT_FUTURES,),
        _http_client=SimpleNamespace(request_position_status_reports=request_position_status_reports),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            instrument_ids=lambda venue=None: [instrument.id],
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 999),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            info=lambda *_args, **_kwargs: None,
            exception=lambda *_args, **_kwargs: None,
        ),
    )

    reports = await BitgetExecutionClient.generate_position_status_reports(
        dummy,  # type: ignore[arg-type]
        command,
    )

    assert len(reports) == 1
    report = reports[0]
    assert isinstance(report, PositionStatusReport)
    assert report.position_side == PositionSide.LONG
    assert report.venue_position_id == PositionId("POS-001")


@pytest.mark.asyncio
async def test_generate_position_status_reports_maps_uta_futures_payload() -> None:
    instrument = _make_futures_instrument()

    async def request_position_status_reports(**kwargs):
        assert kwargs["symbol"] is None
        assert kwargs["account_mode"] == "UTA"
        assert kwargs["allow_cash_borrowing"] is False
        assert kwargs["margin_mode"] == "cross"
        assert kwargs["position_mode"] == "one_way"
        return [
            {
                "symbol": "BTCUSDT",
                "total": "0.500",
                "posSide": "long",
                "avgPrice": "100000.0",
                "updatedTime": "1700000004000",
            },
        ]

    command = SimpleNamespace(instrument_id=None, start=None, end=None)

    dummy = SimpleNamespace(
        account_id=AccountId("BITGET-001"),
        _config=SimpleNamespace(
            account_mode="UTA",
            allow_cash_borrowing=False,
            margin_mode="cross",
            position_mode="one_way",
        ),
        _product_types=(nautilus_pyo3.BitgetProductType.USDT_FUTURES,),
        _http_client=SimpleNamespace(request_position_status_reports=request_position_status_reports),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            instrument_ids=lambda venue=None: [instrument.id],
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 999),
        _log=SimpleNamespace(
            debug=lambda *_args, **_kwargs: None,
            info=lambda *_args, **_kwargs: None,
            exception=lambda *_args, **_kwargs: None,
        ),
    )

    reports = await BitgetExecutionClient.generate_position_status_reports(
        dummy,  # type: ignore[arg-type]
        command,
    )

    assert len(reports) == 1
    report = reports[0]
    assert isinstance(report, PositionStatusReport)
    assert report.position_side == PositionSide.LONG
    assert report.venue_position_id is None


@pytest.mark.asyncio
async def test_batch_cancel_orders_uses_http_batch_endpoint() -> None:
    calls: list[dict] = []
    instrument = _make_spot_instrument()
    order_one = SimpleNamespace(
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CID-001"),
        venue_order_id=None,
        is_closed=False,
    )
    order_two = SimpleNamespace(
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CID-002"),
        venue_order_id=VenueOrderId("12345"),
        is_closed=False,
    )

    async def batch_cancel_orders(**kwargs):
        calls.append(kwargs)
        return {"successList": [{"clientOid": "CID-001"}, {"orderId": "12345"}], "failureList": []}

    cache_orders = {
        order_one.client_order_id: order_one,
        order_two.client_order_id: order_two,
    }

    dummy = SimpleNamespace(
        _http_client=SimpleNamespace(batch_cancel_orders=batch_cancel_orders),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            order=lambda client_order_id: cache_orders.get(client_order_id),
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 111),
        _log=SimpleNamespace(error=lambda *_args, **_kwargs: None),
        generate_order_cancel_rejected=lambda **kwargs: (_ for _ in ()).throw(
            AssertionError(f"unexpected rejection {kwargs}"),
        ),
    )
    command = SimpleNamespace(
        cancels=[
            SimpleNamespace(
                instrument_id=instrument.id,
                client_order_id=order_one.client_order_id,
                venue_order_id=None,
            ),
            SimpleNamespace(
                instrument_id=instrument.id,
                client_order_id=order_two.client_order_id,
                venue_order_id=order_two.venue_order_id,
            ),
        ],
    )

    await BitgetExecutionClient._batch_cancel_orders(dummy, command)  # type: ignore[arg-type]

    assert calls == [
        {
            "product_type": nautilus_pyo3.BitgetProductType.SPOT,
            "symbol": "BTCUSDT",
            "margin_coin": None,
            "client_oids": ["CID-001"],
            "order_ids": ["12345"],
        },
    ]


@pytest.mark.asyncio
async def test_cancel_all_orders_routes_to_http_endpoint() -> None:
    calls: list[dict] = []
    instrument = _make_spot_instrument()

    async def cancel_all_orders(**kwargs):
        calls.append(kwargs)
        return [{"clientOid": "CID-001"}]

    dummy = SimpleNamespace(
        _http_client=SimpleNamespace(cancel_all_orders=cancel_all_orders),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _log=SimpleNamespace(
            warning=lambda *_args, **_kwargs: None,
            error=lambda *_args, **_kwargs: None,
        ),
    )
    command = SimpleNamespace(
        instrument_id=instrument.id,
        order_side=OrderSide.NO_ORDER_SIDE,
    )

    await BitgetExecutionClient._cancel_all_orders(dummy, command)  # type: ignore[arg-type]

    assert calls == [
        {
            "product_type": nautilus_pyo3.BitgetProductType.SPOT,
            "symbol": "BTCUSDT",
            "margin_coin": None,
            "account_mode": None,
            "allow_cash_borrowing": False,
            "margin_mode": None,
            "position_mode": None,
        },
    ]


@pytest.mark.asyncio
async def test_cancel_all_orders_passes_uta_margin_fields_for_spot_borrowing() -> None:
    calls: list[dict] = []
    instrument = _make_spot_instrument()

    async def cancel_all_orders(**kwargs):
        calls.append(kwargs)
        return [{"clientOid": "CID-001"}]

    dummy = SimpleNamespace(
        _config=SimpleNamespace(
            account_mode="UTA",
            allow_cash_borrowing=True,
            margin_mode="cross",
            position_mode="one_way",
        ),
        _http_client=SimpleNamespace(cancel_all_orders=cancel_all_orders),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _log=SimpleNamespace(
            warning=lambda *_args, **_kwargs: None,
            error=lambda *_args, **_kwargs: None,
        ),
    )
    command = SimpleNamespace(
        instrument_id=instrument.id,
        order_side=OrderSide.NO_ORDER_SIDE,
    )

    await BitgetExecutionClient._cancel_all_orders(dummy, command)  # type: ignore[arg-type]

    assert calls == [
        {
            "product_type": nautilus_pyo3.BitgetProductType.SPOT,
            "symbol": "BTCUSDT",
            "margin_coin": None,
            "account_mode": "UTA",
            "allow_cash_borrowing": True,
            "margin_mode": "cross",
            "position_mode": "one_way",
        },
    ]


@pytest.mark.asyncio
async def test_cancel_order_failure_generates_cancel_rejected() -> None:
    rejected: list[dict] = []
    instrument = _make_spot_instrument()
    order = SimpleNamespace(
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CID-001"),
        venue_order_id=None,
        is_closed=False,
    )

    async def cancel_order(**kwargs):
        raise RuntimeError("boom")

    dummy = SimpleNamespace(
        _http_client=SimpleNamespace(cancel_order=cancel_order),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            order=lambda client_order_id: order if client_order_id == order.client_order_id else None,
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 222),
        _log=SimpleNamespace(error=lambda *_args, **_kwargs: None),
        generate_order_cancel_rejected=lambda **kwargs: rejected.append(kwargs),
    )
    command = SimpleNamespace(
        instrument_id=instrument.id,
        client_order_id=order.client_order_id,
        venue_order_id=None,
    )

    await BitgetExecutionClient._cancel_order(dummy, command)  # type: ignore[arg-type]

    assert rejected[0]["client_order_id"] == ClientOrderId("CID-001")
    assert rejected[0]["reason"] == "boom"


@pytest.mark.asyncio
async def test_cancel_order_passes_uta_margin_fields_and_normalizes_http_error() -> None:
    rejected: list[dict] = []
    instrument = _make_spot_instrument()
    order = SimpleNamespace(
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("CID-UTA-CANCEL"),
        venue_order_id=None,
        is_closed=False,
    )

    async def cancel_order(**kwargs):
        assert kwargs["account_mode"] == "UTA"
        assert kwargs["allow_cash_borrowing"] is True
        assert kwargs["margin_mode"] == "cross"
        assert kwargs["position_mode"] == "one_way"
        raise RuntimeError(
            'HTTP request failed with status 400 body={"code":"22001","msg":"insufficient balance"}',
        )

    dummy = SimpleNamespace(
        _config=SimpleNamespace(
            account_mode="UTA",
            allow_cash_borrowing=True,
            margin_mode="cross",
            position_mode="one_way",
        ),
        _http_client=SimpleNamespace(cancel_order=cancel_order),
        _cache=SimpleNamespace(
            instrument=lambda instrument_id: instrument if instrument_id == instrument.id else None,
            order=lambda client_order_id: order if client_order_id == order.client_order_id else None,
        ),
        _instrument_provider=SimpleNamespace(find=lambda instrument_id: instrument),
        _clock=SimpleNamespace(timestamp_ns=lambda: 222),
        _log=SimpleNamespace(error=lambda *_args, **_kwargs: None),
        generate_order_cancel_rejected=lambda **kwargs: rejected.append(kwargs),
    )
    command = SimpleNamespace(
        instrument_id=instrument.id,
        client_order_id=order.client_order_id,
        venue_order_id=None,
    )

    await BitgetExecutionClient._cancel_order(dummy, command)  # type: ignore[arg-type]

    assert rejected[0]["client_order_id"] == ClientOrderId("CID-UTA-CANCEL")
    assert rejected[0]["reason"] == (
        "bitget_http_error: status=400 code=22001 msg=insufficient balance"
    )
