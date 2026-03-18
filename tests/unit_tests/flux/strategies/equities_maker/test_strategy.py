from __future__ import annotations

import inspect
from decimal import Decimal

import pytest

from nautilus_trader.flux.strategies import EquitiesMakerStrategy as EquitiesMakerStrategyFromRoot
from nautilus_trader.flux.strategies import (
    EquitiesMakerStrategyConfig as EquitiesMakerStrategyConfigFromRoot,
)
from nautilus_trader.flux.strategies.equities_maker import EquitiesMakerStrategy
from nautilus_trader.flux.strategies.equities_maker import EquitiesMakerStrategyConfig
from nautilus_trader.flux.strategies.makerv4.market_data import IbkrQuoteSnapshot
from nautilus_trader.flux.strategies.makerv4.wire import MakerFill
from nautilus_trader.flux.strategies.registry import get_strategy_identity
from nautilus_trader.flux.strategies.registry import get_strategy_spec
from nautilus_trader.flux.strategies.registry import resolve_strategy_spec_for_strategy_id
from nautilus_trader.model.identifiers import InstrumentId


_OVERNIGHT_TS_MS = 1_742_176_800_000


def _config(**overrides) -> EquitiesMakerStrategyConfig:
    base = {
        "maker_instrument_id": InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        "reference_instrument_id": InstrumentId.from_str("AAPL.NASDAQ"),
        "order_qty": Decimal("1"),
        "external_strategy_id": "aapl_tradexyz_maker",
        "strategy_id": "aapl_tradexyz_maker",
        "outside_rth_hedge_enabled": True,
    }
    base.update(overrides)
    return EquitiesMakerStrategyConfig(**base)


def _quote(*, bid: str = "190.00", ask: str = "190.04", age_ms: int = 25) -> IbkrQuoteSnapshot:
    return IbkrQuoteSnapshot(
        instrument_id="AAPL.NASDAQ",
        bid=Decimal(bid),
        ask=Decimal(ask),
        age_ms=age_ms,
        ts_ms=1_000,
    )


def _fill(
    *,
    fill_id: str = "fill-overnight-policy",
    side: str = "BUY",
    qty: str = "2",
    px: str = "190.00",
    ts_ms: int = _OVERNIGHT_TS_MS,
) -> MakerFill:
    return MakerFill(
        fill_id=fill_id,
        side=side,
        qty=Decimal(qty),
        price=Decimal(px),
        ts_ms=ts_ms,
    )


def test_canonical_strategy_exports_match_root_surface() -> None:
    assert EquitiesMakerStrategyFromRoot is EquitiesMakerStrategy
    assert EquitiesMakerStrategyConfigFromRoot is EquitiesMakerStrategyConfig


def test_registry_exports_equities_maker_spec_and_suffix_resolution() -> None:
    identity = get_strategy_identity("equities_maker")
    spec = get_strategy_spec("equities_maker")
    resolved = resolve_strategy_spec_for_strategy_id("aapl_tradexyz_maker")

    assert identity.strategy_id == "equities_maker"
    assert identity.strategy_family == "equities_maker"
    assert identity.strategy_version == "v1"
    assert identity.param_set == "equities_maker"
    assert identity.profile_key == "equities_maker"
    assert spec.strategy_cls is EquitiesMakerStrategy
    assert spec.config_cls is EquitiesMakerStrategyConfig
    assert resolved is spec


def test_equities_maker_config_omits_local_inventory_ownership_fields() -> None:
    parameters = inspect.signature(EquitiesMakerStrategyConfig).parameters

    for removed_name in (
        "des_qty_local",
        "max_qty_local",
        "max_skew_bps_local",
    ):
        assert removed_name not in parameters

    with pytest.raises((TypeError, ValueError), match="des_qty_local"):
        EquitiesMakerStrategyConfig(
            maker_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
            order_qty=Decimal("1"),
            strategy_id="aapl_tradexyz_maker",
            external_strategy_id="aapl_tradexyz_maker",
            des_qty_local=1.0,
        )


def test_equities_maker_forces_maker_mode_and_preserves_overnight_immediate_hedge() -> None:
    strategy = EquitiesMakerStrategy(config=_config())

    assert strategy._execution_mode() == "maker_hedge"
    strategy._runtime_params["execution_mode"] = "take_take"
    assert strategy._execution_mode() == "maker_hedge"

    order = strategy.record_maker_fill(
        fill=_fill(),
        quote=_quote(),
        maker_fee_bps=Decimal("0.25"),
    )

    assert order is not None
    assert order.route == "SMART"
    assert order.time_in_force == "IOC"
    assert order.outside_rth is True
    assert order.include_overnight is True
    assert order.cancel_after_ms is None
