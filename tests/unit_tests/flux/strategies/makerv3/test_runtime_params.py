from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

import pytest

from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_DEFAULTS
from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_REGISTRY
from nautilus_trader.flux.strategies.makerv3 import MakerV3Strategy
from nautilus_trader.flux.strategies.makerv3 import MakerV3StrategyConfig
from nautilus_trader.flux.strategies.makerv3 import runtime_params as runtime_params_mod
from nautilus_trader.model.identifiers import InstrumentId


def test_refresh_runtime_params_is_idempotent_and_noop_when_unchanged(strategy_factory) -> None:
    strategy = strategy_factory()

    class _ParamsManager:
        def __init__(self) -> None:
            self.calls = 0

        def load(self) -> dict[str, int | bool]:
            self.calls += 1
            return {"bot_on": True, "max_age_ms": 100}

    manager = _ParamsManager()
    strategy.set_params_manager(manager)

    initial_runtime_params = dict(strategy._runtime_params)
    initial_order_qty = strategy._order_qty

    strategy._refresh_runtime_params(now_ns=1_000_000_000)
    runtime_params_after_first_refresh = dict(strategy._runtime_params)
    strategy._refresh_runtime_params(now_ns=1_100_000_000)
    strategy._refresh_runtime_params(now_ns=1_700_000_000)

    assert manager.calls == 2
    assert runtime_params_after_first_refresh == initial_runtime_params
    assert strategy._runtime_params == runtime_params_after_first_refresh
    assert strategy._order_qty is initial_order_qty


def test_initial_runtime_params_use_registry_defaults_when_config_omits_values() -> None:
    config = MakerV3StrategyConfig(
        maker_instrument_id=InstrumentId.from_str("MAKER.SIM"),
        reference_instrument_id=InstrumentId.from_str("REF.SIM"),
        order_qty=Decimal(1),
    )

    runtime_params = runtime_params_mod.initial_runtime_params(config)

    assert runtime_params["qty"] == Decimal(1)
    for name in MAKERV3_RUNTIME_PARAM_REGISTRY.names:
        if name == "qty":
            continue
        assert runtime_params[name] == runtime_params_mod.coerce_runtime_param_value(
            name,
            MAKERV3_RUNTIME_PARAM_DEFAULTS[name],
        )


def test_initial_runtime_params_seed_order_reject_alert_thresholds_from_config() -> None:
    config = MakerV3StrategyConfig(
        maker_instrument_id=InstrumentId.from_str("MAKER.SIM"),
        reference_instrument_id=InstrumentId.from_str("REF.SIM"),
        order_qty=Decimal(1),
        order_reject_alert_after_count=5,
        order_reject_alert_after_s=12.0,
    )

    runtime_params = runtime_params_mod.initial_runtime_params(config)

    assert runtime_params["order_reject_alert_after_count"] == 5
    assert runtime_params["order_reject_alert_after_s"] == Decimal(12)


def test_apply_runtime_param_updates_rejects_unknown_keys(strategy_factory) -> None:
    strategy = strategy_factory()

    with pytest.raises(ValueError, match="Unsupported runtime param"):
        strategy._apply_runtime_param_updates({"not_a_param": 1})


def test_apply_runtime_param_updates_rejects_bounded_depth_overflow(strategy_factory) -> None:
    strategy = strategy_factory()

    with pytest.raises(ValueError, match="n_orders1"):
        strategy._apply_runtime_param_updates({"n_orders1": 21})


def test_set_params_manager_rejects_strategy_identity_mismatch(strategy_factory) -> None:
    strategy = strategy_factory()
    manager = SimpleNamespace(strategy_id="other_strategy")

    with pytest.raises(ValueError, match="strategy_id"):
        strategy.set_params_manager(manager)


def test_params_manager_factory_uses_stable_identity_for_updates_and_payloads(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory(
        [1, 2],
        external_strategy_id="maker_v3_identity_test",
    )
    strategy._maker_instrument = object()
    strategy._order_qty = object()

    class _FactoryManager:
        def __init__(self, strategy_id: str) -> None:
            self.strategy_id = strategy_id

        def load(self) -> dict[str, object]:
            return {"bot_on": False}

    factory_calls: list[str] = []
    built_manager: _FactoryManager | None = None

    def _factory(instance: MakerV3Strategy) -> _FactoryManager:
        nonlocal built_manager
        factory_calls.append(instance.runtime_strategy_id)
        built_manager = _FactoryManager(instance.runtime_strategy_id)
        return built_manager

    strategy.set_params_manager_factory(_factory)
    strategy._refresh_runtime_params(now_ns=1_000_000_000, force=True)

    payloads: list[dict[str, object]] = []
    strategy._publish_json = lambda _topic, payload: payloads.append(payload)
    strategy._publish_event = MakerV3Strategy._publish_event.__get__(strategy, MakerV3Strategy)
    strategy._publish_event("identity_test")

    assert factory_calls == ["maker_v3_identity_test"]
    assert built_manager is not None
    assert strategy._params_manager is built_manager
    assert strategy._runtime_params["bot_on"] is False
    assert payloads[-1]["strategy_id"] == "maker_v3_identity_test"


def test_apply_runtime_param_updates_rejects_non_positive_qty_without_mutating_state(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._runtime_params["qty"] = Decimal(1)
    sentinel_order_qty = object()
    strategy._order_qty = sentinel_order_qty
    strategy._maker_instrument = SimpleNamespace(make_qty=lambda value: f"qty:{value}")

    with pytest.raises(ValueError, match="qty"):
        strategy._apply_runtime_param_updates({"qty": Decimal(0)})

    assert strategy._runtime_params["qty"] == Decimal(1)
    assert strategy._order_qty is sentinel_order_qty


def test_apply_runtime_param_updates_qty_conversion_is_atomic_on_failure(strategy_factory) -> None:
    strategy = strategy_factory()
    strategy._runtime_params["qty"] = Decimal(1)
    sentinel_order_qty = object()
    strategy._order_qty = sentinel_order_qty

    def _raise_conversion(_value: Decimal) -> object:
        raise ValueError("conversion failed")

    strategy._maker_instrument = SimpleNamespace(make_qty=_raise_conversion)

    with pytest.raises(RuntimeError, match="Failed to convert runtime qty"):
        strategy._apply_runtime_param_updates({"qty": Decimal(2)})

    assert strategy._runtime_params["qty"] == Decimal(1)
    assert strategy._order_qty is sentinel_order_qty


def test_params_manager_factory_defaults_align_with_strategy_runtime_defaults(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory(
        [1],
        max_age_ms=321,
        n_orders2=4,
        bid_edge1=0.77,
    )

    factory = MakerV3Strategy.params_manager_factory(redis_client=object())
    manager = factory(strategy)

    assert manager.defaults["max_age_ms"] == strategy.config.max_age_ms
    assert manager.defaults["n_orders2"] == strategy.config.n_orders2
    assert manager.defaults["bid_edge1"] == pytest.approx(strategy.config.bid_edge1)
