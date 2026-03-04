"""Run MakerV3 quote-cycle refresh logic and stale-data safety gates."""

from __future__ import annotations

from decimal import Decimal
from typing import TYPE_CHECKING

from nautilus_trader.flux.strategies.makerv3 import pricing as pricing_mod
from nautilus_trader.flux.strategies.makerv3 import publisher as publisher_mod
from nautilus_trader.flux.strategies.makerv3.constants import ALERT_COOLDOWN_BLOCKED_MS
from nautilus_trader.flux.strategies.makerv3.constants import ALERT_KEY_MARKET_DATA_BLOCKED
from nautilus_trader.flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_BLOCKED
from nautilus_trader.flux.strategies.makerv3.constants import QUOTE_CYCLE_EVENT_COMPLETED
from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_BOOK_UNAVAILABLE
from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_MAKER_MD_STALE
from nautilus_trader.flux.strategies.makerv3.constants import REASON_BLOCKED_REFERENCE_MD_STALE
from nautilus_trader.flux.strategies.makerv3.constants import REASON_COMPLETED_NO_ACTIONS
from nautilus_trader.flux.strategies.makerv3.constants import REASON_COMPLETED_NO_TARGETS
from nautilus_trader.flux.strategies.makerv3.constants import REASON_COMPLETED_REBALANCED
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price


if TYPE_CHECKING:
    from nautilus_trader.flux.strategies.makerv3.strategy import MakerV3Strategy


_to_decimal = pricing_mod.to_decimal
_price_to_decimal = pricing_mod.price_to_decimal
_round_price_to_tick = pricing_mod.round_price_to_tick
_clamp_post_only_price = pricing_mod.clamp_post_only_price
_nudge_unique_price = pricing_mod.nudge_unique_price
_apply_inventory_skew_to_edges = pricing_mod.apply_inventory_skew_to_edges
build_ladder_place_cancel_levels_from_bps = pricing_mod.build_ladder_place_cancel_levels_from_bps

_decimal_to_json_str = publisher_mod.decimal_to_json_str


def handle_stale_quote_block(
    strategy: MakerV3Strategy,
    *,
    now_ns: int,
    state: str,
    cancel_reason: str,
    reason_code: str,
    quote_cycle_id: str,
    warning_message: str,
) -> None:
    """Cancel managed quotes once per cooldown and publish a blocked state/event."""
    managed_orders = strategy._managed_orders()
    cooldown_ns = strategy.STALE_CANCEL_COOLDOWN_MS * 1_000_000
    if strategy._last_stale_cancel_ns <= 0 or now_ns - strategy._last_stale_cancel_ns >= cooldown_ns:
        strategy._cancel_managed_quotes(cancel_reason, managed_orders=managed_orders)
        strategy._last_stale_cancel_ns = now_ns
    from_state = getattr(strategy, "_last_state_name", None)
    blocked_transition = not bool(getattr(strategy, "_state_is_blocked", False))
    strategy._publish_state(state, managed_orders_count=len(managed_orders))
    strategy._publish_quote_cycle_event(
        now_ns=now_ns,
        quote_cycle_event=QUOTE_CYCLE_EVENT_BLOCKED,
        reason_code=reason_code,
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
) -> None:
    """Publish a recovery state transition when leaving a blocked state."""
    if not bool(getattr(strategy, "_state_is_blocked", False)):
        return
    strategy._publish_state("running", managed_orders_count=managed_orders_count)


