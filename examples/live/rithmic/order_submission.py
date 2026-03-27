#!/usr/bin/env python3
"""
Example: submit, modify, and cancel a safe working order on Rithmic.

This example:
1. Connects to Rithmic from canonical environment variables
2. Resolves the current front-month contract for a product root
3. Waits for a live quote
4. Submits a resting buy limit order below the best bid
5. Modifies the working order closer to the market
6. Cancels the order before it can fill

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
    RITHMIC_ORDER_ROOT                   Default: MNQ
    RITHMIC_ORDER_EXCHANGE               Default: CME
    RITHMIC_ORDER_QUANTITY               Default: 1
    RITHMIC_INITIAL_LIMIT_POINTS_BELOW_BID   Default: 20
    RITHMIC_MODIFIED_LIMIT_POINTS_BELOW_BID  Default: 10

Notes on app credentials:
    If your credentials were not issued with app details, a temporary working
    fallback is already available, so users do not need to run Rithmic
    conformance themselves unless instructed otherwise.
    For direct API onboarding later: https://www.rithmic.com/api-request

WARNING:
    This sends a real order to the configured account. Use a demo account first.
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
DEFAULT_INITIAL_LIMIT_POINTS_BELOW_BID = 20.0
DEFAULT_MODIFIED_LIMIT_POINTS_BELOW_BID = 10.0
EVENT_TIMEOUT_SECONDS = 20.0


def build_client_order_id(prefix: str) -> str:
    return f"{prefix}_{time.time_ns()}"


def round_down_to_tick(price: float, tick_size: float) -> float:
    ticks = math.floor(price / tick_size)
    return ticks * tick_size


def describe_execution_event(event) -> str:
    if event.is_submitted():
        submitted = event.as_submitted()
        return (
            "Submitted("
            f"client_order_id={submitted.client_order_id}, "
            f"venue_order_id={submitted.venue_order_id})"
        )
    if event.is_accepted():
        accepted = event.as_accepted()
        return (
            "Accepted("
            f"client_order_id={accepted.client_order_id}, "
            f"venue_order_id={accepted.venue_order_id})"
        )
    if event.is_modified():
        modified = event.as_modified()
        return (
            "Modified("
            f"client_order_id={modified.client_order_id}, "
            f"venue_order_id={modified.venue_order_id}, "
            f"new_price={modified.new_price}, new_qty={modified.new_qty})"
        )
    if event.is_cancelled():
        cancelled = event.as_cancelled()
        return (
            "Cancelled("
            f"client_order_id={cancelled.client_order_id}, "
            f"venue_order_id={cancelled.venue_order_id})"
        )
    if event.is_filled():
        filled = event.as_filled()
        return (
            "Filled("
            f"client_order_id={filled.client_order_id}, "
            f"venue_order_id={filled.venue_order_id}, "
            f"fill_price={filled.fill_price}, fill_qty={filled.fill_qty})"
        )
    if event.is_rejected():
        rejected = event.as_rejected()
        return (
            "Rejected("
            f"client_order_id={rejected.client_order_id}, "
            f"reason={rejected.reason})"
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


async def wait_for_execution_event(
    execution_queue: asyncio.Queue,
    client_order_id: str,
    label: str,
    *,
    allow_submitted: bool = False,
    allow_accepted: bool = False,
    allow_modified: bool = False,
    allow_cancelled: bool = False,
) -> object:
    while True:
        event = await asyncio.wait_for(execution_queue.get(), timeout=EVENT_TIMEOUT_SECONDS)
        print(f"{label}: {describe_execution_event(event)}")

        if event.is_error():
            raise RuntimeError(f"Execution error: {event.as_error()}")

        if event.is_rejected():
            rejected = event.as_rejected()
            if rejected.client_order_id == client_order_id:
                raise RuntimeError(f"Order rejected: {rejected.reason}")
            continue

        if event.is_filled():
            filled = event.as_filled()
            if filled.client_order_id == client_order_id:
                raise RuntimeError(
                    "Order filled unexpectedly; the example expects a resting order"
                )
            continue

        if allow_submitted and event.is_submitted():
            submitted = event.as_submitted()
            if submitted.client_order_id == client_order_id:
                return event

        if allow_accepted and event.is_accepted():
            accepted = event.as_accepted()
            if accepted.client_order_id == client_order_id:
                return event

        if allow_modified and event.is_modified():
            modified = event.as_modified()
            if modified.client_order_id == client_order_id:
                return event

        if allow_cancelled and event.is_cancelled():
            cancelled = event.as_cancelled()
            if cancelled.client_order_id == client_order_id:
                return event


async def main() -> None:
    profile = os.getenv("RITHMIC_PROFILE")
    product = os.getenv("RITHMIC_ORDER_ROOT", DEFAULT_PRODUCT).strip().upper()
    exchange = os.getenv("RITHMIC_ORDER_EXCHANGE", DEFAULT_EXCHANGE).strip().upper()
    quantity = int(os.getenv("RITHMIC_ORDER_QUANTITY", str(DEFAULT_QUANTITY)))
    initial_points_below_bid = float(
        os.getenv(
            "RITHMIC_INITIAL_LIMIT_POINTS_BELOW_BID",
            str(DEFAULT_INITIAL_LIMIT_POINTS_BELOW_BID),
        )
    )
    modified_points_below_bid = float(
        os.getenv(
            "RITHMIC_MODIFIED_LIMIT_POINTS_BELOW_BID",
            str(DEFAULT_MODIFIED_LIMIT_POINTS_BELOW_BID),
        )
    )

    if quantity <= 0:
        raise ValueError("RITHMIC_ORDER_QUANTITY must be positive")

    if initial_points_below_bid <= 0 or modified_points_below_bid <= 0:
        raise ValueError("Limit offsets must be positive")

    print("Order Submission Example")
    print("=" * 50)
    print(f"Requested root: {product}:{exchange}")
    print(f"Quantity: {quantity}")
    print(f"Initial limit offset below bid: {initial_points_below_bid}")
    print(f"Modified limit offset below bid: {modified_points_below_bid}")
    print()
    print("WARNING: this submits a real order to the configured account.")
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
    venue_order_id: str | None = None
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

        initial_limit_price = round_down_to_tick(
            quote.bid_price - initial_points_below_bid,
            instrument.tick_size,
        )
        modified_limit_price = round_down_to_tick(
            quote.bid_price - modified_points_below_bid,
            instrument.tick_size,
        )
        if initial_limit_price <= 0.0 or modified_limit_price <= 0.0:
            raise RuntimeError("Calculated limit price was not positive")

        client_order_id = build_client_order_id("demo_limit")
        print(
            "Submitting resting limit order: "
            f"client_order_id={client_order_id} price={initial_limit_price:.2f}"
        )
        await execution_client.submit_order(
            symbol=instrument.symbol,
            exchange=instrument.exchange,
            side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=quantity,
            client_order_id=client_order_id,
            price=initial_limit_price,
            time_in_force=TimeInForce.DAY,
        )

        first_event = await wait_for_execution_event(
            execution_queue,
            client_order_id,
            "submit",
            allow_submitted=True,
            allow_accepted=True,
        )
        if first_event.is_submitted():
            submitted = first_event.as_submitted()
            venue_order_id = submitted.venue_order_id or venue_order_id
            accepted_event = await wait_for_execution_event(
                execution_queue,
                client_order_id,
                "accept",
                allow_accepted=True,
            )
            accepted = accepted_event.as_accepted()
            venue_order_id = accepted.venue_order_id
        else:
            accepted = first_event.as_accepted()
            venue_order_id = accepted.venue_order_id

        if not venue_order_id:
            raise RuntimeError("Venue did not provide a venue_order_id")

        print(
            "Modifying working order: "
            f"venue_order_id={venue_order_id} new_price={modified_limit_price:.2f}"
        )
        await execution_client.modify_order(
            venue_order_id=venue_order_id,
            symbol=instrument.symbol,
            exchange=instrument.exchange,
            new_qty=quantity,
            new_price=modified_limit_price,
            order_type=OrderType.LIMIT,
        )
        await wait_for_execution_event(
            execution_queue,
            client_order_id,
            "modify",
            allow_modified=True,
        )

        print(f"Cancelling order: venue_order_id={venue_order_id}")
        await execution_client.cancel_order(venue_order_id)
        await wait_for_execution_event(
            execution_queue,
            client_order_id,
            "cancel",
            allow_cancelled=True,
        )
        terminal = True

        print()
        print("Order example completed successfully.")
        print("The order was submitted, modified, and cancelled without filling.")
    finally:
        print("\nCleaning up...")

        if execution_client is not None and venue_order_id and not terminal:
            try:
                print(f"Best-effort cancel for venue_order_id={venue_order_id}")
                await execution_client.cancel_order(venue_order_id)
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
