from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace


def test_cancel_managed_quotes_idempotency_with_tracked_ids_and_cache_visibility(
    strategy_factory,
) -> None:
    strategy = strategy_factory()

    cached_order = SimpleNamespace(client_order_id="RESTING-1")
    snapshots: list[list[SimpleNamespace]] = [[cached_order], [], []]
    strategy._managed_orders = lambda: snapshots.pop(0)
    strategy._managed_client_order_ids = {"RESTING-1"}

    canceled_orders: list[str] = []
    canceled_all: list[str] = []
    events: list[tuple[str, dict[str, object]]] = []
    strategy.cancel_order = lambda order: canceled_orders.append(order.client_order_id)
    strategy.cancel_all_orders = lambda instrument_id: canceled_all.append(str(instrument_id))
    strategy._publish_event = lambda event, **kwargs: events.append((event, kwargs))

    strategy._cancel_managed_quotes("stale")
    assert strategy._managed_client_order_ids == {"RESTING-1"}

    strategy._cancel_managed_quotes("stale")
    strategy._cancel_managed_quotes("stale")

    assert canceled_orders == ["RESTING-1"]
    assert canceled_all == []
    assert [name for name, _ in events] == ["quotes_canceled", "quotes_canceled", "quotes_canceled"]
    assert events[0][1]["cancel_attempts"] == 1
    assert events[0][1]["cancel_exceptions"] == 0
    assert events[0][1]["cancel_success"] == 1
    assert events[0][1]["cancel_all_instrument"] is False
    assert events[1][1]["cancel_attempts"] == 0
    assert events[1][1]["cancel_exceptions"] == 0
    assert events[1][1]["cancel_success"] == 0
    assert events[1][1]["cancel_all_instrument"] is False
    assert events[2][1]["cancel_attempts"] == 0
    assert events[2][1]["cancel_exceptions"] == 0
    assert events[2][1]["cancel_success"] == 0
    assert events[2][1]["cancel_all_instrument"] is False
    assert strategy._managed_client_order_ids == {"RESTING-1"}


def test_cancel_managed_quotes_escape_hatch_can_cancel_all_instrument_orders(
    strategy_factory,
) -> None:
    strategy = strategy_factory(cancel_all_instrument_orders=True)

    strategy._managed_orders = lambda: [SimpleNamespace(client_order_id="RESTING-1")]
    strategy._managed_client_order_ids = {"RESTING-1"}

    canceled_orders: list[str] = []
    canceled_all: list[str] = []
    events: list[tuple[str, dict[str, object]]] = []
    strategy.cancel_order = lambda order: canceled_orders.append(order.client_order_id)
    strategy.cancel_all_orders = lambda instrument_id: canceled_all.append(str(instrument_id))
    strategy._publish_event = lambda event, **kwargs: events.append((event, kwargs))

    strategy._cancel_managed_quotes("stale")

    assert canceled_orders == ["RESTING-1"]
    assert canceled_all == [str(strategy.config.maker_instrument_id)]
    assert [name for name, _ in events] == ["quotes_canceled"]
    assert events[0][1]["cancel_all_instrument"] is True


def test_on_start_resets_restart_latches(strategy_factory) -> None:
    maker_id = strategy_factory().config.maker_instrument_id
    strategy = strategy_factory(reference_instrument_id=maker_id)
    strategy._runtime_params_failed = True
    strategy._quote_failure_circuit_open = True
    strategy._quote_failures_ns = [1, 2]
    strategy._last_stale_cancel_ns = 123
    strategy._last_state_name = "blocked_reference_md"
    strategy._state_is_blocked = True
    strategy._last_actionable_alert_ns = {"alert": 1}
    strategy._last_actionable_alert_transition = {"alert": "old->new"}

    strategy._publish_alert = lambda *_args, **_kwargs: None
    stopped: list[bool] = []
    strategy.stop = lambda: stopped.append(True)

    strategy.on_start()

    assert stopped == [True]
    assert strategy._runtime_params_failed is False
    assert strategy._quote_failure_circuit_open is False
    assert strategy._quote_failures_ns == []
    assert strategy._last_stale_cancel_ns == 0
    assert strategy._last_state_name is None
    assert strategy._state_is_blocked is False
    assert strategy._last_actionable_alert_ns == {}
    assert strategy._last_actionable_alert_transition == {}


