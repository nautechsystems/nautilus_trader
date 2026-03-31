from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

import nautilus_trader.flux.strategies.makerv3.quote_engine as quote_engine_mod
from nautilus_trader.flux.common.keys import FluxRedisKeys
from nautilus_trader.flux.common.portfolio_inventory import encode_portfolio_inventory
from nautilus_trader.flux.strategies.makerv3 import inventory as inventory_mod
from nautilus_trader.flux.strategies.makerv3 import rebalancing as rebalancing_mod
from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_REFERENCE_MD_STALE
from nautilus_trader.flux.strategies.makerv3.constants import REASON_CANCEL_FREE_SLOT_FOR_MISSING_LEVEL
from nautilus_trader.flux.strategies.makerv3.constants import REASON_CANCEL_FRONT_VIOLATION
from nautilus_trader.flux.strategies.makerv3.constants import REASON_CANCEL_STALE_ORDER
from nautilus_trader.flux.strategies.makerv3.constants import REASON_CANCEL_TOO_AGGRESSIVE
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId


class _FakeRedis:
    def __init__(self) -> None:
        self.values: dict[str, bytes] = {}

    def get(self, key: str) -> bytes | None:
        return self.values.get(key)

    def set(self, key: str, value: str | bytes) -> bool:
        self.values[key] = value.encode() if isinstance(value, str) else value
        return True


def _desired_levels(*prices: str) -> list[tuple[Decimal, Decimal, Decimal]]:
    return [
        (Decimal(price), Decimal(price), Decimal(0))
        for price in prices
    ]


def _bounded_convergence_summary(**kwargs) -> dict[str, object]:
    planner = getattr(rebalancing_mod, "plan_side_bounded_convergence", None)
    assert callable(planner), "bounded convergence planner surface missing"
    return quote_engine_mod._bounded_convergence_summary(planner(**kwargs))


def test_shared_quote_health_distinguishes_old_quote_from_feed_down() -> None:
    from nautilus_trader.flux.strategies.shared.quote_health import evaluate_quote_health

    health = evaluate_quote_health(
        leg_role="maker",
        bid=Decimal("100"),
        ask=Decimal("101"),
        quote_age_ms=12_000,
        max_quote_age_ms=10_000,
        transport_connected=True,
        subscription_healthy=True,
    )

    assert health.feed_state == "ok"
    assert health.quote_state == "old"
    assert health.usable_for_pricing is False


def test_refresh_quotes_blocks_when_maker_market_data_is_stale(strategy_factory) -> None:
    strategy = strategy_factory()

    cancels: list[str] = []
    states: list[str] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: cancels.append(
        f"{reason}:{force}",
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 200_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    strategy._refresh_quotes(now_ns=now_ns)

    assert "maker_md_stale:False" in cancels
    assert states == ["blocked_maker_md"]
    assert strategy._last_requote_ns == now_ns


def test_refresh_quotes_blocks_when_reference_market_data_is_stale(strategy_factory) -> None:
    strategy = strategy_factory()

    cancels: list[str] = []
    states: list[str] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: cancels.append(
        f"{reason}:{force}",
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 200_000_000

    strategy._refresh_quotes(now_ns=now_ns)

    assert "reference_md_stale:False" in cancels
    assert states == ["blocked_reference_md"]
    assert strategy._last_requote_ns == now_ns


def test_refresh_quotes_treats_age_equal_to_max_age_ms_as_stale(strategy_factory) -> None:
    strategy = strategy_factory()

    cancels: list[str] = []
    states: list[str] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: cancels.append(
        f"{reason}:{force}",
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 100_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    strategy._refresh_quotes(now_ns=now_ns)

    assert cancels == ["maker_md_stale:False"]
    assert states == ["blocked_maker_md"]


def test_refresh_quotes_does_not_consult_resting_order_age_for_rebalance(
    strategy_factory,
    monkeypatch,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = lambda: [
        SimpleNamespace(
            client_order_id="RESTING-1",
            price=Decimal("100"),
            quantity=Decimal("1"),
            side=OrderSide.BUY,
            ts_init=1,
        ),
    ]
    strategy.cancel_order = lambda _order: None
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_quote_cycle_event = lambda **_kwargs: None
    strategy._publish_state = lambda *_args, **_kwargs: None

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    monkeypatch.setattr(
        quote_engine_mod,
        "build_ladder_place_cancel_levels_from_bps",
        lambda **_kwargs: ([(Decimal("100"), Decimal("100"))], []),
    )

    def _raise_if_called(*_args, **_kwargs):
        raise AssertionError("_is_stale_order should not be used by quote refresh")

    strategy._is_stale_order = _raise_if_called

    strategy._refresh_quotes(now_ns=now_ns)


def test_refresh_quotes_uses_shared_quote_health_for_maker_stale_block(
    strategy_factory,
    monkeypatch,
) -> None:
    strategy = strategy_factory()

    cancels: list[str] = []
    states: list[str] = []
    health_calls: list[dict[str, object]] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: cancels.append(
        f"{reason}:{force}",
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))

    def _fake_quote_health(**kwargs: object) -> SimpleNamespace:
        health_calls.append(dict(kwargs))
        return SimpleNamespace(
            leg_role=kwargs["leg_role"],
            feed_state="ok",
            quote_state="old" if kwargs["leg_role"] == "maker" else "fresh",
            quote_age_ms=kwargs["quote_age_ms"],
            usable_for_pricing=kwargs["leg_role"] != "maker",
            usable_for_hedging=kwargs["leg_role"] != "maker",
            reason_code="maker_quote_old" if kwargs["leg_role"] == "maker" else None,
            alert_level="warning" if kwargs["leg_role"] == "maker" else None,
        )

    monkeypatch.setattr(
        quote_engine_mod,
        "evaluate_quote_health",
        _fake_quote_health,
        raising=False,
    )

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    strategy._refresh_quotes(now_ns=now_ns)

    assert health_calls == [
        {
            "leg_role": "maker",
            "bid": Decimal("100"),
            "ask": Decimal("101"),
            "quote_age_ms": 10,
            "max_quote_age_ms": 99,
            "transport_connected": True,
            "subscription_healthy": True,
        },
    ]
    assert cancels == ["maker_md_stale:False"]
    assert states == ["blocked_maker_md"]


def test_stale_cancel_first_detection_allows_cancel_before_cooldown_window(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy.STALE_CANCEL_COOLDOWN_MS = 1_000

    cancels: list[str] = []
    states: list[str] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: cancels.append(
        f"{reason}:{force}",
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)

    strategy._handle_stale_quote_block(
        now_ns=100_000_000,
        state="blocked_reference_md",
        cancel_reason="reference_md_stale",
        reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
        quote_cycle_id="cycle-1",
        warning_message="blocked",
    )

    assert cancels == ["reference_md_stale:False"]
    assert states == ["blocked_reference_md"]
    assert strategy._last_stale_cancel_ns == 100_000_000


def test_refresh_quotes_stale_path_calls_managed_orders_once_per_cycle(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1, 2, 3])

    strategy._maker_instrument = object()
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000 - 200_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000 - 10_000_000

    calls = {"count": 0}

    def _managed_orders() -> list[SimpleNamespace]:
        calls["count"] += 1
        return []

    strategy._managed_orders = _managed_orders

    strategy._refresh_quotes(now_ns=1_000_000_000)

    assert calls["count"] == 1


def test_stale_cooldown_resets_on_unblocked_transition_for_new_block_episode(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1])
    strategy.STALE_CANCEL_COOLDOWN_MS = 1_000

    strategy._managed_orders = list
    strategy._publish_json = lambda *_args, **_kwargs: None

    cancels: list[str] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: cancels.append(
        f"{reason}:{force}",
    )

    strategy._handle_stale_quote_block(
        now_ns=200_000_000,
        state="blocked_reference_md",
        cancel_reason="reference_md_stale",
        reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
        quote_cycle_id="cycle-1",
        warning_message="blocked",
    )
    strategy._handle_stale_quote_block(
        now_ns=250_000_000,
        state="blocked_reference_md",
        cancel_reason="reference_md_stale",
        reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
        quote_cycle_id="cycle-2",
        warning_message="blocked",
    )
    strategy._publish_state("running")
    strategy._handle_stale_quote_block(
        now_ns=260_000_000,
        state="blocked_reference_md",
        cancel_reason="reference_md_stale",
        reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
        quote_cycle_id="cycle-3",
        warning_message="blocked",
    )
    strategy._handle_stale_quote_block(
        now_ns=270_000_000,
        state="blocked_reference_md",
        cancel_reason="reference_md_stale",
        reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
        quote_cycle_id="cycle-4",
        warning_message="blocked",
    )

    assert cancels == ["reference_md_stale:False", "reference_md_stale:False"]
    assert strategy._last_stale_cancel_ns == 260_000_000


def test_refresh_quotes_recovers_from_blocked_state_without_rebalance(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_001])

    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000 - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000 - 10_000_000
    strategy._state_is_blocked = True
    strategy._last_state_name = "blocked_reference_md"
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._managed_orders = list

    transition_events: list[tuple[str, dict[str, object]]] = []
    strategy._publish_event = lambda event, **kwargs: transition_events.append((event, kwargs))
    strategy._publish_json = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=1_000_000_000)

    state_transitions = [
        payload for name, payload in transition_events if name == "state_transition"
    ]
    assert len(state_transitions) == 1
    assert state_transitions[0]["from_state"] == "blocked_reference_md"
    assert state_transitions[0]["to_state"] == "running"
    assert strategy._state_is_blocked is False
    assert strategy._last_state_name == "running"


