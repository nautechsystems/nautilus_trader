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
"""
An inferred reconciliation fill must never be priced at zero.

The ``make_price(0.0)`` fallback in ``create_inferred_order_filled_event`` was guarded by
``if order.avg_px is None:``. That guard is **dead code**: ``Order.avg_px`` is a C double
(``cdef readonly double`` in ``orders/base.pxd``), so it is 0.0 before the first fill and
never None. The named fallback was unreachable, and so was the ``report.price`` arm beside
it.

The zero-price fill is real, but it was booked one branch down: with no local fills,
``filled_cost`` is 0.0; a ``None`` report avg_px makes ``report_cost`` 0.0; so the
back-solve yields ``inferred_px == 0.0``. The result is a fill booked at 0.0 that silently
corrupts the position's avg_px and realized PnL.

Reachability is not theoretical: the engine's position-reconciliation path has a "no price
information, fall back to generated MARKET order" branch that mints a FILLED
``OrderStatusReport`` with ``avg_px=None`` and no ``price``, which is exactly this input.
Venues whose position reports carry no cost basis take it during startup and feed-outage
windows.
"""

from decimal import Decimal

import pytest

from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.live.reconciliation import create_inferred_order_filled_event
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ACCOUNT_ID = AccountId("SIM-001")


def _unfilled_order():
    """
    Return an accepted order with nothing booked against it — ``avg_px == 0.0``.
    """
    order = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
    )
    # Pin the fact that makes the plan's named guard dead, so this test fails loudly if
    # `avg_px` ever becomes genuinely optional.
    assert order.avg_px == 0.0
    assert order.avg_px is not None
    assert float(order.filled_qty) == 0.0
    return order


def _filled_order(px: str, qty: int = 40_000):
    """
    Return an order with a real booked fill at an exact price, so ``avg_px`` is strictly
    positive.

    Built explicitly rather than via ``make_filled_order``, which prices the fill from the
    order and does not accept a fill price.
    """
    order = TestExecStubs.make_accepted_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(qty),
        price=AUDUSD_SIM.make_price(Decimal(px)),
        account_id=ACCOUNT_ID,
    )
    order.apply(
        TestEventStubs.order_filled(
            order=order,
            instrument=AUDUSD_SIM,
            account_id=ACCOUNT_ID,
            last_qty=Quantity.from_int(qty),
            last_px=AUDUSD_SIM.make_price(Decimal(px)),
        ),
    )
    assert float(order.avg_px) == pytest.approx(float(Decimal(px)))
    assert float(order.filled_qty) == float(qty)
    return order


def _report(*, avg_px, price, filled_qty=100_000):
    return OrderStatusReport(
        instrument_id=AUDUSD_SIM.id,
        account_id=ACCOUNT_ID,
        venue_order_id=VenueOrderId("1"),
        order_side=OrderSide.BUY,
        order_type=OrderType.MARKET,
        time_in_force=TimeInForce.IOC,
        order_status=OrderStatus.FILLED,
        quantity=Quantity.from_int(100_000),
        filled_qty=Quantity.from_int(filled_qty),
        avg_px=avg_px,
        price=price,
        report_id=UUID4(),
        ts_accepted=0,
        ts_last=0,
        ts_init=0,
    )


def test_unpriceable_inferred_fill_raises_instead_of_booking_zero():
    """
    The regression this item exists to prevent.

    Asserted as "raises ValueError" rather than "price != 0" because the caller
    `_handle_fill_quantity_mismatch` already treats ValueError as "reconciliation for this
    order failed", logging at ERROR and returning False. Refusing leaves the divergence
    visible and retryable; booking zero is unrecoverable.
    """
    order = _unfilled_order()
    report = _report(avg_px=None, price=None)

    with pytest.raises(ValueError, match="cannot infer a fill price"):
        create_inferred_order_filled_event(
            order=order,
            ts_now=0,
            report=report,
            instrument=AUDUSD_SIM,
        )


def test_report_avg_px_prices_the_fill_when_order_has_no_local_fills():
    """
    The refusal must be narrow: a priceable report still books, at the venue's average.
    """
    order = _unfilled_order()
    report = _report(avg_px=Decimal("0.80500"), price=None)

    fill = create_inferred_order_filled_event(
        order=order,
        ts_now=0,
        report=report,
        instrument=AUDUSD_SIM,
    )

    assert fill.last_px == AUDUSD_SIM.make_price(Decimal("0.80500"))
    assert fill.last_qty == Quantity.from_int(100_000)


def test_report_price_prices_the_fill_when_no_avg_px_anywhere():
    """
    The arm the dead guard used to hide: a report with a resting price but no avg_px is
    priceable, and before this change it was not reached.
    """
    order = _unfilled_order()
    report = _report(avg_px=None, price=AUDUSD_SIM.make_price(Decimal("0.79000")))

    fill = create_inferred_order_filled_event(
        order=order,
        ts_now=0,
        report=report,
        instrument=AUDUSD_SIM,
    )

    assert fill.last_px == AUDUSD_SIM.make_price(Decimal("0.79000"))