def test_on_start_rejects_duplicate_instrument_ids(strategy_factory) -> None:
    maker_id = strategy_factory().config.maker_instrument_id
    strategy = strategy_factory(reference_instrument_id=maker_id)
    published: list[str] = []
    strategy._publish_alert = lambda message, **_kwargs: published.append(str(message))
    stopped: list[bool] = []
    strategy.stop = lambda: stopped.append(True)

    strategy.on_start()

    assert stopped == [True]
    assert published
    assert "distinct" in published[-1].lower()


def test_cancel_managed_quotes_records_cancel_all_exception_fields(strategy_factory) -> None:
    strategy = strategy_factory(cancel_all_instrument_orders=True)

    strategy._managed_orders = lambda: [SimpleNamespace(client_order_id="RESTING-1")]
    strategy._managed_client_order_ids = {"RESTING-1"}

    canceled_orders: list[str] = []
    events: list[tuple[str, dict[str, object]]] = []
    strategy.cancel_order = lambda order: canceled_orders.append(order.client_order_id)

    def _cancel_all_raises(_instrument_id: object) -> None:
        raise RuntimeError("cancel_all failed")

    strategy.cancel_all_orders = _cancel_all_raises
    strategy._publish_event = lambda event, **kwargs: events.append((event, kwargs))

    strategy._cancel_managed_quotes("stale")

    assert canceled_orders == ["RESTING-1"]
    assert [name for name, _ in events] == ["quotes_canceled"]
    assert events[0][1]["cancel_all_attempted"] is True
    assert events[0][1]["cancel_all_exceptions"] == 1
    assert events[0][1]["cancel_exceptions"] == 0


def test_cancel_managed_quotes_aggregates_cancel_order_exceptions_in_single_event(
    strategy_factory,
) -> None:
    strategy = strategy_factory()

    strategy._managed_orders = lambda: [
        SimpleNamespace(client_order_id="FAIL-1"),
        SimpleNamespace(client_order_id="OK-1"),
    ]
    strategy._managed_client_order_ids = {"FAIL-1", "OK-1"}

    canceled_orders: list[str] = []
    events: list[tuple[str, dict[str, object]]] = []

    def _cancel_order(order: SimpleNamespace) -> None:
        if order.client_order_id == "FAIL-1":
            raise RuntimeError("cancel failed")
        canceled_orders.append(order.client_order_id)

    strategy.cancel_order = _cancel_order
    strategy._publish_event = lambda event, **kwargs: events.append((event, kwargs))

    strategy._cancel_managed_quotes("stale")

    assert canceled_orders == ["OK-1"]
    assert len(events) == 1
    assert events[0][0] == "quotes_canceled"
    assert events[0][1]["cancel_attempts"] == 2
    assert events[0][1]["cancel_exceptions"] == 1
    assert events[0][1]["cancel_success"] == 1
    assert events[0][1]["cancel_all_instrument"] is False


def test_publish_state_transition_events_only_on_blocked_boundary_crossings(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1, 2, 3, 4, 5, 6])

    transition_events: list[tuple[str, dict[str, object]]] = []
    strategy._managed_orders = list
    strategy._publish_event = lambda event, **kwargs: transition_events.append((event, kwargs))
    strategy._publish_json = lambda *_args, **_kwargs: None

    strategy._publish_state("running")
    strategy._publish_state("blocked_maker_md")
    strategy._publish_state("blocked_reference_md")
    strategy._publish_state("blocked_reference_md")
    strategy._publish_state("running")
    strategy._publish_state("running")

    assert [name for name, _ in transition_events] == ["state_transition", "state_transition"]
    assert transition_events[0][1] == {
        "from_state": "running",
        "to_state": "blocked_maker_md",
        "from_blocked": False,
        "to_blocked": True,
    }
    assert transition_events[1][1] == {
        "from_state": "blocked_reference_md",
        "to_state": "running",
        "from_blocked": True,
        "to_blocked": False,
    }


