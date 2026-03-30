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
from nautilus_trader.flux.strategies.makerv4.managed_orders import ManagedMakerOrderState
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


def _install_lifecycle_clock(strategy: EquitiesMakerStrategy, monkeypatch, *, now_ns: int = 10_000_000_000):
    class _FakeClock:
        def __init__(self) -> None:
            self.now = now_ns
            self._timers: dict[str, tuple[object, object]] = {}

        def timestamp_ns(self) -> int:
            return self.now

        def set_timer(self, *, name, interval, callback) -> None:
            self._timers[name] = (interval, callback)

        def cancel_timer(self, name) -> None:
            self._timers.pop(name, None)

        @property
        def timer_names(self) -> tuple[str, ...]:
            return tuple(self._timers)

    fake_clock = _FakeClock()
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))
    return fake_clock


def _prepare_strategy_lifecycle(strategy: EquitiesMakerStrategy, monkeypatch) -> tuple[InstrumentId, InstrumentId]:
    maker_id, ref_id = _configure_strategy_for_quotes(strategy)
    _install_lifecycle_clock(strategy, monkeypatch)
    strategy._load_runtime_params = lambda: None
    strategy._reclaim_managed_maker_orders_from_cache = lambda: None
    strategy._publish_balances = lambda: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    return maker_id, ref_id


