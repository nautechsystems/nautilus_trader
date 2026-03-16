"""
Provide pure pricing and ladder-construction helpers for MakerV3.
"""

from __future__ import annotations

from collections.abc import Iterable
from contextlib import suppress
from decimal import ROUND_CEILING
from decimal import ROUND_FLOOR
from decimal import Decimal
from typing import Any
from typing import cast


MAX_UNIQUE_PRICE_NUDGES = 200
EDGE_VALIDATION_RULE = "Bid/ask edges may be signed; place edges must be non-negative."


def to_decimal(value: Decimal | float | str) -> Decimal:
    """
    Convert a numeric-like value to `Decimal`.
    """
    parsed = value if isinstance(value, Decimal) else Decimal(str(value))
    if not parsed.is_finite():
        raise ValueError(f"Non-finite decimal value: {value!r}")
    return parsed


def to_decimal_or_none(value: Any) -> Decimal | None:
    """
    Convert a numeric-like value to `Decimal` or return `None`.
    """
    if value is None:
        return None
    with suppress(Exception):
        return to_decimal(value)
    as_decimal = getattr(value, "as_decimal", None)
    if callable(as_decimal):
        with suppress(Exception):
            return to_decimal(as_decimal())
    return None


def to_int_or_default(value: Any, default: Any) -> int:
    """
    Return `value` as an int or fallback to `default`.
    """
    try:
        return int(value)
    except Exception:
        return int(default)


def clamp_decimal(value: Decimal, lower: Decimal, upper: Decimal) -> Decimal:
    """
    Clamp a decimal between inclusive lower and upper bounds.
    """
    return max(lower, min(upper, value))


def bps_to_price_offset(anchor_price: Decimal, bps: Decimal | float | str) -> Decimal:
    """
    Return an absolute price offset for a basis-point delta.
    """
    return anchor_price * to_decimal(bps) / Decimal(10000)


def price_to_decimal(value: Any) -> Decimal:
    """
    Convert a Nautilus `Price`-like object or scalar to `Decimal`.
    """
    as_decimal = getattr(value, "as_decimal", None)
    if callable(as_decimal):
        with suppress(Exception):
            return to_decimal(as_decimal())
    return to_decimal(value)


def round_price_to_tick(
    price: Decimal,
    *,
    tick: Decimal,
    is_buy: bool,
    round_in: bool,
) -> Decimal:
    """
    Round a price to a tick using post-only in/out semantics.
    """
    if tick <= 0:
        return price
    if round_in:
        rounding = ROUND_CEILING if is_buy else ROUND_FLOOR
    else:
        rounding = ROUND_FLOOR if is_buy else ROUND_CEILING
    ticks = (price / tick).to_integral_value(rounding=rounding)
    rounded = ticks * tick
    try:
        return rounded.quantize(tick)
    except Exception:
        return rounded


def clamp_post_only_price(
    *,
    price: Decimal,
    is_buy: bool,
    top_bid: Decimal,
    top_ask: Decimal,
    tick: Decimal,
) -> Decimal:
    """
    Clamp a target price so a post-only order does not cross the spread.
    """
    if is_buy and top_ask > 0 and price >= top_ask:
        adjusted = max(Decimal(0), top_ask - tick)
        return round_price_to_tick(adjusted, tick=tick, is_buy=True, round_in=False)
    if (not is_buy) and top_bid > 0 and price <= top_bid:
        adjusted = top_bid + tick
        return round_price_to_tick(adjusted, tick=tick, is_buy=False, round_in=False)
    return price


def nudge_unique_price(
    *,
    price: Decimal,
    tick: Decimal,
    is_buy: bool,
    seen: set[str],
) -> Decimal | None:
    """
    Nudge a price by one tick until it is unique on a side.
    """
    if tick <= 0:
        key = str(price)
        if key in seen:
            return None
        return price

    out = price
    for _ in range(MAX_UNIQUE_PRICE_NUDGES):
        if out <= 0:
            return None
        key = str(out)
        if key not in seen:
            return out
        out = out - tick if is_buy else out + tick
        if out <= 0:
            return None
        out = round_price_to_tick(out, tick=tick, is_buy=is_buy, round_in=False)
    return None


def apply_inventory_skew_to_edges(
    *,
    bid_edge_bps: Decimal,
    ask_edge_bps: Decimal,
    total_skew_bps: Decimal,
) -> tuple[Decimal, Decimal]:
    """
    Apply total inventory skew to bid/ask edge basis points.
    """
    skew_abs = abs(total_skew_bps)
    if total_skew_bps > 0:
        return bid_edge_bps - skew_abs, ask_edge_bps + skew_abs
    if total_skew_bps < 0:
        return bid_edge_bps + skew_abs, ask_edge_bps - skew_abs
    return bid_edge_bps, ask_edge_bps