def test_publish_state_if_due_does_not_emit_running_while_blocked(clocked_strategy_factory) -> None:
    strategy = clocked_strategy_factory([1, 300_000_000])

    transition_events: list[tuple[str, dict[str, object]]] = []
    strategy._managed_orders = list
    strategy._publish_event = lambda event, **kwargs: transition_events.append((event, kwargs))
    strategy._publish_json = lambda *_args, **_kwargs: None

    strategy._publish_state("blocked_reference_md")
    strategy._publish_state_if_due()

    assert [name for name, _ in transition_events] == ["state_transition"]
    assert strategy._last_state_name == "blocked_reference_md"


def test_publish_state_resets_stale_cancel_cooldown_when_leaving_blocked(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([1, 2])

    strategy._managed_orders = list
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_json = lambda *_args, **_kwargs: None

    strategy._publish_state("blocked_reference_md")
    strategy._last_stale_cancel_ns = 123_000_000
    strategy._publish_state("running")

    assert strategy._last_stale_cancel_ns == 0


def test_timer_enforces_stale_market_data_blocks_when_feed_goes_silent(
    clocked_strategy_factory,
) -> None:
    strategy = clocked_strategy_factory([500_000_000])
    strategy._refresh_runtime_params = lambda **_kwargs: None
    strategy._effective_bot_on = lambda: True
    strategy._managed_client_order_ids = {"RESTING-1"}
    strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id] = 100_000_000
    strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id] = 100_000_000

    canceled: list[str] = []
    states: list[str] = []
    strategy._managed_orders = lambda: [SimpleNamespace(client_order_id="RESTING-1")]
    strategy._cancel_managed_quotes = lambda reason, **_kwargs: canceled.append(reason)
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_quote_cycle_event = lambda **_kwargs: None
    strategy._publish_actionable_alert = lambda **_kwargs: None

    strategy.on_time_event(SimpleNamespace(name=strategy._params_timer_name))

    assert canceled == ["maker_md_stale"]
    assert states[-1] == "blocked_maker_md"


def test_timer_triggers_balances_publish_check(clocked_strategy_factory) -> None:
    strategy = clocked_strategy_factory([500_000_000])
    strategy._refresh_runtime_params = lambda **_kwargs: None
    strategy._last_bot_on = False
    strategy._effective_bot_on = lambda: False

    balance_checks: list[str] = []
    strategy._publish_balances_if_due = lambda: balance_checks.append("called")

    strategy.on_time_event(SimpleNamespace(name=strategy._params_timer_name))

    assert balance_checks == ["called"]


def test_timer_publishes_balances_when_due_after_startup_snapshot(clocked_strategy_factory) -> None:
    interval_ns = 10_000 * 1_000_000
    startup_publish_ns = 1_000_000_000
    not_due_ns = startup_publish_ns + interval_ns - 1
    due_ns = startup_publish_ns + interval_ns
    strategy = clocked_strategy_factory([not_due_ns, not_due_ns, due_ns, due_ns])
    strategy._refresh_runtime_params = lambda **_kwargs: None
    strategy._last_bot_on = False
    strategy._effective_bot_on = lambda: False

    publish_calls: list[str] = []
    strategy._last_balances_ns = startup_publish_ns
    strategy._publish_balances = lambda: publish_calls.append("publish")
    strategy.on_time_event(SimpleNamespace(name=strategy._params_timer_name))

    assert publish_calls == []

    strategy.on_time_event(SimpleNamespace(name=strategy._params_timer_name))

    assert publish_calls == ["publish"]


def test_lifecycle_handlers_reconcile_local_managed_order_state(strategy_factory) -> None:
    strategy = strategy_factory()
    strategy._managed_client_order_ids = {"A", "B", "C"}

    strategy.on_order_rejected(SimpleNamespace(client_order_id="A"))
    strategy.on_order_canceled(SimpleNamespace(client_order_id="B"))
    strategy.on_order_expired(SimpleNamespace(client_order_id="C"))

    assert strategy._managed_client_order_ids == set()


