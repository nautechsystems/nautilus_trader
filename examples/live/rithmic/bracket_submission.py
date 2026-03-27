#!/usr/bin/env python3
"""
Example: submit and cancel a safe native Rithmic bracket order.

This example:
1. Connects to Rithmic from canonical environment variables
2. Resolves the current front-month contract for a product root
3. Waits for a live quote
4. Submits a resting native bracket entry order away from the market
5. Waits for the parent order to be accepted
6. Cancels the parent order before it can fill

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
    RITHMIC_BRACKET_ROOT                  Default: MNQ
    RITHMIC_BRACKET_EXCHANGE              Default: CME
    RITHMIC_BRACKET_QUANTITY              Default: 1
    RITHMIC_BRACKET_ENTRY_SIDE            Default: BUY
    RITHMIC_BRACKET_ENTRY_POINTS_FROM_MARKET Default: 20
    RITHMIC_BRACKET_PROFIT_TICKS          Default: 20
    RITHMIC_BRACKET_STOP_TICKS            Default: 10
    RITHMIC_BRACKET_HOLD_SECONDS          Default: 2

Notes on app credentials:
    If your credentials were not issued with app details, a temporary working
    fallback is already available, so users do not need to run Rithmic
    conformance themselves unless instructed otherwise.
    For direct API onboarding later: https://www.rithmic.com/api-request

WARNING:
    This sends a real native bracket request to the configured account.
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
DEFAULT_ENTRY_SIDE = "BUY"
DEFAULT_ENTRY_POINTS_FROM_MARKET = 20.0
DEFAULT_PROFIT_TICKS = 20
DEFAULT_STOP_TICKS = 10
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
    raise ValueError("RITHMIC_BRACKET_ENTRY_SIDE must be BUY or SELL")


def describe_execution_event(event) -> str:
    def suffix(payload) -> str:
        bracket_type = getattr(payload, "bracket_type", None)
        original_basket_id = getattr(payload, "original_basket_id", None)
        linked_basket_ids = getattr(payload, "linked_basket_ids", None)
        parts = []
        if bracket_type:
            parts.append(f"bracket_type={bracket_type}")
        if original_basket_id:
            parts.append(f"original_basket_id={original_basket_id}")
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
    if event.is_modified():
        modified = event.as_modified()
        return (
            "Modified("
            f"client_order_id={modified.client_order_id}, "
            f"venue_order_id={modified.venue_order_id}, "
            f"new_price={modified.new_price}, new_qty={modified.new_qty}"
            f"{suffix(modified)})"
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


async def wait_for_execution_event(
    execution_queue: asyncio.Queue,
    client_order_id: str,
    label: str,
    *,
    allow_submitted: bool = False,
    allow_accepted: bool = False,
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
                raise RuntimeError(f"Bracket order rejected: {rejected.reason}")
            continue

        if event.is_filled():
            filled = event.as_filled()
            if filled.client_order_id == client_order_id:
                raise RuntimeError(
                    "Bracket entry filled unexpectedly; the example expects a resting order"
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

        if allow_cancelled and event.is_cancelled():
            cancelled = event.as_cancelled()
            if cancelled.client_order_id == client_order_id:
                return event


async def main() -> None:
    profile = os.getenv("RITHMIC_PROFILE")
    product = os.getenv("RITHMIC_BRACKET_ROOT", DEFAULT_PRODUCT).strip().upper()
    exchange = os.getenv("RITHMIC_BRACKET_EXCHANGE", DEFAULT_EXCHANGE).strip().upper()
    quantity = int(os.getenv("RITHMIC_BRACKET_QUANTITY", str(DEFAULT_QUANTITY)))
    entry_side = parse_side(os.getenv("RITHMIC_BRACKET_ENTRY_SIDE", DEFAULT_ENTRY_SIDE))
    entry_points_from_market = float(
        os.getenv(
            "RITHMIC_BRACKET_ENTRY_POINTS_FROM_MARKET",
            str(DEFAULT_ENTRY_POINTS_FROM_MARKET),
        )
    )
    profit_ticks = int(os.getenv("RITHMIC_BRACKET_PROFIT_TICKS", str(DEFAULT_PROFIT_TICKS)))
    stop_ticks = int(os.getenv("RITHMIC_BRACKET_STOP_TICKS", str(DEFAULT_STOP_TICKS)))
    hold_seconds = float(os.getenv("RITHMIC_BRACKET_HOLD_SECONDS", str(DEFAULT_HOLD_SECONDS)))

    if quantity <= 0:
        raise ValueError("RITHMIC_BRACKET_QUANTITY must be positive")
    if entry_points_from_market <= 0:
        raise ValueError("RITHMIC_BRACKET_ENTRY_POINTS_FROM_MARKET must be positive")
    if profit_ticks <= 0 or stop_ticks <= 0:
        raise ValueError("RITHMIC_BRACKET_PROFIT_TICKS and RITHMIC_BRACKET_STOP_TICKS must be positive")
    if hold_seconds < 0:
        raise ValueError("RITHMIC_BRACKET_HOLD_SECONDS cannot be negative")

    print("Bracket Submission Example")
    print("=" * 50)
    print(f"Requested root: {product}:{exchange}")
    print(f"Side: {'BUY' if entry_side == OrderSide.BUY else 'SELL'}")
    print(f"Quantity: {quantity}")
    print(f"Entry offset from market: {entry_points_from_market}")
    print(f"Profit ticks: {profit_ticks}")
    print(f"Stop ticks: {stop_ticks}")
    print(f"Hold seconds before cancel: {hold_seconds}")
    print()
    print("WARNING: this submits a real native bracket request to the configured account.")
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

        if entry_side == OrderSide.BUY:
            entry_price = round_down_to_tick(
                quote.bid_price - entry_points_from_market,
                instrument.tick_size,
            )
        else:
            entry_price = round_up_to_tick(
                quote.ask_price + entry_points_from_market,
                instrument.tick_size,
            )

        if entry_price <= 0.0:
            raise RuntimeError("Calculated entry price was not positive")

        client_order_id = build_client_order_id("demo_bracket")
        print(
            "Submitting native bracket: "
            f"client_order_id={client_order_id} price={entry_price:.2f} "
            f"profit_ticks={profit_ticks} stop_ticks={stop_ticks}"
        )
        await execution_client.submit_bracket_order(
            symbol=instrument.symbol,
            exchange=instrument.exchange,
            side=entry_side,
            order_type=OrderType.LIMIT,
            quantity=quantity,
            client_order_id=client_order_id,
            profit_ticks=profit_ticks,
            stop_ticks=stop_ticks,
            price=entry_price,
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
            raise RuntimeError("Venue did not provide a venue_order_id for the bracket parent")

        if hold_seconds > 0:
            print(f"Holding bracket parent open for {hold_seconds:.1f}s before cancel...")
            await asyncio.sleep(hold_seconds)

        print(f"Cancelling bracket parent: venue_order_id={venue_order_id}")
        await execution_client.cancel_order(venue_order_id)
        await wait_for_execution_event(
            execution_queue,
            client_order_id,
            "cancel",
            allow_cancelled=True,
        )
        terminal = True

        print()
        print("Bracket example completed successfully.")
        print("The native bracket request was accepted and the parent order was cancelled before fill.")
    finally:
        print("\nCleaning up...")

        if execution_client is not None and venue_order_id and not terminal:
            try:
                print(f"Best-effort cancel for bracket parent venue_order_id={venue_order_id}")
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
