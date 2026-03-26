#!/usr/bin/env python3
"""
Example: submit and cancel a safe native Rithmic OCO order pair.

This example:
1. Connects to Rithmic from canonical environment variables
2. Resolves the current front-month contract for a product root
3. Waits for a live quote
4. Submits a native OCO pair with one resting limit leg and one stop leg
5. Waits for both legs to be accepted
6. Cancels one leg and confirms the peer leg is cancelled by the venue

Required environment variables:
    RITHMIC_USERNAME
    RITHMIC_PASSWORD
    RITHMIC_SYSTEM_NAME
    RITHMIC_ACCOUNT_ID

Optional environment variables:
    RITHMIC_PROFILE
    RITHMIC_ENV
    RITHMIC_FCM_ID
    RITHMIC_IB_ID
    RITHMIC_APP_NAME
    RITHMIC_APP_VERSION
    RITHMIC_OCO_ROOT                     Default: MNQ
    RITHMIC_OCO_EXCHANGE                 Default: CME
    RITHMIC_OCO_QUANTITY                 Default: 1
    RITHMIC_OCO_SIDE                     Default: BUY
    RITHMIC_OCO_LIMIT_POINTS_FROM_MARKET Default: 20
    RITHMIC_OCO_STOP_POINTS_FROM_MARKET  Default: 20
    RITHMIC_OCO_HOLD_SECONDS             Default: 2

WARNING:
    This sends a real native OCO request to the configured account.
    Use a demo account first.
"""

from __future__ import annotations

import asyncio
import math
import os
import time

from nautilus_trader.adapters.rithmic import RithmicDataClient, RithmicGateway
from nautilus_trader.adapters.rithmic.bindings import (
    OrderSide,
    OrderType,
    RithmicExecutionClient,
    RithmicInstrumentProvider,
    TimeInForce,
)

DEFAULT_PRODUCT = "MNQ"
DEFAULT_EXCHANGE = "CME"
DEFAULT_QUANTITY = 1
DEFAULT_SIDE = "BUY"
DEFAULT_LIMIT_POINTS_FROM_MARKET = 20.0
DEFAULT_STOP_POINTS_FROM_MARKET = 20.0
DEFAULT_HOLD_SECONDS = 2.0
EVENT_TIMEOUT_SECONDS = 20.0


def build_client_order_id(prefix: str) -> str:
    return f"{prefix}_{time.time_ns()}"


def round_down_to_tick(price: float, tick_size: float) -> float:
    ticks = math.floor(price / tick_size)
    return ticks * tick_size


def round_up_to_tick(price: float, tick_size: float) -> float:
    ticks = math.ceil(price / tick_size)
    return ticks * tick_size


def parse_side(value: str) -> OrderSide:
    normalized = value.strip().upper()
    if normalized == "BUY":
        return OrderSide.BUY
    if normalized == "SELL":
        return OrderSide.SELL
    raise ValueError("RITHMIC_OCO_SIDE must be BUY or SELL")


def describe_execution_event(event) -> str:
    def suffix(payload) -> str:
        linked_basket_ids = getattr(payload, "linked_basket_ids", None)
        parts = []
        if linked_basket_ids:
            parts.append(f"linked_basket_ids={linked_basket_ids}")
        return f", {'; '.join(parts)}" if parts else ""

    if event.is_submitted():
        submitted = event.as_submitted()
        return (
            "Submitted("
            f"client_order_id={submitted.client_order_id}, "
            f"venue_order_id={submitted.venue_order_id}"
            f"{suffix(submitted)})"
        )
    if event.is_accepted():
        accepted = event.as_accepted()
        return (
            "Accepted("
            f"client_order_id={accepted.client_order_id}, "
            f"venue_order_id={accepted.venue_order_id}"
            f"{suffix(accepted)})"
        )
    if event.is_cancelled():
        cancelled = event.as_cancelled()
        return (
            "Cancelled("
            f"client_order_id={cancelled.client_order_id}, "
            f"venue_order_id={cancelled.venue_order_id}"
            f"{suffix(cancelled)})"
        )
    if event.is_filled():
        filled = event.as_filled()
        return (
            "Filled("
            f"client_order_id={filled.client_order_id}, "
            f"venue_order_id={filled.venue_order_id}, "
            f"fill_price={filled.fill_price}, fill_qty={filled.fill_qty}"
            f"{suffix(filled)})"
        )
    if event.is_rejected():
        rejected = event.as_rejected()
        return (
            "Rejected("
            f"client_order_id={rejected.client_order_id}, "
            f"reason={rejected.reason}"
            f"{suffix(rejected)})"
        )
    if event.is_error():
        return f"Error({event.as_error()})"
    return repr(event)


