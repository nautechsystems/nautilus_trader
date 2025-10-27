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

from decimal import Decimal

from nautilus_trader.adapters.interactive_brokers.common import IB
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveDataClientFactory
from nautilus_trader.adapters.sandbox.config import SandboxExecutionClientConfig
from nautilus_trader.adapters.sandbox.factory import SandboxLiveExecClientFactory
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.persistence.catalog import ParquetDataCatalog


# Load instruments from a Parquet catalog
CATALOG_PATH = "/path/to/catalog"
catalog = ParquetDataCatalog(CATALOG_PATH)
SANDBOX_INSTRUMENTS = catalog.instruments(instrument_ids=["EUR/USD.IDEALPRO"])

# Set up the Interactive Brokers gateway configuration, this is applicable only when using Docker.
dockerized_gateway = DockerizedIBGatewayConfig(
    username=None,
    password=None,
    trading_mode="paper",
    read_only_api=True,
)

instrument_provider = InteractiveBrokersInstrumentProviderConfig(
    build_futures_chain=False,
    build_options_chain=False,
    min_expiry_days=10,
    max_expiry_days=60,
    load_ids=frozenset(str(instrument.id) for instrument in SANDBOX_INSTRUMENTS),
)

# Set up the execution clients (required per venue)
SANDBOX_VENUES = {str(instrument.venue) for instrument in SANDBOX_INSTRUMENTS}
exec_clients = {}
for venue in SANDBOX_VENUES:
    exec_clients[venue] = SandboxExecutionClientConfig(
        venue=venue,
        base_currency="USD",
        starting_balances=["1_000_000 USD"],
        instrument_provider=instrument_provider,
    )


# Configure the trading node
config_node = TradingNodeConfig(
    trader_id="SANDBOX-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        IB: InteractiveBrokersDataClientConfig(
            ibg_host="127.0.0.1",
            ibg_port=7497,
            ibg_client_id=1,
            use_regular_trading_hours=True,
            instrument_provider=instrument_provider,
            dockerized_gateway=dockerized_gateway,
        ),
    },
    exec_clients=exec_clients,  # type: ignore
    data_engine=LiveDataEngineConfig(
        time_bars_timestamp_on_close=False,
        validate_data_sequence=True,
    ),
    timeout_connection=90.0,
    timeout_reconciliation=5.0,
    timeout_portfolio=5.0,
    timeout_disconnection=5.0,
    timeout_post_stop=2.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Can manually set instruments for sandbox exec client
for instrument in SANDBOX_INSTRUMENTS:
    node.cache.add_instrument(instrument)

# Instantiate strategies
strategies = {}
for instrument in SANDBOX_INSTRUMENTS:
    # Configure your strategy
    strategy_config = EMACrossConfig(
        instrument_id=instrument.id,
        bar_type=BarType.from_str(f"{instrument.id}-30-SECOND-MID-EXTERNAL"),
        trade_size=Decimal(100_000),
        subscribe_quote_ticks=True,
    )
    # Instantiate your strategy
    strategy = EMACross(config=strategy_config)
    # Add your strategies and modules
    node.trader.add_strategy(strategy)

    strategies[str(instrument.id)] = strategy


# Register client factories with the node
for data_client in config_node.data_clients:
    node.add_data_client_factory(data_client, InteractiveBrokersLiveDataClientFactory)
for exec_client in config_node.exec_clients:
    node.add_exec_client_factory(exec_client, SandboxLiveExecClientFactory)

node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
