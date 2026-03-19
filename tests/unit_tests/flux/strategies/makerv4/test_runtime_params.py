from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

import pytest

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
    config = _build_config()

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
    assert defaults["maker_fee_source"] == "config"
    assert defaults["hedge_fee_source"] == "config"
    assert getattr(config, "hedge_fee_plan", None) == "ibkr_pro_tiered"
    assert defaults["hedge_fee_plan"] == "ibkr_pro_tiered"
    assert defaults["ibkr_fee_plan"] == "tiered"
    assert defaults["ibkr_fee_min_usd"] == 0.35
    assert defaults["maker_taker_fee_bps"] == 4.5
    assert defaults["maker_maker_fee_bps"] == 0.25
    assert defaults["assumed_hedge_fee_bps"] == 1.0
    assert schema["hedge_style"]["type"] == "select"
    assert schema["hedge_style"]["options"] == [
        ["ioc_through_mid", "IOC Through Mid"],
    ]
    assert schema["maker_fee_source"]["type"] == "select"
    assert schema["maker_fee_source"]["options"] == [
        ["config", "Configured Assumption"],
        ["hyperliquid_api", "Legacy Hyperliquid API"],
    ]
    assert schema["hedge_fee_source"]["type"] == "select"
    assert schema["hedge_fee_source"]["options"] == [
        ["config", "Configured Assumption"],
    ]
    assert schema["hedge_fee_plan"]["type"] == "select"
    assert schema["hedge_fee_plan"]["options"] == [
        ["ibkr_pro_tiered", "IBKR Pro Tiered"],
    ]
    assert schema["ibkr_fee_plan"]["type"] == "select"
    assert schema["ibkr_fee_plan"]["options"] == [
        ["fixed", "Fixed"],
        ["tiered", "Tiered"],
    ]
    assert schema["ibkr_fee_min_usd"]["type"] == "number"
    assert schema["maker_taker_fee_bps"]["type"] == "number"
    assert schema["maker_maker_fee_bps"]["type"] == "number"


def test_makerv4_execution_mode_defaults_include_take_take_controls() -> None:
    defaults = runtime_params_mod.MAKERV4_RUNTIME_PARAM_DEFAULTS
    schema = runtime_params_mod.MAKERV4_RUNTIME_PARAM_SCHEMA

    assert defaults["execution_mode"] == "maker_hedge"
    assert defaults["bid_edge_take_bps"] == 5.0
    assert defaults["ask_edge_take_bps"] == 5.0
    assert defaults["take_cooldown_ms"] == 1_000
    assert schema["execution_mode"]["type"] == "select"
    assert schema["execution_mode"]["options"] == [
        ["maker_hedge", "Maker Hedge"],
        ["take_take", "Take-Take"],
    ]
    assert schema["bid_edge_take_bps"]["type"] == "number"
    assert schema["ask_edge_take_bps"]["type"] == "number"
    assert schema["take_cooldown_ms"]["type"] == "number"


def test_resolve_fee_rules_exposes_explicit_hedge_fee_plan_without_account_id() -> None:
    rules = fees_mod.resolve_fee_rules(
        runtime_params={
            "maker_fee_source": "config",
            "hedge_fee_source": "config",
            "hedge_fee_plan": "ibkr_pro_tiered",
            "assumed_hedge_fee_bps": 1.25,
        },
        maker_fee_bps=Decimal("0.35"),
    )

    assert getattr(rules, "hedge_fee_plan", None) == "ibkr_pro_tiered"


def test_params_manager_factory_uses_makerv4_param_set_and_select_defaults() -> None:
    redis_client = _FakeRedis()
    strategy = MakerV4Strategy(config=_build_config())
    manager = runtime_params_mod.params_manager_factory(redis_client=redis_client)(strategy)

    payload = manager.publish_update(
        {
            "instant_hedge_enabled": True,
            "execution_mode": "take_take",
            "bid_edge_take_bps": 7.5,
            "ask_edge_take_bps": 8.5,
            "take_cooldown_ms": 2_500,
            "hedge_style": "ioc_through_mid",
            "maker_fee_source": "config",
            "hedge_fee_source": "config",
            "hedge_fee_plan": "ibkr_pro_tiered",
            "ibkr_fee_plan": "tiered",
            "ibkr_fee_min_usd": 0.35,
            "maker_taker_fee_bps": 4.5,
            "maker_maker_fee_bps": 0.25,
            "assumed_hedge_fee_bps": 1.5,
        },
        ts_ms=123,
    )

    assert payload["param_set"] == "makerv4"
    assert manager.defaults["instant_hedge_enabled"] is True
    assert manager.defaults["execution_mode"] == "maker_hedge"
    assert manager.defaults["bid_edge_take_bps"] == 5.0
    assert manager.defaults["ask_edge_take_bps"] == 5.0
    assert manager.defaults["take_cooldown_ms"] == 1_000
    assert manager.defaults["hedge_style"] == "ioc_through_mid"
    assert manager.defaults["maker_fee_source"] == "config"
    assert manager.defaults["hedge_fee_source"] == "config"
    assert manager.defaults["hedge_fee_plan"] == "ibkr_pro_tiered"
    assert manager.defaults["ibkr_fee_plan"] == "tiered"
    assert manager.defaults["ibkr_fee_min_usd"] == 0.35
    assert manager.defaults["maker_taker_fee_bps"] == 4.5
    assert manager.defaults["maker_maker_fee_bps"] == 0.25
    assert manager.defaults["assumed_hedge_fee_bps"] == 1.0


