from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

from nautilus_trader.flux.strategies.makerv3 import failures as failures_mod
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


def _fake_order(
    *,
    client_order_id: str,
    price: str,
    side: OrderSide,
    ts_init: int = 0,
) -> SimpleNamespace:
    return SimpleNamespace(
        client_order_id=client_order_id,
        price=Decimal(price),
        side=side,
        ts_init=ts_init,
    )


def _cancel_rejected_event(*, reason: str, client_order_id: str = "RESTING-1") -> OrderCancelRejected:
    return OrderCancelRejected(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("SCALPER-001"),
        instrument_id=InstrumentId(Symbol("MAKER"), Venue("SIM")),
        client_order_id=ClientOrderId(client_order_id),
        venue_order_id=VenueOrderId("V-1"),
        account_id=TestIdStubs.account_id(),
        reason=reason,
        ts_event=1,
        event_id=UUID4(),
        ts_init=1,
    )


def test_rebalance_side_marks_cancel_requests_pending_without_removing_orders(
    strategy_factory,
) -> None:
    strategy = strategy_factory()

    canceled: list[str] = []
    strategy.cancel_order = lambda order: canceled.append(str(order.client_order_id))

    active_orders = [
        _fake_order(client_order_id="RESTING-1", price="100", side=OrderSide.BUY),
    ]

    cancel_count = strategy._rebalance_side(
        side=OrderSide.BUY,
        active_orders=active_orders,
        desired_levels=[(Decimal("99"), Decimal("99"), Decimal("0"))],
        now_ns=1,
        max_age_ms=100,
    )

    assert cancel_count == 1
    assert canceled == ["RESTING-1"]
    assert [order.client_order_id for order in active_orders] == ["RESTING-1"]
    assert strategy._pending_cancel_client_order_ids == {"RESTING-1"}


def test_place_missing_levels_skips_replacement_while_cancel_is_pending(
    strategy_factory,
    monkeypatch,
) -> None:
    strategy = strategy_factory()
    strategy._order_qty = object()
    strategy._pending_cancel_client_order_ids = {"RESTING-1"}
    strategy._register_managed_order = lambda _order: None

    placed: list[str] = []
    strategy.submit_order = lambda order, **_kwargs: placed.append(str(order.client_order_id))

    fake_factory = SimpleNamespace(
        limit=lambda **kwargs: SimpleNamespace(
            client_order_id="NEW-1",
            price=kwargs["price"],
            side=kwargs["order_side"],
        ),
    )
    monkeypatch.setattr(type(strategy), "order_factory", property(lambda _self: fake_factory))

    place_count = strategy._place_missing_levels(
        side=OrderSide.BUY,
        active_orders=[
            _fake_order(client_order_id="RESTING-1", price="100", side=OrderSide.BUY),
        ],
        desired_levels=[(Decimal("99"), Decimal("99"), Decimal("0"))],
        best_bid_px=Decimal("98"),
        best_ask_px=Decimal("101"),
    )

    assert place_count == 0
    assert placed == []


def test_place_missing_levels_skips_replacement_while_cancel_reject_cooldown_is_active(
    strategy_factory,
    monkeypatch,
) -> None:
    strategy = strategy_factory()
    strategy._order_qty = object()
    strategy._cancel_reject_retry_after_ns_by_client_order_id = {"RESTING-1": 2}
    strategy._register_managed_order = lambda _order: None

    placed: list[str] = []
    strategy.submit_order = lambda order, **_kwargs: placed.append(str(order.client_order_id))

    fake_factory = SimpleNamespace(
        limit=lambda **kwargs: SimpleNamespace(
            client_order_id="NEW-1",
            price=kwargs["price"],
            side=kwargs["order_side"],
        ),
    )
    fake_clock = SimpleNamespace(timestamp_ns=lambda: 1)
    monkeypatch.setattr(type(strategy), "order_factory", property(lambda _self: fake_factory))
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))

    place_count = strategy._place_missing_levels(
        side=OrderSide.BUY,
        active_orders=[
            _fake_order(client_order_id="RESTING-1", price="100", side=OrderSide.BUY),
        ],
        desired_levels=[(Decimal("99"), Decimal("99"), Decimal("0"))],
        best_bid_px=Decimal("98"),
        best_ask_px=Decimal("101"),
    )

    assert place_count == 0
    assert placed == []


def test_order_rejected_rate_limit_triggers_venue_protection_circuit(strategy_factory) -> None:
    strategy = strategy_factory(cancel_all_instrument_orders=True)

    canceled: list[tuple[str, bool, bool | None]] = []
    alerts: list[dict[str, object]] = []
    states: list[str] = []
    stopped: list[bool] = []
    events: list[tuple[str, dict[str, object]]] = []

    strategy._cancel_managed_quotes = lambda reason, force=False, **kwargs: canceled.append(
        (reason, force, kwargs.get("allow_instrument_cancel")),
    )
    strategy._publish_actionable_alert = lambda **kwargs: alerts.append(kwargs) or True
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_event = lambda name, **payload: events.append((name, payload))
    strategy.stop_immediately = lambda: stopped.append(True)

    raw_reason = "Too many visits. Exceeded the API Rate Limit."
    strategy.on_order_rejected(
        SimpleNamespace(
            client_order_id="RESTING-1",
            instrument_id=strategy.config.maker_instrument_id,
            reason=raw_reason,
            due_post_only=False,
            ts_event=1,
        ),
    )

    assert stopped == [True]
    assert canceled == [("venue_protection_circuit_breaker", True, False)]
    assert states[-1] == "blocked_venue_protection"
    assert alerts[-1]["alert_key"] == "venue_protection_circuit_breaker"
    assert alerts[-1]["source_event"] == "order_rejected"
    assert alerts[-1]["raw_reason"] == raw_reason
    assert events[-1][0] == "venue_protection_circuit_breaker"
    assert events[-1][1]["raw_reason"] == raw_reason