def test_refresh_quotes_records_last_completed_quote_progress(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_001])

    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000 - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000 - 10_000_000
    strategy._managed_orders = list
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=1_000_000_000)

    assert strategy._last_completed_quote_ns == 1_000_000_000


def test_refresh_quotes_passes_bounded_convergence_budgets_and_planned_levels_per_side(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = lambda: []
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    events: list[dict[str, object]] = []
    strategy._publish_quote_cycle_event = lambda **kwargs: events.append(kwargs)
    strategy._runtime_params["max_cancels_per_side_per_cycle"] = 2
    strategy._runtime_params["max_places_per_side_per_cycle"] = 1
    strategy._runtime_params["max_total_actions_per_cycle"] = 2
    strategy._runtime_params["max_pending_cancels_per_side"] = 1

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    rebalance_calls: list[tuple[OrderSide, str, int, int, int]] = []
    place_calls: list[tuple[OrderSide, tuple[int, ...]]] = []

    def _rebalance_side(**kwargs) -> int:
        rebalance_calls.append(
            (
                kwargs["side"],
                kwargs["backlog_mode"],
                kwargs["max_reprice_cancel_actions"],
                kwargs["max_place_actions"],
                kwargs["max_total_actions"],
            ),
        )
        return 0

    def _place_missing_levels(**kwargs) -> int:
        place_calls.append((kwargs["side"], tuple(kwargs["level_indices"])))
        return 0

    strategy._rebalance_side = _rebalance_side
    strategy._place_missing_levels = _place_missing_levels

    strategy._refresh_quotes(now_ns=now_ns)

    assert rebalance_calls == [
        (OrderSide.BUY, "normal", 2, 1, 2),
        (OrderSide.SELL, "normal", 2, 1, 2),
    ]
    assert place_calls == [
        (OrderSide.BUY, (0,)),
        (OrderSide.SELL, (0,)),
    ]
    bounded_convergence = events[-1]["payload"]["decision_context_json"]["bounded_convergence"]
    assert bounded_convergence["buy"]["planned_place_count"] == 1
    assert bounded_convergence["buy"]["budget_limited"] is True
    assert bounded_convergence["buy"]["cancel_reason_counts"] == {}
    assert bounded_convergence["sell"]["planned_place_count"] == 1
    assert bounded_convergence["sell"]["budget_limited"] is True


def test_bounded_convergence_summary_stale_stable_stack_is_no_op() -> None:
    summary = _bounded_convergence_summary(
        side="buy",
        active_prices=[Decimal("100"), Decimal("99"), Decimal("98")],
        active_stale=[False, False, True],
        desired_levels=_desired_levels("100", "99", "98"),
        stale_cancel_budget=1,
        max_reprice_cancel_actions=1,
        max_place_actions=1,
        max_total_actions=2,
        backlog_mode="normal",
    )

    assert summary["stack_action_mode"] == "no_op"
    assert summary["planned_cancel_count"] == 0
    assert summary["planned_place_count"] == 0
    assert summary["cancel_reason_counts"].get(REASON_CANCEL_STALE_ORDER, 0) == 0


def test_bounded_convergence_summary_inward_deque_cycle_reports_front_back_only_mode() -> None:
    summary = _bounded_convergence_summary(
        side="buy",
        active_prices=[Decimal("100"), Decimal("99"), Decimal("98")],
        active_stale=[False, False, False],
        desired_levels=_desired_levels("101", "100", "99"),
        stale_cancel_budget=0,
        max_reprice_cancel_actions=1,
        max_place_actions=1,
        max_total_actions=2,
        backlog_mode="normal",
    )

    assert summary["stack_action_mode"] == "place_front_cancel_back"
    assert summary["planned_cancel_count"] == 1
    assert summary["planned_place_count"] == 1
    assert summary["cancel_reason_counts"].get(REASON_CANCEL_FREE_SLOT_FOR_MISSING_LEVEL, 0) == 0
    assert summary["cancel_reason_counts"].get(REASON_CANCEL_TOO_AGGRESSIVE, 0) == 0


def test_refresh_quotes_alternates_side_priority_when_total_action_budget_is_one(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = lambda: []
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_quote_cycle_event = lambda **_kwargs: None
    strategy._publish_state = lambda *_args, **_kwargs: None
    strategy._runtime_params["max_cancels_per_side_per_cycle"] = 0
    strategy._runtime_params["max_places_per_side_per_cycle"] = 1
    strategy._runtime_params["max_total_actions_per_cycle"] = 1

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    place_calls: list[OrderSide] = []
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = (
        lambda **kwargs: place_calls.append(kwargs["side"]) or 1
    )

    strategy._refresh_quotes(now_ns=now_ns)
    strategy._refresh_quotes(now_ns=now_ns + 1_000_000)

    assert place_calls == [OrderSide.BUY, OrderSide.SELL]


def test_refresh_quotes_skips_when_cancel_reject_cooldown_is_active(strategy_factory) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000
    strategy._managed_orders = lambda: [
        SimpleNamespace(
            client_order_id="RESTING-1",
            price=Decimal("100"),
            side=OrderSide.BUY,
            ts_init=0,
        ),
    ]
    strategy._cancel_reject_retry_after_ns_by_client_order_id = {
        "RESTING-1": now_ns + 1_000_000_000,
    }

    events: list[dict[str, object]] = []
    strategy._publish_quote_cycle_event = lambda **kwargs: events.append(kwargs)
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_state = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=now_ns)

    assert events[-1]["quote_cycle_event"] == "skipped"
    assert events[-1]["reason_code"] == "skip_cancel_reject_cooldown"


def test_refresh_quotes_blocks_when_pending_cancel_is_old_and_no_quote_progress(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = lambda: []
    strategy._last_state_name = "running"
    strategy._runtime_params["max_pending_cancels_per_side"] = 1
    strategy._runtime_params["pending_cancel_block_after_ms"] = 100
    strategy._runtime_params["quote_liveness_stall_after_ms"] = 150
    strategy._cache = SimpleNamespace(
        order=lambda client_order_id: SimpleNamespace(
            client_order_id=client_order_id,
            side=OrderSide.BUY,
        ),
    )

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000
    strategy._track_pending_cancel("RESTING-1", now_ns=800_000_000)

    states: list[str] = []
    events: list[dict[str, object]] = []
    alerts: list[dict[str, object]] = []
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_quote_cycle_event = lambda **kwargs: events.append(kwargs)
    strategy._publish_actionable_alert = lambda **kwargs: alerts.append(kwargs) or True
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=now_ns)

    assert states == ["blocked_pending_cancel"]
    assert events[-1]["quote_cycle_event"] == "blocked"
    assert events[-1]["reason_code"] == "pending_cancel_stuck"
    assert alerts[-1]["alert_key"] == "quote_liveness_blocked"
    assert alerts[-1]["reason_code"] == "pending_cancel_stuck"
    assert alerts[-1]["transition"] == "running->blocked_pending_cancel"


def test_refresh_quotes_skips_when_pending_cancel_is_recent(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = lambda: []
    strategy._runtime_params["pending_cancel_block_after_ms"] = 500
    strategy._runtime_params["max_pending_cancels_per_side"] = 2
    strategy._last_state_name = "running"
    strategy._cache = SimpleNamespace(
        order=lambda client_order_id: SimpleNamespace(
            client_order_id=client_order_id,
            side=OrderSide.BUY,
        ),
    )

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000
    strategy._track_pending_cancel("RESTING-1", now_ns=now_ns - 100_000_000)

    rebalance_calls: list[tuple[OrderSide, str]] = []
    place_calls: list[tuple[OrderSide, tuple[int, ...]]] = []
    states: list[str] = []
    events: list[dict[str, object]] = []
    alerts: list[dict[str, object]] = []
    strategy._rebalance_side = (
        lambda **kwargs: rebalance_calls.append((kwargs["side"], kwargs["backlog_mode"])) or 0
    )
    strategy._place_missing_levels = (
        lambda **kwargs: place_calls.append((kwargs["side"], tuple(kwargs["level_indices"]))) or 0
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_quote_cycle_event = lambda **kwargs: events.append(kwargs)
    strategy._publish_actionable_alert = lambda **kwargs: alerts.append(kwargs) or True
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=now_ns)

    assert rebalance_calls == [
        (OrderSide.BUY, "normal"),
        (OrderSide.SELL, "normal"),
    ]
    assert place_calls == [
        (OrderSide.BUY, (0,)),
        (OrderSide.SELL, (0,)),
    ]
    assert states == ["running"]
    assert events[-1]["quote_cycle_event"] == "skipped"
    assert events[-1]["reason_code"] == "skip_pending_cancels"
    assert events[-1]["oldest_pending_cancel_age_ms"] == 100
    assert events[-1]["payload"]["oldest_pending_cancel_age_ms"] == 100
    assert events[-1]["payload"]["backlog_mode"] == "normal"
    assert alerts == []


def test_refresh_quotes_pending_cancel_soft_throttle_skips_repricing_when_backlog_present(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = lambda: []
    strategy._last_state_name = "running"
    strategy._runtime_params["max_pending_cancels_per_side"] = 1
    strategy._runtime_params["max_cancels_per_side_per_cycle"] = 1
    strategy._runtime_params["max_places_per_side_per_cycle"] = 1
    strategy._runtime_params["max_total_actions_per_cycle"] = 2
    strategy._runtime_params["pending_cancel_block_after_ms"] = 500
    strategy._cache = SimpleNamespace(
        order=lambda client_order_id: SimpleNamespace(
            client_order_id=client_order_id,
            side=OrderSide.BUY,
        ),
    )
    strategy._track_pending_cancel("RESTING-1", now_ns=900_000_000)

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    rebalance_calls: list[tuple[OrderSide, str]] = []
    place_calls: list[tuple[OrderSide, tuple[int, ...]]] = []

    def _rebalance_side(**kwargs) -> int:
        rebalance_calls.append((kwargs["side"], kwargs["backlog_mode"]))
        return 0

    def _place_missing_levels(**kwargs) -> int:
        place_calls.append((kwargs["side"], tuple(kwargs["level_indices"])))
        return 0

    states: list[str] = []
    events: list[dict[str, object]] = []
    strategy._rebalance_side = _rebalance_side
    strategy._place_missing_levels = _place_missing_levels
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_quote_cycle_event = lambda **kwargs: events.append(kwargs)
    strategy._publish_actionable_alert = lambda **_kwargs: True
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=now_ns)

    assert rebalance_calls == [
        (OrderSide.BUY, "soft_throttle"),
        (OrderSide.SELL, "normal"),
    ]
    assert (OrderSide.BUY, (0,)) not in place_calls
    assert (OrderSide.SELL, (0,)) in place_calls
    assert "blocked_pending_cancel" not in states
    assert events[-1]["payload"].get("backlog_mode") == "soft_throttle"
    bounded_convergence = events[-1]["payload"]["decision_context_json"]["bounded_convergence"]
    assert bounded_convergence["buy"]["backlog_mode"] == "soft_throttle"
    assert bounded_convergence["buy"]["backlog_limited"] is True
    assert bounded_convergence["sell"]["backlog_mode"] == "normal"


def test_refresh_quotes_does_not_same_cycle_backfill_after_front_cancel(
    strategy_factory,
    monkeypatch,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = lambda: [
        SimpleNamespace(
            client_order_id="RESTING-BUY-1",
            side=OrderSide.BUY,
            price=Decimal("100"),
            quantity=Decimal("1"),
            ts_init=1,
        ),
    ]
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_quote_cycle_event = lambda **_kwargs: None
    strategy._publish_state = lambda *_args, **_kwargs: None

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    buy_plan = rebalancing_mod.BoundedConvergencePlan(
        cancel_actions=(
            rebalancing_mod.CancelAction(
                index=0,
                reason_code=REASON_CANCEL_FRONT_VIOLATION,
            ),
        ),
        place_level_indices=(2,),
        diagnostics=rebalancing_mod.ConvergenceDiagnostics(
            stack_action_mode="cancel_front_place_back",
            backlog_mode="normal",
            matched_level_count=0,
            keep_level_count=0,
            missing_level_count=1,
            frontier_missing_level_count=1,
            interior_hole_count=0,
            planned_stale_replacement_count=0,
            total_missing_level_count=0,
            excess_cancel_candidate_count=0,
            aggressive_cancel_candidate_count=1,
            stale_cancel_candidate_count=0,
            room_cancel_candidate_count=0,
            budget_limited=False,
            backlog_limited=False,
            depth_before=1,
            depth_after=1,
            temporary_oversize_depth=1,
            front_changed=True,
            back_changed=True,
        ),
    )
    sell_plan = rebalancing_mod.BoundedConvergencePlan(
        cancel_actions=(),
        place_level_indices=(),
        diagnostics=rebalancing_mod.ConvergenceDiagnostics(
            stack_action_mode="no_op",
            backlog_mode="normal",
            matched_level_count=0,
            keep_level_count=0,
            missing_level_count=0,
            frontier_missing_level_count=0,
            interior_hole_count=0,
            planned_stale_replacement_count=0,
            total_missing_level_count=0,
            excess_cancel_candidate_count=0,
            aggressive_cancel_candidate_count=0,
            stale_cancel_candidate_count=0,
            room_cancel_candidate_count=0,
            budget_limited=False,
            backlog_limited=False,
            depth_before=0,
            depth_after=0,
            temporary_oversize_depth=0,
            front_changed=False,
            back_changed=False,
        ),
    )

    def _plan_side_bounded_convergence(*, side: str, **_kwargs):
        return buy_plan if side == "buy" else sell_plan

    monkeypatch.setattr(
        rebalancing_mod,
        "plan_side_bounded_convergence",
        _plan_side_bounded_convergence,
    )

    rebalance_calls: list[tuple[OrderSide, tuple[tuple[int, str], ...]]] = []
    place_calls: list[tuple[OrderSide, tuple[int, ...]]] = []

    def _rebalance_side(**kwargs) -> int:
        rebalance_calls.append(
            (
                kwargs["side"],
                tuple((action.index, action.reason_code) for action in kwargs["cancel_actions"]),
            ),
        )
        return 1 if kwargs["side"] == OrderSide.BUY else 0

    def _place_missing_levels(**kwargs) -> int:
        place_calls.append((kwargs["side"], tuple(kwargs["level_indices"])))
        return 0

    strategy._rebalance_side = _rebalance_side
    strategy._place_missing_levels = _place_missing_levels

    strategy._refresh_quotes(now_ns=now_ns)

    assert rebalance_calls == [
        (OrderSide.BUY, ((0, REASON_CANCEL_FRONT_VIOLATION),)),
        (OrderSide.SELL, ()),
    ]
    assert place_calls == []


def test_refresh_quotes_pending_cancel_hard_freeze_stays_unblocked_until_stall_threshold(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = lambda: []
    strategy._last_state_name = "running"
    strategy._runtime_params["max_pending_cancels_per_side"] = 1
    strategy._runtime_params["pending_cancel_block_after_ms"] = 100
    strategy._runtime_params["quote_liveness_stall_after_ms"] = 500
    strategy._cache = SimpleNamespace(
        order=lambda client_order_id: SimpleNamespace(
            client_order_id=client_order_id,
            side=OrderSide.BUY,
        ),
    )
    strategy._track_pending_cancel("RESTING-1", now_ns=800_000_000)

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    rebalance_calls: list[tuple[OrderSide, str]] = []
    states: list[str] = []
    events: list[dict[str, object]] = []
    alerts: list[dict[str, object]] = []

    def _rebalance_side(**kwargs) -> int:
        rebalance_calls.append((kwargs["side"], kwargs["backlog_mode"]))
        return 0

    strategy._rebalance_side = _rebalance_side
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_quote_cycle_event = lambda **kwargs: events.append(kwargs)
    strategy._publish_actionable_alert = lambda **kwargs: alerts.append(kwargs) or True
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=now_ns)

    assert rebalance_calls == [
        (OrderSide.BUY, "hard_freeze"),
        (OrderSide.SELL, "normal"),
    ]
    assert "blocked_pending_cancel" not in states
    assert events[-1]["quote_cycle_event"] != "blocked"
    assert events[-1]["payload"].get("backlog_mode") == "hard_freeze"
    assert alerts == []


def test_refresh_quotes_pending_cancel_blocked_path_never_reprices_pathological_backlog(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = lambda: []
    strategy._last_state_name = "running"
    strategy._runtime_params["max_pending_cancels_per_side"] = 1
    strategy._runtime_params["pending_cancel_block_after_ms"] = 100
    strategy._runtime_params["quote_liveness_stall_after_ms"] = 150
    strategy._cache = SimpleNamespace(
        order=lambda client_order_id: SimpleNamespace(
            client_order_id=client_order_id,
            side=OrderSide.BUY,
        ),
    )
    strategy._track_pending_cancel("RESTING-1", now_ns=800_000_000)

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    rebalance_calls: list[OrderSide] = []
    states: list[str] = []
    events: list[dict[str, object]] = []
    strategy._rebalance_side = lambda **kwargs: rebalance_calls.append(kwargs["side"]) or 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_quote_cycle_event = lambda **kwargs: events.append(kwargs)
    strategy._publish_actionable_alert = lambda **_kwargs: True
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=now_ns)

    assert rebalance_calls == []
    assert states == ["blocked_pending_cancel"]
    assert events[-1]["quote_cycle_event"] == "blocked"
    assert events[-1]["reason_code"] == "pending_cancel_stuck"
    assert events[-1]["payload"].get("backlog_mode") == "blocked"


def test_refresh_quotes_unknown_side_pending_cancel_escalates_to_blocked(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = lambda: []
    strategy._last_state_name = "running"
    strategy._runtime_params["max_pending_cancels_per_side"] = 1
    strategy._runtime_params["pending_cancel_block_after_ms"] = 100
    strategy._runtime_params["quote_liveness_stall_after_ms"] = 150
    strategy._cache = SimpleNamespace(
        order=lambda client_order_id: SimpleNamespace(
            client_order_id=client_order_id,
        ),
    )
    strategy._track_pending_cancel("RESTING-1", now_ns=800_000_000)

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    rebalance_calls: list[OrderSide] = []
    states: list[str] = []
    events: list[dict[str, object]] = []
    alerts: list[dict[str, object]] = []
    strategy._rebalance_side = lambda **kwargs: rebalance_calls.append(kwargs["side"]) or 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_quote_cycle_event = lambda **kwargs: events.append(kwargs)
    strategy._publish_actionable_alert = lambda **kwargs: alerts.append(kwargs) or True
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=now_ns)

    assert rebalance_calls == []
    assert states == ["blocked_pending_cancel"]
    assert events[-1]["quote_cycle_event"] == "blocked"
    assert events[-1]["reason_code"] == "pending_cancel_stuck"
    assert events[-1]["payload"].get("backlog_mode") == "blocked"
    assert alerts[-1]["alert_key"] == "quote_liveness_blocked"


def test_refresh_quotes_clears_orphaned_pending_cancel_when_cache_has_no_live_order(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = lambda: []
    strategy._pending_cancel_client_order_ids = {"ORPHAN-1"}
    strategy._last_state_name = "running"
    strategy._cache = SimpleNamespace(order=lambda _client_order_id: None)

    now_ns = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_state = lambda *_args, **_kwargs: None
    strategy._publish_quote_cycle_event = lambda **_kwargs: None
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=now_ns)

    assert strategy._pending_cancel_client_order_ids == set()


def test_refresh_quotes_blocks_when_shared_portfolio_inventory_is_degraded(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        base_currency=SimpleNamespace(code="PLUME"),
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
        id=strategy.config.maker_instrument_id,
    )
    strategy._instruments = {
        strategy.config.maker_instrument_id: strategy._maker_instrument,
        strategy.config.reference_instrument_id: SimpleNamespace(
            base_currency=SimpleNamespace(code="PLUME"),
            id=strategy.config.reference_instrument_id,
        ),
    }
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = list

    fake_redis = _FakeRedis()
    aggregate_key = FluxRedisKeys.portfolio_inventory(
        portfolio_id="tokenmm",
        base_currency="PLUME",
    )
    fake_redis.set(
        aggregate_key,
        encode_portfolio_inventory(
            {
                "portfolio_id": "tokenmm",
                "base_currency": "PLUME",
                "global_qty": None,
                "ts_ms": 1_000,
                "stale_after_ms": 3_000,
                "components": [],
                "missing_required": ["strategy_02"],
                "degraded": True,
            },
        ),
    )
    strategy.configure_portfolio_inventory_feed(
        redis_client=fake_redis,
        portfolio_id="tokenmm",
        namespace="flux",
        schema_version="v1",
    )

    now_ns = 1_500_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    cancels: list[str] = []
    states: list[str] = []
    alerts: list[dict[str, object]] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: cancels.append(
        f"{reason}:{force}",
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_actionable_alert = lambda **kwargs: alerts.append(kwargs) or True

    strategy._refresh_quotes(now_ns=now_ns)

    assert cancels == ["portfolio_inventory_unavailable:False"]
    assert states == ["blocked_portfolio_inventory"]
    assert alerts[-1]["reason_code"] == "blocked_portfolio_inventory_unavailable"


def test_refresh_quotes_blocks_when_controller_private_path_is_stale(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._maker_instrument = SimpleNamespace(
        base_currency=SimpleNamespace(code="PLUME"),
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
        id=strategy.config.maker_instrument_id,
    )
    strategy._instruments = {
        strategy.config.maker_instrument_id: strategy._maker_instrument,
        strategy.config.reference_instrument_id: SimpleNamespace(
            base_currency=SimpleNamespace(code="PLUME"),
            id=strategy.config.reference_instrument_id,
        ),
    }
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = list
    strategy._controller_private_path_health = {
        "healthy": False,
        "state": "stale",
        "last_error_type": "TimeoutError",
        "timeout_count": 2,
    }

    now_ns = 1_500_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    cancels: list[str] = []
    states: list[str] = []
    alerts: list[dict[str, object]] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: cancels.append(
        f"{reason}:{force}",
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_actionable_alert = lambda **kwargs: alerts.append(kwargs) or True

    strategy._refresh_quotes(now_ns=now_ns)

    assert cancels == ["private_path_unavailable:False"]
    assert states == ["blocked_private_path"]
    assert alerts[-1]["reason_code"] == "blocked_private_path_unavailable"


def test_refresh_quotes_allows_partial_shared_portfolio_inventory_when_enabled(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_500_000_000])
    strategy._maker_instrument = SimpleNamespace(
        base_currency=SimpleNamespace(code="PLUME"),
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
        id=strategy.config.maker_instrument_id,
    )
    strategy._instruments = {
        strategy.config.maker_instrument_id: strategy._maker_instrument,
        strategy.config.reference_instrument_id: SimpleNamespace(
            base_currency=SimpleNamespace(code="PLUME"),
            id=strategy.config.reference_instrument_id,
        ),
    }
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._managed_orders = list
    strategy._refresh_quotes_to_target = lambda *_args, **_kwargs: None
    strategy._compute_fv = lambda *_args, **_kwargs: Decimal("100.5")
    strategy._managed_orders = lambda: []
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    fake_redis = _FakeRedis()
    aggregate_key = FluxRedisKeys.portfolio_inventory(
        portfolio_id="tokenmm",
        base_currency="PLUME",
    )
    fake_redis.set(
        aggregate_key,
        encode_portfolio_inventory(
            {
                "portfolio_id": "tokenmm",
                "base_currency": "PLUME",
                "global_qty": "129016.69578451",
                "aggregation_mode": "partial",
                "global_qty_complete": False,
                "ts_ms": 1_000,
                "stale_after_ms": 3_000,
                "components": [],
                "missing_required": ["strategy_02"],
                "stale_required": [],
                "null_qty_required": [],
                "degraded": True,
            },
        ),
    )
    strategy.configure_portfolio_inventory_feed(
        redis_client=fake_redis,
        portfolio_id="tokenmm",
        namespace="flux",
        schema_version="v1",
        allow_partial_global_risk=True,
    )

    now_ns = 1_500_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    cancels: list[str] = []
    states: list[str] = []
    alerts: list[dict[str, object]] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: cancels.append(
        f"{reason}:{force}",
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_actionable_alert = lambda **kwargs: alerts.append(kwargs) or True

    strategy._refresh_quotes(now_ns=now_ns)

    assert cancels == []
    assert states == []
    assert alerts == []


def test_refresh_quotes_uses_canonical_portfolio_asset_id_for_shared_inventory_gate(
    clocked_strategy_factory,
) -> None:
    maker_instrument_id = InstrumentId.from_str("xyz:AMD-USD-PERP.HYPERLIQUID")
    reference_instrument_id = InstrumentId.from_str("AMD.NASDAQ")
    strategy = clocked_strategy_factory(
        [1_500_000_000],
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
        portfolio_asset_id="AMD",
    )
    strategy._maker_instrument = SimpleNamespace(
        base_currency=SimpleNamespace(code="XYZ:AMD"),
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
        id=maker_instrument_id,
    )
    strategy._instruments = {
        maker_instrument_id: strategy._maker_instrument,
        reference_instrument_id: SimpleNamespace(
            base_currency=SimpleNamespace(code="AMD"),
            id=reference_instrument_id,
        ),
    }
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._refresh_quotes_to_target = lambda *_args, **_kwargs: None
    strategy._compute_fv = lambda *_args, **_kwargs: Decimal("100.5")
    strategy._managed_orders = lambda: []
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    fake_redis = _FakeRedis()
    aggregate_key = FluxRedisKeys.portfolio_inventory(
        portfolio_id="equities",
        base_currency="AMD",
    )
    fake_redis.set(
        aggregate_key,
        encode_portfolio_inventory(
            {
                "portfolio_id": "equities",
                "base_currency": "AMD",
                "global_qty": "0",
                "global_qty_base": "0",
                "global_qty_complete": True,
                "global_qty_base_complete": True,
                "ts_ms": 1_000,
                "stale_after_ms": 3_000,
                "components": [],
                "missing_required": [],
                "stale_required": [],
                "null_qty_required": [],
                "degraded": False,
            },
        ),
    )
    strategy.configure_portfolio_inventory_feed(
        redis_client=fake_redis,
        portfolio_id="equities",
        namespace="flux",
        schema_version="v1",
    )

    now_ns = 1_500_000_000
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = now_ns - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = now_ns - 10_000_000

    cancels: list[str] = []
    states: list[str] = []
    alerts: list[dict[str, object]] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: cancels.append(
        f"{reason}:{force}",
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_actionable_alert = lambda **kwargs: alerts.append(kwargs) or True

    strategy._refresh_quotes(now_ns=now_ns)

    assert cancels == []
    assert states == []
    assert alerts == []


def test_refresh_quotes_uses_runtime_snapshot_without_runtime_getters(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_001])

    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000 - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000 - 10_000_000
    strategy._managed_orders = list
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0

    strategy._runtime_decimal = lambda _name: (_ for _ in ()).throw(
        AssertionError("refresh path should use cached typed runtime params"),
    )
    strategy._runtime_int = lambda _name: (_ for _ in ()).throw(
        AssertionError("refresh path should use cached typed runtime params"),
    )

    strategy._refresh_quotes(now_ns=1_000_000_000)


def test_refresh_quotes_caches_inventory_skew_with_order_event_invalidation(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1, 2, 3, 4])
    strategy.INVENTORY_SKEW_CACHE_TTL_MS = 200
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000
    strategy._managed_orders = list
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    calls = {"count": 0}

    def _compute_inventory_skew(*_args, **_kwargs) -> dict[str, object]:
        calls["count"] += 1
        return {
            "inventory_qty": Decimal(0),
            "inventory_source": "positions",
            "base_currency": "BTC",
            "position_qty": Decimal(0),
            "spot_qty": Decimal(0),
            "global_position_qty": Decimal(0),
            "global_spot_qty": Decimal(0),
            "global_inventory_qty": Decimal(0),
            "global_inventory_source": "positions",
            "local_position_qty": Decimal(0),
            "local_spot_qty": Decimal(0),
            "local_inventory_qty": Decimal(0),
            "local_inventory_source": "positions",
            "des_qty_global": Decimal(0),
            "max_qty_global": Decimal(1),
            "max_skew_bps_global": Decimal(0),
            "des_qty_local": Decimal(0),
            "max_qty_local": Decimal(1),
            "max_skew_bps_local": Decimal(0),
            "linear_offset_bps": Decimal(0),
            "global_ratio": Decimal(0),
            "global_skew_bps": Decimal(0),
            "local_ratio": Decimal(0),
            "local_skew_bps": Decimal(0),
            "total_skew_bps": Decimal(0),
        }

    strategy._compute_inventory_skew = _compute_inventory_skew

    strategy._refresh_quotes(now_ns=1_090_000_000)
    strategy._refresh_quotes(now_ns=1_095_000_000)

    strategy.on_order_rejected(
        SimpleNamespace(
            client_order_id="CLIENT-1",
        ),
    )
    strategy._refresh_quotes(now_ns=1_096_000_000)

    assert calls["count"] == 2


def test_refresh_quotes_recomputes_inventory_skew_after_ttl_expiry(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1, 2, 3])
    strategy.INVENTORY_SKEW_CACHE_TTL_MS = 5
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_050_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_050_000_000
    strategy._managed_orders = list
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    calls = {"count": 0}

    def _compute_inventory_skew(*_args, **_kwargs) -> dict[str, object]:
        calls["count"] += 1
        return {
            "inventory_qty": Decimal(0),
            "inventory_source": "positions",
            "base_currency": "BTC",
            "position_qty": Decimal(0),
            "spot_qty": Decimal(0),
            "global_position_qty": Decimal(0),
            "global_spot_qty": Decimal(0),
            "global_inventory_qty": Decimal(0),
            "global_inventory_source": "positions",
            "local_position_qty": Decimal(0),
            "local_spot_qty": Decimal(0),
            "local_inventory_qty": Decimal(0),
            "local_inventory_source": "positions",
            "des_qty_global": Decimal(0),
            "max_qty_global": Decimal(1),
            "max_skew_bps_global": Decimal(0),
            "des_qty_local": Decimal(0),
            "max_qty_local": Decimal(1),
            "max_skew_bps_local": Decimal(0),
            "linear_offset_bps": Decimal(0),
            "global_ratio": Decimal(0),
            "global_skew_bps": Decimal(0),
            "local_ratio": Decimal(0),
            "local_skew_bps": Decimal(0),
            "total_skew_bps": Decimal(0),
        }

    strategy._compute_inventory_skew = _compute_inventory_skew

    strategy._refresh_quotes(now_ns=1_090_000_000)
    strategy._refresh_quotes(now_ns=1_092_000_000)
    strategy._refresh_quotes(now_ns=1_100_000_000)

    assert calls["count"] == 2


def test_refresh_quotes_exposes_split_inventory_fields_in_pricing_debug(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_001])
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000
    strategy._managed_orders = list
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._compute_inventory_skew = lambda *_args, **_kwargs: {
        "inventory_qty": Decimal(1004),
        "inventory_source": "positions_plus_spot",
        "base_currency": "PLUME",
        "position_qty": Decimal(1),
        "spot_qty": Decimal(1003),
        "global_position_qty": Decimal(1),
        "global_spot_qty": Decimal(1003),
        "global_inventory_qty": Decimal(1004),
        "global_inventory_source": "positions_plus_spot",
        "local_position_qty": Decimal(2),
        "local_spot_qty": Decimal(3),
        "local_inventory_qty": Decimal(5),
        "local_inventory_source": "positions_plus_spot",
        "des_qty_global": Decimal(0),
        "max_qty_global": Decimal(2000),
        "max_skew_bps_global": Decimal(5),
        "des_qty_local": Decimal(0),
        "max_qty_local": Decimal(10),
        "max_skew_bps_local": Decimal(9),
        "linear_offset_bps": Decimal(0),
        "global_ratio": Decimal("0.502"),
        "global_skew_bps": Decimal("2.51"),
        "local_ratio": Decimal("0.5"),
        "local_skew_bps": Decimal("4.5"),
        "total_skew_bps": Decimal("7.01"),
    }

    strategy._refresh_quotes(now_ns=1_000_000_000)

    skew = strategy._last_pricing_debug["skew"]
    assert skew["global_inventory_qty"] == "1004"
    assert skew["global_inventory_source"] == "positions_plus_spot"
    assert skew["local_inventory_qty"] == "5"
    assert skew["local_inventory_source"] == "positions_plus_spot"
    assert skew["global_position_qty"] == "1"
    assert skew["global_spot_qty"] == "1003"
    assert skew["local_position_qty"] == "2"
    assert skew["local_spot_qty"] == "3"


def test_refresh_quotes_short_inventory_moves_quotes_up_and_exports_component_sum(
    clocked_strategy_factory,
) -> None:
    def _configured_strategy() -> object:
        strategy = clocked_strategy_factory([1_000_000_001])
        strategy._maker_instrument = SimpleNamespace(
            price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
            make_price=lambda value: Decimal(str(value)),
        )
        strategy._order_qty = object()
        strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
        strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000
        strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000
        strategy._managed_orders = list
        strategy._rebalance_side = lambda **_kwargs: 0
        strategy._place_missing_levels = lambda **_kwargs: 0
        strategy._publish_json = lambda *_args, **_kwargs: None
        strategy._publish_event = lambda *_args, **_kwargs: None
        strategy._maker_base_currency_code = lambda: "PLUME"
        runtime_params = dict(strategy._quote_runtime_params_snapshot())
        runtime_params.update(
            {
                "des_qty_global": Decimal(0),
                "max_qty_global": Decimal(100),
                "max_skew_bps_global": Decimal(20),
                "des_qty_local": Decimal(0),
                "max_qty_local": Decimal(40),
                "max_skew_bps_local": Decimal(10),
                "linear_offset_bps": Decimal(1),
            }
        )
        strategy._quote_runtime_params_snapshot = lambda: runtime_params
        strategy._spot_balance_total = lambda _base_currency: Decimal(0)
        strategy._maker_local_spot_qty = lambda _base_currency: Decimal(0)
        return strategy

    flat_strategy = _configured_strategy()
    flat_strategy._position_exposure_summary = lambda _base_currency: inventory_mod.PositionExposureSummary(
        venue_qty=Decimal(0),
        base_qty=Decimal(0),
    )
    flat_strategy._maker_local_position_summary = lambda _base_currency: inventory_mod.PositionExposureSummary(
        venue_qty=Decimal(0),
        base_qty=Decimal(0),
    )
    flat_strategy._refresh_quotes(now_ns=1_000_000_000)

    short_strategy = _configured_strategy()
    short_strategy._position_exposure_summary = lambda _base_currency: inventory_mod.PositionExposureSummary(
        venue_qty=Decimal(-50),
        base_qty=Decimal(-50),
    )
    short_strategy._maker_local_position_summary = lambda _base_currency: inventory_mod.PositionExposureSummary(
        venue_qty=Decimal(-20),
        base_qty=Decimal(-20),
    )
    short_strategy._refresh_quotes(now_ns=1_000_000_000)

    baseline_pricing = flat_strategy._last_pricing_debug["pricing"]
    short_pricing = short_strategy._last_pricing_debug["pricing"]
    short_skew = short_strategy._last_pricing_debug["skew"]

    assert Decimal(short_pricing["total_skew_bps"]) > 0
    assert Decimal(short_pricing["place_bid"]) > Decimal(baseline_pricing["place_bid"])
    assert Decimal(short_pricing["place_ask"]) > Decimal(baseline_pricing["place_ask"])
    assert Decimal(short_skew["total_skew_bps"]) == (
        Decimal(short_skew["linear_offset_bps"])
        + Decimal(short_skew["global_skew_bps"])
        + Decimal(short_skew["local_skew_bps"])
    )


def test_refresh_quotes_exposes_l1_quote_targets_in_pricing_debug(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_001])
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000
    strategy._managed_orders = list
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=1_000_000_000)

    pricing = strategy._last_pricing_debug["pricing"]
    assert pricing["place_bid"] == "99.88"
    assert pricing["cancel_bid"] == "99.9"
    assert pricing["place_ask"] == "101.13"
    assert pricing["cancel_ask"] == "101.11"


def test_refresh_quotes_caches_coherent_quote_snapshot_for_operator_export(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_001])
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000
    strategy._managed_orders = list
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=1_000_000_000)

    pricing = strategy._last_pricing_debug["pricing"]
    snapshot = strategy._last_quote_snapshot
    assert snapshot["ts_ms"] == 1_000
    assert snapshot["maker_top_bid"] == pricing["maker_top_bid"]
    assert snapshot["maker_top_ask"] == pricing["maker_top_ask"]
    assert snapshot["ref_bid"] == pricing["ref_bid"]
    assert snapshot["ref_ask"] == pricing["ref_ask"]
    assert snapshot["place_bid"] == pricing["place_bid"]
    assert snapshot["place_ask"] == pricing["place_ask"]
    assert snapshot["cancel_bid"] == pricing["cancel_bid"]
    assert snapshot["cancel_ask"] == pricing["cancel_ask"]
    assert snapshot["eff_bid_edge_bps"] == pricing["bid_edge1_eff_bps"]
    assert snapshot["eff_ask_edge_bps"] == pricing["ask_edge1_eff_bps"]
    assert snapshot["skew_bps_signed"] == pricing["total_skew_bps"]
    assert snapshot["place_edge_bps"] == pricing["place_edge_bps"]


