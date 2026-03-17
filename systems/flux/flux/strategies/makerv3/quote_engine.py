"""
Run MakerV3 quote-cycle refresh logic and stale-data safety gates.
"""

from __future__ import annotations

from decimal import Decimal
from typing import TYPE_CHECKING
from typing import Any

from flux.strategies.makerv3 import pricing as pricing_mod
from flux.strategies.makerv3 import publisher as publisher_mod
from flux.strategies.makerv3 import rebalancing as rebalancing_mod
from flux.strategies.makerv3.constants import ALERT_COOLDOWN_BLOCKED_MS
from flux.strategies.makerv3.constants import ALERT_KEY_MARKET_DATA_BLOCKED
from flux.strategies.makerv3.constants import ALERT_KEY_PORTFOLIO_INVENTORY_BLOCKED
from flux.strategies.makerv3.constants import ALERT_KEY_QUOTE_LIVENESS_BLOCKED
from flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_BLOCKED
from flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_COMPLETED
from flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_SKIPPED
from flux.strategies.makerv3.constants import REASON_CANCEL_MAKER_BOOK_UNAVAILABLE
from flux.strategies.makerv3.constants import REASON_CANCEL_MAKER_MD_STALE
from flux.strategies.makerv3.constants import REASON_CANCEL_NO_TARGETS
from flux.strategies.makerv3.constants import REASON_CANCEL_REFERENCE_MD_STALE
from flux.strategies.makerv3.constants import REASON_BLOCKED_STARTUP_CLEANUP
from flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_BOOK_UNAVAILABLE
from flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_MD_STALE
from flux.strategies.makerv3.constants import REASON_BLOCKED_PENDING_CANCEL
from flux.strategies.makerv3.constants import REASON_BLOCKED_PORTFOLIO_INVENTORY_UNAVAILABLE
from flux.strategies.makerv3.constants import REASON_BLOCKED_REFERENCE_MD_STALE
from flux.strategies.makerv3.constants import REASON_COMPLETED_NO_ACTIONS
from flux.strategies.makerv3.constants import REASON_COMPLETED_NO_TARGETS
from flux.strategies.makerv3.constants import REASON_COMPLETED_REBALANCED
from flux.strategies.makerv3.constants import REASON_SKIPPED_CANCEL_REJECT_COOLDOWN
from flux.strategies.makerv3.constants import REASON_SKIPPED_PENDING_CANCELS
from flux.strategies.makerv3.wire import QuoteCycleContext
from flux.strategies.shared.quote_health import evaluate_quote_health
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price


if TYPE_CHECKING:
    from flux.strategies.makerv3.strategy import MakerV3Strategy


_to_decimal = pricing_mod.to_decimal
_price_to_decimal = pricing_mod.price_to_decimal
_round_price_to_tick = pricing_mod.round_price_to_tick
_clamp_post_only_price = pricing_mod.clamp_post_only_price
_nudge_unique_price = pricing_mod.nudge_unique_price
_apply_inventory_skew_to_edges = pricing_mod.apply_inventory_skew_to_edges
build_ladder_place_cancel_levels_from_bps = pricing_mod.build_ladder_place_cancel_levels_from_bps

_decimal_to_json_str = publisher_mod.decimal_to_json_str

_BACKLOG_MODE_RANK = {
    "normal": 0,
    "soft_throttle": 1,
    "hard_freeze": 2,
    "blocked": 3,
}


