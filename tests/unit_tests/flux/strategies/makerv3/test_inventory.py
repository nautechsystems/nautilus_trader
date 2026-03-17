from __future__ import annotations

from collections.abc import Mapping
from decimal import Decimal
from typing import Any

from nautilus_trader.flux.strategies.makerv3.inventory import InventorySkewCache
from nautilus_trader.flux.strategies.makerv3.inventory import compute_inventory_skew


def _runtime_params(**overrides: Decimal | float | str) -> dict[str, Decimal]:
    params: dict[str, Decimal] = {
        "des_qty_global": Decimal(0),
        "max_qty_global": Decimal(1),
        "max_skew_bps_global": Decimal(0),
        "des_qty_local": Decimal(0),
        "max_qty_local": Decimal(1),
        "max_skew_bps_local": Decimal(0),
        "linear_offset_bps": Decimal(0),
    }
    for name, value in overrides.items():
        params[name] = Decimal(str(value))
    return params


def test_compute_inventory_skew_treats_long_inventory_as_negative_quoted_fv_shift() -> None:
    skew = compute_inventory_skew(
        global_position_qty_venue=Decimal("0.5"),
        global_position_qty_base=Decimal(5),
        global_spot_qty=Decimal(9),
        local_position_qty_venue=Decimal("0.2"),
        local_position_qty_base=Decimal(2),
        local_spot_qty=Decimal(1),
        base_currency="BTC",
        runtime_params=_runtime_params(
            max_qty_global=2,
            max_skew_bps_global=10,
            max_qty_local=10,
            max_skew_bps_local=5,
            linear_offset_bps=1,
        ),
    )

    assert skew["inventory_source"] == "positions_plus_spot"
    assert skew["inventory_qty_base"] == Decimal(14)
    assert skew["inventory_qty"] == Decimal(14)
    assert skew["position_qty_base"] == Decimal(5)
    assert skew["position_qty_venue"] == Decimal("0.5")
    assert skew["global_position_qty_base"] == Decimal(5)
    assert skew["global_position_qty_venue"] == Decimal("0.5")
    assert skew["global_position_qty"] == Decimal(5)
    assert skew["global_spot_qty"] == Decimal(9)
    assert skew["global_inventory_qty_base"] == Decimal(14)
    assert skew["global_inventory_qty"] == Decimal(14)
    assert skew["global_inventory_source"] == "positions_plus_spot"
    assert skew["local_position_qty_base"] == Decimal(2)
    assert skew["local_position_qty_venue"] == Decimal("0.2")
    assert skew["local_position_qty"] == Decimal(2)
    assert skew["local_spot_qty"] == Decimal(1)
    assert skew["local_inventory_qty_base"] == Decimal(3)
    assert skew["local_inventory_qty"] == Decimal(3)
    assert skew["local_inventory_source"] == "positions_plus_spot"
    # Positive skew means quoted FV up / quotes richer. Long inventory must
    # create a negative skew so the strategy sells back toward target.
    assert skew["global_ratio"] == Decimal(-1)
    assert skew["global_skew_bps"] == Decimal(-10)
    assert skew["local_ratio"] == Decimal("-0.3")
    assert skew["local_skew_bps"] == Decimal("-1.5")
    assert skew["total_skew_bps"] == Decimal("-10.5")


def test_compute_inventory_skew_treats_short_inventory_as_positive_quoted_fv_shift() -> None:
    skew = compute_inventory_skew(
        global_position_qty_venue=Decimal("0.5"),
        global_position_qty_base=Decimal(5),
        global_spot_qty=Decimal(9),
        local_position_qty_venue=Decimal("0.2"),
        local_position_qty_base=Decimal(2),
        local_spot_qty=Decimal(1),
        base_currency="BTC",
        runtime_params=_runtime_params(
            des_qty_global=20,
            max_qty_global=20,
            max_skew_bps_global=10,
            des_qty_local=5,
            max_qty_local=10,
            max_skew_bps_local=5,
        ),
    )

    # Short inventory relative to target must create a positive skew so the
    # strategy buys back toward target.
    assert skew["global_inventory_qty"] == Decimal(14)
    assert skew["local_inventory_qty"] == Decimal(3)
    assert skew["global_ratio"] == Decimal("0.3")
    assert skew["global_skew_bps"] == Decimal(3)
    assert skew["local_ratio"] == Decimal("0.2")
    assert skew["local_skew_bps"] == Decimal(1)
    assert skew["total_skew_bps"] == Decimal(4)