def test_refresh_quotes_stamps_pricing_debug_with_quote_cycle_timestamp(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1_000_000_001])
    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000
    strategy._managed_orders = list
    strategy._rebalance_side = lambda **_kwargs: 0
    strategy._place_missing_levels = lambda **_kwargs: 0
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None

    strategy._refresh_quotes(now_ns=1_000_000_000)

    pricing = strategy._last_pricing_debug["pricing"]
    assert pricing["ts_ms"] == 1000


def test_refresh_quotes_calls_managed_orders_once_per_quote_cycle(clocked_strategy_factory) -> None:
    strategy = clocked_strategy_factory([1_000_000_001, 1_000_000_002, 1_000_000_003])

    strategy._maker_instrument = SimpleNamespace(
        price_increment=SimpleNamespace(as_decimal=lambda: Decimal("0.01")),
        make_price=lambda value: Decimal(str(value)),
    )
    strategy._order_qty = object()
    strategy._best_bid_ask = lambda _instrument_id: (Decimal(100), Decimal(101))
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 1_000_000_000 - 10_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 1_000_000_000 - 10_000_000
    strategy._publish_json = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._rebalance_side = lambda **_kwargs: 1
    strategy._place_missing_levels = lambda **_kwargs: 0

    calls = {"count": 0}

    def _managed_orders() -> list[SimpleNamespace]:
        calls["count"] += 1
        return []

    strategy._managed_orders = _managed_orders

    strategy._refresh_quotes(now_ns=1_000_000_000)

    assert calls["count"] == 1


def test_allow_cash_borrowing_sell_only_policy_enables_only_spot_sells(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory(
        [1],
        maker_instrument_id=InstrumentId.from_str("PLUMEUSDT-SPOT.BYBIT"),
        spot_cash_borrowing_policy="sell_only",
    )
    assert strategy._should_allow_cash_borrowing(OrderSide.SELL) is True
    assert strategy._should_allow_cash_borrowing(OrderSide.BUY) is False


def test_allow_cash_borrowing_both_sides_policy_enables_spot_buys_and_sells(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory(
        [1],
        maker_instrument_id=InstrumentId.from_str("PLUMEUSDT-SPOT.BYBIT"),
        spot_cash_borrowing_policy="both_sides",
    )
    assert strategy._should_allow_cash_borrowing(OrderSide.SELL) is True
    assert strategy._should_allow_cash_borrowing(OrderSide.BUY) is True