def test_params_manager_rejects_unsupported_makerv4_select_updates() -> None:
    redis_client = _FakeRedis()
    strategy = MakerV4Strategy(config=_build_config())
    manager = runtime_params_mod.params_manager_factory(redis_client=redis_client)(strategy)

    with pytest.raises(ValueError, match="Invalid option value"):
        manager.publish_update({"hedge_style": "resting_limit"}, ts_ms=123)

    with pytest.raises(ValueError, match="Invalid option value"):
        manager.publish_update({"maker_fee_source": "manual"}, ts_ms=123)

    with pytest.raises(ValueError, match="Invalid option value"):
        manager.publish_update({"hedge_fee_source": "live_oracle"}, ts_ms=123)

    with pytest.raises(ValueError, match="Invalid option value"):
        manager.publish_update({"hedge_fee_plan": "ibkr_lite"}, ts_ms=123)

    with pytest.raises(ValueError, match="Invalid option value"):
        manager.publish_update({"ibkr_fee_plan": "smart"}, ts_ms=123)


def test_params_manager_factory_loads_legacy_hyperliquid_maker_fee_source() -> None:
    redis_client = _FakeRedis()
    redis_client.hashes["flux:v1:params:aapl_tradexyz_makerv4"] = {
        "maker_fee_source": b"hyperliquid_api",
    }
    strategy = MakerV4Strategy(config=_build_config())
    manager = runtime_params_mod.params_manager_factory(redis_client=redis_client)(strategy)

    loaded = manager.load()

    assert loaded["maker_fee_source"] == "hyperliquid_api"


def test_params_manager_rejects_unsupported_execution_mode_updates() -> None:
    redis_client = _FakeRedis()
    strategy = MakerV4Strategy(config=_build_config())
    manager = runtime_params_mod.params_manager_factory(redis_client=redis_client)(strategy)

    with pytest.raises(ValueError, match="Invalid option value"):
        manager.publish_update({"execution_mode": "maker_take"}, ts_ms=123)


def test_resolve_fee_rules_uses_live_maker_fee_and_assumed_hedge_fee() -> None:
    rules = fees_mod.resolve_fee_rules(
        runtime_params={
            "maker_fee_source": "config",
            "hedge_fee_source": "config",
            "assumed_hedge_fee_bps": 1.25,
        },
        maker_fee_bps=Decimal("0.35"),
        fee_snapshot_age_s=Decimal("9"),
    )

    assert rules.maker_fee_source == "config"
    assert rules.hedge_fee_source == "config"
    assert rules.maker_fee_bps == Decimal("0.35")
    assert rules.hedge_fee_bps == Decimal("1.25")
    assert rules.fee_snapshot_age_s == Decimal("9")


def test_resolve_fee_rules_rejects_unsupported_fee_sources() -> None:
    with pytest.raises(ValueError, match="Unsupported maker fee source"):
        fees_mod.resolve_fee_rules(
            runtime_params={
                "maker_fee_source": "manual",
                "hedge_fee_source": "config",
                "assumed_hedge_fee_bps": 1.25,
            },
            maker_fee_bps=Decimal("0.35"),
        )

    with pytest.raises(ValueError, match="Unsupported hedge fee source"):
        fees_mod.resolve_fee_rules(
            runtime_params={
                "maker_fee_source": "config",
                "hedge_fee_source": "live_oracle",
                "assumed_hedge_fee_bps": 1.25,
            },
            maker_fee_bps=Decimal("0.35"),
        )


def test_instant_hedge_disabled_fails_closed_without_creating_hedge_request() -> None:
    strategy = MakerV4Strategy(config=_build_config())
    strategy._runtime_params["instant_hedge_enabled"] = False
    strategy._latest_quotes = {
        strategy.config.reference_instrument_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": 1_000_000_000,
        },
    }
    strategy._publish_json = lambda *_args, **_kwargs: None

    strategy.on_order_filled(
        SimpleNamespace(
            instrument_id=strategy.config.maker_instrument_id,
            trade_id="fill-1",
            order_side="BUY",
            last_qty=SimpleNamespace(as_decimal=lambda: Decimal("1")),
            last_px=SimpleNamespace(as_decimal=lambda: Decimal("190.00")),
            ts_event=1_050_000_000,
        )
    )

    assert strategy.hedge_request_count == 0
    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "instant_hedge_disabled"


def test_unsupported_hedge_style_fails_closed() -> None:
    strategy = MakerV4Strategy(config=_build_config())
    strategy._runtime_params["hedge_style"] = "resting_limit"
    strategy._latest_quotes = {
        strategy.config.reference_instrument_id: {
            "bid": Decimal("190.00"),
            "ask": Decimal("190.04"),
            "ts_ns": 1_000_000_000,
        },
    }
    strategy._publish_json = lambda *_args, **_kwargs: None

    strategy.on_order_filled(
        SimpleNamespace(
            instrument_id=strategy.config.maker_instrument_id,
            trade_id="fill-2",
            order_side="BUY",
            last_qty=SimpleNamespace(as_decimal=lambda: Decimal("1")),
            last_px=SimpleNamespace(as_decimal=lambda: Decimal("190.00")),
            ts_event=1_050_000_000,
        )
    )

    assert strategy.hedge_request_count == 0
    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "unsupported_hedge_style"
