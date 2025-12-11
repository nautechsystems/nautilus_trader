#!/usr/bin/env python3
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
Ping-pong place/cancel loop using only the built Lighter adapter (no external WS libs).

- Connects to public WS via `nautilus_pyo3.lighter.LighterWebSocketClient`.
- Fetches best bid/ask per loop.
- Places $50 notional bid 5% below best bid and ask 5% above best ask.
- Cancels both immediately.
- Repeats every `--interval` seconds for `--iterations` loops.

Env:
  LIGHTER_HTTP_BASE (optional, defaults mainnet)
  LIGHTER_ACCOUNT_INDEX
  LIGHTER_API_KEY_INDEX
  LIGHTER_API_KEY_PRIVATE_KEY or LIGHTER_API_SECRET
  Signer binaries under /tmp/lighter-python/lighter/signers

"""

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import os
import time
from decimal import Decimal
from typing import Any

import requests

from nautilus_trader.adapters.lighter.constants import LIGHTER_MAINNET_HTTP_BASE
from nautilus_trader.adapters.lighter.constants import LIGHTER_TESTNET_HTTP_BASE
from nautilus_trader.adapters.lighter.providers import LighterInstrumentProvider
from nautilus_trader.adapters.lighter.signer import LighterSigner
from nautilus_trader.common.providers import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.identifiers import InstrumentId


logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
LOG = logging.getLogger("lighter_ping_pong")

MARKET_MAP = {"btc": 1, "eth": 0}


def env(name: str, default: str | None = None) -> str:
    value = os.getenv(name, default)
    if value is None:
        raise SystemExit(f"Missing required env var {name}")
    return value


def fetch_market_meta(base_http: str, market_id: int) -> dict:
    resp = requests.get(
        f"{base_http}/api/v1/orderBookDetails",
        params={"market_id": market_id},
        timeout=10,
    )
    resp.raise_for_status()
    return resp.json()["order_book_details"][0]


def fetch_nonce(base_http: str, account_index: int, api_key_index: int) -> int:
    resp = requests.get(
        f"{base_http}/api/v1/nextNonce",
        params={"account_index": account_index, "api_key_index": api_key_index},
        timeout=10,
    )
    resp.raise_for_status()
    return resp.json()["nonce"]


def send_tx(base_http: str, tx_type: int, tx_info: str) -> dict:
    resp = requests.post(
        f"{base_http}/api/v1/sendTx",
        data={"tx_type": tx_type, "tx_info": tx_info, "price_protection": True},
        timeout=10,
    )
    resp.raise_for_status()
    return resp.json()


def price_to_int(price: Decimal, decimals: int) -> int:
    return int(price * (Decimal(10) ** decimals))


def size_to_int(size: Decimal, decimals: int) -> int:
    return int(size * (Decimal(10) ** decimals))


def get_pyo3_instrument_for_market(provider: LighterInstrumentProvider, market: str) -> Any:
    """
    Get the PyO3 instrument for the specified market (btc/eth).
    """
    target_base = market.upper()
    for inst in provider.instruments_pyo3():
        inst_id_attr = getattr(inst, "id", None)
        inst_id = inst_id_attr() if callable(inst_id_attr) else inst_id_attr
        id_value = getattr(inst_id, "value", str(inst_id))
        if target_base in id_value.upper():
            return inst
    return None


async def top_of_book(
    ws_client: Any,
    market_index: int,
    instruments: list[Any],
) -> tuple[Decimal, Decimal]:
    loop = asyncio.get_event_loop()
    fut = loop.create_future()

    def handler(msg):
        if nautilus_pyo3.is_pycapsule(msg):
            try:
                data = capsule_to_data(msg)
            except Exception as e:
                LOG.warning("Failed to extract capsule: %s", e)
                return
            LOG.debug("Received capsule: type=%s", type(data).__name__)
            # QuoteTick has bid_price and ask_price directly - easiest to use
            if hasattr(data, "bid_price") and hasattr(data, "ask_price"):
                best_bid = Decimal(str(data.bid_price))
                best_ask = Decimal(str(data.ask_price))
                LOG.info("Got QuoteTick: best_bid=%s best_ask=%s", best_bid, best_ask)
                if not fut.done():
                    fut.set_result((best_bid, best_ask))
        else:
            # Log plain text errors from WS client if present
            LOG.debug("Received non-capsule message: %s", type(msg).__name__)
            try:
                payload = json.loads(msg)
                if payload.get("type") == "error":
                    LOG.warning("WS error: %s", payload)
            except Exception:
                pass

    await ws_client.connect(instruments, handler)  # Pass instruments for message parsing
    await ws_client.wait_until_active(timeout_ms=5_000)
    await ws_client.subscribe_order_book(market_index)
    try:
        return await asyncio.wait_for(fut, timeout=10)
    finally:
        await ws_client.close()


async def main() -> None:
    parser = argparse.ArgumentParser(description="Lighter ping-pong place/cancel loop.")
    parser.add_argument("--market", choices=["btc", "eth"], default="btc")
    parser.add_argument("--interval", type=float, default=10.0)
    parser.add_argument("--iterations", type=int, default=1)
    parser.add_argument("--testnet", action="store_true")
    args = parser.parse_args()

    base_http = os.getenv(
        "LIGHTER_HTTP_BASE",
        LIGHTER_TESTNET_HTTP_BASE if args.testnet else LIGHTER_MAINNET_HTTP_BASE,
    ).rstrip("/")
    market_id = MARKET_MAP[args.market]
    account_index = int(env("LIGHTER_ACCOUNT_INDEX"))
    api_key_index = int(env("LIGHTER_API_KEY_INDEX", "2"))
    api_key = env("LIGHTER_API_KEY_PRIVATE_KEY", os.getenv("LIGHTER_API_SECRET", "")).removeprefix("0x")

    meta = fetch_market_meta(base_http, market_id)
    price_decimals = int(meta["price_decimals"])
    size_decimals = int(meta["size_decimals"])
    min_base = Decimal(str(meta["min_base_amount"]))
    LOG.info("Market %s meta: price_decimals=%s size_decimals=%s min_base=%s", args.market, price_decimals, size_decimals, min_base)

    signer = LighterSigner(
        base_url=base_http.removesuffix("/api/v1"),
        account_index=account_index,
        api_key_index=api_key_index,
        api_key_private=api_key,
        chain_id=304 if not args.testnet else 300,
    )

    # Create HTTP client (PyO3) for instrument loading
    # Note: Don't pass base_url_override - the Rust client uses defaults with /api/v1 path
    http_client = nautilus_pyo3.lighter.LighterHttpClient(  # type: ignore[attr-defined]
        is_testnet=args.testnet,
    )

    # Create instrument provider and load instruments
    provider = LighterInstrumentProvider(
        http_client,
        InstrumentProviderConfig(load_all=True),
    )
    await provider.load_all_async()
    LOG.info("Loaded %d instruments", len(provider.instruments_pyo3()))

    # Get PyO3 instrument for our market
    pyo3_instrument = get_pyo3_instrument_for_market(provider, args.market)
    if not pyo3_instrument:
        raise SystemExit(f"Could not find instrument for market {args.market}")

    # Get market index from provider (with fallback to hardcoded)
    instrument_id = InstrumentId.from_str(f"{args.market.upper()}-USD-PERP.LIGHTER")
    resolved_market_id = provider.market_index_for(instrument_id)
    if resolved_market_id is None:
        resolved_market_id = market_id  # fallback to hardcoded MARKET_MAP
        LOG.warning("Could not resolve market_id from provider, using hardcoded: %s", market_id)
    else:
        market_id = resolved_market_id
    LOG.info("Using market_id=%s for %s", market_id, args.market)

    # Create WS client with HTTP client reference (required for instrument metadata)
    ws_client = nautilus_pyo3.lighter.LighterWebSocketClient(  # type: ignore[attr-defined]
        is_testnet=args.testnet,
        http_client=http_client,  # KEY FIX: pass HTTP client for market index lookup
    )

    for i in range(args.iterations):
        bid, ask = await top_of_book(ws_client, market_id, [pyo3_instrument])
        buy_px = (bid * Decimal("0.95")).quantize(Decimal(f"1e-{price_decimals}"))
        sell_px = (ask * Decimal("1.05")).quantize(Decimal(f"1e-{price_decimals}"))
        buy_sz = (Decimal(50) / buy_px).max(min_base)
        sell_sz = (Decimal(50) / sell_px).max(min_base)

        buy_px_int = price_to_int(buy_px, price_decimals)
        sell_px_int = price_to_int(sell_px, price_decimals)
        buy_sz_int = size_to_int(buy_sz, size_decimals)
        sell_sz_int = size_to_int(sell_sz, size_decimals)

        expiry_ms = int((time.time() + 600) * 1000)  # 10 minutes from now
        buy_coi = int(time.time_ns() // 1_000_000)
        sell_coi = buy_coi + 1

        LOG.info(
            "Loop %s: bid=%s ask=%s -> buy %s @ %s, sell %s @ %s",
            i,
            bid,
            ask,
            buy_sz,
            buy_px,
            sell_sz,
            sell_px,
        )

        nonce_buy = fetch_nonce(base_http, account_index, api_key_index)
        signed_buy = signer.sign_create_order(
            market_index=market_id,
            client_order_index=buy_coi,
            base_amount_int=buy_sz_int,
            price_int=buy_px_int,
            is_ask=False,
            order_type=2,  # 0=limit, 1=market, 2=stop_loss (using 2 as workaround)
            time_in_force=0,
            reduce_only=False,
            trigger_price=buy_px_int,  # Must match price for order_type=2; shows as "S/L Market"
            order_expiry=expiry_ms,
            nonce=nonce_buy,
        )
        resp_buy = send_tx(base_http, signed_buy.tx_type, signed_buy.tx_info)
        LOG.info("Submitted BUY tx=%s resp=%s", signed_buy.tx_hash, resp_buy)

        nonce_sell = fetch_nonce(base_http, account_index, api_key_index)
        signed_sell = signer.sign_create_order(
            market_index=market_id,
            client_order_index=sell_coi,
            base_amount_int=sell_sz_int,
            price_int=sell_px_int,
            is_ask=True,
            order_type=2,  # 0=limit, 1=market, 2=stop_loss (using 2 as workaround)
            time_in_force=0,
            reduce_only=False,
            trigger_price=sell_px_int,  # Must match price for order_type=2; shows as "S/L Market"
            order_expiry=expiry_ms,
            nonce=nonce_sell,
        )
        resp_sell = send_tx(base_http, signed_sell.tx_type, signed_sell.tx_info)
        LOG.info("Submitted SELL tx=%s resp=%s", signed_sell.tx_hash, resp_sell)

        # Cancel both
        nonce_cancel_buy = fetch_nonce(base_http, account_index, api_key_index)
        signed_cancel_buy = signer.sign_cancel_order(
            market_index=market_id,
            order_index=buy_coi,
            nonce=nonce_cancel_buy,
        )
        send_tx(base_http, signed_cancel_buy.tx_type, signed_cancel_buy.tx_info)
        LOG.info("Canceled BUY order_index=%s tx=%s", buy_coi, signed_cancel_buy.tx_hash)

        nonce_cancel_sell = fetch_nonce(base_http, account_index, api_key_index)
        signed_cancel_sell = signer.sign_cancel_order(
            market_index=market_id,
            order_index=sell_coi,
            nonce=nonce_cancel_sell,
        )
        send_tx(base_http, signed_cancel_sell.tx_type, signed_cancel_sell.tx_info)
        LOG.info("Canceled SELL order_index=%s tx=%s", sell_coi, signed_cancel_sell.tx_hash)

        if i < args.iterations - 1:
            await asyncio.sleep(args.interval)


if __name__ == "__main__":
    asyncio.run(main())
