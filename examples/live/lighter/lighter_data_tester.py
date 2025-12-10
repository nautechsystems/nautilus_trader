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
r"""
Mainnet-oriented market data smoke test for the Lighter adapter (PR2).

- Loads a filtered set of instruments (or all instruments if requested).
- Subscribes to order book deltas, trades, and market stats streams.
- Streams events through the generic ``DataTester`` actor for quick validation.

Usage:
    uv run python examples/live/lighter/lighter_data_tester.py --symbols BTC ETH
    uv run python examples/live/lighter/lighter_data_tester.py \\
        --instrument-ids BTC-USD-PERP.LIGHTER ETH-USD-PERP.LIGHTER
    uv run python examples/live/lighter/lighter_data_tester.py --testnet

"""

from __future__ import annotations

import argparse
from collections.abc import Sequence

from nautilus_trader.adapters.lighter import LIGHTER
from nautilus_trader.adapters.lighter import LighterDataClientConfig
from nautilus_trader.adapters.lighter import LighterLiveDataClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig


def parse_instrument_ids(
    instrument_ids: Sequence[str] | None,
    symbols: Sequence[str],
    quote: str,
) -> list[InstrumentId]:
    """
    Build instrument IDs from explicit values or base symbols.
    """
    if instrument_ids:
        return [InstrumentId.from_str(value) for value in instrument_ids]

    bases = [symbol.strip().upper() for symbol in symbols if symbol.strip()]
    return [InstrumentId.from_str(f"{base}-{quote}-PERP.{LIGHTER}") for base in bases]


def build_node(args: argparse.Namespace) -> TradingNode:
    instruments = parse_instrument_ids(args.instrument_ids, args.symbols, args.quote)

    filters: dict[str, list[str]] | None = None
    if not args.load_all and instruments:
        bases = []
        quotes = []
        for instrument_id in instruments:
            symbol_part = instrument_id.value.split(".")[0]
            parts = symbol_part.split("-")
            bases.append(parts[0])
            if len(parts) > 1:
                quotes.append(parts[1])

        # Use "bases" filter to match base_currency.code (e.g., "BTC")
        # Not "symbols" which matches the full symbol (e.g., "BTC-USD-PERP")
        filters = {"bases": bases}
        if quotes:
            filters["quotes"] = quotes

    # When using filters, we still need load_all=True to actually load instruments.
    # The filters will narrow down which instruments are kept after loading.
    instrument_provider = InstrumentProviderConfig(
        load_all=True,  # Must be True to load instruments (filters narrow the result)
        filters=filters,
    )

    node_config = TradingNodeConfig(
        trader_id=TraderId(args.trader_id),
        logging=LoggingConfig(
            log_level=args.log_level,
            use_pyo3=True,
        ),
        exec_engine=LiveExecEngineConfig(
            reconciliation=False,
        ),
        data_clients={
            LIGHTER: LighterDataClientConfig(
                testnet=args.testnet,
                base_url_http=args.base_url_http,
                base_url_ws=args.base_url_ws,
                instrument_provider=instrument_provider,
            ),
        },
        timeout_connection=30.0,
        timeout_disconnection=10.0,
        timeout_post_stop=5.0,
    )

    node = TradingNode(config=node_config)

    tester_config = DataTesterConfig(
        instrument_ids=instruments,
        subscribe_book_deltas=True,
        subscribe_trades=not args.no_trades,
        subscribe_mark_prices=not args.no_stats,
        subscribe_index_prices=not args.no_stats,
        subscribe_funding_rates=not args.no_funding,
        manage_book=True,
        use_pyo3_book=True,
        book_depth=args.book_depth,
        book_levels_to_print=args.book_depth,
        log_data=True,
    )
    tester = DataTester(config=tester_config)

    node.trader.add_actor(tester)
    node.add_data_client_factory(LIGHTER, LighterLiveDataClientFactory)
    node.build()

    return node


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Lighter market data tester (mainnet/testnet).",
    )
    parser.add_argument(
        "--symbols",
        nargs="+",
        default=["BTC", "ETH"],
        help="Base symbols to subscribe to (default: BTC ETH).",
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
        "--book-depth",
        type=int,
        default=10,
        help="Depth to manage/print when maintaining books (default: 10).",
    )
    parser.add_argument(
        "--no-trades",
        action="store_true",
        help="Skip trade subscriptions.",
    )
    parser.add_argument(
        "--no-stats",
        action="store_true",
        help="Skip mark/index price subscriptions.",
    )
    parser.add_argument(
        "--no-funding",
        action="store_true",
        help="Skip funding rate subscriptions.",
    )
    parser.add_argument(
        "--testnet",
        action="store_true",
        help="Connect to the Lighter testnet (default: mainnet).",
    )
    parser.add_argument(
        "--load-all",
        action="store_true",
        help="Load all instruments instead of filtering to the provided symbols.",
    )
    parser.add_argument(
        "--base-url-http",
        help="Override the HTTP endpoint (optional).",
    )
    parser.add_argument(
        "--base-url-ws",
        help="Override the WebSocket endpoint (optional).",
    )
    parser.add_argument(
        "--trader-id",
        default="LIGHTER-DATA",
        help="Trading node ID tag (default: LIGHTER-DATA).",
    )
    parser.add_argument(
        "--log-level",
        default="INFO",
        help="Application log level (default: INFO).",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    node = build_node(args)

    try:
        node.run()
    finally:
        node.dispose()


if __name__ == "__main__":
    main()