def test_order_cancel_rejected_nonfatal_reason_sets_retry_cooldown_and_alerts_burst(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._pending_cancel_client_order_ids = {"RESTING-1"}
    strategy._runtime_params["order_reject_alert_after_count"] = 1
    strategy._runtime_params["order_reject_alert_after_s"] = Decimal(10)

    stopped: list[bool] = []
    alerts: list[dict[str, object]] = []
    strategy.stop_immediately = lambda: stopped.append(True)
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_actionable_alert = lambda **kwargs: alerts.append(kwargs) or True
    strategy._publish_state = lambda *_args, **_kwargs: None
    strategy._cancel_managed_quotes = lambda *_args, **_kwargs: None

    strategy.on_order_cancel_rejected(_cancel_rejected_event(reason="temporarily unavailable"))

    assert strategy._pending_cancel_client_order_ids == set()
    assert strategy._cancel_reject_retry_after_ns_by_client_order_id["RESTING-1"] > 1
    assert alerts[-1]["alert_key"] == "order_rejected_burst"
    assert "order_cancel_rejected" in str(alerts[-1]["message"])
    assert stopped == []


def test_rebalance_side_skips_repeat_cancel_while_cancel_reject_cooldown_is_active(
    strategy_factory,
) -> None:
    strategy = strategy_factory()
    strategy._cancel_reject_retry_after_ns_by_client_order_id = {"RESTING-1": 10}

    canceled: list[str] = []
    strategy.cancel_order = lambda order: canceled.append(str(order.client_order_id))

    cancel_count = strategy._rebalance_side(
        side=OrderSide.BUY,
        active_orders=[
            _fake_order(client_order_id="RESTING-1", price="100", side=OrderSide.BUY),
        ],
        desired_levels=[(Decimal("99"), Decimal("99"), Decimal("0"))],
        now_ns=1,
        max_age_ms=100,
    )

    assert cancel_count == 0
    assert canceled == []


def test_order_cancel_rejected_order_limit_triggers_venue_protection_circuit(
    strategy_factory,
) -> None:
    strategy = strategy_factory(cancel_all_instrument_orders=True)
    strategy._pending_cancel_client_order_ids = {"RESTING-1"}

    canceled: list[tuple[str, bool, bool | None]] = []
    alerts: list[dict[str, object]] = []
    states: list[str] = []
    stopped: list[bool] = []

    strategy._cancel_managed_quotes = lambda reason, force=False, **kwargs: canceled.append(
        (reason, force, kwargs.get("allow_instrument_cancel")),
    )
    strategy._publish_actionable_alert = lambda **kwargs: alerts.append(kwargs) or True
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy.stop_immediately = lambda: stopped.append(True)

    strategy.on_order_cancel_rejected(
        _cancel_rejected_event(reason="number of active orders great than limit"),
    )

    assert strategy._pending_cancel_client_order_ids == set()
    assert stopped == [True]
    assert canceled == [("venue_protection_circuit_breaker", True, False)]
    assert states[-1] == "blocked_venue_protection"
    assert alerts[-1]["alert_key"] == "venue_protection_circuit_breaker"


def test_venue_protection_reason_matches_bare_429() -> None:
    assert failures_mod.is_venue_protection_reason("429") is True


def test_on_start_cancels_existing_claimed_orders_with_managed_only_scope(
    strategy_factory,
    monkeypatch,
) -> None:
    strategy = strategy_factory()
    existing_order = _fake_order(
        client_order_id="RESTING-1",
        price="100",
        side=OrderSide.BUY,
    )

    cancel_calls: list[tuple[str, bool, bool | None]] = []
    immediate_stop_requests: list[bool] = []
    states: list[str] = []

    strategy._managed_orders = lambda: [existing_order]
    strategy._cancel_managed_quotes = lambda reason, force=False, **kwargs: cancel_calls.append(
        (reason, force, kwargs.get("allow_instrument_cancel")),
    )
    strategy.request_immediate_stop = lambda value=True: immediate_stop_requests.append(bool(value))
    strategy._publish_alert = lambda *_args, **_kwargs: None
    strategy._publish_event = lambda *_args, **_kwargs: None
    strategy._publish_balances = lambda: None
    strategy._publish_portfolio_inventory_component = lambda *_args, **_kwargs: None
    strategy._publish_state = lambda state, **_kwargs: states.append(state)
    strategy.subscribe_order_book_deltas = lambda *_args, **_kwargs: None

    fake_cache = SimpleNamespace(
        order=lambda _client_order_id: None,
        instrument=lambda instrument_id: SimpleNamespace(
            price_precision=6,
            raw_symbol=str(instrument_id).split(".", maxsplit=1)[0],
            make_qty=lambda value: value,
        ),
    )
    fake_clock = SimpleNamespace(
        timestamp_ns=lambda: 1_700_000_000_000_000_000,
        set_timer=lambda **_kwargs: None,
        timer_names=set(),
        cancel_timer=lambda _name: None,
    )
    monkeypatch.setattr(type(strategy), "cache", property(lambda _self: fake_cache))
    monkeypatch.setattr(type(strategy), "clock", property(lambda _self: fake_clock))

    strategy.on_start()

    assert cancel_calls == [("startup_cleanup", False, False)]
    assert immediate_stop_requests == [False, True]
    assert states[-1] == "blocked_startup_cleanup"
    assert strategy._startup_cleanup_pending is True
