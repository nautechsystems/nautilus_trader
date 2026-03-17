"""
Unit tests for MakerV3 pricing helpers.
"""

from __future__ import annotations

from decimal import Decimal
from typing import Any

import pytest

from nautilus_trader.flux.strategies.makerv3.pricing import apply_inventory_skew_to_edges
from nautilus_trader.flux.strategies.makerv3.pricing import bps_to_price_offset
from nautilus_trader.flux.strategies.makerv3.pricing import (
    build_ladder_place_cancel_levels_from_bps,
)
from nautilus_trader.flux.strategies.makerv3.pricing import clamp_post_only_price
from nautilus_trader.flux.strategies.makerv3.pricing import nudge_unique_price
from nautilus_trader.flux.strategies.makerv3.pricing import round_price_to_tick
from nautilus_trader.flux.strategies.makerv3.pricing import to_decimal


def test_bps_to_price_offset_uses_1e4_denominator() -> None:
    offset = bps_to_price_offset(Decimal("0.0094"), Decimal(10))
    assert offset == Decimal("0.0000094")


@pytest.mark.parametrize(
    "value",
    [
        Decimal("NaN"),
        Decimal("sNaN"),
        Decimal("Infinity"),
        Decimal("-Infinity"),
        "NaN",
        "Infinity",
        "-Infinity",
        float("nan"),
        float("inf"),
        float("-inf"),
    ],
)
def test_to_decimal_rejects_non_finite_values(value: Any) -> None:
    with pytest.raises(ValueError, match="finite"):
        to_decimal(value)


def test_round_price_to_tick_supports_out_and_in_modes() -> None:
    tick = Decimal("0.01")
    price = Decimal("10.034")

    buy_out = round_price_to_tick(price, tick=tick, is_buy=True, round_in=False)
    buy_in = round_price_to_tick(price, tick=tick, is_buy=True, round_in=True)
    sell_out = round_price_to_tick(price, tick=tick, is_buy=False, round_in=False)
    sell_in = round_price_to_tick(price, tick=tick, is_buy=False, round_in=True)

    assert buy_out == Decimal("10.03")
    assert buy_in == Decimal("10.04")
    assert sell_out == Decimal("10.04")
    assert sell_in == Decimal("10.03")


def test_clamp_post_only_price_uses_tick_and_side() -> None:
    tick = Decimal("0.0001")

    bid_clamped = clamp_post_only_price(
        price=Decimal("0.0094"),
        is_buy=True,
        top_bid=Decimal("0.0093"),
        top_ask=Decimal("0.0094"),
        tick=tick,
    )
    ask_clamped = clamp_post_only_price(
        price=Decimal("0.0093"),
        is_buy=False,
        top_bid=Decimal("0.0093"),
        top_ask=Decimal("0.0094"),
        tick=tick,
    )

    assert bid_clamped == Decimal("0.0093")
    assert ask_clamped == Decimal("0.0094")


def test_nudge_unique_price_moves_less_aggressive_until_unique() -> None:
    buy_nudged = nudge_unique_price(
        price=Decimal("0.0093"),
        tick=Decimal("0.0001"),
        is_buy=True,
        seen={"0.0093", "0.0092"},
    )
    sell_nudged = nudge_unique_price(
        price=Decimal("0.0094"),
        tick=Decimal("0.0001"),
        is_buy=False,
        seen={"0.0094", "0.0095"},
    )

    assert buy_nudged == Decimal("0.0091")
    assert sell_nudged == Decimal("0.0096")


def test_build_ladder_place_cancel_levels_from_bps_matches_reference_anchor_pricing() -> None:
    bid_levels, ask_levels = build_ladder_place_cancel_levels_from_bps(
        anchor_bid=Decimal(100),
        anchor_ask=Decimal(101),
        bid_edges_bps=(Decimal(10), Decimal(20), Decimal(30)),
        ask_edges_bps=(Decimal(10), Decimal(20), Decimal(30)),
        place_edges_bps=(Decimal(2), Decimal(3), Decimal(4)),
        distances_bps=(Decimal(5), Decimal(10), Decimal(20)),
        n_orders=(2, 1, 0),
    )

    assert bid_levels == [
        (Decimal("99.8800"), Decimal("99.900")),
        (Decimal("99.82975"), Decimal("99.84975")),
        (Decimal("99.7700"), Decimal("99.800")),
    ]
    assert ask_levels == [
        (Decimal("101.1212"), Decimal("101.101")),
        (Decimal("101.17145"), Decimal("101.15125")),
        (Decimal("101.2323"), Decimal("101.202")),
    ]