async def wait_for_quote(data_queue: asyncio.Queue, symbol: str, exchange: str):
    while True:
        event = await asyncio.wait_for(data_queue.get(), timeout=EVENT_TIMEOUT_SECONDS)
        if event.is_error():
            raise RuntimeError(f"Market data error: {event.as_error()}")
        if event.is_quote():
            quote = event.as_quote()
            if (
                quote.symbol == symbol
                and quote.exchange == exchange
                and quote.bid_price > 0.0
                and quote.ask_price > 0.0
            ):
                return quote


def _buffered_matching_event(
    pending_events: list[object],
    client_order_id: str,
    *,
    allow_submitted: bool,
    allow_accepted: bool,
    allow_cancelled: bool,
):
    for index, event in enumerate(pending_events):
        if event.is_error():
            raise RuntimeError(f"Execution error: {event.as_error()}")

        if event.is_rejected():
            rejected = event.as_rejected()
            if rejected.client_order_id == client_order_id:
                raise RuntimeError(f"OCO order rejected: {rejected.reason}")
            continue

        if event.is_filled():
            filled = event.as_filled()
            if filled.client_order_id == client_order_id:
                raise RuntimeError(
                    "OCO example saw an unexpected fill; choose wider offsets before retrying"
                )
            continue

        if allow_submitted and event.is_submitted():
            submitted = event.as_submitted()
            if submitted.client_order_id == client_order_id:
                return pending_events.pop(index)

        if allow_accepted and event.is_accepted():
            accepted = event.as_accepted()
            if accepted.client_order_id == client_order_id:
                return pending_events.pop(index)

        if allow_cancelled and event.is_cancelled():
            cancelled = event.as_cancelled()
            if cancelled.client_order_id == client_order_id:
                return pending_events.pop(index)

    return None


async def wait_for_execution_event(
    execution_queue: asyncio.Queue,
    pending_events: list[object],
    client_order_id: str,
    label: str,
    *,
    timeout_seconds: float = EVENT_TIMEOUT_SECONDS,
    allow_submitted: bool = False,
    allow_accepted: bool = False,
    allow_cancelled: bool = False,
):
    buffered = _buffered_matching_event(
        pending_events,
        client_order_id,
        allow_submitted=allow_submitted,
        allow_accepted=allow_accepted,
        allow_cancelled=allow_cancelled,
    )
    if buffered is not None:
        print(f"{label}: {describe_execution_event(buffered)}")
        return buffered

    while True:
        event = await asyncio.wait_for(execution_queue.get(), timeout=timeout_seconds)
        print(f"{label}: {describe_execution_event(event)}")

        if event.is_error():
            raise RuntimeError(f"Execution error: {event.as_error()}")

        if event.is_rejected():
            rejected = event.as_rejected()
            if rejected.client_order_id == client_order_id:
                raise RuntimeError(f"OCO order rejected: {rejected.reason}")
            pending_events.append(event)
            continue

        if event.is_filled():
            filled = event.as_filled()
            if filled.client_order_id == client_order_id:
                raise RuntimeError(
                    "OCO example saw an unexpected fill; choose wider offsets before retrying"
                )
            pending_events.append(event)
            continue

        if allow_submitted and event.is_submitted():
            submitted = event.as_submitted()
            if submitted.client_order_id == client_order_id:
                return event

        if allow_accepted and event.is_accepted():
            accepted = event.as_accepted()
            if accepted.client_order_id == client_order_id:
                return event

        if allow_cancelled and event.is_cancelled():
            cancelled = event.as_cancelled()
            if cancelled.client_order_id == client_order_id:
                return event

        pending_events.append(event)