def test_compute_inventory_skew_keeps_global_spot_out_of_local_when_local_inventory_is_absent() -> (
    None
):
    skew = compute_inventory_skew(
        global_position_qty_venue=None,
        global_position_qty_base=None,
        global_spot_qty=Decimal(1000),
        local_position_qty_venue=None,
        local_position_qty_base=None,
        local_spot_qty=None,
        base_currency="BTC",
        runtime_params=_runtime_params(
            max_qty_global=2000,
            max_skew_bps_global=12,
            max_qty_local=50,
            max_skew_bps_local=9,
        ),
    )

    assert skew["inventory_source"] == "spot_balance"
    assert skew["inventory_qty_base"] == Decimal(1000)
    assert skew["inventory_qty"] == Decimal(1000)
    assert skew["global_inventory_source"] == "spot_balance"
    assert skew["global_inventory_qty_base"] == Decimal(1000)
    assert skew["global_inventory_qty"] == Decimal(1000)
    assert skew["local_inventory_source"] == "unavailable"
    assert skew["local_inventory_qty_base"] is None
    assert skew["local_inventory_qty"] is None
    assert skew["global_ratio"] == Decimal("-0.5")
    assert skew["global_skew_bps"] == Decimal(-6)
    assert skew["local_ratio"] is None
    assert skew["local_skew_bps"] is None


def test_inventory_skew_cache_honors_ttl_and_invalidation() -> None:
    cache = InventorySkewCache(ttl_ms=100)
    calls = {"count": 0}

    def _compute(runtime_params: Mapping[str, Any]) -> dict[str, Any]:
        calls["count"] += 1
        value = Decimal(calls["count"])
        return {
            "inventory_qty_base": value,
            "inventory_qty": value,
            "inventory_source": "positions",
            "base_currency": "BTC",
            "position_qty_base": value,
            "position_qty_venue": value,
            "position_qty": value,
            "spot_qty": value,
            "global_position_qty_base": value,
            "global_position_qty_venue": value,
            "global_position_qty": value,
            "global_spot_qty": value,
            "global_inventory_qty_base": value + value,
            "global_inventory_qty": value + value,
            "global_inventory_source": "positions_plus_spot",
            "local_position_qty_base": value,
            "local_position_qty_venue": value,
            "local_position_qty": value,
            "local_spot_qty": None,
            "local_inventory_qty_base": value,
            "local_inventory_qty": value,
            "local_inventory_source": "positions",
            "des_qty_global": runtime_params["des_qty_global"],
            "max_qty_global": runtime_params["max_qty_global"],
            "max_skew_bps_global": runtime_params["max_skew_bps_global"],
            "des_qty_local": runtime_params["des_qty_local"],
            "max_qty_local": runtime_params["max_qty_local"],
            "max_skew_bps_local": runtime_params["max_skew_bps_local"],
            "linear_offset_bps": runtime_params["linear_offset_bps"],
            "global_ratio": value,
            "global_skew_bps": value,
            "local_ratio": value,
            "local_skew_bps": value,
            "total_skew_bps": value,
        }

    params = _runtime_params()

    first = cache.get(now_ns=1_000_000_000, runtime_params=params, compute=_compute)
    second = cache.get(now_ns=1_050_000_000, runtime_params=params, compute=_compute)
    third = cache.get(now_ns=1_150_000_000, runtime_params=params, compute=_compute)
    cache.invalidate()
    fourth = cache.get(now_ns=1_160_000_000, runtime_params=params, compute=_compute)

    assert first["inventory_qty"] == Decimal(1)
    assert second["inventory_qty"] == Decimal(1)
    assert third["inventory_qty"] == Decimal(2)
    assert fourth["inventory_qty"] == Decimal(3)
    assert calls["count"] == 3