def test_a_zero_report_price_is_not_a_price():
    """
    The `report.price` rung obeys the ladder's own rule: strictly positive or unusable.

    A zero Price satisfies `is not None`, and before this change it booked exactly the
    corrupt 0.0-px fill the terminal branch exists to refuse. It must instead fall
    through to the refusal, which the caller treats as a failed (visible, retryable)
    reconciliation.
    """
    order = _unfilled_order()
    report = _report(avg_px=None, price=AUDUSD_SIM.make_price(Decimal("0.00000")))

    with pytest.raises(ValueError, match="cannot infer a fill price"):
        create_inferred_order_filled_event(
            order=order,
            ts_now=0,
            report=report,
            instrument=AUDUSD_SIM,
        )


def test_back_solve_still_prices_the_incremental_fill():
    """
    Behaviour preservation: the normal incremental case must be untouched.

    40_000 booked at 0.80000 (cost 32_000); the venue reports 100_000 at 0.83000
    (cost 83_000). The incremental 60_000 must therefore price at
    (83_000 - 32_000) / 60_000 = 0.85000.
    """
    order = _filled_order("0.80000", qty=40_000)
    report = _report(avg_px=Decimal("0.83000"), price=None, filled_qty=100_000)

    fill = create_inferred_order_filled_event(
        order=order,
        ts_now=0,
        report=report,
        instrument=AUDUSD_SIM,
    )

    assert fill.last_qty == Quantity.from_int(60_000)
    assert fill.last_px == AUDUSD_SIM.make_price(Decimal("0.85000"))


def test_non_positive_back_solve_clamps_to_venue_average():
    """
    The non-positive back-solve clamp (observed in production as an inferred fill at a
    negative price when a duplicate venue order absorbed part of the reported quantity).

    40_000 booked at 0.90000 (cost 36_000) against a venue report of 100_000 at 0.20000
    (cost 20_000) back-solves negative, so the clamp must engage and select the venue's
    average rather than emitting a negative price.
    """
    order = _filled_order("0.90000", qty=40_000)
    report = _report(avg_px=Decimal("0.20000"), price=None, filled_qty=100_000)

    fill = create_inferred_order_filled_event(
        order=order,
        ts_now=0,
        report=report,
        instrument=AUDUSD_SIM,
    )

    assert float(fill.last_px) > 0.0
    assert fill.last_px == AUDUSD_SIM.make_price(Decimal("0.20000"))


def test_priceless_report_against_filled_order_uses_the_orders_own_average():
    """
    With fills booked but no venue price, the order's own average is the best available
    estimate. Previously this case back-solved against a zero report cost and produced a
    negative price.
    """
    order = _filled_order("0.90000", qty=40_000)
    report = _report(avg_px=None, price=None, filled_qty=100_000)

    fill = create_inferred_order_filled_event(
        order=order,
        ts_now=0,
        report=report,
        instrument=AUDUSD_SIM,
    )

    assert fill.last_px == AUDUSD_SIM.make_price(Decimal("0.90000"))


@pytest.mark.parametrize("bad_avg_px", [Decimal(0), Decimal("-1.5")])
def test_a_non_positive_venue_average_is_not_a_price(bad_avg_px):
    """
    A venue average of 0 is a missing value, not a cheap fill.

    A guard of `report.avg_px is not None` would admit 0: with fills booked, the
    back-solve then goes non-positive and a clamp selecting `report.avg_px` would book
    0.0 — the exact corruption this function exists to prevent. Testing strict positivity
    is deliberate and additionally rejects a negative average, which a truthiness check
    would have let through.

    Here the order has a positive average of its own, so the correct answer is to fall back
    to it rather than to raise.
    """
    order = _filled_order("0.90000", qty=40_000)
    report = _report(avg_px=bad_avg_px, price=None, filled_qty=100_000)

    fill = create_inferred_order_filled_event(
        order=order,
        ts_now=0,
        report=report,
        instrument=AUDUSD_SIM,
    )

    assert float(fill.last_px) > 0.0, "a non-positive venue average must never be booked"
    assert fill.last_px == AUDUSD_SIM.make_price(Decimal("0.90000"))


@pytest.mark.parametrize("bad_avg_px", [Decimal(0), Decimal("-1.5")])
def test_a_non_positive_venue_average_with_nothing_to_fall_back_on_raises(bad_avg_px):
    """
    Same rule with no order average to rescue it: refuse rather than book a bad price.
    """
    order = _unfilled_order()
    report = _report(avg_px=bad_avg_px, price=None)

    with pytest.raises(ValueError, match="cannot infer a fill price"):
        create_inferred_order_filled_event(
            order=order,
            ts_now=0,
            report=report,
            instrument=AUDUSD_SIM,
        )