def test_order_filled_reconciles_managed_tracking_without_cache_closed(strategy_factory) -> None:
    strategy = strategy_factory()
    strategy._managed_client_order_ids = {"A"}

    published: list[tuple[str, dict[str, object]]] = []
    strategy._publish_event = lambda name, **payload: published.append((name, payload))
    strategy._publish_json = lambda *_args, **_kwargs: None

    strategy.on_order_filled(
        SimpleNamespace(
            instrument_id=strategy.config.maker_instrument_id,
            client_order_id="A",
            trade_id="T1",
            order_side="BUY",
            last_qty=Decimal(1),
            last_px=Decimal(100),
            ts_event=123,
        ),
    )

    assert strategy._managed_client_order_ids == set()
    assert (
        "order_lifecycle",
        {"lifecycle": "filled", "client_order_id": "A", "tracked_before": True, "tracked_after": 0},
    ) in published


def test_quote_failure_circuit_breaker_triggers_stop(strategy_factory) -> None:
    strategy = strategy_factory()

    canceled: list[tuple[str, bool]] = []
    states: list[str] = []
    stopped: list[bool] = []
    strategy._cancel_managed_quotes = lambda reason, force=False, **_kwargs: canceled.append(
        (reason, force),
    )
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy.stop = lambda: stopped.append(True)

    strategy._handle_quote_failure(now_ns=1_000_000_000, exc=RuntimeError("boom-1"), context="test")
    strategy._handle_quote_failure(now_ns=2_000_000_000, exc=RuntimeError("boom-2"), context="test")

    assert stopped == [True]
    assert canceled[-1] == ("quote_fail_circuit_breaker", True)
    assert states[-1] == "blocked_quote_failures"


def test_quote_failure_circuit_breaker_stops_even_if_side_effects_raise(
    strategy_factory,
    raise_runtime_error,
) -> None:
    strategy = strategy_factory()

    strategy._runtime_params["quote_fail_critical_after_count"] = 1
    strategy._publish_event = raise_runtime_error
    strategy._publish_alert = raise_runtime_error
    strategy._publish_state = raise_runtime_error
    strategy._cancel_managed_quotes = raise_runtime_error

    stopped: list[bool] = []
    strategy.stop = lambda: stopped.append(True)

    strategy._handle_quote_failure(now_ns=1_000_000_000, exc=RuntimeError("boom"), context="test")

    assert strategy._quote_failure_circuit_open is True
    assert stopped == [True]


def test_on_stop_clears_tracked_ids_without_cancel_all_by_default(strategy_factory) -> None:
    strategy = strategy_factory()

    strategy._managed_client_order_ids = {"RESTING-1"}
    strategy._managed_orders = list

    canceled_all: list[str] = []
    states: list[str] = []
    strategy.cancel_all_orders = lambda instrument_id: canceled_all.append(str(instrument_id))
    strategy._publish_state = lambda state: states.append(state)

    strategy.on_stop()
    strategy.on_stop()

    assert canceled_all == []
    assert strategy._managed_client_order_ids == set()
    assert states == ["on_stop", "on_stop"]


def test_cancel_managed_quotes_honors_cancel_all_escape_hatch_without_local_state(
    strategy_factory,
) -> None:
    strategy = strategy_factory(cancel_all_instrument_orders=True)
    strategy._managed_orders = list
    strategy._managed_client_order_ids = set()

    canceled_all: list[str] = []
    events: list[tuple[str, dict[str, object]]] = []
    strategy.cancel_all_orders = lambda instrument_id: canceled_all.append(str(instrument_id))
    strategy._publish_event = lambda event, **kwargs: events.append((event, kwargs))

    strategy._cancel_managed_quotes("stale")

    assert canceled_all == [str(strategy.config.maker_instrument_id)]
    assert [name for name, _ in events] == ["quotes_canceled"]
    assert events[0][1]["cancel_all_instrument"] is True
    assert events[0][1]["cancel_attempts"] == 0
    assert events[0][1]["tracked_count"] == 0