async def main() -> None:
    profile = os.getenv("RITHMIC_PROFILE")
    product = os.getenv("RITHMIC_OCO_ROOT", DEFAULT_PRODUCT).strip().upper()
    exchange = os.getenv("RITHMIC_OCO_EXCHANGE", DEFAULT_EXCHANGE).strip().upper()
    quantity = int(os.getenv("RITHMIC_OCO_QUANTITY", str(DEFAULT_QUANTITY)))
    side = parse_side(os.getenv("RITHMIC_OCO_SIDE", DEFAULT_SIDE))
    limit_points_from_market = float(
        os.getenv(
            "RITHMIC_OCO_LIMIT_POINTS_FROM_MARKET",
            str(DEFAULT_LIMIT_POINTS_FROM_MARKET),
        )
    )
    stop_points_from_market = float(
        os.getenv(
            "RITHMIC_OCO_STOP_POINTS_FROM_MARKET",
            str(DEFAULT_STOP_POINTS_FROM_MARKET),
        )
    )
    hold_seconds = float(os.getenv("RITHMIC_OCO_HOLD_SECONDS", str(DEFAULT_HOLD_SECONDS)))

    if quantity <= 0:
        raise ValueError("RITHMIC_OCO_QUANTITY must be positive")
    if limit_points_from_market <= 0 or stop_points_from_market <= 0:
        raise ValueError("RITHMIC_OCO_*_POINTS_FROM_MARKET must be positive")
    if hold_seconds < 0:
        raise ValueError("RITHMIC_OCO_HOLD_SECONDS cannot be negative")

    print("OCO Submission Example")
    print("=" * 50)
    print(f"Requested root: {product}:{exchange}")
    print(f"Side: {'BUY' if side == OrderSide.BUY else 'SELL'}")
    print(f"Quantity: {quantity}")
    print(f"Limit offset from market: {limit_points_from_market}")
    print(f"Stop offset from market: {stop_points_from_market}")
    print(f"Hold seconds before cancel: {hold_seconds}")
    print()
    print("WARNING: this submits a real native OCO request to the configured account.")
    print()

    try:
        gateway = RithmicGateway.from_env(profile)
    except ValueError as e:
        print(f"Error creating gateway from env: {e}")
        print("Required environment variables:")
        if profile:
            print(f"  RITHMIC_{profile.upper()}_USERNAME")
            print(f"  RITHMIC_{profile.upper()}_PASSWORD")
            print(f"  RITHMIC_{profile.upper()}_SYSTEM_NAME")
            print(f"  RITHMIC_{profile.upper()}_ACCOUNT_ID")
        else:
            print("  RITHMIC_USERNAME")
            print("  RITHMIC_PASSWORD")
            print("  RITHMIC_SYSTEM_NAME")
            print("  RITHMIC_ACCOUNT_ID")
        return

    data_client = None
    execution_client = None
    data_subscriptions: list[tuple[str, str]] = []
    pending_events: list[object] = []
    limit_client_order_id = build_client_order_id("demo_oco_limit")
    stop_client_order_id = build_client_order_id("demo_oco_stop")
    limit_venue_order_id: str | None = None
    terminal = False

    try:
        await gateway.connect()
        account_id = gateway.account_id()
        if not account_id:
            raise RuntimeError("Gateway did not expose an account_id")

        provider = RithmicInstrumentProvider(gateway)
        instrument = await provider.load_front_month_async(product, exchange)
        print(
            "Resolved front month: "
            f"{instrument.symbol}:{instrument.exchange} "
            f"(tick_size={instrument.tick_size}, currency={instrument.currency})"
        )

        data_queue: asyncio.Queue = asyncio.Queue()
        execution_queue: asyncio.Queue = asyncio.Queue()
        loop = asyncio.get_running_loop()

        data_client = RithmicDataClient(gateway)
        execution_client = RithmicExecutionClient(gateway, account_id)

        def on_market_data(event) -> None:
            loop.call_soon_threadsafe(data_queue.put_nowait, event)

        def on_execution(event) -> None:
            loop.call_soon_threadsafe(execution_queue.put_nowait, event)

        data_client.set_data_callback(on_market_data)
        execution_client.set_execution_callback(on_execution)

        await data_client.start_event_loop()
        await execution_client.start_event_loop()

        await data_client.subscribe_quotes(instrument.symbol, instrument.exchange)
        data_subscriptions.append((instrument.symbol, instrument.exchange))

        quote = await wait_for_quote(data_queue, instrument.symbol, instrument.exchange)
        print(
            "Reference quote: "
            f"{quote.symbol}@{quote.exchange} "
            f"bid={quote.bid_price:.2f} ask={quote.ask_price:.2f}"
        )

        if side == OrderSide.BUY:
            limit_price = round_down_to_tick(
                quote.bid_price - limit_points_from_market,
                instrument.tick_size,
            )
            stop_trigger_price = round_up_to_tick(
                quote.ask_price + stop_points_from_market,
                instrument.tick_size,
            )
        else:
            limit_price = round_up_to_tick(
                quote.ask_price + limit_points_from_market,
                instrument.tick_size,
            )
            stop_trigger_price = round_down_to_tick(
                quote.bid_price - stop_points_from_market,
                instrument.tick_size,
            )

        if limit_price <= 0.0 or stop_trigger_price <= 0.0:
            raise RuntimeError("Calculated OCO prices were not positive")

        print(
            "Submitting native OCO pair: "
            f"limit_client_order_id={limit_client_order_id} limit_price={limit_price:.2f}, "
            f"stop_client_order_id={stop_client_order_id} stop_trigger_price={stop_trigger_price:.2f}"
        )
        await execution_client.submit_oco_order(
            leg1_symbol=instrument.symbol,
            leg1_exchange=instrument.exchange,
            leg1_side=side,
            leg1_order_type=OrderType.LIMIT,
            leg1_quantity=quantity,
            leg1_client_order_id=limit_client_order_id,
            leg1_price=limit_price,
            leg1_time_in_force=TimeInForce.DAY,
            leg2_symbol=instrument.symbol,
            leg2_exchange=instrument.exchange,
            leg2_side=side,
            leg2_order_type=OrderType.STOP_MARKET,
            leg2_quantity=quantity,
            leg2_client_order_id=stop_client_order_id,
            leg2_stop_price=stop_trigger_price,
            leg2_time_in_force=TimeInForce.DAY,
        )

        for client_order_id, label in (
            (limit_client_order_id, "limit-leg"),
            (stop_client_order_id, "stop-leg"),
        ):
            first_event = await wait_for_execution_event(
                execution_queue,
                pending_events,
                client_order_id,
                f"{label}-submit",
                allow_submitted=True,
                allow_accepted=True,
            )
            if first_event.is_submitted():
                submitted = first_event.as_submitted()
                if client_order_id == limit_client_order_id:
                    limit_venue_order_id = submitted.venue_order_id or limit_venue_order_id
                try:
                    accepted_event = await wait_for_execution_event(
                        execution_queue,
                        pending_events,
                        client_order_id,
                        f"{label}-accept",
                        timeout_seconds=5.0,
                        allow_accepted=True,
                    )
                    accepted = accepted_event.as_accepted()
                    if client_order_id == limit_client_order_id:
                        limit_venue_order_id = accepted.venue_order_id
                except TimeoutError:
                    tracked = execution_client.get_order(client_order_id)
                    if not tracked or not tracked.get("venue_order_id"):
                        raise RuntimeError(
                            f"Venue did not expose a tracked order id for {client_order_id}"
                        ) from None
                    print(
                        f"{label}-accept: no distinct Accepted event observed; "
                        f"continuing with tracked venue_order_id={tracked['venue_order_id']}"
                    )
                    if client_order_id == limit_client_order_id:
                        limit_venue_order_id = tracked["venue_order_id"]
            else:
                accepted = first_event.as_accepted()
                if client_order_id == limit_client_order_id:
                    limit_venue_order_id = accepted.venue_order_id

        if not limit_venue_order_id:
            tracked = execution_client.get_order(limit_client_order_id)
            if tracked:
                limit_venue_order_id = tracked.get("venue_order_id")
        if not limit_venue_order_id:
            raise RuntimeError("Venue did not provide a venue_order_id for the limit OCO leg")

        if hold_seconds > 0:
            print(f"Holding OCO pair open for {hold_seconds:.1f}s before cancel...")
            await asyncio.sleep(hold_seconds)

        print(f"Cancelling OCO limit leg: venue_order_id={limit_venue_order_id}")
        await execution_client.cancel_order(limit_venue_order_id)
        await wait_for_execution_event(
            execution_queue,
            pending_events,
            limit_client_order_id,
            "limit-leg-cancel",
            allow_cancelled=True,
        )
        await wait_for_execution_event(
            execution_queue,
            pending_events,
            stop_client_order_id,
            "stop-leg-cancel",
            allow_cancelled=True,
        )
        terminal = True

        print()
        print("OCO example completed successfully.")
        print("The native OCO pair was accepted and cancelling one leg cancelled the peer leg.")
    finally:
        print("\nCleaning up...")

        if execution_client is not None and limit_venue_order_id and not terminal:
            try:
                print(f"Best-effort cancel for OCO limit leg venue_order_id={limit_venue_order_id}")
                await execution_client.cancel_order(limit_venue_order_id)
            except Exception as e:
                print(f"Best-effort cancel failed: {e}")

        if data_client is not None:
            data_client.clear_data_callback()
            data_client.stop_event_loop()
            for symbol, exchange_name in data_subscriptions:
                try:
                    await data_client.unsubscribe(symbol, exchange_name)
                except Exception as e:
                    print(f"Failed to unsubscribe {symbol}@{exchange_name}: {e}")

        if execution_client is not None:
            execution_client.clear_execution_callback()
            execution_client.stop_event_loop()

        if gateway.is_connected():
            await gateway.disconnect()

        print("Done!")


if __name__ == "__main__":
    asyncio.run(main())
