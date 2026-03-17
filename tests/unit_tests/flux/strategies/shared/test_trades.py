from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

from flux.strategies.shared.trades import build_trade_payload


def _instrument() -> SimpleNamespace:
    return SimpleNamespace(
        raw_symbol="PLUMEUSDT",
        base_currency=SimpleNamespace(code="PLUME"),
        quote_currency=SimpleNamespace(code="USDT"),
    )


def _event(*, commission: object) -> SimpleNamespace:
    return SimpleNamespace(
        instrument_id="PLUMEUSDT-LINEAR.BYBIT",
        client_order_id="O-1",
        trade_id="T-1",
        order_side="BUY",
        last_qty=Decimal("1000"),
        last_px=Decimal("0.012736"),
        ts_event=1_773_751_508_406_000_000,
        commission=commission,
    )


def test_build_trade_payload_parses_string_commission_with_currency_suffix() -> None:
    payload = build_trade_payload(
        strategy_id="plumeusdt_bybit_perp_makerv3",
        event=_event(commission="0.00127360 USDT"),
        instrument_lookup=lambda _instrument_id: _instrument(),
        trade_role="maker",
    )

    assert payload["fee"] == "0.00127360"
    assert payload["fee_amount_raw"] == "0.00127360"
    assert payload["fee_asset_raw"] == "USDT"
    assert payload["fee_quote"] == "0.00127360"
