#!/usr/bin/env python3
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
Lighter NVDA RWA composite market making example (Python v2).

Builds a live node with Databento ``NVDA.EQUS`` quotes as the signal instrument and Lighter
``NVDA-PERP.LIGHTER`` data and execution as the target instrument, running the native Rust
``CompositeMarketMaker`` strategy. This is the Python counterpart of the Rust tutorial binary
``examples/tutorials/src/bin/lighter_nvda_composite_mm.rs``.

The default path builds the node and exits without connecting. Pass --run to connect. Running
live submits post-only orders, so --run requires --live-orders as an explicit confirmation.

Required credential environment variables:
- DATABENTO_API_KEY.
- LIGHTER_TESTNET_ACCOUNT_INDEX, LIGHTER_TESTNET_API_KEY_INDEX, and LIGHTER_TESTNET_API_SECRET
  for the testnet environment (the default).
- LIGHTER_ACCOUNT_INDEX, LIGHTER_API_KEY_INDEX, and LIGHTER_API_SECRET for mainnet.

"""

from __future__ import annotations

import argparse
import os
from pathlib import Path

from nautilus_trader.adapters.databento import DatabentoDataClientFactory
from nautilus_trader.adapters.databento import DatabentoLiveClientConfig
from nautilus_trader.adapters.lighter import LIGHTER
from nautilus_trader.adapters.lighter import LighterDataClientConfig
from nautilus_trader.adapters.lighter import LighterDataClientFactory
from nautilus_trader.adapters.lighter import LighterEnvironment
from nautilus_trader.adapters.lighter import LighterExecClientConfig
from nautilus_trader.adapters.lighter import LighterExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TraderId
from nautilus_trader.trading import CompositeMarketMakerConfig


def main() -> None:
    args = parse_args()

    if args.run and not args.live_orders:
        raise SystemExit("Running live submits orders; pass --live-orders to confirm.")

    databento_api_key = args.databento_api_key
    if not databento_api_key:
        raise SystemExit("DATABENTO_API_KEY must be set (or pass --databento-api-key).")

    lighter_environment = lighter_environment_from_name(args.lighter_environment)
    trader_id = TraderId.from_str(args.trader_id)
    account_id = AccountId.from_str(args.account_id)
    instrument_id = InstrumentId.from_str(args.instrument)
    signal_instrument_id = InstrumentId.from_str(args.signal_instrument)

    builder = (
        LiveNode.builder("LIGHTER-NVDA-COMPOSITE-MM-001", trader_id, Environment.LIVE)
        .with_reconciliation(args.run)
        .with_delay_post_stop_secs(5)
        .add_data_client(
            None,
            DatabentoDataClientFactory(),
            DatabentoLiveClientConfig(
                api_key=databento_api_key,
                publishers_filepath=args.publishers_filepath,
                use_exchange_as_venue=True,
            ),
        )
        .add_data_client(
            None,
            LighterDataClientFactory(),
            LighterDataClientConfig(environment=lighter_environment),
        )
        .add_exec_client(
            None,
            LighterExecutionClientFactory(),
            LighterExecClientConfig(
                trader_id=trader_id,
                account_id=account_id,
                environment=lighter_environment,
            ),
        )
    )

    node = builder.build()
    node.add_native_strategy(
        "CompositeMarketMaker",
        CompositeMarketMakerConfig(
            instrument_id=instrument_id,
            signal_instrument_id=signal_instrument_id,
            max_position=Quantity.from_str(args.max_position),
            strategy_id=StrategyId.from_str("NVDA_COMPOSITE_MM-001"),
            order_id_tag="001",
            trade_size=Quantity.from_str(args.trade_size),
            half_spread_bps=args.half_spread_bps,
            inventory_skew_factor=args.inventory_skew_factor,
            signal_skew_factor=args.signal_skew_factor,
            requote_threshold_bps=args.requote_threshold_bps,
            on_cancel_resubmit=args.on_cancel_resubmit,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built Lighter NVDA composite market maker node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build or run the Lighter NVDA composite market maker (Python v2).",
    )
    parser.add_argument("--lighter-environment", choices=["testnet", "mainnet"], default="testnet")
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--account-id", default="LIGHTER-001")
    parser.add_argument("--instrument", default=f"NVDA-PERP.{LIGHTER}")
    parser.add_argument("--signal-instrument", default="NVDA.EQUS")
    parser.add_argument("--max-position", default="0.20")
    parser.add_argument("--trade-size", default="0.05")
    parser.add_argument("--half-spread-bps", type=int, default=25)
    parser.add_argument("--inventory-skew-factor", type=float, default=2.0)
    parser.add_argument("--signal-skew-factor", type=float, default=55.0)
    parser.add_argument("--requote-threshold-bps", type=int, default=5)
    parser.add_argument("--on-cancel-resubmit", action="store_true")
    parser.add_argument("--databento-api-key", default=os.environ.get("DATABENTO_API_KEY", ""))
    parser.add_argument("--publishers-filepath", type=Path, default=publishers_filepath())
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--live-orders", action="store_true")
    return parser.parse_args()


def publishers_filepath() -> Path:
    return Path(__file__).resolve().parents[3] / "crates/adapters/databento/publishers.json"


def lighter_environment_from_name(name: str) -> LighterEnvironment:
    if name == "mainnet":
        return LighterEnvironment.MAINNET

    return LighterEnvironment.TESTNET


if __name__ == "__main__":
    main()
