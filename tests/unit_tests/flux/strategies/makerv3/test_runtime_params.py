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
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def _okx_linear_perpetual() -> CryptoPerpetual:
    return CryptoPerpetual(
        instrument_id=InstrumentId(
            symbol=Symbol("PLUME-USDT-SWAP"),
            venue=Venue("OKX"),
        ),
        raw_symbol=Symbol("PLUME-USDT-SWAP"),
        base_currency=Currency.from_str("PLUME"),
        quote_currency=Currency.from_str("USDT"),
        settlement_currency=Currency.from_str("USDT"),
        is_inverse=False,
        price_precision=6,
        size_precision=0,
        price_increment=Price.from_str("0.000001"),
        size_increment=Quantity.from_str("1"),
        multiplier=Quantity.from_str("10"),
        lot_size=Quantity.from_str("1"),
        ts_event=0,
        ts_init=0,
        info={
            "okx_ct_val": "10",
            "okx_ct_val_ccy": "PLUME",
            "okx_ct_type": "linear",
            "okx_lot_sz": "1",
            "base_exposure_mode": "exact_multiplier",
        },
    )


def _hyperliquid_identity_perpetual() -> CryptoPerpetual:
    return CryptoPerpetual(
        instrument_id=InstrumentId(
            symbol=Symbol("xyz:AAPL-USD-PERP"),
            venue=Venue("HYPERLIQUID"),
        ),
        raw_symbol=Symbol("xyz:AAPL"),
        base_currency=Currency.from_str("xyz:AAPL"),
        quote_currency=Currency.from_str("USD"),
        settlement_currency=Currency.from_str("USDC"),
        is_inverse=False,
        price_precision=3,
        size_precision=3,
        price_increment=Price.from_str("0.001"),
        size_increment=Quantity.from_str("0.001"),
        multiplier=Quantity.from_str("1"),
        lot_size=Quantity.from_str("1"),
        ts_event=0,
        ts_init=0,
        info={"base_exposure_mode": "identity"},
    )


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


def test_initial_runtime_params_accept_signed_linear_offset_bps() -> None:
    config = MakerV3StrategyConfig(
        maker_instrument_id=InstrumentId.from_str("MAKER.SIM"),
        reference_instrument_id=InstrumentId.from_str("REF.SIM"),
        order_qty=Decimal(1),
        linear_offset_bps=-3.5,
    )

    runtime_params = runtime_params_mod.initial_runtime_params(config)

    assert runtime_params["linear_offset_bps"] == Decimal("-3.5")


def test_runtime_param_coercion_accepts_negative_linear_offset_bps() -> None:
    assert runtime_params_mod.coerce_runtime_param_value("linear_offset_bps", -7.25) == Decimal(
        "-7.25",
    )


def test_runtime_param_schema_describes_linear_offset_as_quoted_fv_adjustment() -> None:
    description = MAKERV3_RUNTIME_PARAM_REGISTRY.schema["linear_offset_bps"]["description"]

    assert "quoted fv" in description.lower()
    assert "reference market" in description.lower()


def test_runtime_param_schema_describes_skew_caps_as_inventory_driven_quoted_fv_components() -> None:
    global_description = MAKERV3_RUNTIME_PARAM_REGISTRY.schema["max_skew_bps_global"]["description"]
    local_description = MAKERV3_RUNTIME_PARAM_REGISTRY.schema["max_skew_bps_local"]["description"]

    assert "inventory-driven" in global_description.lower()
    assert "quoted fv" in global_description.lower()
    assert "inventory-driven" in local_description.lower()
    assert "quoted fv" in local_description.lower()


def test_initial_runtime_params_seed_pending_cancel_budgets_from_config() -> None:
    config = MakerV3StrategyConfig(
        maker_instrument_id=InstrumentId.from_str("MAKER.SIM"),
        reference_instrument_id=InstrumentId.from_str("REF.SIM"),
        order_qty=Decimal(1),
        pending_cancel_grace_ms=250,
        pending_cancel_block_after_ms=1_500,
        quote_liveness_stall_after_ms=3_000,
        quote_liveness_recover_after_ms=900,
    )

    runtime_params = runtime_params_mod.initial_runtime_params(config)

    assert runtime_params["pending_cancel_grace_ms"] == 250
    assert runtime_params["pending_cancel_block_after_ms"] == 1_500
    assert runtime_params["quote_liveness_stall_after_ms"] == 3_000
    assert runtime_params["quote_liveness_recover_after_ms"] == 900


def test_initial_runtime_params_seed_bounded_convergence_budgets_from_config() -> None:
    config = MakerV3StrategyConfig(
        maker_instrument_id=InstrumentId.from_str("MAKER.SIM"),
        reference_instrument_id=InstrumentId.from_str("REF.SIM"),
        order_qty=Decimal(1),
        max_cancels_per_side_per_cycle=2,
        max_places_per_side_per_cycle=3,
        max_total_actions_per_cycle=4,
        max_pending_cancels_per_side=1,
    )

    runtime_params = runtime_params_mod.initial_runtime_params(config)

    assert runtime_params["max_cancels_per_side_per_cycle"] == 2
    assert runtime_params["max_places_per_side_per_cycle"] == 3
    assert runtime_params["max_total_actions_per_cycle"] == 4
    assert runtime_params["max_pending_cancels_per_side"] == 1


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