def _wire_shared_quote_runtime(
    strategy: EquitiesMakerStrategy,
    *,
    supervisor: NodeQuoteFeedSupervisor,
    control_emitter: QuoteFeedControlEmitter,
) -> None:
    strategy.configure_quote_feed_runtime(
        supervisor=supervisor,
        control_emitter=control_emitter,
    )
    for claim_spec in strategy.quote_feed_claim_specs():
        subscribe = lambda feed_identity=claim_spec.feed_identity: control_emitter.subscribe(
            feed_identity,
        )
        reset = None
        unsubscribe = lambda feed_identity=claim_spec.feed_identity: control_emitter.unsubscribe(
            feed_identity,
        )
        if claim_spec.node_scoped_lifecycle:
            reset = lambda feed_identity=claim_spec.feed_identity: control_emitter.reset(
                feed_identity,
            )
        supervisor.ensure_feed(
            claim_spec.feed_identity,
            reset=reset,
            subscribe=subscribe,
            unsubscribe=unsubscribe,
            blocker_key=claim_spec.blocker_key,
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


def test_equities_maker_shared_recovery_attachment_moves_timer_resubscribe_to_supervisor(
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

    _wire_shared_quote_runtime(
        strategy,
        supervisor=supervisor,
        control_emitter=control_emitter,
    )
    strategy._register_quote_feed_interest()
    control_emitter.commands.clear()
    for claim_spec in strategy.quote_feed_claim_specs():
        supervisor.record_quote(
            claim_spec.feed_identity,
            ts_ns=1_000_000_000,
        )
    strategy.on_time_event(SimpleNamespace(name=strategy._liveness_timer_name))

    assert strategy._quote_feed_supervisor is supervisor
    assert strategy._quote_feed_control_emitter is control_emitter
    assert unsubscribed == []
    assert subscribed == []
    assert [(command.action, command.feed_identity.topic) for command in control_emitter.commands] == [
        ("reset", "maker_quote_ticks"),
    ]


def test_equities_maker_supervisor_runtime_dedupes_startup_interest_and_preserves_direct_quote_delivery(
    monkeypatch,
) -> None:
    maker_strategy = EquitiesMakerStrategy(config=_config(strategy_id="aapl_tradexyz_maker"))
    taker_strategy = EquitiesMakerStrategy(
        config=_config(
            strategy_id="aapl_tradexyz_taker",
            external_strategy_id="aapl_tradexyz_taker",
        )
    )
    strategies = (maker_strategy, taker_strategy)
    supervisor = NodeQuoteFeedSupervisor()
    control_emitter = QuoteFeedControlEmitter(node_scoped_id="aapl_tradexyz")
    direct_subscribes: list[tuple[str, InstrumentId]] = []

    for strategy in strategies:
        _prepare_strategy_lifecycle(strategy, monkeypatch)
        _wire_shared_quote_runtime(
            strategy,
            supervisor=supervisor,
            control_emitter=control_emitter,
        )
        monkeypatch.setattr(
            strategy,
            "subscribe_quote_ticks",
            lambda *, instrument_id, client_id=None, strategy=strategy: direct_subscribes.append(
                (strategy.config.external_strategy_id, instrument_id)
            ),
        )

    maker_strategy.on_start()
    taker_strategy.on_start()

    assert sorted(direct_subscribes) == sorted(
        [
            ("aapl_tradexyz_maker", maker_strategy.config.maker_instrument_id),
            ("aapl_tradexyz_maker", maker_strategy.config.reference_instrument_id),
            ("aapl_tradexyz_taker", taker_strategy.config.maker_instrument_id),
            ("aapl_tradexyz_taker", taker_strategy.config.reference_instrument_id),
        ]
    )
    assert [(command.action, command.feed_identity.topic) for command in control_emitter.commands] == [
        ("subscribe", "maker_quote_ticks"),
        ("subscribe", "reference_quote_ticks"),
    ]


def test_equities_maker_supervisor_timer_reports_stale_feed_without_direct_resubscribe(
    monkeypatch,
) -> None:
    maker_strategy = EquitiesMakerStrategy(config=_config(strategy_id="aapl_tradexyz_maker"))
    taker_strategy = EquitiesMakerStrategy(
        config=_config(
            strategy_id="aapl_tradexyz_taker",
            external_strategy_id="aapl_tradexyz_taker",
        )
    )
    strategies = (maker_strategy, taker_strategy)
    supervisor = NodeQuoteFeedSupervisor()
    control_emitter = QuoteFeedControlEmitter(node_scoped_id="aapl_tradexyz")
    direct_subscribes: list[InstrumentId] = []
    direct_unsubscribes: list[InstrumentId] = []

    for strategy in strategies:
        maker_id, ref_id = _prepare_strategy_lifecycle(strategy, monkeypatch)
        strategy._runtime_params["quote_liveness_stall_after_ms"] = 3_000
        strategy._runtime_params["quote_liveness_recover_after_ms"] = 900
        strategy._latest_quotes = {
            maker_id: {"bid": Decimal("190.00"), "ask": Decimal("190.02"), "ts_ns": 1_000_000_000},
            ref_id: {"bid": Decimal("189.90"), "ask": Decimal("189.92"), "ts_ns": 1_000_000_000},
        }
        _wire_shared_quote_runtime(
            strategy,
            supervisor=supervisor,
            control_emitter=control_emitter,
        )
        monkeypatch.setattr(strategy, "_attach_local_quote_topic", lambda instrument_id: None, raising=False)
        monkeypatch.setattr(strategy, "subscribe_quote_ticks", lambda *, instrument_id, client_id=None: direct_subscribes.append(instrument_id))
        monkeypatch.setattr(strategy, "unsubscribe_quote_ticks", lambda *, instrument_id, client_id=None: direct_unsubscribes.append(instrument_id))
        strategy.on_start()

    direct_subscribes.clear()
    control_emitter.commands.clear()
    maker_strategy.on_time_event(SimpleNamespace(name=maker_strategy._liveness_timer_name))
    taker_strategy.on_time_event(SimpleNamespace(name=taker_strategy._liveness_timer_name))

    assert direct_subscribes == []
    assert direct_unsubscribes == []
    assert [(command.action, command.feed_identity.topic) for command in control_emitter.commands] == [
        ("reset", "maker_quote_ticks"),
    ]


def test_equities_maker_supervisor_retries_startup_blocked_feed_after_blocker_clears(
    monkeypatch,
) -> None:
    strategy = EquitiesMakerStrategy(config=_config())
    _prepare_strategy_lifecycle(strategy, monkeypatch)
    supervisor = NodeQuoteFeedSupervisor()
    control_emitter = QuoteFeedControlEmitter(node_scoped_id="aapl_tradexyz")
    maker_blocker_key = strategy.quote_feed_claim_specs()[0].blocker_key

    supervisor.set_blocker(maker_blocker_key, blocked=True, reason="session_down")
    _wire_shared_quote_runtime(
        strategy,
        supervisor=supervisor,
        control_emitter=control_emitter,
    )
    monkeypatch.setattr(strategy, "_attach_local_quote_topic", lambda instrument_id: None, raising=False)
    monkeypatch.setattr(strategy, "subscribe_quote_ticks", lambda *, instrument_id, client_id=None: None)
    monkeypatch.setattr(strategy, "unsubscribe_quote_ticks", lambda *, instrument_id, client_id=None: None)

    strategy.on_start()
    control_emitter.commands.clear()

    supervisor.set_blocker(maker_blocker_key, blocked=False, reason=None)
    strategy.on_time_event(SimpleNamespace(name=strategy._liveness_timer_name))

    assert [(command.action, command.feed_identity.topic) for command in control_emitter.commands] == [
        ("reset", "maker_quote_ticks"),
    ]


def test_equities_maker_runtime_param_refresh_updates_supervisor_freshness_budget(
    monkeypatch,
) -> None:
    strategy = EquitiesMakerStrategy(config=_config(max_ibkr_quote_age_ms=5_000))
    maker_id, ref_id = _prepare_strategy_lifecycle(strategy, monkeypatch)
    supervisor = NodeQuoteFeedSupervisor()
    control_emitter = QuoteFeedControlEmitter(node_scoped_id="aapl_tradexyz")

    strategy._runtime_params["quote_liveness_stall_after_ms"] = 5_000
    strategy._runtime_params["quote_liveness_recover_after_ms"] = 5_000
    strategy._latest_quotes = {
        maker_id: {"bid": Decimal("190.00"), "ask": Decimal("190.02"), "ts_ns": 1_000_000_000},
        ref_id: {"bid": Decimal("189.90"), "ask": Decimal("189.92"), "ts_ns": 1_000_000_000},
    }
    _wire_shared_quote_runtime(
        strategy,
        supervisor=supervisor,
        control_emitter=control_emitter,
    )
    monkeypatch.setattr(strategy, "_attach_local_quote_topic", lambda instrument_id: None, raising=False)
    monkeypatch.setattr(strategy, "subscribe_quote_ticks", lambda *, instrument_id, client_id=None: None)
    monkeypatch.setattr(strategy, "unsubscribe_quote_ticks", lambda *, instrument_id, client_id=None: None)
    strategy.clock.now = 1_000_000_000
    strategy.on_start()
    control_emitter.commands.clear()

    strategy.clock.now = 4_000_000_000
    strategy._last_runtime_params_refresh_ns = 0
    strategy._load_runtime_params = lambda: strategy._runtime_params.update(
        {
            "quote_liveness_stall_after_ms": 1_000,
            "quote_liveness_recover_after_ms": 1_000,
        }
    )

    strategy.on_time_event(SimpleNamespace(name=strategy._liveness_timer_name))

    assert [(command.action, command.feed_identity.topic) for command in control_emitter.commands] == [
        ("reset", "maker_quote_ticks"),
    ]


def test_equities_maker_supervisor_runtime_keeps_unsubscribe_ownership_in_supervisor(
    monkeypatch,
) -> None:
    maker_strategy = EquitiesMakerStrategy(config=_config(strategy_id="aapl_tradexyz_maker"))
    taker_strategy = EquitiesMakerStrategy(
        config=_config(
            strategy_id="aapl_tradexyz_taker",
            external_strategy_id="aapl_tradexyz_taker",
        )
    )
    strategies = (maker_strategy, taker_strategy)
    supervisor = NodeQuoteFeedSupervisor()
    control_emitter = QuoteFeedControlEmitter(node_scoped_id="aapl_tradexyz")
    direct_subscribes: list[tuple[str, InstrumentId]] = []
    direct_unsubscribes: list[tuple[str, InstrumentId]] = []

    for strategy in strategies:
        _prepare_strategy_lifecycle(strategy, monkeypatch)
        _wire_shared_quote_runtime(
            strategy,
            supervisor=supervisor,
            control_emitter=control_emitter,
        )
        monkeypatch.setattr(
            strategy,
            "subscribe_quote_ticks",
            lambda *, instrument_id, client_id=None, strategy=strategy: direct_subscribes.append(
                (strategy.config.external_strategy_id, instrument_id)
            ),
        )
        monkeypatch.setattr(
            strategy,
            "unsubscribe_quote_ticks",
            lambda *, instrument_id, client_id=None, strategy=strategy: direct_unsubscribes.append(
                (strategy.config.external_strategy_id, instrument_id)
            ),
        )
        strategy.on_start()

    assert sorted(direct_subscribes) == sorted(
        [
            ("aapl_tradexyz_maker", maker_strategy.config.maker_instrument_id),
            ("aapl_tradexyz_maker", maker_strategy.config.reference_instrument_id),
            ("aapl_tradexyz_taker", taker_strategy.config.maker_instrument_id),
            ("aapl_tradexyz_taker", taker_strategy.config.reference_instrument_id),
        ]
    )

    control_emitter.commands.clear()
    maker_strategy.on_stop()
    assert sorted(direct_unsubscribes) == sorted(
        [
            ("aapl_tradexyz_maker", maker_strategy.config.maker_instrument_id),
            ("aapl_tradexyz_maker", maker_strategy.config.reference_instrument_id),
        ]
    )
    assert control_emitter.commands == []

    taker_strategy.on_stop()
    assert sorted(direct_unsubscribes) == sorted(
        [
            ("aapl_tradexyz_maker", maker_strategy.config.maker_instrument_id),
            ("aapl_tradexyz_maker", maker_strategy.config.reference_instrument_id),
            ("aapl_tradexyz_taker", taker_strategy.config.maker_instrument_id),
            ("aapl_tradexyz_taker", taker_strategy.config.reference_instrument_id),
        ]
    )
    assert [(command.action, command.feed_identity.topic) for command in control_emitter.commands] == [
        ("unsubscribe", "maker_quote_ticks"),
        ("unsubscribe", "reference_quote_ticks"),
    ]


def test_equities_maker_on_quote_tick_reports_fresh_timestamp_to_supervisor(monkeypatch) -> None:
    strategy = EquitiesMakerStrategy(config=_config())
    maker_id, _ref_id = _configure_strategy_for_quotes(strategy)
    supervisor = NodeQuoteFeedSupervisor()
    control_emitter = QuoteFeedControlEmitter(node_scoped_id="aapl_tradexyz")
    maker_feed = strategy.quote_feed_claim_specs()[0].feed_identity

    supervisor.register_claimant(
        maker_feed,
        claimant_id="aapl_tradexyz_maker",
        unusable_after_ms=3_000,
        reset=lambda: None,
    )
    strategy.configure_quote_feed_runtime(
        supervisor=supervisor,
        control_emitter=control_emitter,
    )
    strategy._refresh_runtime_params_if_due = lambda **_kwargs: None
    strategy._retry_hedge_backlog = lambda **_kwargs: None
    strategy._refresh_maker_quotes = lambda **_kwargs: None
    strategy._refresh_quote_tradeability = lambda **_kwargs: True
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None

    strategy.on_quote_tick(
        SimpleNamespace(
            instrument_id=maker_id,
            bid_price=SimpleNamespace(as_decimal=lambda: Decimal("190.00")),
            ask_price=SimpleNamespace(as_decimal=lambda: Decimal("190.02")),
            ts_event=2_000_000_000,
        )
    )

    assert supervisor.snapshot(maker_feed).state == "healthy"


def test_equities_maker_supervisor_non_tradeable_pair_pulls_working_quotes(
    monkeypatch,
) -> None:
    strategy = EquitiesMakerStrategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quotes(strategy)
    supervisor = NodeQuoteFeedSupervisor()
    control_emitter = QuoteFeedControlEmitter(node_scoped_id="aapl_tradexyz")
    cancelled: list[InstrumentId] = []
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy.cancel_all_orders = lambda instrument_id: cancelled.append(instrument_id)
    strategy._managed_maker_orders = {
        "BUY": ManagedMakerOrderState(
            client_order_id="maker-order-1",
            instrument_id=str(maker_id),
            side="BUY",
            quantity=Decimal("1"),
            price=Decimal("190.00"),
            post_only=True,
            pending_cancel=False,
        ),
    }
    _install_lifecycle_clock(strategy, monkeypatch, now_ns=2_500_000_000)
    strategy._runtime_params["bot_on"] = True
    _wire_shared_quote_runtime(
        strategy,
        supervisor=supervisor,
        control_emitter=control_emitter,
    )
    for claim_spec in strategy.quote_feed_claim_specs():
        supervisor.register_claimant(
            claim_spec.feed_identity,
            claimant_id=claim_spec.claimant_id,
            unusable_after_ms=claim_spec.unusable_after_ms,
            reset=(lambda: None) if claim_spec.node_scoped_lifecycle else None,
            blocker_key=claim_spec.blocker_key,
        )
    supervisor.record_quote(
        strategy.quote_feed_claim_specs()[0].feed_identity,
        ts_ns=2_000_000_000,
    )
    supervisor.record_quote(
        strategy.quote_feed_claim_specs()[1].feed_identity,
        ts_ns=2_000_000_000,
    )
    supervisor.set_blocker("ibkr.shared_publisher", blocked=True, reason="publisher_down")
    strategy._latest_quotes = {
        maker_id: {"bid": Decimal("190.00"), "ask": Decimal("190.02"), "ts_ns": 2_000_000_000},
        ref_id: {"bid": Decimal("189.90"), "ask": Decimal("189.92"), "ts_ns": 2_000_000_000},
    }

    strategy.on_quote_tick(
        SimpleNamespace(
            instrument_id=maker_id,
            bid_price=SimpleNamespace(as_decimal=lambda: Decimal("190.00")),
            ask_price=SimpleNamespace(as_decimal=lambda: Decimal("190.02")),
            ts_event=2_500_000_000,
        )
    )

    assert cancelled == [maker_id]
    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "stale_quote"


def test_equities_maker_on_start_pulls_reclaimed_quotes_when_required_feed_is_blocked(
    monkeypatch,
) -> None:
    strategy = EquitiesMakerStrategy(config=_config())
    maker_id, ref_id = _prepare_strategy_lifecycle(strategy, monkeypatch)
    supervisor = NodeQuoteFeedSupervisor()
    control_emitter = QuoteFeedControlEmitter(node_scoped_id="aapl_tradexyz")
    cancelled: list[InstrumentId] = []

    strategy._runtime_params["bot_on"] = True
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy.cancel_all_orders = lambda instrument_id: cancelled.append(instrument_id)
    strategy._latest_quotes = {
        maker_id: {"bid": Decimal("190.00"), "ask": Decimal("190.02"), "ts_ns": 2_000_000_000},
        ref_id: {"bid": Decimal("189.90"), "ask": Decimal("189.92"), "ts_ns": 2_000_000_000},
    }
    strategy._reclaim_managed_maker_orders_from_cache = lambda: strategy._managed_maker_orders.update(
        {
            "BUY": ManagedMakerOrderState(
                client_order_id="maker-order-1",
                instrument_id=str(maker_id),
                side="BUY",
                quantity=Decimal("1"),
                price=Decimal("190.00"),
                post_only=True,
                pending_cancel=False,
            ),
        }
    )

    _wire_shared_quote_runtime(
        strategy,
        supervisor=supervisor,
        control_emitter=control_emitter,
    )
    monkeypatch.setattr(strategy, "_attach_local_quote_topic", lambda instrument_id: None, raising=False)
    monkeypatch.setattr(strategy, "subscribe_quote_ticks", lambda *, instrument_id, client_id=None: None)
    monkeypatch.setattr(strategy, "unsubscribe_quote_ticks", lambda *, instrument_id, client_id=None: None)
    supervisor.set_blocker("ibkr.shared_publisher", blocked=True, reason="publisher_down")

    strategy.on_start()

    assert cancelled == [maker_id]
    assert strategy.tradeable is False
    assert strategy.hedge_disabled_reason == "stale_quote"


def test_equities_maker_supervisor_states_drive_component_lifecycle(
    monkeypatch,
) -> None:
    strategy = EquitiesMakerStrategy(config=_config())
    maker_id, ref_id = _configure_strategy_for_quotes(strategy)
    supervisor = NodeQuoteFeedSupervisor(max_attempts=1)
    control_emitter = QuoteFeedControlEmitter(node_scoped_id="aapl_tradexyz")
    lifecycle_calls: list[str] = []

    _install_lifecycle_clock(strategy, monkeypatch, now_ns=2_500_000_000)
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None
    strategy._runtime_params["quote_liveness_stall_after_ms"] = 1_000
    strategy._runtime_params["quote_liveness_recover_after_ms"] = 1_000

    def _disable(reason: str) -> None:
        strategy.tradeable = False
        strategy.hedge_disabled_reason = reason
        lifecycle_calls.append(f"disable:{reason}")

    strategy._disable_hedging = _disable
    strategy.degrade = lambda: lifecycle_calls.append("degrade")
    strategy.resume = lambda: lifecycle_calls.append("resume")
    strategy.fault = lambda: lifecycle_calls.append("fault")
    _wire_shared_quote_runtime(
        strategy,
        supervisor=supervisor,
        control_emitter=control_emitter,
    )
    strategy._register_quote_feed_interest()
    strategy._latest_quotes = {
        maker_id: {"bid": Decimal("190.00"), "ask": Decimal("190.02"), "ts_ns": 2_000_000_000},
        ref_id: {"bid": Decimal("189.90"), "ask": Decimal("189.92"), "ts_ns": 2_000_000_000},
    }
    for claim_spec in strategy.quote_feed_claim_specs():
        supervisor.record_quote(
            claim_spec.feed_identity,
            ts_ns=2_000_000_000,
        )

    supervisor.set_blocker("ibkr.shared_publisher", blocked=True, reason="publisher_down")
    assert strategy._refresh_quote_tradeability(now_ns=2_500_000_000) is False
    assert lifecycle_calls == ["disable:stale_quote"]

    assert strategy._refresh_quote_tradeability(now_ns=3_600_000_000) is False
    assert lifecycle_calls == ["disable:stale_quote", "degrade"]

    supervisor.set_blocker("ibkr.shared_publisher", blocked=False, reason=None)
    supervisor.record_quote(strategy.quote_feed_claim_specs()[1].feed_identity, ts_ns=3_700_000_000)
    strategy._latest_quotes[ref_id]["ts_ns"] = 3_700_000_000
    strategy._latest_quotes[maker_id]["ts_ns"] = 3_700_000_000
    supervisor.record_quote(strategy.quote_feed_claim_specs()[0].feed_identity, ts_ns=3_700_000_000)
    assert strategy._refresh_quote_tradeability(now_ns=3_700_000_000) is True
    assert lifecycle_calls[2] == "resume"

    maker_feed = strategy.quote_feed_claim_specs()[0].feed_identity
    assert supervisor.request_recovery(
        maker_feed,
        now_ns=3_000_000_000,
        requested_by=strategy.config.external_strategy_id,
    )
    supervisor.ingest_recovery_result(
        maker_feed,
        now_ns=3_000_000_100,
        ok=False,
        error_summary="transport_failed",
    )
    assert strategy._refresh_quote_tradeability(now_ns=3_000_000_100) is False
    assert "fault" in lifecycle_calls
