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
Quick sanity check for the Lighter adapter PR1 surface.

- Loads instruments from the public `orderBooks` endpoint.
- Prints market indices and precision for a subset of instruments.
- Optionally fetches one order book snapshot to verify REST wiring.

Usage:
    python examples/live/lighter/lighter_instrument_probe.py               # testnet
    python examples/live/lighter/lighter_instrument_probe.py --mainnet    # mainnet
    python examples/live/lighter/lighter_instrument_probe.py --limit 10
    python examples/live/lighter/lighter_instrument_probe.py --snapshot
"""

from __future__ import annotations

import argparse
import asyncio
import pathlib
import sys
from typing import Any
import aiohttp

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from nautilus_trader.adapters.lighter.providers import LighterInstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
try:
    # Prefer the PyO3 HTTP client if the extension is built with Lighter bindings.
    from nautilus_trader.core import nautilus_pyo3
    from nautilus_trader.core.nautilus_pyo3 import lighter as lighter_mod

    LighterHttpClient = getattr(lighter_mod, "LighterHttpClient", None)
except Exception:  # pragma: no cover - runtime resolution
    LighterHttpClient = None  # type: ignore
    nautilus_pyo3 = None  # type: ignore


async def fetch_snapshot(client: Any, instrument_pyo3: Any) -> None:
    """
    Fetch a single order book snapshot and print best bid/ask if convertible.
    """

    if nautilus_pyo3 is None:
        print("Snapshot not attempted (PyO3 bindings unavailable).")
        return

    print(f"\nRequesting order book snapshot for {instrument_pyo3.id}...")
    try:
        deltas = await client.get_order_book_snapshot(instrument_pyo3)
        # The snapshot is represented as a batch of deltas (CLEAR + ADD entries).
        num_deltas = len(deltas.deltas) if deltas.deltas else 0
        print(f"Snapshot received with {num_deltas} deltas")

        # Find best bid and ask from the deltas
        best_bid = None
        best_ask = None
        for delta in deltas.deltas:
            if delta.action.name == "ADD":
                if delta.order.side.name == "BUY":
                    if best_bid is None or delta.order.price > best_bid.price:
                        best_bid = delta.order
                elif delta.order.side.name == "SELL":
                    if best_ask is None or delta.order.price < best_ask.price:
                        best_ask = delta.order

        if best_bid and best_ask:
            print(
                f"Best bid={best_bid.price} x {best_bid.size}, "
                f"ask={best_ask.price} x {best_ask.size}",
            )
        elif num_deltas > 1:
            print("Snapshot has deltas but no bid/ask levels found.")
        else:
            print("Snapshot has no book levels.")
    except Exception as exc:  # pragma: no cover - example script
        print(f"Snapshot fetch failed: {exc}")


async def load_via_adapter(args: argparse.Namespace) -> None:
    assert LighterHttpClient is not None

    client = LighterHttpClient(is_testnet=not args.mainnet)
    provider = LighterInstrumentProvider(client, InstrumentProviderConfig())

    await provider.load_all_async()
    instruments = provider.list_all()
    pyo3_instruments = provider.instruments_pyo3()

    network = "mainnet" if args.mainnet else "testnet"
    print(f"Loaded {len(instruments)} instruments from {network} ({client})")

    limit = args.limit or len(instruments)
    for instrument in instruments[:limit]:
        market_index = provider.market_index_for(instrument.id)
        print(
            f"{instrument.id.value:30} | market_index={market_index} | "
            f"price_decimals={instrument.price_precision} | "
            f"size_decimals={instrument.size_precision}",
        )

    if args.snapshot and pyo3_instruments:
        await fetch_snapshot(client, pyo3_instruments[0])


async def load_via_http(args: argparse.Namespace) -> None:
    base_url = (
        "https://mainnet.zklighter.elliot.ai"
        if args.mainnet
        else "https://testnet.zklighter.elliot.ai"
    )
    url = f"{base_url}/api/v1/orderBooks"

    async with aiohttp.ClientSession() as session:
        async with session.get(url, timeout=10) as resp:
            resp.raise_for_status()
            payload = await resp.json()

    books = payload.get("order_books") or payload.get("orderBooks") or []
    print(f"Loaded {len(books)} markets from {url}")

    limit = args.limit or len(books)
    for book in books[:limit]:
        print(
            f"{book.get('market_name', book.get('symbol', 'UNKNOWN')):30} | "
            f"market_index={book.get('market_index')} | "
            f"price_decimals={book.get('supported_price_decimals')} | "
            f"size_decimals={book.get('supported_size_decimals')} | "
            f"min_base_amount={book.get('min_base_amount')}",
        )


async def main(args: argparse.Namespace) -> None:
    if LighterHttpClient is None:
        print("PyO3 Lighter bindings not found; using direct HTTP probe instead.")
        await load_via_http(args)
        return

    await load_via_adapter(args)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Lighter instrument probe (public REST).")
    parser.add_argument(
        "--mainnet",
        action="store_true",
        help="Use mainnet endpoints instead of testnet.",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=5,
        help="Limit how many instruments to print (default: 5).",
    )
    parser.add_argument(
        "--snapshot",
        action="store_true",
        help="Fetch one order book snapshot for the first instrument.",
    )
    return parser.parse_args()


if __name__ == "__main__":
    asyncio.run(main(parse_args()))
