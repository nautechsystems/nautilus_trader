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
Example TradingNode script demonstrating the event_slug_builder feature.

This script shows how to efficiently load niche Polymarket markets using
dynamically generated event slugs instead of downloading all 151k+ markets.

The event_slug_builder takes a fully qualified path to a callable that returns
a list of event slugs. The provider fetches only those specific events from
the Gamma API.

Usage:
    python examples/live/polymarket/polymarket_slug_builder_tester.py

Environment variables required (set these before running):
    export POLYMARKET_PK="your_private_key"
    export POLYMARKET_API_KEY="your_api_key"
    export POLYMARKET_API_SECRET="your_api_secret"
    export POLYMARKET_PASSPHRASE="your_passphrase"

To get Polymarket API credentials:
    1. Go to https://polymarket.com and connect your wallet
    2. Navigate to Settings -> API Keys
    3. Create new API credentials

"""

from nautilus_trader.adapters.polymarket import POLYMARKET
from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket import PolymarketLiveDataClientFactory
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import TraderId


# Configure the instrument provider with event_slug_builder
instrument_config = PolymarketInstrumentProviderConfig(
    event_slug_builder="examples.live.polymarket.slug_builders:build_btc_updown_slugs",
)

# Alternative slug builders you can try:
# - "examples.live.polymarket.slug_builders:build_eth_updown_slugs"  # ETH 15-min UpDown
# - "examples.live.polymarket.slug_builders:build_crypto_updown_slugs"  # BTC, ETH, SOL
# - "examples.live.polymarket.slug_builders:build_sample_slugs"  # Static sample slugs

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("SLUG-BUILDER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=False,  # Not applicable for data-only
    ),
    data_clients={
        POLYMARKET: PolymarketDataClientConfig(
            private_key=None,  # 'POLYMARKET_PK' env var
            api_key=None,  # 'POLYMARKET_API_KEY' env var
            api_secret=None,  # 'POLYMARKET_API_SECRET' env var
            passphrase=None,  # 'POLYMARKET_PASSPHRASE' env var
            instrument_config=instrument_config,
            compute_effective_deltas=True,
            update_instruments_interval_mins=15,  # Refresh every 15 mins for UpDown markets
        ),
    },
    timeout_connection=30.0,
    timeout_disconnection=10.0,
    timeout_post_stop=1.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Register your client factories with the node
node.add_data_client_factory(POLYMARKET, PolymarketLiveDataClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
