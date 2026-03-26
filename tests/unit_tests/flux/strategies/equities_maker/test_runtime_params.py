from __future__ import annotations

from decimal import Decimal

import pytest

from nautilus_trader.flux.strategies.equities_maker import EquitiesMakerStrategy
from nautilus_trader.flux.strategies.equities_maker import EquitiesMakerStrategyConfig
from nautilus_trader.flux.strategies.equities_maker import runtime_params as runtime_params_mod
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


def _build_config() -> EquitiesMakerStrategyConfig:
    return EquitiesMakerStrategyConfig(
        maker_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
        order_qty=Decimal("1"),
        strategy_id="aapl_tradexyz_maker",
        external_strategy_id="aapl_tradexyz_maker",
    )


def test_equities_maker_defaults_expose_maker_only_runtime_surface() -> None:
    defaults = runtime_params_mod.EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS
    schema = runtime_params_mod.EQUITIES_MAKER_RUNTIME_PARAM_SCHEMA

    assert runtime_params_mod.PARAM_SET == "equities_maker"
    assert runtime_params_mod.profile_key() == "equities_maker"
    assert defaults["qty"] == 1.0
    assert defaults["max_qty_global"] == 100.0
    assert defaults["max_skew_bps_global"] == 10.0
    assert defaults["instant_hedge_enabled"] is True
    assert defaults["hedge_style"] == "ioc_through_mid"
    assert defaults["hedge_ioc_cross_mid_bps"] == 2.0
    assert defaults["hedge_ioc_max_cross_bps"] == 10.0
    assert defaults["maker_fee_source"] == "hyperliquid_api"
    assert defaults["hedge_fee_source"] == "config"
    assert defaults["hedge_fee_plan"] == "ibkr_pro_tiered"
    assert defaults["ibkr_fee_plan"] == "tiered"
    assert defaults["ibkr_fee_min_usd"] == 0.35
    assert defaults["hl_taker_fee_bps"] == 4.5
    assert defaults["hl_maker_fee_bps"] == 0.25
    assert defaults["assumed_hedge_fee_bps"] == 1.0

    for removed_name in (
        "execution_mode",
        "bid_edge_take_bps",
        "ask_edge_take_bps",
        "take_cooldown_ms",
        "des_qty_local",
        "max_qty_local",
        "max_skew_bps_local",
    ):
        assert removed_name not in defaults
        assert removed_name not in schema


def test_equities_maker_params_manager_uses_family_param_set() -> None:
    redis_client = _FakeRedis()
    strategy = EquitiesMakerStrategy(config=_build_config())
    manager = runtime_params_mod.params_manager_factory(redis_client=redis_client)(strategy)

    payload = manager.publish_update(
        {
            "instant_hedge_enabled": True,
            "hedge_style": "ioc_through_mid",
            "maker_fee_source": "hyperliquid_api",
            "hedge_fee_source": "config",
            "hedge_fee_plan": "ibkr_pro_tiered",
            "ibkr_fee_plan": "tiered",
            "ibkr_fee_min_usd": 0.35,
            "hl_taker_fee_bps": 4.5,
            "hl_maker_fee_bps": 0.25,
            "assumed_hedge_fee_bps": 1.5,
        },
        ts_ms=123,
    )

    assert payload["param_set"] == "equities_maker"
    assert manager.defaults["instant_hedge_enabled"] is True
    assert manager.defaults["hedge_style"] == "ioc_through_mid"
    assert "execution_mode" not in manager.defaults
    assert "des_qty_local" not in manager.defaults


def test_equities_maker_params_manager_rejects_take_and_local_controls() -> None:
    redis_client = _FakeRedis()
    strategy = EquitiesMakerStrategy(config=_build_config())
    manager = runtime_params_mod.params_manager_factory(redis_client=redis_client)(strategy)

    with pytest.raises(ValueError, match="Unknown parameter|Unsupported runtime param"):
        manager.publish_update({"execution_mode": "take_take"}, ts_ms=123)

    with pytest.raises(ValueError, match="Unknown parameter|Unsupported runtime param"):
        manager.publish_update({"bid_edge_take_bps": 7.5}, ts_ms=123)

    with pytest.raises(ValueError, match="Unknown parameter|Unsupported runtime param"):
        manager.publish_update({"des_qty_local": 0}, ts_ms=123)