def test_build_ladder_place_cancel_levels_from_bps_applies_tick_min_offset_per_level() -> None:
    bid_levels, ask_levels = build_ladder_place_cancel_levels_from_bps(
        anchor_bid=Decimal(100),
        anchor_ask=Decimal(101),
        bid_edges_bps=(Decimal(0), Decimal(0), Decimal(0)),
        ask_edges_bps=(Decimal(0), Decimal(0), Decimal(0)),
        place_edges_bps=(Decimal(0), Decimal(0), Decimal(0)),
        distances_bps=(Decimal(1), Decimal(0), Decimal(0)),
        n_orders=(2, 0, 0),
        tick=Decimal("0.5"),
    )

    assert bid_levels == [
        (Decimal(100), Decimal(100)),
        (Decimal("99.5"), Decimal("99.5")),
    ]
    assert ask_levels == [
        (Decimal(101), Decimal(101)),
        (Decimal("101.5"), Decimal("101.5")),
    ]


def test_build_ladder_place_cancel_levels_from_bps_allows_signed_bid_ask_edges() -> None:
    bid_levels, ask_levels = build_ladder_place_cancel_levels_from_bps(
        anchor_bid=Decimal(100),
        anchor_ask=Decimal(101),
        bid_edges_bps=(Decimal(-5), Decimal(0), Decimal(0)),
        ask_edges_bps=(Decimal(-5), Decimal(0), Decimal(0)),
        place_edges_bps=(Decimal(1), Decimal(0), Decimal(0)),
        distances_bps=(Decimal(0), Decimal(0), Decimal(0)),
        n_orders=(1, 0, 0),
    )

    assert bid_levels == [(Decimal("100.0400"), Decimal("100.0500"))]
    assert ask_levels == [(Decimal("100.9596"), Decimal("100.9495"))]


def test_build_ladder_place_cancel_levels_from_bps_rejects_negative_place_edges() -> None:
    with pytest.raises(ValueError, match="place edges must be non-negative"):
        build_ladder_place_cancel_levels_from_bps(
            anchor_bid=Decimal(100),
            anchor_ask=Decimal(101),
            bid_edges_bps=(Decimal(5), Decimal(0), Decimal(0)),
            ask_edges_bps=(Decimal(5), Decimal(0), Decimal(0)),
            place_edges_bps=(Decimal(-1), Decimal(0), Decimal(0)),
            distances_bps=(Decimal(0), Decimal(0), Decimal(0)),
            n_orders=(1, 0, 0),
        )


def test_apply_inventory_skew_to_edges_handles_positive_negative_and_zero() -> None:
    # Skew and edge are separate concepts: positive skew means quoted FV up /
    # quotes richer, which reduces bid edge and increases ask edge.
    bid_up, ask_up = apply_inventory_skew_to_edges(
        bid_edge_bps=Decimal(10),
        ask_edge_bps=Decimal(20),
        total_skew_bps=Decimal(3),
    )
    bid_down, ask_down = apply_inventory_skew_to_edges(
        bid_edge_bps=Decimal(10),
        ask_edge_bps=Decimal(20),
        total_skew_bps=Decimal(-3),
    )
    bid_flat, ask_flat = apply_inventory_skew_to_edges(
        bid_edge_bps=Decimal(10),
        ask_edge_bps=Decimal(20),
        total_skew_bps=Decimal(0),
    )

    assert (bid_up, ask_up) == (Decimal(7), Decimal(23))
    assert (bid_down, ask_down) == (Decimal(13), Decimal(17))
    assert (bid_flat, ask_flat) == (Decimal(10), Decimal(20))