def validate_three_band_input[T](values: Iterable[T], name: str) -> tuple[T, T, T]:
    """
    Validate that a three-band input contains exactly three values.
    """
    parsed = tuple(values)
    if len(parsed) != 3:
        raise ValueError(f"{name}: expected three bands, received {len(parsed)}")
    return cast(tuple[T, T, T], parsed)


def _validate_non_negative(values: Iterable[Decimal | int], name: str) -> None:
    if any(v < 0 for v in values):
        raise ValueError(f"{name} must be non-negative")


def build_ladder_targets(
    anchor_bid: Decimal | float | str,
    anchor_ask: Decimal | float | str,
    bid_edges: Iterable[Decimal | float | str],
    ask_edges: Iterable[Decimal | float | str],
    distances: Iterable[Decimal | float | str],
    n_orders: Iterable[int],
) -> tuple[list[Decimal], list[Decimal]]:
    """
    Build bid/ask ladder target prices for three distance bands.
    """
    bid_edge_1, bid_edge_2, bid_edge_3 = validate_three_band_input(bid_edges, "bid_edges")
    ask_edge_1, ask_edge_2, ask_edge_3 = validate_three_band_input(ask_edges, "ask_edges")
    distance_1, distance_2, distance_3 = validate_three_band_input(distances, "distances")
    n_1, n_2, n_3 = validate_three_band_input(n_orders, "n_orders")

    bid_edges = (to_decimal(bid_edge_1), to_decimal(bid_edge_2), to_decimal(bid_edge_3))
    ask_edges = (to_decimal(ask_edge_1), to_decimal(ask_edge_2), to_decimal(ask_edge_3))
    distances = (to_decimal(distance_1), to_decimal(distance_2), to_decimal(distance_3))
    n_orders = (int(n_1), int(n_2), int(n_3))

    _validate_non_negative(distances, "distances")
    _validate_non_negative(n_orders, "n_orders")

    anchor_bid_dec = to_decimal(anchor_bid)
    anchor_ask_dec = to_decimal(anchor_ask)

    bid_targets: list[Decimal] = []
    ask_targets: list[Decimal] = []

    for band_idx in range(3):
        for level in range(n_orders[band_idx]):
            step = distances[band_idx] * level
            bid_targets.append(anchor_bid_dec - bid_edges[band_idx] - step)
            ask_targets.append(anchor_ask_dec + ask_edges[band_idx] + step)

    return bid_targets, ask_targets


def build_ladder_place_cancel_levels(
    anchor_bid: Decimal | float | str,
    anchor_ask: Decimal | float | str,
    bid_edges: Iterable[Decimal | float | str],
    ask_edges: Iterable[Decimal | float | str],
    place_edges: Iterable[Decimal | float | str],
    distances: Iterable[Decimal | float | str],
    n_orders: Iterable[int],
) -> tuple[list[tuple[Decimal, Decimal]], list[tuple[Decimal, Decimal]]]:
    """
    Build place/cancel ladder levels from absolute edge distances.
    """
    bid_edge_1, bid_edge_2, bid_edge_3 = validate_three_band_input(bid_edges, "bid_edges")
    ask_edge_1, ask_edge_2, ask_edge_3 = validate_three_band_input(ask_edges, "ask_edges")
    place_edge_1, place_edge_2, place_edge_3 = validate_three_band_input(place_edges, "place_edges")
    distance_1, distance_2, distance_3 = validate_three_band_input(distances, "distances")
    n_1, n_2, n_3 = validate_three_band_input(n_orders, "n_orders")

    bid_edges_dec = (to_decimal(bid_edge_1), to_decimal(bid_edge_2), to_decimal(bid_edge_3))
    ask_edges_dec = (to_decimal(ask_edge_1), to_decimal(ask_edge_2), to_decimal(ask_edge_3))
    place_edges_dec = (
        to_decimal(place_edge_1),
        to_decimal(place_edge_2),
        to_decimal(place_edge_3),
    )
    distances_dec = (to_decimal(distance_1), to_decimal(distance_2), to_decimal(distance_3))
    n_orders_int = (int(n_1), int(n_2), int(n_3))

    _validate_non_negative(place_edges_dec, "place edges")
    _validate_non_negative(distances_dec, "distances")
    _validate_non_negative(n_orders_int, "n_orders")

    anchor_bid_dec = to_decimal(anchor_bid)
    anchor_ask_dec = to_decimal(anchor_ask)

    bid_levels: list[tuple[Decimal, Decimal]] = []
    ask_levels: list[tuple[Decimal, Decimal]] = []

    for band_idx in range(3):
        for level in range(n_orders_int[band_idx]):
            step = distances_dec[band_idx] * level

            bid_cancel = anchor_bid_dec - bid_edges_dec[band_idx] - step
            bid_place = bid_cancel - place_edges_dec[band_idx]
            bid_levels.append((bid_place, bid_cancel))

            ask_cancel = anchor_ask_dec + ask_edges_dec[band_idx] + step
            ask_place = ask_cancel + place_edges_dec[band_idx]
            ask_levels.append((ask_place, ask_cancel))

    return bid_levels, ask_levels


