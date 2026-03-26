from __future__ import annotations

from decimal import Decimal

import pytest

from nautilus_trader.flux.strategies.equities_taker import EquitiesTakerStrategy
from nautilus_trader.flux.strategies.equities_taker import EquitiesTakerStrategyConfig
from nautilus_trader.flux.strategies.equities_taker import runtime_params as runtime_params_mod
from nautilus_trader.model.identifiers import InstrumentId


class _FakeRedis:
    def __init__(self) -> None:
        self.hashes: dict[str, dict[str, bytes]] = {}
        self.hset_calls: list[tuple[str, dict[str, str]]] = []
        self.publish_calls: list[tuple[str, str]] = []

    def hmget(self, key: str, fields: list[str]) -> list[bytes | None]:
        mapping = self.hashes.get(key, {})
        return [mapping.get(field) for field in fields]

    def hkeys(self, key: str) -> list[str]:
        return list(self.hashes.get(key, {}).keys())

    def hset(self, key: str, mapping: dict[str, str]) -> int:
        self.hset_calls.append((key, dict(mapping)))
        target = self.hashes.setdefault(key, {})
        for field, value in mapping.items():
            target[field] = value.encode("utf-8")
        return len(mapping)

    def publish(self, channel: str, payload: str) -> int:
        self.publish_calls.append((channel, payload))
        return 1


def _build_config() -> EquitiesTakerStrategyConfig:
    return EquitiesTakerStrategyConfig(
        maker_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
        order_qty=Decimal("1"),
        strategy_id="aapl_tradexyz_taker",
        external_strategy_id="aapl_tradexyz_taker",
    )


def test_equities_taker_defaults_expose_taker_only_runtime_surface() -> None:
    defaults = runtime_params_mod.EQUITIES_TAKER_RUNTIME_PARAM_DEFAULTS
    schema = runtime_params_mod.EQUITIES_TAKER_RUNTIME_PARAM_SCHEMA

    assert runtime_params_mod.PARAM_SET == "equities_taker"
    assert runtime_params_mod.profile_key() == "equities_taker"
    assert defaults["qty"] == 1.0
    assert defaults["max_qty_global"] == 100.0
    assert defaults["max_skew_bps_global"] == 10.0
    assert defaults["bid_edge_take_bps"] == 5.0
    assert defaults["ask_edge_take_bps"] == 5.0
    assert defaults["take_cooldown_ms"] == 1_000
    assert defaults["hedge_ioc_cross_mid_bps"] == 2.0
    assert defaults["hedge_ioc_max_cross_bps"] == 10.0
    assert defaults["ibkr_fee_plan"] == "tiered"
    assert defaults["ibkr_fee_min_usd"] == 0.35
    assert defaults["hl_taker_fee_bps"] == 4.5
    assert defaults["hl_maker_fee_bps"] == 0.25
    assert defaults["assumed_hedge_fee_bps"] == 1.0

    for removed_name in (
        "execution_mode",
        "instant_hedge_enabled",
        "hedge_style",
        "maker_fee_source",
        "hedge_fee_source",
        "hedge_fee_plan",
        "linear_offset_bps",
        "bid_edge1",
        "ask_edge1",
        "place_edge1",
        "distance1",
        "n_orders1",
        "bid_edge2",
        "ask_edge2",
        "place_edge2",
        "distance2",
        "n_orders2",
        "bid_edge3",
        "ask_edge3",
        "place_edge3",
        "distance3",
        "n_orders3",
        "des_qty_local",
        "max_qty_local",
        "max_skew_bps_local",
    ):
        assert removed_name not in defaults
        assert removed_name not in schema


def test_equities_taker_params_manager_uses_family_param_set() -> None:
    redis_client = _FakeRedis()
    strategy = EquitiesTakerStrategy(config=_build_config())
    manager = runtime_params_mod.params_manager_factory(redis_client=redis_client)(strategy)

    payload = manager.publish_update(
        {
            "bid_edge_take_bps": 7.5,
            "ask_edge_take_bps": 8.5,
            "take_cooldown_ms": 2_500,
            "hedge_ioc_cross_mid_bps": 3.0,
            "hedge_ioc_max_cross_bps": 12.0,
            "ibkr_fee_plan": "tiered",
            "ibkr_fee_min_usd": 0.35,
            "hl_taker_fee_bps": 4.5,
            "hl_maker_fee_bps": 0.25,
            "assumed_hedge_fee_bps": 1.5,
        },
        ts_ms=123,
    )

    assert payload["param_set"] == "equities_taker"
    assert manager.defaults["bid_edge_take_bps"] == 5.0
    assert manager.defaults["ask_edge_take_bps"] == 5.0
    assert manager.defaults["take_cooldown_ms"] == 1_000
    assert "execution_mode" not in manager.defaults
    assert "bid_edge1" not in manager.defaults
    assert "des_qty_local" not in manager.defaults


def test_equities_taker_params_manager_rejects_maker_and_local_controls() -> None:
    redis_client = _FakeRedis()
    strategy = EquitiesTakerStrategy(config=_build_config())
    manager = runtime_params_mod.params_manager_factory(redis_client=redis_client)(strategy)

    with pytest.raises(ValueError, match="Unknown parameter|Unsupported runtime param"):
        manager.publish_update({"execution_mode": "take_take"}, ts_ms=123)

    with pytest.raises(ValueError, match="Unknown parameter|Unsupported runtime param"):
        manager.publish_update({"bid_edge1": 5.0}, ts_ms=123)

    with pytest.raises(ValueError, match="Unknown parameter|Unsupported runtime param"):
        manager.publish_update({"des_qty_local": 0}, ts_ms=123)