def _pending_cancel_backlog_snapshot(
    strategy: MakerV3Strategy,
    *,
    now_ns: int,
) -> dict[str, Any]:
    pending_cancel_first_seen_ns = getattr(
        strategy,
        "_pending_cancel_first_seen_ns_by_client_order_id",
        {},
    )
    counts_by_side = {
        OrderSide.BUY: 0,
        OrderSide.SELL: 0,
    }
    oldest_age_ms_by_side = {
        OrderSide.BUY: None,
        OrderSide.SELL: None,
    }
    total_oldest_age_ms: int | None = None
    unknown_side_count = 0

    pending_ids = tuple(getattr(strategy, "_pending_cancel_client_order_ids", ()))
    pending_order = getattr(strategy, "_pending_cancel_order", None)
    for client_order_id in pending_ids:
        first_seen_ns = int(pending_cancel_first_seen_ns.get(client_order_id, 0) or 0)
        age_ms = (
            max(0, (now_ns - first_seen_ns) // 1_000_000)
            if first_seen_ns > 0 and now_ns >= first_seen_ns
            else None
        )
        if age_ms is not None and (
            total_oldest_age_ms is None or age_ms > total_oldest_age_ms
        ):
            total_oldest_age_ms = age_ms

        order = pending_order(client_order_id) if callable(pending_order) else None
        side = getattr(order, "side", None)
        target_sides = (
            (side,)
            if side in counts_by_side
            else (OrderSide.BUY, OrderSide.SELL)
        )
        if side not in counts_by_side:
            unknown_side_count += 1
        for target_side in target_sides:
            counts_by_side[target_side] += 1
            side_oldest_age_ms = oldest_age_ms_by_side[target_side]
            if age_ms is not None and (side_oldest_age_ms is None or age_ms > side_oldest_age_ms):
                oldest_age_ms_by_side[target_side] = age_ms

    return {
        "counts_by_side": counts_by_side,
        "oldest_age_ms_by_side": oldest_age_ms_by_side,
        "total_count": len(pending_ids),
        "total_oldest_age_ms": total_oldest_age_ms,
        "unknown_side_count": unknown_side_count,
    }


def _classify_pending_cancel_backlog_mode(
    *,
    runtime_params: dict[str, Any],
    pending_count: int,
    oldest_age_ms: int | None,
) -> str:
    if pending_count <= 0:
        return "normal"

    soft_count_threshold = max(
        0,
        int(runtime_params.get("max_pending_cancels_per_side", 0) or 0),
    )
    hard_count_threshold = max(2, soft_count_threshold + 1)
    blocked_count_threshold = max(3, soft_count_threshold + 2)

    pending_cancel_grace_ms = max(
        0,
        int(runtime_params.get("pending_cancel_grace_ms", 0) or 0),
    )
    pending_cancel_block_after_ms = max(
        0,
        int(runtime_params.get("pending_cancel_block_after_ms", 0) or 0),
    )
    quote_liveness_stall_after_ms = max(
        pending_cancel_block_after_ms,
        int(runtime_params.get("quote_liveness_stall_after_ms", 0) or 0),
    )

    if pending_count >= blocked_count_threshold or (
        oldest_age_ms is not None
        and quote_liveness_stall_after_ms > 0
        and oldest_age_ms >= quote_liveness_stall_after_ms
    ):
        return "blocked"
    if pending_count >= hard_count_threshold or (
        oldest_age_ms is not None
        and pending_cancel_block_after_ms > 0
        and oldest_age_ms >= pending_cancel_block_after_ms
    ):
        return "hard_freeze"
    if (soft_count_threshold <= 0 or pending_count >= soft_count_threshold) or (
        oldest_age_ms is not None
        and pending_cancel_grace_ms > 0
        and oldest_age_ms >= pending_cancel_grace_ms
    ):
        return "soft_throttle"
    return "normal"


def _worst_backlog_mode(modes: list[str]) -> str:
    worst_mode = "normal"
    worst_rank = _BACKLOG_MODE_RANK[worst_mode]
    for mode in modes:
        mode_rank = _BACKLOG_MODE_RANK.get(str(mode), 0)
        if mode_rank > worst_rank:
            worst_mode = str(mode)
            worst_rank = mode_rank
    return worst_mode


def _bounded_convergence_side_order(
    strategy: MakerV3Strategy,
) -> tuple[tuple[OrderSide, str], tuple[OrderSide, str]]:
    start_side = getattr(strategy, "_bounded_convergence_next_start_side", OrderSide.BUY)
    if start_side == OrderSide.SELL:
        return (
            (OrderSide.SELL, "sell"),
            (OrderSide.BUY, "buy"),
        )
    return (
        (OrderSide.BUY, "buy"),
        (OrderSide.SELL, "sell"),
    )


def _advance_bounded_convergence_side_order(
    strategy: MakerV3Strategy,
    *,
    current_start_side: OrderSide,
) -> None:
    strategy._bounded_convergence_next_start_side = (
        OrderSide.SELL if current_start_side == OrderSide.BUY else OrderSide.BUY
    )


def _bounded_convergence_summary(plan: rebalancing_mod.BoundedConvergencePlan) -> dict[str, Any]:
    cancel_reason_counts: dict[str, int] = {}
    for action in plan.cancel_actions:
        reason_code = str(action.reason_code)
        cancel_reason_counts[reason_code] = cancel_reason_counts.get(reason_code, 0) + 1

    diagnostics = plan.diagnostics
    return {
        "backlog_mode": diagnostics.backlog_mode,
        "matched_level_count": diagnostics.matched_level_count,
        "keep_level_count": diagnostics.keep_level_count,
        "frontier_missing_level_count": diagnostics.frontier_missing_level_count,
        "planned_stale_replacement_count": diagnostics.planned_stale_replacement_count,
        "total_missing_level_count": diagnostics.total_missing_level_count,
        "excess_cancel_candidate_count": diagnostics.excess_cancel_candidate_count,
        "aggressive_cancel_candidate_count": diagnostics.aggressive_cancel_candidate_count,
        "stale_cancel_candidate_count": diagnostics.stale_cancel_candidate_count,
        "room_cancel_candidate_count": diagnostics.room_cancel_candidate_count,
        "budget_limited": diagnostics.budget_limited,
        "backlog_limited": diagnostics.backlog_limited,
        "planned_cancel_count": len(plan.cancel_actions),
        "planned_place_count": len(plan.place_level_indices),
        "executed_cancel_count": 0,
        "executed_place_count": 0,
        "cancel_reason_counts": cancel_reason_counts,
    }


def handle_stale_quote_block(
    strategy: MakerV3Strategy,
    *,
    now_ns: int,
    state: str,
    cancel_reason: str,
    reason_code: str,
    quote_cycle: QuoteCycleContext | None = None,
    quote_cycle_id: str | None = None,
    warning_message: str,
) -> None:
    """
    Cancel managed quotes once per cooldown and publish a blocked state/event.
    """
    managed_orders = strategy._managed_orders()
    cooldown_ns = strategy.STALE_CANCEL_COOLDOWN_MS * 1_000_000
    if (
        strategy._last_stale_cancel_ns <= 0
        or now_ns - strategy._last_stale_cancel_ns >= cooldown_ns
    ):
        strategy._cancel_managed_quotes(
            cancel_reason,
            managed_orders=managed_orders,
            now_ns=now_ns,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            reason_code={
                "maker_book_unavailable": REASON_CANCEL_MAKER_BOOK_UNAVAILABLE,
                "maker_md_stale": REASON_CANCEL_MAKER_MD_STALE,
                "reference_md_stale": REASON_CANCEL_REFERENCE_MD_STALE,
            }.get(cancel_reason),
            decision_context_json=strategy._quote_cycle_decision_context(
                managed_orders=managed_orders,
            ),
        )
        strategy._last_stale_cancel_ns = now_ns
    from_state = getattr(strategy, "_last_state_name", None)
    blocked_transition = not bool(getattr(strategy, "_state_is_blocked", False))
    strategy._publish_state(
        state,
        managed_orders_count=len(managed_orders),
        managed_orders=managed_orders,
    )
    strategy._publish_quote_cycle_event(
        now_ns=now_ns,
        quote_cycle_event=QUOTE_CYCLE_EVENT_BLOCKED,
        reason_code=reason_code,
        quote_cycle=quote_cycle,
        quote_cycle_id=quote_cycle_id,
        payload={
            "from_state": from_state,
            "to_state": state,
            "blocked_transition": blocked_transition,
            "managed_orders": len(managed_orders),
        },
    )
    if blocked_transition:
        strategy._publish_actionable_alert(
            alert_key=ALERT_KEY_MARKET_DATA_BLOCKED,
            message=warning_message,
            level="warning",
            reason_code=reason_code,
            cooldown_ms=ALERT_COOLDOWN_BLOCKED_MS,
            transition=f"{from_state}->{state}",
            now_ns=now_ns,
        )
    strategy._last_requote_ns = now_ns
    strategy.log.warning(warning_message)


def publish_recovery_state_if_blocked(
    strategy: MakerV3Strategy,
    *,
    managed_orders_count: int | None = None,
    managed_orders: list[Any] | None = None,
) -> None:
    """
    Publish a recovery state transition when leaving a blocked state.
    """
    if not bool(getattr(strategy, "_state_is_blocked", False)):
        return
    strategy._publish_state(
        "running",
        managed_orders_count=managed_orders_count,
        managed_orders=managed_orders,
    )


def handle_startup_cleanup_block(
    strategy: MakerV3Strategy,
    *,
    now_ns: int,
    quote_cycle: QuoteCycleContext | None = None,
    quote_cycle_id: str | None = None,
    managed_orders: list[Any],
) -> None:
    """
    Block quoting while startup cleanup is still unwinding claimed orders.
    """
    from_state = getattr(strategy, "_last_state_name", None)
    blocked_transition = not bool(getattr(strategy, "_state_is_blocked", False))
    strategy._publish_state(
        "blocked_startup_cleanup",
        managed_orders_count=len(managed_orders),
        managed_orders=managed_orders,
    )
    strategy._publish_quote_cycle_event(
        now_ns=now_ns,
        quote_cycle_event=QUOTE_CYCLE_EVENT_BLOCKED,
        reason_code=REASON_BLOCKED_STARTUP_CLEANUP,
        quote_cycle=quote_cycle,
        quote_cycle_id=quote_cycle_id,
        payload={
            "from_state": from_state,
            "to_state": "blocked_startup_cleanup",
            "blocked_transition": blocked_transition,
            "managed_orders": len(managed_orders),
            "pending_cancels": len(getattr(strategy, "_pending_cancel_client_order_ids", ())),
        },
    )
    strategy._last_requote_ns = now_ns


def handle_portfolio_inventory_block(
    strategy: MakerV3Strategy,
    *,
    now_ns: int,
    quote_cycle: QuoteCycleContext | None = None,
    quote_cycle_id: str | None = None,
    managed_orders: list[Any],
) -> None:
    """
    Block quoting when shared portfolio inventory is degraded.
    """
    state = "blocked_portfolio_inventory"
    from_state = getattr(strategy, "_last_state_name", None)
    blocked_transition = not bool(getattr(strategy, "_state_is_blocked", False))
    strategy._cancel_managed_quotes(
        "portfolio_inventory_unavailable",
        managed_orders=managed_orders,
        now_ns=now_ns,
        quote_cycle=quote_cycle,
        quote_cycle_id=quote_cycle_id,
        decision_context_json=strategy._quote_cycle_decision_context(
            managed_orders=managed_orders,
        ),
    )
    strategy._publish_state(
        state,
        managed_orders_count=len(managed_orders),
        managed_orders=managed_orders,
    )
    strategy._publish_quote_cycle_event(
        now_ns=now_ns,
        quote_cycle_event=QUOTE_CYCLE_EVENT_BLOCKED,
        reason_code=REASON_BLOCKED_PORTFOLIO_INVENTORY_UNAVAILABLE,
        quote_cycle=quote_cycle,
        quote_cycle_id=quote_cycle_id,
        payload={
            "from_state": from_state,
            "to_state": state,
            "blocked_transition": blocked_transition,
            "managed_orders": len(managed_orders),
        },
    )
    if blocked_transition:
        strategy._publish_actionable_alert(
            alert_key=ALERT_KEY_PORTFOLIO_INVENTORY_BLOCKED,
            message=(
                "Quoting blocked (shared portfolio inventory unavailable) "
                f"strategy_id={strategy._external_strategy_id}"
            ),
            level="warning",
            reason_code=REASON_BLOCKED_PORTFOLIO_INVENTORY_UNAVAILABLE,
            cooldown_ms=ALERT_COOLDOWN_BLOCKED_MS,
            transition=f"{from_state}->{state}",
            now_ns=now_ns,
        )
    strategy._last_requote_ns = now_ns


def refresh_quotes(  # noqa: C901
    strategy: MakerV3Strategy,
    *,
    now_ns: int,
    quote_cycle_id: str | None = None,
    quote_cycle: QuoteCycleContext | None = None,
) -> None:
    """
    Compute desired quote ladder and rebalance managed orders to match it.
    """
    if strategy._quote_management_suspended():
        return
    if strategy._maker_instrument is None or strategy._order_qty is None:
        return
    if quote_cycle is None:
        if quote_cycle_id is None:
            quote_cycle = strategy._begin_quote_cycle(
                now_ns=now_ns,
                trigger_source="timer_guard",
                trigger_instrument_id=strategy.config.maker_instrument_id,
                trigger_md_ts_event_ns=int(
                    strategy._last_bbo_event_ts_ns.get(strategy.config.maker_instrument_id, 0) or 0,
                )
                or None,
                trigger_md_ts_init_ns=int(
                    strategy._last_bbo_init_ts_ns.get(strategy.config.maker_instrument_id, 0) or 0,
                )
                or None,
            )
        else:
            quote_cycle = strategy._quote_cycle_context_from_id(
                now_ns=now_ns,
                quote_cycle_id=quote_cycle_id,
            )
    runtime_params = strategy._quote_runtime_params_snapshot()

    maker_bbo = strategy._best_bid_ask(strategy.config.maker_instrument_id)
    if maker_bbo is None:
        handle_stale_quote_block(
            strategy,
            now_ns=now_ns,
            state="blocked_maker_md",
            cancel_reason="maker_book_unavailable",
            reason_code=REASON_BLOCKED_MAKER_BOOK_UNAVAILABLE,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            warning_message=f"Quoting blocked (maker book unavailable) strategy_id={strategy._external_strategy_id}",
        )
        return
    best_bid_px, best_ask_px = maker_bbo
    maker_mid = (best_bid_px + best_ask_px) / Decimal(2)
    max_age_ms = int(runtime_params["max_age_ms"])
    maker_health = strategy._quote_health(
        instrument_id=strategy.config.maker_instrument_id,
        leg_role="maker",
        now_ns=now_ns,
        max_quote_age_ms=max_age_ms,
    )
    if not maker_health.usable_for_pricing:
        handle_stale_quote_block(
            strategy,
            now_ns=now_ns,
            state="blocked_maker_md",
            cancel_reason="maker_md_stale",
            reason_code=REASON_BLOCKED_MAKER_MD_STALE,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            warning_message=(
                f"Quoting blocked (maker data stale) strategy_id={strategy._external_strategy_id} "
                f"age_ms={maker_health.quote_age_ms} max_age_ms={max_age_ms}"
            ),
        )
        return

    ref_bbo = strategy._best_bid_ask(strategy.config.reference_instrument_id)
    reference_health = strategy._quote_health(
        instrument_id=strategy.config.reference_instrument_id,
        leg_role="reference",
        now_ns=now_ns,
        max_quote_age_ms=max_age_ms,
    )
    if ref_bbo is None or not reference_health.usable_for_pricing:
        handle_stale_quote_block(
            strategy,
            now_ns=now_ns,
            state="blocked_reference_md",
            cancel_reason="reference_md_stale",
            reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            warning_message=(
                f"Quoting blocked (reference data stale) strategy_id={strategy._external_strategy_id} "
                f"age_ms={reference_health.quote_age_ms} max_age_ms={max_age_ms}"
            ),
        )
        return

    ref_bid, ref_ask = ref_bbo
    anchor_bid = ref_bid
    anchor_ask = ref_ask
    anchor_source = "reference_leg"

    reference_mid = (
        (ref_bid + ref_ask) / Decimal(2) if ref_bid is not None and ref_ask is not None else None
    )
    if reference_mid is not None:
        fair_value = (maker_mid + reference_mid) / Decimal(2)
    else:
        fair_value = maker_mid

    bps_anchor = (anchor_bid + anchor_ask) / Decimal(2)
    if bps_anchor <= 0:
        return
    active_orders = strategy._managed_orders()
    if strategy._startup_cleanup_active(managed_orders=active_orders):
        handle_startup_cleanup_block(
            strategy,
            now_ns=now_ns,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            managed_orders=active_orders,
        )
        return
    base_currency = strategy._maker_base_currency_code()
    portfolio_asset_id = strategy._portfolio_asset_id() or base_currency
    if strategy._portfolio_inventory_portfolio_id:
        _, portfolio_block_reason, _portfolio_diagnostics = (
            strategy._shared_portfolio_inventory_qty_and_block_reason(
            portfolio_asset_id,
            )
        )
        if portfolio_block_reason == REASON_BLOCKED_PORTFOLIO_INVENTORY_UNAVAILABLE:
            handle_portfolio_inventory_block(
                strategy,
                now_ns=now_ns,
                quote_cycle=quote_cycle,
                quote_cycle_id=quote_cycle_id,
                managed_orders=active_orders,
            )
            return
    publish_recovery_state_if_blocked(
        strategy,
        managed_orders_count=len(active_orders),
        managed_orders=active_orders,
    )
    cooldown_order_ids = strategy._active_cancel_reject_cooldown_order_ids(
        now_ns=now_ns,
        managed_orders=active_orders,
    )
    if cooldown_order_ids:
        strategy._publish_quote_cycle_event(
            now_ns=now_ns,
            quote_cycle_event=QUOTE_CYCLE_EVENT_SKIPPED,
            reason_code=REASON_SKIPPED_CANCEL_REJECT_COOLDOWN,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            payload={
                "managed_orders": len(active_orders),
                "cooldown_order_count": len(cooldown_order_ids),
                "cooldown_order_ids": cooldown_order_ids,
            },
        )
        strategy._last_requote_ns = now_ns
        return

    skew_ctx = strategy._cached_inventory_skew(now_ns=now_ns, runtime_params=runtime_params)
    total_skew_bps = _to_decimal(skew_ctx.get("total_skew_bps", Decimal(0)))

    # `total_skew_bps` is the signed quoted-FV adjustment relative to the
    # reference anchor. Positive means quote richer / higher; negative means
    # quote cheaper / lower.
    bid_edge1_eff_bps, ask_edge1_eff_bps = _apply_inventory_skew_to_edges(
        bid_edge_bps=_to_decimal(runtime_params["bid_edge1"]),
        ask_edge_bps=_to_decimal(runtime_params["ask_edge1"]),
        total_skew_bps=total_skew_bps,
    )
    bid_edge2_eff_bps, ask_edge2_eff_bps = _apply_inventory_skew_to_edges(
        bid_edge_bps=_to_decimal(runtime_params["bid_edge2"]),
        ask_edge_bps=_to_decimal(runtime_params["ask_edge2"]),
        total_skew_bps=total_skew_bps,
    )
    bid_edge3_eff_bps, ask_edge3_eff_bps = _apply_inventory_skew_to_edges(
        bid_edge_bps=_to_decimal(runtime_params["bid_edge3"]),
        ask_edge_bps=_to_decimal(runtime_params["ask_edge3"]),
        total_skew_bps=total_skew_bps,
    )

    tick = strategy._maker_instrument.price_increment.as_decimal()

    bid_levels, ask_levels = build_ladder_place_cancel_levels_from_bps(
        anchor_bid=anchor_bid,
        anchor_ask=anchor_ask,
        bid_edges_bps=(bid_edge1_eff_bps, bid_edge2_eff_bps, bid_edge3_eff_bps),
        ask_edges_bps=(ask_edge1_eff_bps, ask_edge2_eff_bps, ask_edge3_eff_bps),
        place_edges_bps=(
            _to_decimal(runtime_params["place_edge1"]),
            _to_decimal(runtime_params["place_edge2"]),
            _to_decimal(runtime_params["place_edge3"]),
        ),
        distances_bps=(
            _to_decimal(runtime_params["distance1"]),
            _to_decimal(runtime_params["distance2"]),
            _to_decimal(runtime_params["distance3"]),
        ),
        n_orders=(
            int(runtime_params["n_orders1"]),
            int(runtime_params["n_orders2"]),
            int(runtime_params["n_orders3"]),
        ),
        tick=tick,
    )
    match_tol = tick / Decimal(2) if tick > 0 else Decimal(0)

    desired_buys: list[tuple[Price, Decimal, Decimal]] = []
    desired_sells: list[tuple[Price, Decimal, Decimal]] = []
    seen_buy_prices: set[str] = set()
    seen_sell_prices: set[str] = set()
    for bid_place, bid_cancel in bid_levels:
        bid_place_rounded = _round_price_to_tick(
            bid_place,
            tick=tick,
            is_buy=True,
            round_in=False,
        )
        bid_cancel_rounded = _round_price_to_tick(
            bid_cancel,
            tick=tick,
            is_buy=True,
            round_in=False,
        )
        bid_place_rounded = _clamp_post_only_price(
            price=bid_place_rounded,
            is_buy=True,
            top_bid=best_bid_px,
            top_ask=best_ask_px,
            tick=tick,
        )
        bid_place_unique = _nudge_unique_price(
            price=bid_place_rounded,
            tick=tick,
            is_buy=True,
            seen=seen_buy_prices,
        )
        if bid_place_unique is None:
            continue
        seen_buy_prices.add(str(bid_place_unique))
        if bid_place_unique > 0 and bid_cancel_rounded > 0:
            desired_buys.append(
                (
                    strategy._maker_instrument.make_price(bid_place_unique),
                    bid_cancel_rounded,
                    match_tol,
                ),
            )
    for ask_place, ask_cancel in ask_levels:
        ask_place_rounded = _round_price_to_tick(
            ask_place,
            tick=tick,
            is_buy=False,
            round_in=False,
        )
        ask_cancel_rounded = _round_price_to_tick(
            ask_cancel,
            tick=tick,
            is_buy=False,
            round_in=False,
        )
        ask_place_rounded = _clamp_post_only_price(
            price=ask_place_rounded,
            is_buy=False,
            top_bid=best_bid_px,
            top_ask=best_ask_px,
            tick=tick,
        )
        ask_place_unique = _nudge_unique_price(
            price=ask_place_rounded,
            tick=tick,
            is_buy=False,
            seen=seen_sell_prices,
        )
        if ask_place_unique is None:
            continue
        seen_sell_prices.add(str(ask_place_unique))
        if ask_place_unique > 0 and ask_cancel_rounded > 0:
            desired_sells.append(
                (
                    strategy._maker_instrument.make_price(ask_place_unique),
                    ask_cancel_rounded,
                    match_tol,
                ),
            )

    l1_place_bid = _price_to_decimal(desired_buys[0][0]) if desired_buys else None
    l1_cancel_bid = desired_buys[0][1] if desired_buys else None
    l1_place_ask = _price_to_decimal(desired_sells[0][0]) if desired_sells else None
    l1_cancel_ask = desired_sells[0][1] if desired_sells else None

    strategy._last_pricing_debug = {
        "pricing": {
            "ts_ms": now_ns // 1_000_000,
            "anchor_source": anchor_source,
            "fv": _decimal_to_json_str(fair_value),
            "anchor_bid": _decimal_to_json_str(anchor_bid),
            "anchor_ask": _decimal_to_json_str(anchor_ask),
            "ref_bid": _decimal_to_json_str(ref_bid),
            "ref_ask": _decimal_to_json_str(ref_ask),
            "ref_mid": _decimal_to_json_str(reference_mid),
            "maker_top_bid": _decimal_to_json_str(best_bid_px),
            "maker_top_ask": _decimal_to_json_str(best_ask_px),
            "maker_mid": _decimal_to_json_str(maker_mid),
            "reference_mid": _decimal_to_json_str(reference_mid),
            "anchor_spread_bps": _decimal_to_json_str(
                ((anchor_ask - anchor_bid) / bps_anchor) * Decimal(10000)
                if bps_anchor > 0
                else None,
            ),
            "bid_edge1_cfg_bps": _decimal_to_json_str(runtime_params["bid_edge1"]),
            "ask_edge1_cfg_bps": _decimal_to_json_str(runtime_params["ask_edge1"]),
            "bid_edge1_eff_bps": _decimal_to_json_str(bid_edge1_eff_bps),
            "ask_edge1_eff_bps": _decimal_to_json_str(ask_edge1_eff_bps),
            "place_bid": _decimal_to_json_str(l1_place_bid),
            "cancel_bid": _decimal_to_json_str(l1_cancel_bid),
            "place_ask": _decimal_to_json_str(l1_place_ask),
            "cancel_ask": _decimal_to_json_str(l1_cancel_ask),
            "place_edge_bps": _decimal_to_json_str(runtime_params["place_edge1"]),
            "effective_skew_bps": _decimal_to_json_str(total_skew_bps),
            "total_skew_bps": _decimal_to_json_str(total_skew_bps),
        },
        "skew": {
            "inventory_qty": _decimal_to_json_str(skew_ctx["inventory_qty"]),
            "inventory_source": skew_ctx["inventory_source"],
            "position_qty": _decimal_to_json_str(skew_ctx["position_qty"]),
            "spot_base_total": _decimal_to_json_str(skew_ctx["spot_qty"]),
            "global_position_qty": _decimal_to_json_str(skew_ctx["global_position_qty"]),
            "global_spot_qty": _decimal_to_json_str(skew_ctx["global_spot_qty"]),
            "global_inventory_qty": _decimal_to_json_str(skew_ctx["global_inventory_qty"]),
            "global_inventory_source": skew_ctx["global_inventory_source"],
            "local_position_qty": _decimal_to_json_str(skew_ctx["local_position_qty"]),
            "local_spot_qty": _decimal_to_json_str(skew_ctx["local_spot_qty"]),
            "local_inventory_qty": _decimal_to_json_str(skew_ctx["local_inventory_qty"]),
            "local_inventory_source": skew_ctx["local_inventory_source"],
            "base_currency": skew_ctx["base_currency"],
            "des_qty_global": _decimal_to_json_str(skew_ctx["des_qty_global"]),
            "max_qty_global": _decimal_to_json_str(skew_ctx["max_qty_global"]),
            "max_skew_bps_global": _decimal_to_json_str(skew_ctx["max_skew_bps_global"]),
            "des_qty_local": _decimal_to_json_str(skew_ctx["des_qty_local"]),
            "max_qty_local": _decimal_to_json_str(skew_ctx["max_qty_local"]),
            "max_skew_bps_local": _decimal_to_json_str(skew_ctx["max_skew_bps_local"]),
            "linear_offset_bps": _decimal_to_json_str(skew_ctx["linear_offset_bps"]),
            "global_ratio": _decimal_to_json_str(skew_ctx["global_ratio"]),
            "global_skew_bps": _decimal_to_json_str(skew_ctx["global_skew_bps"]),
            "local_ratio": _decimal_to_json_str(skew_ctx["local_ratio"]),
            "local_skew_bps": _decimal_to_json_str(skew_ctx["local_skew_bps"]),
            "total_skew_bps": _decimal_to_json_str(skew_ctx["total_skew_bps"]),
        },
        "md_health": {
            "maker_age_ms": maker_health.quote_age_ms,
            "reference_age_ms": reference_health.quote_age_ms,
            "maker_fresh": maker_health.usable_for_pricing,
            "reference_fresh": reference_health.usable_for_pricing,
        },
    }
    strategy._last_quote_snapshot = {
        "ts_ms": now_ns // 1_000_000,
        "maker_top_bid": _decimal_to_json_str(best_bid_px),
        "maker_top_ask": _decimal_to_json_str(best_ask_px),
        "ref_bid": _decimal_to_json_str(ref_bid),
        "ref_ask": _decimal_to_json_str(ref_ask),
        "place_bid": _decimal_to_json_str(l1_place_bid),
        "place_ask": _decimal_to_json_str(l1_place_ask),
        "cancel_bid": _decimal_to_json_str(l1_cancel_bid),
        "cancel_ask": _decimal_to_json_str(l1_cancel_ask),
        "eff_bid_edge_bps": _decimal_to_json_str(bid_edge1_eff_bps),
        "eff_ask_edge_bps": _decimal_to_json_str(ask_edge1_eff_bps),
        "skew_bps_signed": _decimal_to_json_str(total_skew_bps),
        "place_edge_bps": _decimal_to_json_str(runtime_params["place_edge1"]),
    }
    per_level_outcomes: list[dict[str, Any]] = []
    decision_context_json = strategy._quote_cycle_decision_context(
        runtime_params=runtime_params,
        managed_orders=active_orders,
        per_level_outcomes=per_level_outcomes,
    )

    if not desired_buys and not desired_sells:
        strategy._cancel_managed_quotes(
            "no_targets",
            managed_orders=active_orders,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            now_ns=now_ns,
            reason_code=REASON_CANCEL_NO_TARGETS,
            decision_context_json=decision_context_json,
        )
        strategy._last_requote_ns = now_ns
        strategy._last_completed_quote_ns = now_ns
        strategy._publish_quote_cycle_event(
            now_ns=now_ns,
            quote_cycle_event=QUOTE_CYCLE_EVENT_COMPLETED,
            reason_code=REASON_COMPLETED_NO_TARGETS,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            payload={
                "cancel_count": len(active_orders),
                "place_count": 0,
                "bid_levels": 0,
                "ask_levels": 0,
                "decision_context_json": strategy._quote_cycle_decision_context(
                    runtime_params=runtime_params,
                    managed_orders=active_orders,
                    per_level_outcomes=per_level_outcomes,
                ),
            },
        )
        return

    active_buys = sorted(
        [order for order in active_orders if order.side == OrderSide.BUY],
        key=lambda order: _price_to_decimal(order.price),
        reverse=True,
    )
    active_sells = sorted(
        [order for order in active_orders if order.side == OrderSide.SELL],
        key=lambda order: _price_to_decimal(order.price),
    )
    bounded_convergence: dict[str, dict[str, Any]] = {}

    clear_orphans = getattr(strategy, "_clear_orphaned_pending_cancels", None)
    cleared_orphans: tuple[str, ...] = ()
    if callable(clear_orphans):
        cleared_orphans = tuple(clear_orphans())

    pending_backlog = _pending_cancel_backlog_snapshot(strategy, now_ns=now_ns)
    if cleared_orphans and int(pending_backlog["total_count"]) <= 0:
        strategy._last_requote_ns = now_ns
        strategy._last_completed_quote_ns = now_ns
        strategy._publish_state(getattr(strategy, "_last_state_name", None) or "running")
        strategy._publish_quote_cycle_event(
            now_ns=now_ns,
            quote_cycle_event=QUOTE_CYCLE_EVENT_COMPLETED,
            reason_code=REASON_COMPLETED_NO_ACTIONS,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            payload={
                "cancel_count": 0,
                "place_count": 0,
                "cleared_orphaned_pending_cancels": list(cleared_orphans),
                "decision_context_json": strategy._quote_cycle_decision_context(
                    runtime_params=runtime_params,
                    managed_orders=[*active_buys, *active_sells],
                    per_level_outcomes=per_level_outcomes,
                    bounded_convergence=bounded_convergence or None,
                ),
            },
        )
        return

    if int(pending_backlog.get("unknown_side_count", 0)) > 0:
        shared_backlog_mode = _classify_pending_cancel_backlog_mode(
            runtime_params=runtime_params,
            pending_count=int(pending_backlog["total_count"]),
            oldest_age_ms=pending_backlog["total_oldest_age_ms"],
        )
        side_backlog_modes = {
            OrderSide.BUY: shared_backlog_mode,
            OrderSide.SELL: shared_backlog_mode,
        }
    else:
        side_backlog_modes = {
            OrderSide.BUY: _classify_pending_cancel_backlog_mode(
                runtime_params=runtime_params,
                pending_count=int(pending_backlog["counts_by_side"][OrderSide.BUY]),
                oldest_age_ms=pending_backlog["oldest_age_ms_by_side"][OrderSide.BUY],
            ),
            OrderSide.SELL: _classify_pending_cancel_backlog_mode(
                runtime_params=runtime_params,
                pending_count=int(pending_backlog["counts_by_side"][OrderSide.SELL]),
                oldest_age_ms=pending_backlog["oldest_age_ms_by_side"][OrderSide.SELL],
            ),
        }
    initial_backlog_mode = _worst_backlog_mode(list(side_backlog_modes.values()))
    if initial_backlog_mode == "blocked":
        from_state = getattr(strategy, "_last_state_name", None)
        oldest_pending_cancel_age_ms = pending_backlog["total_oldest_age_ms"]
        strategy._last_requote_ns = now_ns
        strategy._publish_state("blocked_pending_cancel")
        strategy._publish_quote_cycle_event(
            now_ns=now_ns,
            quote_cycle_event=QUOTE_CYCLE_EVENT_BLOCKED,
            reason_code=REASON_BLOCKED_PENDING_CANCEL,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            payload={
                "cancel_count": 0,
                "pending_cancels": int(pending_backlog["total_count"]),
                "oldest_pending_cancel_age_ms": oldest_pending_cancel_age_ms,
                "bid_levels": len(desired_buys),
                "ask_levels": len(desired_sells),
                "backlog_mode": initial_backlog_mode,
                "buy_backlog_mode": side_backlog_modes[OrderSide.BUY],
                "sell_backlog_mode": side_backlog_modes[OrderSide.SELL],
                "decision_context_json": strategy._quote_cycle_decision_context(
                    runtime_params=runtime_params,
                    managed_orders=[*active_buys, *active_sells],
                    per_level_outcomes=per_level_outcomes,
                    bounded_convergence=bounded_convergence or None,
                ),
            },
            oldest_pending_cancel_age_ms=oldest_pending_cancel_age_ms,
        )
        if from_state != "blocked_pending_cancel":
            strategy._publish_actionable_alert(
                alert_key=ALERT_KEY_QUOTE_LIVENESS_BLOCKED,
                message=(
                    "Quoting blocked (pending cancel stuck) "
                    f"strategy_id={strategy._external_strategy_id}"
                ),
                level="warning",
                reason_code=REASON_BLOCKED_PENDING_CANCEL,
                cooldown_ms=ALERT_COOLDOWN_BLOCKED_MS,
                transition=f"{from_state}->blocked_pending_cancel",
                now_ns=now_ns,
                pending_cancel_count=int(pending_backlog["total_count"]),
            )
        return

    cancels = 0
    places = 0
    remaining_total_actions = max(
        0,
        int(runtime_params.get("max_total_actions_per_cycle", 0) or 0),
    )
    max_reprice_cancel_actions = max(
        0,
        int(runtime_params.get("max_cancels_per_side_per_cycle", 0) or 0),
    )
    max_place_actions = max(
        0,
        int(runtime_params.get("max_places_per_side_per_cycle", 0) or 0),
    )
    side_order = _bounded_convergence_side_order(strategy)
    side_orders = tuple(
        (
            side,
            active_buys if side == OrderSide.BUY else active_sells,
            desired_buys if side == OrderSide.BUY else desired_sells,
            side_name,
        )
        for side, side_name in side_order
    )
    blocked_after_actions = False
    for side, side_active_orders, desired_levels, side_name in side_orders:
        backlog_mode = side_backlog_modes[side]
        side_total_actions = remaining_total_actions
        desired_dec = [
            (_price_to_decimal(target_price), cancel_px, match_tol)
            for target_price, cancel_px, match_tol in desired_levels
        ]
        active_prices = [_price_to_decimal(order.price) for order in side_active_orders]
        active_stale = [
            strategy._is_stale_order(order, now_ns, max_age_ms=max_age_ms)
            for order in side_active_orders
        ]
        side_plan = rebalancing_mod.plan_side_bounded_convergence(
            side="buy" if side == OrderSide.BUY else "sell",
            active_prices=active_prices,
            active_stale=active_stale,
            desired_levels=desired_dec,
            stale_cancel_budget=strategy.STALE_CANCELS_PER_SIDE_PER_CYCLE,
            max_reprice_cancel_actions=max_reprice_cancel_actions,
            max_place_actions=max_place_actions,
            max_total_actions=side_total_actions,
            backlog_mode=backlog_mode,
        )
        bounded_convergence[side_name] = _bounded_convergence_summary(side_plan)

        side_cancel_count = strategy._rebalance_side(
            side=side,
            active_orders=side_active_orders,
            desired_levels=desired_levels,
            now_ns=now_ns,
            max_age_ms=max_age_ms,
            max_reprice_cancel_actions=max_reprice_cancel_actions,
            max_place_actions=max_place_actions,
            max_total_actions=side_total_actions,
            backlog_mode=backlog_mode,
            cancel_actions=side_plan.cancel_actions,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            decision_context_json=decision_context_json,
        )
        bounded_convergence[side_name]["executed_cancel_count"] = side_cancel_count
        cancels += side_cancel_count
        remaining_total_actions = max(0, remaining_total_actions - side_cancel_count)

        if side_cancel_count > 0:
            pending_backlog["counts_by_side"][side] += side_cancel_count
            pending_backlog["total_count"] += side_cancel_count
            if pending_backlog["oldest_age_ms_by_side"][side] is None:
                pending_backlog["oldest_age_ms_by_side"][side] = 0
            if pending_backlog["total_oldest_age_ms"] is None:
                pending_backlog["total_oldest_age_ms"] = 0
            side_backlog_modes[side] = _classify_pending_cancel_backlog_mode(
                runtime_params=runtime_params,
                pending_count=int(pending_backlog["counts_by_side"][side]),
                oldest_age_ms=pending_backlog["oldest_age_ms_by_side"][side],
            )
            if side_backlog_modes[side] == "blocked":
                blocked_after_actions = True
                break

        if (
            side_cancel_count > 0
            or side_backlog_modes[side] != "normal"
            or remaining_total_actions <= 0
        ):
            continue

        allowed_level_indices = tuple(
            int(level_index)
            for level_index in side_plan.place_level_indices[:remaining_total_actions]
        )
        if not allowed_level_indices:
            continue
        side_place_count = strategy._place_missing_levels(
            side=side,
            active_orders=side_active_orders,
            desired_levels=desired_levels,
            best_bid_px=best_bid_px,
            best_ask_px=best_ask_px,
            now_ns=now_ns,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            decision_context_json=decision_context_json,
            per_level_outcomes=per_level_outcomes,
            level_indices=allowed_level_indices,
            pending_backlog_mode=side_backlog_modes[side],
        )
        bounded_convergence[side_name]["executed_place_count"] = side_place_count
        places += side_place_count
        remaining_total_actions = max(0, remaining_total_actions - side_place_count)

    current_backlog_mode = _worst_backlog_mode(list(side_backlog_modes.values()))
    oldest_pending_cancel_age_ms = pending_backlog["total_oldest_age_ms"]
    _advance_bounded_convergence_side_order(
        strategy,
        current_start_side=side_orders[0][0],
    )
    if blocked_after_actions or current_backlog_mode == "blocked":
        from_state = getattr(strategy, "_last_state_name", None)
        strategy._last_requote_ns = now_ns
        strategy._publish_state("blocked_pending_cancel")
        strategy._publish_quote_cycle_event(
            now_ns=now_ns,
            quote_cycle_event=QUOTE_CYCLE_EVENT_BLOCKED,
            reason_code=REASON_BLOCKED_PENDING_CANCEL,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            payload={
                "cancel_count": cancels,
                "pending_cancels": int(pending_backlog["total_count"]),
                "oldest_pending_cancel_age_ms": oldest_pending_cancel_age_ms,
                "bid_levels": len(desired_buys),
                "ask_levels": len(desired_sells),
                "backlog_mode": "blocked",
                "buy_backlog_mode": side_backlog_modes[OrderSide.BUY],
                "sell_backlog_mode": side_backlog_modes[OrderSide.SELL],
                "decision_context_json": strategy._quote_cycle_decision_context(
                    runtime_params=runtime_params,
                    managed_orders=[*active_buys, *active_sells],
                    per_level_outcomes=per_level_outcomes,
                    bounded_convergence=bounded_convergence or None,
                ),
            },
            oldest_pending_cancel_age_ms=oldest_pending_cancel_age_ms,
        )
        if from_state != "blocked_pending_cancel":
            strategy._publish_actionable_alert(
                alert_key=ALERT_KEY_QUOTE_LIVENESS_BLOCKED,
                message=(
                    "Quoting blocked (pending cancel stuck) "
                    f"strategy_id={strategy._external_strategy_id}"
                ),
                level="warning",
                reason_code=REASON_BLOCKED_PENDING_CANCEL,
                cooldown_ms=ALERT_COOLDOWN_BLOCKED_MS,
                transition=f"{from_state}->blocked_pending_cancel",
                now_ns=now_ns,
                pending_cancel_count=int(pending_backlog["total_count"]),
            )
        return
    if int(pending_backlog["total_count"]) > 0 and not cancels and not places:
        strategy._last_requote_ns = now_ns
        strategy._publish_state(getattr(strategy, "_last_state_name", None) or "running")
        strategy._publish_quote_cycle_event(
            now_ns=now_ns,
            quote_cycle_event=QUOTE_CYCLE_EVENT_SKIPPED,
            reason_code=REASON_SKIPPED_PENDING_CANCELS,
            quote_cycle=quote_cycle,
            quote_cycle_id=quote_cycle_id,
            payload={
                "cancel_count": cancels,
                "pending_cancels": int(pending_backlog["total_count"]),
                "oldest_pending_cancel_age_ms": oldest_pending_cancel_age_ms,
                "backlog_mode": current_backlog_mode,
                "buy_backlog_mode": side_backlog_modes[OrderSide.BUY],
                "sell_backlog_mode": side_backlog_modes[OrderSide.SELL],
                "decision_context_json": strategy._quote_cycle_decision_context(
                    runtime_params=runtime_params,
                    managed_orders=[*active_buys, *active_sells],
                    per_level_outcomes=per_level_outcomes,
                    bounded_convergence=bounded_convergence or None,
                ),
            },
            oldest_pending_cancel_age_ms=oldest_pending_cancel_age_ms,
        )
        return

    strategy._last_requote_ns = now_ns
    strategy._last_completed_quote_ns = now_ns
    cycle_reason = REASON_COMPLETED_REBALANCED if cancels or places else REASON_COMPLETED_NO_ACTIONS
    strategy._publish_quote_cycle_event(
        now_ns=now_ns,
        quote_cycle_event=QUOTE_CYCLE_EVENT_COMPLETED,
        reason_code=cycle_reason,
        quote_cycle=quote_cycle,
        quote_cycle_id=quote_cycle_id,
        payload={
            "cancel_count": cancels,
            "place_count": places,
            "bid_levels": len(desired_buys),
            "ask_levels": len(desired_sells),
            "backlog_mode": current_backlog_mode,
            "buy_backlog_mode": side_backlog_modes[OrderSide.BUY],
            "sell_backlog_mode": side_backlog_modes[OrderSide.SELL],
            "cleared_orphaned_pending_cancels": list(cleared_orphans),
            "decision_context_json": strategy._quote_cycle_decision_context(
                runtime_params=runtime_params,
                managed_orders=[*active_buys, *active_sells],
                per_level_outcomes=per_level_outcomes,
                bounded_convergence=bounded_convergence or None,
            ),
        },
    )
    if cancels or places:
        strategy._publish_event(
            "quotes_rebalanced",
            bid_levels=len(desired_buys),
            ask_levels=len(desired_sells),
            cancels=cancels,
            places=places,
        )
        strategy._publish_state(
            "quotes_replaced",
            managed_orders=[*active_buys, *active_sells],
        )


__all__ = [
    "handle_stale_quote_block",
    "publish_recovery_state_if_blocked",
    "refresh_quotes",
]