def build_ladder_place_cancel_levels_from_bps(
    anchor_bid: Decimal | float | str,
    anchor_ask: Decimal | float | str,
    bid_edges_bps: Iterable[Decimal | float | str],
    ask_edges_bps: Iterable[Decimal | float | str],
    place_edges_bps: Iterable[Decimal | float | str],
    distances_bps: Iterable[Decimal | float | str],
    n_orders: Iterable[int],
    tick: Decimal | float | str = Decimal(0),
) -> tuple[list[tuple[Decimal, Decimal]], list[tuple[Decimal, Decimal]]]:
    """
    Build place/cancel ladder levels from basis-point inputs.
    """
    bid_edge_1, bid_edge_2, bid_edge_3 = validate_three_band_input(bid_edges_bps, "bid_edges_bps")
    ask_edge_1, ask_edge_2, ask_edge_3 = validate_three_band_input(ask_edges_bps, "ask_edges_bps")
    place_edge_1, place_edge_2, place_edge_3 = validate_three_band_input(
        place_edges_bps,
        "place_edges_bps",
    )
    distance_1, distance_2, distance_3 = validate_three_band_input(distances_bps, "distances_bps")
    n_1, n_2, n_3 = validate_three_band_input(n_orders, "n_orders")

    bid_edges_dec = (to_decimal(bid_edge_1), to_decimal(bid_edge_2), to_decimal(bid_edge_3))
    ask_edges_dec = (to_decimal(ask_edge_1), to_decimal(ask_edge_2), to_decimal(ask_edge_3))
    place_edges_dec = (to_decimal(place_edge_1), to_decimal(place_edge_2), to_decimal(place_edge_3))
    distances_dec = (to_decimal(distance_1), to_decimal(distance_2), to_decimal(distance_3))
    n_orders_int = (int(n_1), int(n_2), int(n_3))
    tick_dec = to_decimal(tick)

    _validate_non_negative(place_edges_dec, "place edges")
    _validate_non_negative(distances_dec, "distances")
    _validate_non_negative(n_orders_int, "n_orders")

    anchor_bid_dec = to_decimal(anchor_bid)
    anchor_ask_dec = to_decimal(anchor_ask)
    mid_primary = (anchor_bid_dec + anchor_ask_dec) / Decimal(2)
    if mid_primary <= 0:
        return [], []

    bid_levels: list[tuple[Decimal, Decimal]] = []
    ask_levels: list[tuple[Decimal, Decimal]] = []

    for band_idx in range(3):
        bid_edge_frac = bid_edges_dec[band_idx] / Decimal(10000)
        ask_edge_frac = ask_edges_dec[band_idx] / Decimal(10000)
        place_edge_pos = max(Decimal(0), place_edges_dec[band_idx])
        bid_place_edge_frac = (bid_edges_dec[band_idx] + place_edge_pos) / Decimal(10000)
        ask_place_edge_frac = (ask_edges_dec[band_idx] + place_edge_pos) / Decimal(10000)

        bid_cancel_base = anchor_bid_dec * (Decimal(1) - bid_edge_frac)
        ask_cancel_base = anchor_ask_dec * (Decimal(1) + ask_edge_frac)
        bid_place_base = anchor_bid_dec * (Decimal(1) - bid_place_edge_frac)
        ask_place_base = anchor_ask_dec * (Decimal(1) + ask_place_edge_frac)

        for level in range(n_orders_int[band_idx]):
            offset_px = (mid_primary * distances_dec[band_idx] * Decimal(level)) / Decimal(10000)
            if tick_dec > 0 and level > 0:
                min_offset = tick_dec * Decimal(level)
                if offset_px < min_offset:
                    offset_px = min_offset

            bid_cancel = bid_cancel_base - offset_px
            bid_place = bid_place_base - offset_px
            bid_levels.append((bid_place, bid_cancel))

            ask_cancel = ask_cancel_base + offset_px
            ask_place = ask_place_base + offset_px
            ask_levels.append((ask_place, ask_cancel))

    return bid_levels, ask_levels


__all__ = [
    "EDGE_VALIDATION_RULE",
    "MAX_UNIQUE_PRICE_NUDGES",
    "apply_inventory_skew_to_edges",
    "bps_to_price_offset",
    "build_ladder_place_cancel_levels",
    "build_ladder_place_cancel_levels_from_bps",
    "build_ladder_targets",
    "clamp_decimal",
    "clamp_post_only_price",
    "nudge_unique_price",
    "price_to_decimal",
    "round_price_to_tick",
    "to_decimal",
    "to_decimal_or_none",
    "to_int_or_default",
    "validate_three_band_input",
]
