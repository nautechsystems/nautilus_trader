# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
Reconciliation functions for live trading.
"""

from decimal import Decimal

from nautilus_trader.cache.transformers import transform_instrument_to_pyo3
from nautilus_trader.common.component import Logger
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.currencies import register_currency
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderTriggered
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order


def is_within_single_unit_tolerance(
    value1: Decimal,
    value2: Decimal,
    precision: int,
) -> bool:
    """
    Check if two decimal values are within single unit tolerance based on precision.

    Handles rounding discrepancies from venues (e.g., OKX fillSz vs accFillSz).

    Parameters
    ----------
    value1 : Decimal
        The first value to compare.
    value2 : Decimal
        The second value to compare.
    precision : int
        The decimal precision for tolerance calculation.

    Returns
    -------
    bool

    """
    # Only apply tolerance for fractional quantities (precision > 0)
    if precision == 0:
        return value1 == value2  # Integer quantities require exact match

    tolerance = Decimal(10) ** -precision

    return abs(value1 - value2) <= tolerance


def get_existing_fill_for_trade_id(
    order: Order,
    trade_id: TradeId,
) -> OrderFilled | None:
    """
    Find an existing fill event for a trade ID in the order's event history.

    Parameters
    ----------
    order : Order
        The order to search.
    trade_id : TradeId
        The trade ID to find.

    Returns
    -------
    OrderFilled or ``None``

    """
    for event in order.events:
        if isinstance(event, OrderFilled) and event.trade_id == trade_id:
            return event

    return None


def create_order_rejected_event(
    order: Order,
    ts_now: int,
    report: OrderStatusReport | None = None,
    reason: str | None = None,
) -> OrderRejected:
    """
    Create an OrderRejected event for reconciliation.

    This function unifies the creation of OrderRejected events across different
    reconciliation paths (startup with report, continuous without report).

    Parameters
    ----------
    order : Order
        The order to create the rejection event for.
    ts_now : int
        The current timestamp in nanoseconds.
    report : OrderStatusReport, optional
        The order status report from the venue (if available).
    reason : str, optional
        The rejection reason (used when no report is available).

    Returns
    -------
    OrderRejected

    """
    if report:
        # Use report data when available (startup reconciliation)
        return OrderRejected(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            account_id=report.account_id,
            reason=report.cancel_reason or reason or "UNKNOWN",
            event_id=UUID4(),
            ts_event=report.ts_last,
            ts_init=ts_now,
            reconciliation=True,
        )
    else:
        # Use current timestamp and provided reason (continuous reconciliation)
        return OrderRejected(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            account_id=order.account_id,
            reason=reason or "UNKNOWN",
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
            reconciliation=True,
        )


def create_order_canceled_event(
    order: Order,
    ts_now: int,
    report: OrderStatusReport | None = None,
) -> OrderCanceled:
    """
    Create an OrderCanceled event for reconciliation.

    This function unifies the creation of OrderCanceled events across different
    reconciliation paths (startup with report, continuous without report).

    Parameters
    ----------
    order : Order
        The order to create the cancellation event for.
    ts_now : int
        The current timestamp in nanoseconds.
    report : OrderStatusReport, optional
        The order status report from the venue (if available).

    Returns
    -------
    OrderCanceled

    """
    if report:
        # Use report data when available (startup reconciliation)
        return OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            account_id=report.account_id,
            event_id=UUID4(),
            ts_event=report.ts_last,
            ts_init=ts_now,
            reconciliation=True,
        )
    else:
        # Use current timestamp (continuous reconciliation)
        return OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
            reconciliation=True,
        )


def create_order_expired_event(
    order: Order,
    ts_now: int,
    report: OrderStatusReport,
) -> OrderExpired:
    """
    Create an OrderExpired event for reconciliation.

    Parameters
    ----------
    order : Order
        The order to create the expiration event for.
    ts_now : int
        The current timestamp in nanoseconds.
    report : OrderStatusReport
        The order status report from the venue.

    Returns
    -------
    OrderExpired

    """
    return OrderExpired(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=report.instrument_id,
        client_order_id=report.client_order_id,
        venue_order_id=report.venue_order_id,
        account_id=report.account_id,
        event_id=UUID4(),
        ts_event=report.ts_last,
        ts_init=ts_now,
        reconciliation=True,
    )


def create_order_accepted_event(
    trader_id: TraderId,
    order: Order,
    ts_now: int,
    report: OrderStatusReport,
) -> OrderAccepted:
    """
    Create an OrderAccepted event for reconciliation.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the order.
    order : Order
        The order to create the acceptance event for.
    ts_now : int
        The current timestamp in nanoseconds.
    report : OrderStatusReport
        The order status report from the venue.

    Returns
    -------
    OrderAccepted

    """
    return OrderAccepted(
        trader_id=trader_id,
        strategy_id=order.strategy_id,
        instrument_id=report.instrument_id,
        client_order_id=report.client_order_id,
        venue_order_id=report.venue_order_id,
        account_id=report.account_id,
        event_id=UUID4(),
        ts_event=report.ts_accepted,
        ts_init=ts_now,
        reconciliation=True,
    )


def create_order_triggered_event(
    trader_id: TraderId,
    order: Order,
    ts_now: int,
    report: OrderStatusReport,
) -> OrderTriggered:
    """
    Create an OrderTriggered event for reconciliation.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the order.
    order : Order
        The order to create the trigger event for.
    ts_now : int
        The current timestamp in nanoseconds.
    report : OrderStatusReport
        The order status report from the venue.

    Returns
    -------
    OrderTriggered

    """
    return OrderTriggered(
        trader_id=trader_id,
        strategy_id=order.strategy_id,
        instrument_id=report.instrument_id,
        client_order_id=report.client_order_id,
        venue_order_id=report.venue_order_id,
        account_id=report.account_id,
        event_id=UUID4(),
        ts_event=report.ts_triggered,
        ts_init=ts_now,
        reconciliation=True,
    )


def create_order_updated_event(
    trader_id: TraderId,
    order: Order,
    ts_now: int,
    report: OrderStatusReport,
) -> OrderUpdated:
    """
    Create an OrderUpdated event for reconciliation.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID for the order.
    order : Order
        The order to create the update event for.
    ts_now : int
        The current timestamp in nanoseconds.
    report : OrderStatusReport
        The order status report from the venue.

    Returns
    -------
    OrderUpdated

    """
    return OrderUpdated(
        trader_id=trader_id,
        strategy_id=order.strategy_id,
        instrument_id=report.instrument_id,
        client_order_id=report.client_order_id,
        venue_order_id=report.venue_order_id,
        account_id=report.account_id,
        quantity=report.quantity,
        price=report.price,
        trigger_price=report.trigger_price,
        event_id=UUID4(),
        ts_event=report.ts_last,
        ts_init=ts_now,
        reconciliation=True,
    )


def create_order_filled_event(
    order: Order,
    ts_now: int,
    report: FillReport,
    instrument: Instrument,
) -> OrderFilled:
    """
    Create an OrderFilled event for reconciliation.

    Parameters
    ----------
    order : Order
        The order to create the fill event for.
    ts_now : int
        The current timestamp in nanoseconds.
    report : FillReport
        The fill report from the venue.
    instrument : Instrument
        The instrument for the order.

    Returns
    -------
    OrderFilled

    """
    return OrderFilled(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=report.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=report.venue_order_id,
        account_id=report.account_id,
        trade_id=report.trade_id,
        position_id=report.venue_position_id,
        order_side=order.side,
        order_type=order.order_type,
        last_qty=report.last_qty,
        last_px=report.last_px,
        currency=instrument.quote_currency,
        commission=report.commission,
        liquidity_side=report.liquidity_side,
        event_id=UUID4(),
        ts_event=report.ts_event,
        ts_init=ts_now,
        reconciliation=True,
    )


def create_inferred_order_filled_event(
    order: Order,
    ts_now: int,
    report: OrderStatusReport,
    instrument: Instrument,
) -> OrderFilled:
    """
    Create an inferred OrderFilled event for reconciliation.

    This function is used when fill details are missing but can be inferred
    from order status reports showing filled quantities.

    Parameters
    ----------
    order : Order
        The order to create the inferred fill for.
    ts_now : int
        The current timestamp in nanoseconds.
    report : OrderStatusReport
        The order status report showing filled quantity.
    instrument : Instrument
        The instrument for the order.

    Returns
    -------
    OrderFilled

    """
    # Infer liquidity side
    liquidity_side: LiquiditySide = LiquiditySide.NO_LIQUIDITY_SIDE

    if order.order_type in (
        OrderType.MARKET,
        OrderType.STOP_MARKET,
        OrderType.TRAILING_STOP_MARKET,
    ):
        liquidity_side = LiquiditySide.TAKER
    elif report.post_only:
        liquidity_side = LiquiditySide.MAKER

    # Calculate last qty
    last_qty: Quantity = instrument.make_qty(report.filled_qty - order.filled_qty)

    # Calculate last px
    if order.avg_px is None:
        # For the first fill, use the report's average price
        if report.avg_px:
            last_px: Price = instrument.make_price(report.avg_px)
        elif report.price is not None:
            # If no avg_px but we have a price (e.g., from LIMIT order), use that
            last_px = report.price
        else:
            # Retain original fallback for now
            last_px = instrument.make_price(0.0)
    else:
        report_cost: float = float(report.avg_px or 0.0) * float(report.filled_qty)
        filled_cost = float(order.avg_px) * float(order.filled_qty)
        incremental_cost = report_cost - filled_cost

        if float(last_qty) > 0:
            last_px = instrument.make_price(incremental_cost / float(last_qty))
        else:
            last_px = instrument.make_price(report.avg_px)

    notional_value: Money = instrument.notional_value(last_qty, last_px)
    commission: Money = Money(notional_value * instrument.taker_fee, instrument.quote_currency)

    return OrderFilled(
        trader_id=order.trader_id,
        strategy_id=order.strategy_id,
        instrument_id=report.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=report.venue_order_id,
        account_id=report.account_id,
        position_id=report.venue_position_id or PositionId(f"{instrument.id}-EXTERNAL"),
        trade_id=TradeId(UUID4().value),
        order_side=order.side,
        order_type=order.order_type,
        last_qty=last_qty,
        last_px=last_px,
        currency=instrument.quote_currency,
        commission=commission,
        liquidity_side=liquidity_side,
        event_id=UUID4(),
        ts_event=report.ts_last,
        ts_init=ts_now,
        reconciliation=True,
    )


def calculate_reconciliation_price(
    current_position_qty: Decimal,
    current_position_avg_px: Decimal | None,
    target_position_qty: Decimal,
    target_position_avg_px: Decimal | None,
    instrument: Instrument,
) -> Price | None:
    """
    Calculate the price needed for a reconciliation order to achieve target position.

    This is a pure function that calculates what price a fill would need to have
    to move from the current position state to the target position state with the
    correct average price, accounting for the netting simulation logic.

    Parameters
    ----------
    current_position_qty : Decimal
        The current signed position quantity (positive for long, negative for short).
    current_position_avg_px : Decimal, optional
        The current position average price (can be None for flat position).
    target_position_qty : Decimal
        The target signed position quantity.
    target_position_avg_px : Decimal, optional
        The target position average price.
    instrument : Instrument
        The instrument for price precision.

    Returns
    -------
    Price or ``None``

    Notes
    -----
    The function handles three scenarios:
    1. Flat to position: reconciliation_px = target_avg_px
    2. Position flip (sign change): reconciliation_px = target_avg_px (due to value reset in simulation)
    3. Accumulation/reduction: weighted average formula

    """
    result = nautilus_pyo3.calculate_reconciliation_price(
        current_position_qty,
        current_position_avg_px,
        target_position_qty,
        target_position_avg_px,
    )

    if result is None:
        return None

    return instrument.make_price(result)

def adjust_fills_for_partial_window_single(
    mass_status: ExecutionMassStatus,
    instrument: Instrument,
    logger: Logger | None = None,
) -> tuple[dict[VenueOrderId, OrderStatusReport], dict[VenueOrderId, list[FillReport]]]:
    """
    Adjust fills to account for incomplete position lifecycle at window start.
    """
    return adjust_fills_for_partial_window(mass_status, [instrument], logger)[instrument.id]


def adjust_fills_for_partial_window(
    mass_status: ExecutionMassStatus,
    instruments: list[Instrument],
    logger: Logger | None = None,
) -> dict[InstrumentId, tuple[dict[VenueOrderId, OrderStatusReport], dict[VenueOrderId, list[FillReport]]]]:
    """
    Adjust fills to account for incomplete position lifecycle at window start.

    This function analyzes fill reports from a lookback window and adjusts them
    to ensure the simulated position matches the venue's reported position, accounting
    for scenarios where:
    - The position lifecycle started before the lookback window
    - Multiple position lifecycles occurred (with zero-crossings)
    - Fill reports from old lifecycles should be excluded

    Parameters
    ----------
    mass_status : ExecutionMassStatus
        The execution mass status containing order, fill, and position reports.
    instruments : list[Instrument]
        The instruments to adjust fills for (all instruments in the mass status).
    logger : Logger, optional
        The logger for diagnostic output.

    Returns
    -------
    tuple[dict[VenueOrderId, OrderStatusReport], dict[VenueOrderId, list[FillReport]]]
        Tuple of (adjusted order reports, adjusted fill reports) matching venue position.

    """
    # Register all required commission currencies
    seen_currencies: set[Currency] = set()
    for fill_list in mass_status.fill_reports.values():
        for fill in fill_list:
            currency = fill.commission.currency
            if currency not in seen_currencies:
                register_currency(currency)
                if logger:
                    logger.debug(f"Registered currency: {currency}")
                seen_currencies.add(currency)

    pyo3_mass_status = mass_status.to_pyo3()

    pyo3_instruments = [transform_instrument_to_pyo3(instrument) for instrument in instruments]
    results: dict[InstrumentId, tuple[dict[VenueOrderId, OrderStatusReport], dict[VenueOrderId, list[FillReport]]]] = {}

    for instrument, pyo3_instrument in zip(instruments, pyo3_instruments, strict=False):
        assert instrument.id.value == pyo3_instrument.id.value
        pyo3_orders, pyo3_fills = nautilus_pyo3.adjust_fills_for_partial_window(
            pyo3_mass_status,
            pyo3_instrument,
        )
        orders: dict[VenueOrderId, OrderStatusReport] = {}

        for venue_order_id_str, pyo3_order in pyo3_orders.items():
            venue_order_id = VenueOrderId(venue_order_id_str)
            order = OrderStatusReport.from_pyo3(pyo3_order)
            orders[venue_order_id] = order

        fills: dict[VenueOrderId, list[FillReport]] = {}

        for venue_order_id_str, pyo3_reports in pyo3_fills.items():
            venue_order_id = VenueOrderId(venue_order_id_str)
            reports = []

            for pyo3_report in pyo3_reports:
                report = FillReport.from_pyo3(pyo3_report)
                reports.append(report)

            fills[venue_order_id] = reports

            if logger:
                logger.debug(f"Adjusted fills for {instrument.id}: {len(orders)} orders, {len(fills)} fills")

        results[instrument.id] = (orders, fills)

    return results
