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
Direct WebSocket smoke test for the Lighter adapter (mainnet/testnet).

- Loads instruments via the adapter's HTTP path to obtain market indices.
- Connects the PyO3 WebSocket client and subscribes to order books, trades, and market stats.
- Prints counts (and optionally one sample) for each message type observed during the session.

Usage:
    uv run python examples/live/lighter/lighter_ws_tester.py --symbols BTC ETH
    uv run python examples/live/lighter/lighter_ws_tester.py --duration 60 --no-stats
    uv run python examples/live/lighter/lighter_ws_tester.py --testnet
"""

from __future__ import annotations

import argparse
import asyncio
import pathlib
import sys
from collections import Counter
from typing import Any
from typing import Iterable

REPO_ROOT = pathlib.Path(__file__).resolve().parents[3]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from nautilus_trader.adapters.lighter import LIGHTER
from nautilus_trader.adapters.lighter.providers import LighterInstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.identifiers import InstrumentId

try:
    from nautilus_trader.core.nautilus_pyo3 import lighter as lighter_mod
except Exception as exc:  # pragma: no cover - runtime guard for missing bindings
    raise SystemExit(
        "Lighter PyO3 bindings are unavailable. Build extensions with `make build-debug` "
        "before running this tester.",
    ) from exc


def parse_instrument_ids(
    instrument_ids: Iterable[str] | None,
    symbols: Iterable[str],
    quote: str,
) -> list[InstrumentId]:
    """
    Build instrument IDs from explicit values or base symbols.
    """

    if instrument_ids:
        return [InstrumentId.from_str(value) for value in instrument_ids]

    bases = [symbol.strip().upper() for symbol in symbols if symbol.strip()]
    return [InstrumentId.from_str(f"{base}-{quote}-PERP.{LIGHTER}") for base in bases]


def filter_config(instruments: list[InstrumentId], load_all: bool) -> InstrumentProviderConfig:
    if load_all or not instruments:
        return InstrumentProviderConfig(load_all=True)

    symbols = []
    quotes = []
    for instrument_id in instruments:
        symbol_part = instrument_id.value.split(".")[0]
        parts = symbol_part.split("-")
        symbols.append(parts[0])
        if len(parts) > 1:
            quotes.append(parts[1])

    filters: dict[str, list[str]] = {"symbols": symbols}
    if quotes:
        filters["quotes"] = quotes

    return InstrumentProviderConfig(load_all=False, filters=filters)


def select_python_instruments(
    provider: LighterInstrumentProvider,
    instrument_ids: list[InstrumentId],
    max_markets: int | None,
) -> list[Any]:
    """
    Resolve Python instruments that were loaded and requested.
    """

    selected = []
    lookup = provider.get_all()

    for instrument_id in instrument_ids:
        instrument = lookup.get(instrument_id)
        if instrument:
            selected.append(instrument)

    if not selected:
        selected = list(lookup.values())

    if max_markets is not None:
        selected = selected[:max_markets]

    return selected


def select_pyo3_instruments(
    provider: LighterInstrumentProvider,
    python_instruments: list[Any],
) -> list[Any]:
    """
    Match PyO3 instrument handles to the selected Python instruments.
    """

    allowed = {instrument.id.value for instrument in python_instruments}
    resolved: list[Any] = []

    for instrument in provider.instruments_pyo3():
        instrument_id_attr = getattr(instrument, "id", None)
        try:
            instrument_id = instrument_id_attr() if callable(instrument_id_attr) else instrument_id_attr
        except Exception:  # pragma: no cover - defensive
            continue

        key = getattr(instrument_id, "value", str(instrument_id))
        if key in allowed:
            resolved.append(instrument)

    return resolved


async def subscribe_streams(
    ws_client: Any,
    provider: LighterInstrumentProvider,
    instruments: list[Any],
    args: argparse.Namespace,
) -> None:
    for instrument in instruments:
        market_index = provider.market_index_for(instrument.id)
        if market_index is None:
            continue

        if not args.no_book:
            await ws_client.subscribe_order_book(market_index)
        if not args.no_trades:
            await ws_client.subscribe_trades(market_index)
        if not args.no_stats:
            await ws_client.subscribe_market_stats(market_index)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Direct Lighter WS tester (no TradingNode required).",
    )
    parser.add_argument(
        "--symbols",
        nargs="+",
        default=["BTC", "ETH"],
        help="Base symbols to subscribe to when building instrument IDs (default: BTC ETH).",
    )
    parser.add_argument(
        "--instrument-ids",
        nargs="+",
        help="Explicit instrument IDs to subscribe to; overrides --symbols.",
    )
    parser.add_argument(
        "--quote",
        default="USD",
        help="Quote currency when constructing instrument IDs (default: USD).",
    )
    parser.add_argument(
        "--max-markets",
        type=int,
        default=2,
        help="Subscribe to at most this many markets (default: 2).",
    )
    parser.add_argument(
        "--duration",
        type=int,
        default=30,
        help="How long to stream before exiting (seconds, default: 30).",
    )
    parser.add_argument(
        "--sample",
        action="store_true",
        help="Print the first message observed for each type.",
    )
    parser.add_argument(
        "--no-book",
        action="store_true",
        help="Skip order book subscriptions.",
    )
    parser.add_argument(
        "--no-trades",
        action="store_true",
        help="Skip trade subscriptions.",
    )
    parser.add_argument(
        "--no-stats",
        action="store_true",
        help="Skip mark/index/funding subscriptions.",
    )
    parser.add_argument(
        "--testnet",
        action="store_true",
        help="Use Lighter testnet instead of mainnet.",
    )
    parser.add_argument(
        "--load-all",
        action="store_true",
        help="Ignore symbol filters and load every market definition.",
    )
    parser.add_argument(
        "--base-url-http",
        help="Override the HTTP endpoint (optional).",
    )
    parser.add_argument(
        "--base-url-ws",
        help="Override the WebSocket endpoint (optional).",
    )
    return parser.parse_args()


async def main(args: argparse.Namespace) -> None:
    instrument_ids = parse_instrument_ids(args.instrument_ids, args.symbols, args.quote)
    provider = LighterInstrumentProvider(
        lighter_mod.LighterHttpClient(
            is_testnet=args.testnet,
            base_url_override=args.base_url_http,
        ),
        filter_config(instrument_ids, args.load_all),
    )

    await provider.load_all_async()

    python_instruments = select_python_instruments(
        provider=provider,
        instrument_ids=instrument_ids,
        max_markets=args.max_markets,
    )
    pyo3_instruments = select_pyo3_instruments(provider, python_instruments)

    if not pyo3_instruments:
        raise SystemExit("No instruments matched the provided filters.")

    counts: Counter[str] = Counter()
    samples: dict[str, Any] = {}

    def handle_message(msg: Any) -> None:
        if nautilus_pyo3.is_pycapsule(msg):
            data = capsule_to_data(msg)
            kind = data.__class__.__name__
            counts[kind] += 1
            if args.sample and kind not in samples:
                samples[kind] = data
                print(f"\nSample {kind}: {data}")
        elif isinstance(msg, nautilus_pyo3.FundingRateUpdate):
            kind = "FundingRateUpdate"
            counts[kind] += 1
            if args.sample and kind not in samples:
                samples[kind] = msg
                print(f"\nSample {kind}: {msg}")
        else:
            kind = type(msg).__name__
            counts[kind] += 1

    ws_client = lighter_mod.LighterWebSocketClient(
        is_testnet=args.testnet,
        base_url_override=args.base_url_ws,
        http_client=provider._client,  # Reuse HTTP client metadata
    )

    await ws_client.connect(pyo3_instruments, handle_message)
    await ws_client.wait_until_active(timeout_ms=10_000)

    await subscribe_streams(ws_client, provider, python_instruments, args)

    network = "testnet" if args.testnet else "mainnet"
    markets = [inst.id.value for inst in python_instruments]
    print(
        f"Streaming {network} data for {len(markets)} market(s): "
        f"{', '.join(markets)}\n",
    )

    try:
        await asyncio.sleep(args.duration)
    finally:
        await ws_client.close()

    print("\nMessage counts:")
    for kind, count in counts.most_common():
        print(f"- {kind}: {count}")


if __name__ == "__main__":
    asyncio.run(main(parse_args()))
