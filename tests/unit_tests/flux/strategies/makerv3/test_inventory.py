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


def test_compute_inventory_skew_prefers_position_and_clamps_ratios() -> None:
    skew = compute_inventory_skew(
        position_qty=Decimal(5),
        spot_qty=Decimal(9),
        base_currency="BTC",
        runtime_params=_runtime_params(
            max_qty_global=2,
            max_skew_bps_global=10,
            max_qty_local=10,
            max_skew_bps_local=5,
            linear_offset_bps=1,
        ),
    )

    assert skew["inventory_source"] == "maker_position"
    assert skew["inventory_qty"] == Decimal(5)
    assert skew["global_ratio"] == Decimal(1)
    assert skew["global_skew_bps"] == Decimal(10)
    assert skew["local_ratio"] == Decimal("0.5")
    assert skew["local_skew_bps"] == Decimal("2.5")
    assert skew["total_skew_bps"] == Decimal("13.5")


def test_compute_inventory_skew_uses_spot_inventory_when_position_unavailable() -> None:
    skew = compute_inventory_skew(
        position_qty=None,
        spot_qty=Decimal(3),
        base_currency="BTC",
        runtime_params=_runtime_params(
            max_qty_global=6,
            max_skew_bps_global=12,
        ),
    )

    assert skew["inventory_source"] == "maker_spot_balance"
    assert skew["inventory_qty"] == Decimal(3)
    assert skew["global_ratio"] == Decimal("0.5")
    assert skew["global_skew_bps"] == Decimal(6)


def test_inventory_skew_cache_honors_ttl_and_invalidation() -> None:
    cache = InventorySkewCache(ttl_ms=100)
    calls = {"count": 0}

    def _compute(runtime_params: Mapping[str, Any]) -> dict[str, Any]:
        calls["count"] += 1
        value = Decimal(calls["count"])
        return {
            "inventory_qty": value,
            "inventory_source": "maker_position",
            "base_currency": "BTC",
            "position_qty": value,
            "spot_qty": value,
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