def test_apply_runtime_param_updates_qty_unit_venue_preserves_direct_make_qty(
    strategy_factory,
) -> None:
    strategy = strategy_factory(qty_unit="venue")
    seen_values: list[Decimal] = []
    strategy._maker_instrument = SimpleNamespace(
        make_qty=lambda value: seen_values.append(Decimal(str(value))) or f"qty:{value}",
    )

    strategy._apply_runtime_param_updates({"qty": Decimal(7)})

    assert strategy._runtime_params["qty"] == Decimal(7)
    assert seen_values == [Decimal(7)]
    assert strategy._order_qty == "qty:7.0"


def test_apply_runtime_param_updates_qty_unit_base_converts_before_make_qty(
    strategy_factory,
) -> None:
    strategy = strategy_factory(qty_unit="base")
    strategy._maker_instrument = _okx_linear_perpetual()

    strategy._apply_runtime_param_updates({"qty": Decimal(3430)})

    assert strategy._runtime_params["qty"] == Decimal(3430)
    assert strategy._order_qty.as_decimal() == Decimal(343)


def test_apply_runtime_param_updates_qty_unit_base_supports_hyperliquid_identity_perps(
    strategy_factory,
) -> None:
    strategy = strategy_factory(qty_unit="base")
    strategy._maker_instrument = _hyperliquid_identity_perpetual()

    strategy._apply_runtime_param_updates({"qty": Decimal(1)})

    assert strategy._runtime_params["qty"] == Decimal(1)
    assert strategy._order_qty.as_decimal() == Decimal(1)


def test_apply_runtime_param_updates_qty_unit_base_rejects_non_integral_venue_qty(
    strategy_factory,
) -> None:
    strategy = strategy_factory(qty_unit="base")
    strategy._runtime_params["qty"] = Decimal(1000)
    sentinel_order_qty = object()
    strategy._order_qty = sentinel_order_qty
    strategy._maker_instrument = _okx_linear_perpetual()

    with pytest.raises(RuntimeError, match="non_integral_venue_qty"):
        strategy._apply_runtime_param_updates({"qty": Decimal(3435)})

    assert strategy._runtime_params["qty"] == Decimal(1000)
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
        max_cancels_per_side_per_cycle=2,
    )

    factory = MakerV3Strategy.params_manager_factory(redis_client=object())
    manager = factory(strategy)

    assert manager.defaults["max_age_ms"] == strategy.config.max_age_ms
    assert manager.defaults["n_orders2"] == strategy.config.n_orders2
    assert manager.defaults["bid_edge1"] == pytest.approx(strategy.config.bid_edge1)
    assert (
        manager.defaults["max_cancels_per_side_per_cycle"]
        == strategy.config.max_cancels_per_side_per_cycle
    )


def test_prepare_runtime_params_for_startup_keeps_persisted_bot_on_unchanged(
    strategy_factory,
) -> None:
    class _ParamsManager:
        def __init__(self) -> None:
            self.stored = {"bot_on": True, "max_age_ms": 100}
            self.control_revision = "rev-start"
            self.update_calls: list[dict[str, bool]] = []
            self.publish_calls: list[tuple[dict[str, bool], int | None]] = []

        def load(self) -> dict[str, int | bool]:
            return dict(self.stored)

        def update(self, updates: dict[str, bool]) -> dict[str, bool]:
            coerced = dict(updates)
            self.update_calls.append(coerced)
            self.stored.update(coerced)
            return coerced

        def publish_update(
            self,
            updates: dict[str, bool],
            *,
            ts_ms: int | None = None,
        ) -> dict[str, object]:
            coerced = dict(updates)
            self.publish_calls.append((coerced, ts_ms))
            return {"updates": coerced, "ts_ms": ts_ms}

        def load_bot_on_control_revision(self) -> str:
            return self.control_revision

    strategy = strategy_factory(force_bot_off_on_start=True)
    manager = _ParamsManager()
    strategy.set_params_manager(manager)

    runtime_params_mod.prepare_runtime_params_for_startup(strategy)

    assert manager.update_calls == []
    assert manager.publish_calls == []
    assert strategy._runtime_params["bot_on"] is True
    assert strategy._effective_bot_on() is False


def test_refresh_runtime_params_clears_startup_bot_off_latch_after_explicit_bot_on_control_update(
    strategy_factory,
) -> None:
    class _ParamsManager:
        def __init__(self) -> None:
            self.stored = {"bot_on": True, "max_age_ms": 100}
            self.control_revision = "rev-start"

        def load(self) -> dict[str, int | bool]:
            return dict(self.stored)

        def load_bot_on_control_revision(self) -> str:
            return self.control_revision

    strategy = strategy_factory(force_bot_off_on_start=True)
    manager = _ParamsManager()
    strategy.set_params_manager(manager)

    runtime_params_mod.prepare_runtime_params_for_startup(strategy)
    runtime_params_mod.refresh_runtime_params(strategy, now_ns=1_000_000_000, force=True)

    assert strategy._runtime_params["bot_on"] is True
    assert strategy._effective_bot_on() is False
    assert strategy._startup_bot_off_active is True

    manager.control_revision = "rev-operator-1"
    runtime_params_mod.refresh_runtime_params(strategy, now_ns=2_000_000_000, force=True)

    assert strategy._startup_bot_off_active is False
    assert strategy._effective_bot_on() is True
