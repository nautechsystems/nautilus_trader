from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_REFERENCE_MD_STALE


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
        "inventory_qty": Decimal("1004"),
        "inventory_source": "positions_plus_spot",
        "base_currency": "PLUME",
        "position_qty": Decimal("1"),
        "spot_qty": Decimal("1003"),
        "global_position_qty": Decimal("1"),
        "global_spot_qty": Decimal("1003"),
        "global_inventory_qty": Decimal("1004"),
        "global_inventory_source": "positions_plus_spot",
        "local_position_qty": Decimal("2"),
        "local_spot_qty": Decimal("3"),
        "local_inventory_qty": Decimal("5"),
        "local_inventory_source": "positions_plus_spot",
        "des_qty_global": Decimal(0),
        "max_qty_global": Decimal("2000"),
        "max_skew_bps_global": Decimal("5"),
        "des_qty_local": Decimal(0),
        "max_qty_local": Decimal("10"),
        "max_skew_bps_local": Decimal("9"),
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
