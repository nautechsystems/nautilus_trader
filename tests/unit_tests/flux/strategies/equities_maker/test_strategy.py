from __future__ import annotations

import inspect
from decimal import Decimal
from types import SimpleNamespace

import pytest

from flux.runners.shared.quote_feed_supervisor import NodeQuoteFeedSupervisor
from flux.runners.shared.quote_feed_supervisor import QuoteFeedControlEmitter
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


def _instrument(*, raw_symbol: str, multiplier: str = "1") -> SimpleNamespace:
    return SimpleNamespace(
        raw_symbol=raw_symbol,
        price_precision=2,
        price_increment=Decimal("0.01"),
        base_currency=SimpleNamespace(code="AAPL"),
        quote_currency=SimpleNamespace(code="USD"),
        settlement_currency=SimpleNamespace(code="USD"),
        multiplier=Decimal(multiplier),
        is_inverse=False,
        make_qty=lambda value: Decimal(str(value)),
        make_price=lambda value: Decimal(str(value)),
        calculate_base_exposure_qty=lambda qty, _price=None: Decimal(str(qty)),
    )


def _configure_strategy_for_quotes(strategy: EquitiesMakerStrategy) -> tuple[InstrumentId, InstrumentId]:
    maker_id = strategy.config.maker_instrument_id
    ref_id = strategy.config.reference_instrument_id
    strategy._runtime_params.update({"bot_on": False})
    strategy._instruments = {
        maker_id: _instrument(raw_symbol="AAPL/USD"),
        ref_id: _instrument(raw_symbol="AAPL"),
    }
    strategy._cache = SimpleNamespace(
        instrument=lambda instrument_id: strategy._instruments.get(instrument_id),
        positions_open=lambda *args, **kwargs: [],
        accounts=lambda: [],
    )
    strategy._publish_json = lambda *_args, **_kwargs: None
    return maker_id, ref_id


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


def test_equities_maker_seeds_runtime_params_from_config() -> None:
    strategy = EquitiesMakerStrategy(
        config=_config(
            bot_on=True,
            qty=Decimal("3"),
            des_qty_global=4.0,
            max_qty_global=5.0,
            max_skew_bps_global=6.0,
            linear_offset_bps=1.5,
            max_age_ms=2_500,
            bid_edge1=7.0,
            ask_edge1=8.0,
            place_edge1=0.5,
            n_orders1=2,
        )
    )

    assert strategy._runtime_params["bot_on"] is True
    assert Decimal(str(strategy._runtime_params["qty"])) == Decimal("3")
    assert strategy._runtime_params["des_qty_global"] == 4.0
    assert strategy._runtime_params["max_qty_global"] == 5.0
    assert strategy._runtime_params["max_skew_bps_global"] == 6.0
    assert strategy._runtime_params["linear_offset_bps"] == 1.5
    assert strategy._runtime_params["max_age_ms"] == 2_500
    assert strategy._runtime_params["bid_edge1"] == 7.0
    assert strategy._runtime_params["ask_edge1"] == 8.0
    assert strategy._runtime_params["place_edge1"] == 0.5
    assert strategy._runtime_params["n_orders1"] == 2


def test_equities_maker_timer_resubscribes_stalled_quotes(
    monkeypatch,
) -> None:
    strategy = EquitiesMakerStrategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quotes(strategy)

    class _FakeClock:
        def __init__(self) -> None:
            self.now = 10_000_000_000

        def timestamp_ns(self) -> int:
            return self.now

    fake_clock = _FakeClock()
    subscribed: list[InstrumentId] = []
    unsubscribed: list[InstrumentId] = []

    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    monkeypatch.setattr(
        strategy,
        "subscribe_quote_ticks",
        lambda *, instrument_id, client_id=None: subscribed.append(instrument_id),
    )
    monkeypatch.setattr(
        strategy,
        "unsubscribe_quote_ticks",
        lambda *, instrument_id, client_id=None: unsubscribed.append(instrument_id),
    )
    strategy._publish_balances_if_due = lambda: None
    strategy._runtime_params["quote_liveness_stall_after_ms"] = 3_000
    strategy._runtime_params["quote_liveness_recover_after_ms"] = 900
    strategy._latest_quotes = {
        maker_id: {"bid": Decimal("190.00"), "ask": Decimal("190.02"), "ts_ns": 1_000_000_000},
        ref_id: {"bid": Decimal("189.90"), "ask": Decimal("189.92"), "ts_ns": 1_000_000_000},
    }

    strategy.on_time_event(SimpleNamespace(name=strategy._liveness_timer_name))

    assert unsubscribed == [maker_id, ref_id]
    assert subscribed == [maker_id, ref_id]


def test_equities_maker_shared_recovery_attachment_preserves_timer_resubscribe_behavior(
    monkeypatch,
) -> None:
    strategy = EquitiesMakerStrategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quotes(strategy)

    class _FakeClock:
        def __init__(self) -> None:
            self.now = 10_000_000_000

        def timestamp_ns(self) -> int:
            return self.now

    fake_clock = _FakeClock()
    subscribed: list[InstrumentId] = []
    unsubscribed: list[InstrumentId] = []
    supervisor = NodeQuoteFeedSupervisor()
    control_emitter = QuoteFeedControlEmitter(node_scoped_id="aapl_tradexyz")

    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    monkeypatch.setattr(
        strategy,
        "subscribe_quote_ticks",
        lambda *, instrument_id, client_id=None: subscribed.append(instrument_id),
    )
    monkeypatch.setattr(
        strategy,
        "unsubscribe_quote_ticks",
        lambda *, instrument_id, client_id=None: unsubscribed.append(instrument_id),
    )
    strategy._publish_balances_if_due = lambda: None
    strategy._runtime_params["quote_liveness_stall_after_ms"] = 3_000
    strategy._runtime_params["quote_liveness_recover_after_ms"] = 900
    strategy._latest_quotes = {
        maker_id: {"bid": Decimal("190.00"), "ask": Decimal("190.02"), "ts_ns": 1_000_000_000},
        ref_id: {"bid": Decimal("189.90"), "ask": Decimal("189.92"), "ts_ns": 1_000_000_000},
    }

    strategy.configure_quote_feed_runtime(
        supervisor=supervisor,
        control_emitter=control_emitter,
    )
    strategy.on_time_event(SimpleNamespace(name=strategy._liveness_timer_name))

    assert strategy._quote_feed_supervisor is supervisor
    assert strategy._quote_feed_control_emitter is control_emitter
    assert unsubscribed == [maker_id, ref_id]
    assert subscribed == [maker_id, ref_id]
