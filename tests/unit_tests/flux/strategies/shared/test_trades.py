from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
import sys
from types import SimpleNamespace
from types import ModuleType


@dataclass(frozen=True)
class _IBOrderTagsStub:
    outsideRth: bool = False
    includeOvernight: bool = False

    @property
    def value(self) -> str:
        return f"outsideRth={self.outsideRth};includeOvernight={self.includeOvernight}"


_IB_COMMON_STUB = ModuleType("nautilus_trader.adapters.interactive_brokers.common")
_IB_COMMON_STUB.IBOrderTags = _IBOrderTagsStub
_IB_COMMON_STUB.IB_CLIENT_ID = "INTERACTIVE_BROKERS"
sys.modules.setdefault(_IB_COMMON_STUB.__name__, _IB_COMMON_STUB)


from flux.strategies.shared.trades import build_trade_payload


def _instrument(*, raw_symbol: str = "PLUMEUSDT") -> SimpleNamespace:
    return SimpleNamespace(
        raw_symbol=raw_symbol,
        base_currency=SimpleNamespace(code="PLUME"),
        quote_currency=SimpleNamespace(code="USDT"),
        multiplier=Decimal("10"),
        info={},
        make_qty=lambda value: Decimal(str(value)),
    )


def _event(
    *,
    commission: object,
    instrument_id: str = "PLUMEUSDT-LINEAR.BYBIT",
    last_qty: Decimal = Decimal("1000"),
) -> SimpleNamespace:
    return SimpleNamespace(
        instrument_id=instrument_id,
        client_order_id="O-1",
        trade_id="T-1",
        order_side="BUY",
        last_qty=last_qty,
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


def test_build_trade_payload_exposes_normalized_quantity_fields_for_exact_multiplier_contracts() -> None:
    payload = build_trade_payload(
        strategy_id="plumeusdt_okx_perp_makerv3",
        event=_event(
            commission="0.00127360 USDT",
            instrument_id="PLUME-USDT-SWAP.OKX",
            last_qty=Decimal("100"),
        ),
        instrument_lookup=lambda _instrument_id: _instrument(raw_symbol="PLUME-USDT-SWAP"),
        trade_role="maker",
    )

    assert payload["qty"] == "100"
    assert payload["qty_base"] == "1000"
    assert payload["qty_venue"] == "100"
    assert payload["qty_conversion_status"] == "exact_multiplier"
    assert payload["qty_conversion_source"] == "generic:multiplier"
