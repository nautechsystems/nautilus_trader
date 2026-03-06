from __future__ import annotations

from types import SimpleNamespace

from nautilus_trader.flux.strategies.makerv3.managed_orders import CANCELLATION_SAFETY_INVARIANT
from nautilus_trader.flux.strategies.makerv3.managed_orders import cancel_managed_quotes
from nautilus_trader.flux.strategies.makerv3.managed_orders import collect_managed_orders
from nautilus_trader.flux.strategies.makerv3.managed_orders import reconcile_managed_order
from nautilus_trader.flux.strategies.makerv3.managed_orders import register_managed_order


def test_collect_managed_orders_dedupes_open_and_inflight_entries() -> None:
    open_order = SimpleNamespace(
        client_order_id="OPEN-1",
        venue_order_id="V-1",
        side="BUY",
        price="100",
        quantity="1",
        ts_init=1,
        is_closed=False,
    )
    inflight_dup = SimpleNamespace(
        client_order_id="OPEN-1",
        venue_order_id="V-1",
        side="BUY",
        price="100",
        quantity="1",
        ts_init=1,
        is_closed=False,
    )
    closed_order = SimpleNamespace(
        client_order_id="CLOSED-1",
        venue_order_id="V-2",
        side="SELL",
        price="101",
        quantity="1",
        ts_init=2,
        is_closed=True,
    )

    cache = SimpleNamespace(
        orders_open=lambda **_kwargs: [open_order, closed_order],
        orders_inflight=lambda **_kwargs: [inflight_dup],
    )

    rows = collect_managed_orders(cache=cache, instrument_id="MAKER.SIM", strategy_id="STRAT-1")

    assert rows == [open_order]


def test_collect_managed_orders_handles_none_sources() -> None:
    cache = SimpleNamespace(
        orders_open=lambda **_kwargs: None,
        orders_inflight=lambda **_kwargs: None,
    )

    rows = collect_managed_orders(cache=cache, instrument_id="MAKER.SIM", strategy_id="STRAT-1")

    assert rows == []


def test_register_and_reconcile_managed_order_tracking() -> None:
    tracked_ids: set[str] = set()

    register_managed_order(tracked_ids, SimpleNamespace(client_order_id="CLIENT-1"))
    had_order = reconcile_managed_order(tracked_ids, "CLIENT-1")
    still_had_order = reconcile_managed_order(tracked_ids, "CLIENT-1")

    assert had_order is True
    assert still_had_order is False
    assert tracked_ids == set()


def test_cancel_managed_quotes_defaults_to_strategy_scoped_cancellation() -> None:
    tracked_ids = {"CLIENT-1"}
    managed_orders = [SimpleNamespace(client_order_id="CLIENT-1")]
    canceled_orders: list[str] = []
    canceled_all: list[str] = []

    result = cancel_managed_quotes(
        reason="stale",
        force=False,
        tracked_ids=tracked_ids,
        managed_orders=managed_orders,
        maker_instrument_id="MAKER.SIM",
        cancel_order=lambda order: canceled_orders.append(order.client_order_id),
        cancel_all_orders=lambda instrument_id: canceled_all.append(str(instrument_id)),
        cancel_all_instrument_orders=False,
    )

    assert canceled_orders == ["CLIENT-1"]
    assert canceled_all == []
    assert result.cancel_all_instrument is False
    assert tracked_ids == {"CLIENT-1"}


def test_cancel_managed_quotes_escape_hatch_allows_instrument_cancel() -> None:
    canceled_all: list[str] = []

    result = cancel_managed_quotes(
        reason="stale",
        force=False,
        tracked_ids=set(),
        managed_orders=[],
        maker_instrument_id="MAKER.SIM",
        cancel_order=lambda _order: None,
        cancel_all_orders=lambda instrument_id: canceled_all.append(str(instrument_id)),
        cancel_all_instrument_orders=True,
    )

    assert canceled_all == ["MAKER.SIM"]
    assert result.cancel_all_instrument is True
    assert result.cancel_attempts == 0
    assert result.cancel_success == 0


def test_cancel_managed_quotes_cancel_all_exception_is_accounted_without_aborting() -> None:
    canceled_orders: list[str] = []

    result = cancel_managed_quotes(
        reason="stale",
        force=False,
        tracked_ids={"CLIENT-1"},
        managed_orders=[SimpleNamespace(client_order_id="CLIENT-1")],
        maker_instrument_id="MAKER.SIM",
        cancel_order=lambda order: canceled_orders.append(order.client_order_id),
        cancel_all_orders=lambda _instrument_id: (_ for _ in ()).throw(RuntimeError("boom")),
        cancel_all_instrument_orders=True,
    )

    assert canceled_orders == ["CLIENT-1"]
    assert result.cancel_all_attempted is True
    assert result.cancel_all_exceptions == 1
    assert result.cancel_exceptions == 0


def test_cancel_managed_quotes_clears_tracked_ids_on_forced_stop() -> None:
    tracked_ids = {"CLIENT-1"}

    cancel_managed_quotes(
        reason="on_stop",
        force=True,
        tracked_ids=tracked_ids,
        managed_orders=[],
        maker_instrument_id="MAKER.SIM",
        cancel_order=lambda _order: None,
        cancel_all_orders=lambda _instrument_id: None,
        cancel_all_instrument_orders=False,
    )

    assert tracked_ids == set()


def test_cancel_managed_quotes_does_not_clear_tracked_ids_on_empty_cache() -> None:
    tracked_ids = {"CLIENT-1"}

    cancel_managed_quotes(
        reason="stale",
        force=False,
        tracked_ids=tracked_ids,
        managed_orders=[],
        maker_instrument_id="MAKER.SIM",
        cancel_order=lambda _order: None,
        cancel_all_orders=lambda _instrument_id: None,
        cancel_all_instrument_orders=False,
    )

    assert tracked_ids == {"CLIENT-1"}


def test_cancellation_safety_invariant_is_explicit() -> None:
    assert "only strategy-managed orders" in CANCELLATION_SAFETY_INVARIANT.lower()
    assert "explicit opt-in" in CANCELLATION_SAFETY_INVARIANT.lower()
