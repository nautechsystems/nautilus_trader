# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

from decimal import Decimal

from nautilus_trader.flux.strategies.makerv3.single_leg_quoter import _bps_to_price_offset
from nautilus_trader.flux.strategies.makerv3.single_leg_quoter import _clamp_post_only_price
from nautilus_trader.flux.strategies.makerv3.single_leg_quoter import _nudge_unique_price
from nautilus_trader.flux.strategies.makerv3.single_leg_quoter import _round_price_to_tick
from nautilus_trader.flux.strategies.makerv3.single_leg_quoter import build_ladder_place_cancel_levels_from_bps


def test_bps_to_price_offset_uses_1e4_denominator() -> None:
    offset = _bps_to_price_offset(Decimal("0.0094"), Decimal("10"))
    assert offset == Decimal("0.0000094")


def test_round_price_to_tick_supports_out_and_in_modes() -> None:
    tick = Decimal("0.01")
    price = Decimal("10.034")

    buy_out = _round_price_to_tick(price, tick=tick, is_buy=True, round_in=False)
    buy_in = _round_price_to_tick(price, tick=tick, is_buy=True, round_in=True)
    sell_out = _round_price_to_tick(price, tick=tick, is_buy=False, round_in=False)
    sell_in = _round_price_to_tick(price, tick=tick, is_buy=False, round_in=True)

    assert buy_out == Decimal("10.03")
    assert buy_in == Decimal("10.04")
    assert sell_out == Decimal("10.04")
    assert sell_in == Decimal("10.03")


def test_clamp_post_only_price_uses_tick_and_side() -> None:
    tick = Decimal("0.0001")

    bid_clamped = _clamp_post_only_price(
        price=Decimal("0.0094"),
        is_buy=True,
        top_bid=Decimal("0.0093"),
        top_ask=Decimal("0.0094"),
        tick=tick,
    )
    ask_clamped = _clamp_post_only_price(
        price=Decimal("0.0093"),
        is_buy=False,
        top_bid=Decimal("0.0093"),
        top_ask=Decimal("0.0094"),
        tick=tick,
    )

    assert bid_clamped == Decimal("0.0093")
    assert ask_clamped == Decimal("0.0094")


def test_nudge_unique_price_moves_less_aggressive_until_unique() -> None:
    buy_nudged = _nudge_unique_price(
        price=Decimal("0.0093"),
        tick=Decimal("0.0001"),
        is_buy=True,
        seen={"0.0093", "0.0092"},
    )
    sell_nudged = _nudge_unique_price(
        price=Decimal("0.0094"),
        tick=Decimal("0.0001"),
        is_buy=False,
        seen={"0.0094", "0.0095"},
    )

    assert buy_nudged == Decimal("0.0091")
    assert sell_nudged == Decimal("0.0096")


def test_build_ladder_place_cancel_levels_from_bps_matches_reference_anchor_pricing() -> None:
    bid_levels, ask_levels = build_ladder_place_cancel_levels_from_bps(
        anchor_bid=Decimal("100"),
        anchor_ask=Decimal("101"),
        bid_edges_bps=(Decimal("10"), Decimal("20"), Decimal("30")),
        ask_edges_bps=(Decimal("10"), Decimal("20"), Decimal("30")),
        place_edges_bps=(Decimal("2"), Decimal("3"), Decimal("4")),
        distances_bps=(Decimal("5"), Decimal("10"), Decimal("20")),
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