def refresh_quotes(strategy: MakerV3Strategy, *, now_ns: int, quote_cycle_id: str | None = None) -> None:
    """Compute desired quote ladder and rebalance managed orders to match it."""
    if strategy._maker_instrument is None or strategy._order_qty is None:
        return
    if quote_cycle_id is None:
        quote_cycle_id = strategy._next_quote_cycle_id(now_ns=now_ns)
    runtime_params = strategy._quote_runtime_params_snapshot()

    maker_bbo = strategy._best_bid_ask(strategy.config.maker_instrument_id)
    if maker_bbo is None:
        handle_stale_quote_block(
            strategy,
            now_ns=now_ns,
            state="blocked_maker_md",
            cancel_reason="maker_md_stale",
            reason_code=REASON_BLOCKED_MAKER_BOOK_UNAVAILABLE,
            quote_cycle_id=quote_cycle_id,
            warning_message=f"Quoting blocked (maker book unavailable) strategy_id={strategy._external_strategy_id}",
        )
        return
    best_bid_px, best_ask_px = maker_bbo
    maker_mid = (best_bid_px + best_ask_px) / Decimal("2")

    maker_age_ms = None
    if strategy._last_bbo_ts_ns.get(strategy.config.maker_instrument_id, 0) > 0:
        maker_age_ms = int(
            (now_ns - strategy._last_bbo_ts_ns[strategy.config.maker_instrument_id]) / 1_000_000,
        )
    reference_age_ms = None
    if strategy._last_bbo_ts_ns.get(strategy.config.reference_instrument_id, 0) > 0:
        reference_age_ms = int(
            (now_ns - strategy._last_bbo_ts_ns[strategy.config.reference_instrument_id]) / 1_000_000,
        )
    max_age_ms = int(runtime_params["max_age_ms"])
    maker_fresh = bool(maker_age_ms is not None and maker_age_ms < max_age_ms)
    reference_fresh = bool(reference_age_ms is not None and reference_age_ms < max_age_ms)
    if not maker_fresh:
        handle_stale_quote_block(
            strategy,
            now_ns=now_ns,
            state="blocked_maker_md",
            cancel_reason="maker_md_stale",
            reason_code=REASON_BLOCKED_MAKER_MD_STALE,
            quote_cycle_id=quote_cycle_id,
            warning_message=(
                f"Quoting blocked (maker data stale) strategy_id={strategy._external_strategy_id} "
                f"age_ms={maker_age_ms} max_age_ms={max_age_ms}"
            ),
        )
        return

    ref_bbo = strategy._best_bid_ask(strategy.config.reference_instrument_id)
    if ref_bbo is None or not reference_fresh:
        handle_stale_quote_block(
            strategy,
            now_ns=now_ns,
            state="blocked_reference_md",
            cancel_reason="reference_md_stale",
            reason_code=REASON_BLOCKED_REFERENCE_MD_STALE,
            quote_cycle_id=quote_cycle_id,
            warning_message=(
                f"Quoting blocked (reference data stale) strategy_id={strategy._external_strategy_id} "
                f"age_ms={reference_age_ms} max_age_ms={max_age_ms}"
            ),
        )
        return

    ref_bid, ref_ask = ref_bbo
    anchor_bid = ref_bid
    anchor_ask = ref_ask
    anchor_source = "reference_leg"

    reference_mid = (ref_bid + ref_ask) / Decimal("2") if ref_bid is not None and ref_ask is not None else None
    if reference_mid is not None:
        fair_value = (maker_mid + reference_mid) / Decimal("2")
    else:
        fair_value = maker_mid

    bps_anchor = (anchor_bid + anchor_ask) / Decimal("2")
    if bps_anchor <= 0:
        return
    active_orders = strategy._managed_orders()
    publish_recovery_state_if_blocked(strategy, managed_orders_count=len(active_orders))

    skew_ctx = strategy._cached_inventory_skew(now_ns=now_ns, runtime_params=runtime_params)
    total_skew_bps = _to_decimal(skew_ctx.get("total_skew_bps", Decimal("0")))

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
    match_tol = tick / Decimal("2") if tick > 0 else Decimal("0")

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
        bid_place_rounded = _nudge_unique_price(
            price=bid_place_rounded,
            tick=tick,
            is_buy=True,
            seen=seen_buy_prices,
        )
        if bid_place_rounded is None:
            continue
        seen_buy_prices.add(str(bid_place_rounded))
        if bid_place_rounded > 0 and bid_cancel_rounded > 0:
            desired_buys.append(
                (
                    strategy._maker_instrument.make_price(bid_place_rounded),
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
        ask_place_rounded = _nudge_unique_price(
            price=ask_place_rounded,
            tick=tick,
            is_buy=False,
            seen=seen_sell_prices,
        )
        if ask_place_rounded is None:
            continue
        seen_sell_prices.add(str(ask_place_rounded))
        if ask_place_rounded > 0 and ask_cancel_rounded > 0:
            desired_sells.append(
                (
                    strategy._maker_instrument.make_price(ask_place_rounded),
                    ask_cancel_rounded,
                    match_tol,
                ),
            )

    strategy._last_pricing_debug = {
        "pricing": {
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
                ((anchor_ask - anchor_bid) / bps_anchor) * Decimal("10000")
                if bps_anchor > 0
                else None,
            ),
            "bid_edge1_cfg_bps": _decimal_to_json_str(runtime_params["bid_edge1"]),
            "ask_edge1_cfg_bps": _decimal_to_json_str(runtime_params["ask_edge1"]),
            "bid_edge1_eff_bps": _decimal_to_json_str(bid_edge1_eff_bps),
            "ask_edge1_eff_bps": _decimal_to_json_str(ask_edge1_eff_bps),
            "effective_skew_bps": _decimal_to_json_str(total_skew_bps),
            "total_skew_bps": _decimal_to_json_str(total_skew_bps),
        },
        "skew": {
            "inventory_qty": _decimal_to_json_str(skew_ctx["inventory_qty"]),
            "inventory_source": skew_ctx["inventory_source"],
            "position_qty": _decimal_to_json_str(skew_ctx["position_qty"]),
            "spot_base_total": _decimal_to_json_str(skew_ctx["spot_qty"]),
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
            "maker_age_ms": maker_age_ms,
            "reference_age_ms": reference_age_ms,
            "maker_fresh": maker_fresh,
            "reference_fresh": reference_fresh,
        },
    }

    if not desired_buys and not desired_sells:
        strategy._cancel_managed_quotes("no_targets", managed_orders=active_orders)
        strategy._last_requote_ns = now_ns
        strategy._publish_quote_cycle_event(
            now_ns=now_ns,
            quote_cycle_event=QUOTE_CYCLE_EVENT_COMPLETED,
            reason_code=REASON_COMPLETED_NO_TARGETS,
            quote_cycle_id=quote_cycle_id,
            payload={
                "cancel_count": len(active_orders),
                "place_count": 0,
                "bid_levels": 0,
                "ask_levels": 0,
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

    cancels = 0
    places = 0
    cancels += strategy._rebalance_side(
        side=OrderSide.BUY,
        active_orders=active_buys,
        desired_levels=desired_buys,
        now_ns=now_ns,
        max_age_ms=max_age_ms,
    )
    cancels += strategy._rebalance_side(
        side=OrderSide.SELL,
        active_orders=active_sells,
        desired_levels=desired_sells,
        now_ns=now_ns,
        max_age_ms=max_age_ms,
    )
    places += strategy._place_missing_levels(
        side=OrderSide.BUY,
        active_orders=active_buys,
        desired_levels=desired_buys,
        best_bid_px=best_bid_px,
        best_ask_px=best_ask_px,
    )
    places += strategy._place_missing_levels(
        side=OrderSide.SELL,
        active_orders=active_sells,
        desired_levels=desired_sells,
        best_bid_px=best_bid_px,
        best_ask_px=best_ask_px,
    )

    strategy._last_requote_ns = now_ns
    cycle_reason = REASON_COMPLETED_REBALANCED if cancels or places else REASON_COMPLETED_NO_ACTIONS
    strategy._publish_quote_cycle_event(
        now_ns=now_ns,
        quote_cycle_event=QUOTE_CYCLE_EVENT_COMPLETED,
        reason_code=cycle_reason,
        quote_cycle_id=quote_cycle_id,
        payload={
            "cancel_count": cancels,
            "place_count": places,
            "bid_levels": len(desired_buys),
            "ask_levels": len(desired_sells),
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
            managed_orders_count=len(active_buys) + len(active_sells),
        )


__all__ = [
    "handle_stale_quote_block",
    "publish_recovery_state_if_blocked",
    "refresh_quotes",
]
