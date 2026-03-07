from __future__ import annotations

from decimal import Decimal

from nautilus_trader.flux.strategies.makerv4 import MakerV4Strategy
from nautilus_trader.flux.strategies.makerv4 import MakerV4StrategyConfig
from nautilus_trader.flux.strategies.makerv4 import fees as fees_mod
from nautilus_trader.flux.strategies.makerv4 import runtime_params as runtime_params_mod
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


def _build_config() -> MakerV4StrategyConfig:
    return MakerV4StrategyConfig(
        maker_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
        order_qty=Decimal("1"),
        strategy_id="aapl_tradexyz_makerv4",
        external_strategy_id="aapl_tradexyz_makerv4",
    )


def test_makerv4_defaults_enable_instant_hedge_and_fee_controls() -> None:
    defaults = runtime_params_mod.MAKERV4_RUNTIME_PARAM_DEFAULTS
    schema = runtime_params_mod.MAKERV4_RUNTIME_PARAM_SCHEMA

    assert defaults["qty"] == 1.0
    assert defaults["max_qty_global"] == 100.0
    assert defaults["max_skew_bps_global"] == 10.0
    assert defaults["n_orders1"] == 3
    assert defaults["n_orders2"] == 0
    assert defaults["n_orders3"] == 0
    assert defaults["instant_hedge_enabled"] is True
    assert defaults["hedge_style"] == "ioc_through_mid"
    assert defaults["hedge_ioc_cross_mid_bps"] == 2.0
    assert defaults["hedge_ioc_max_cross_bps"] == 10.0
    assert defaults["maker_fee_source"] == "hyperliquid_api"
    assert defaults["hedge_fee_source"] == "config"
    assert defaults["assumed_hedge_fee_bps"] == 1.0
    assert schema["hedge_style"]["type"] == "select"
    assert schema["hedge_style"]["options"] == [
        ["ioc_through_mid", "IOC Through Mid"],
    ]
    assert schema["maker_fee_source"]["type"] == "select"
    assert schema["hedge_fee_source"]["type"] == "select"


def test_params_manager_factory_uses_makerv4_param_set_and_select_defaults() -> None:
    redis_client = _FakeRedis()
    strategy = MakerV4Strategy(config=_build_config())
    manager = runtime_params_mod.params_manager_factory(redis_client=redis_client)(strategy)

    payload = manager.publish_update(
        {
            "instant_hedge_enabled": True,
            "hedge_style": "ioc_through_mid",
            "maker_fee_source": "hyperliquid_api",
            "hedge_fee_source": "config",
            "assumed_hedge_fee_bps": 1.5,
        },
        ts_ms=123,
    )

    assert payload["param_set"] == "makerv4"
    assert manager.defaults["instant_hedge_enabled"] is True
    assert manager.defaults["hedge_style"] == "ioc_through_mid"
    assert manager.defaults["maker_fee_source"] == "hyperliquid_api"
    assert manager.defaults["hedge_fee_source"] == "config"
    assert manager.defaults["assumed_hedge_fee_bps"] == 1.0


def test_resolve_fee_rules_uses_live_maker_fee_and_assumed_hedge_fee() -> None:
    rules = fees_mod.resolve_fee_rules(
        runtime_params={
            "maker_fee_source": "hyperliquid_api",
            "hedge_fee_source": "config",
            "assumed_hedge_fee_bps": 1.25,
        },
        maker_fee_bps=Decimal("0.35"),
        fee_snapshot_age_s=Decimal("9"),
    )

    assert rules.maker_fee_source == "hyperliquid_api"
    assert rules.hedge_fee_source == "config"
    assert rules.maker_fee_bps == Decimal("0.35")
    assert rules.hedge_fee_bps == Decimal("1.25")
    assert rules.fee_snapshot_age_s == Decimal("9")
