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

import asyncio
import traceback
from decimal import Decimal

from nautilus_trader.adapters.betfair import BETFAIR
from nautilus_trader.adapters.betfair import BetfairDataClientConfig
from nautilus_trader.adapters.betfair import BetfairExecClientConfig
from nautilus_trader.adapters.betfair import BetfairInstrumentProviderConfig
from nautilus_trader.adapters.betfair import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair import BetfairLiveExecClientFactory
from nautilus_trader.adapters.betfair import get_cached_betfair_client
from nautilus_trader.adapters.betfair import get_cached_betfair_instrument_provider
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.live.node import TradingNode


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


async def main(
    instrument_config: BetfairInstrumentProviderConfig,
    log_level: str = "INFO",
) -> TradingNode:
    # from nautilus_trader.common.component import init_logging
    # from nautilus_trader.common.component import log_level_from_str
    # Connect to Betfair client early to load instruments and account currency
    # Keep a reference to the log guard to prevent it from being immediately garbage collected
    # _ = init_logging(level_stdout=log_level_from_str(log_level), print_config=True)
    client = get_cached_betfair_client(
        username=None,  # Pass here or will source from the `BETFAIR_USERNAME` env var
        password=None,  # Pass here or will source from the `BETFAIR_PASSWORD` env var
        app_key=None,  # Pass here or will source from the `BETFAIR_APP_KEY` env var
    )
    await client.connect()

    # Find instruments for a particular market_id
    provider = get_cached_betfair_instrument_provider(
        client=client,
        config=instrument_config,
    )
    await provider.load_all_async()
    instruments = provider.list_all()
    print(f"Found instruments:\n{[inst.id for inst in instruments]}")

    # Determine account currency - used in execution client
    account = await client.get_account_details()

    # Configure trading node
    config = TradingNodeConfig(
        timeout_connection=30.0,
        timeout_disconnection=10.0,
        timeout_post_stop=5.0,
        logging=LoggingConfig(log_level=log_level, use_pyo3=True),
        # cache=CacheConfig(
        #     database=DatabaseConfig(),
        #     timestamps_as_iso8601=True,
        #     buffer_interval_ms=100,
        #     flush_on_start=False,
        # ),
        exec_engine=LiveExecEngineConfig(
            reconciliation=True,
            # snapshot_orders=True,
            # snapshot_positions=True,
            # snapshot_positions_interval_secs=5.0,
            # open_check_interval_secs=5.0,
        ),
        data_clients={
            BETFAIR: BetfairDataClientConfig(
                account_currency=account.currency_code,
                instrument_config=instrument_config,
                # username=None, # 'BETFAIR_USERNAME' env var
                # password=None, # 'BETFAIR_PASSWORD' env var
                # app_key=None, # 'BETFAIR_APP_KEY' env var
                # certs_dir=None, # 'BETFAIR_CERTS_DIR' env var
                stream_conflate_ms=0,  # Ensures no stream conflation
            ),
        },
        exec_clients={
            BETFAIR: BetfairExecClientConfig(
                account_currency=account.currency_code,
                instrument_config=instrument_config,
                # username=None, # 'BETFAIR_USERNAME' env var
                # password=None, # 'BETFAIR_PASSWORD' env var
                # app_key=None, # 'BETFAIR_APP_KEY' env var
                # certs_dir=None, # 'BETFAIR_CERTS_DIR' env var
                # calculate_account_state=False,
                # request_account_state_secs=0,
                reconcile_market_ids_only=True,
            ),
        },
    )
    strategies = [
        OrderBookImbalance(
            config=OrderBookImbalanceConfig(
                instrument_id=instrument.id,
                max_trade_size=Decimal(10),
                trigger_min_size=2,
                order_id_tag=instrument.selection_id,
                dry_run=True,  # Change to False to submit new orders
            ),
        )
        for instrument in instruments
    ]

    # Set up TradingNode
    node = TradingNode(config=config)
    node.trader.add_strategies(strategies)

    # Register your client factories with the node (can take user-defined factories)
    node.add_data_client_factory(BETFAIR, BetfairLiveDataClientFactory)
    node.add_exec_client_factory(BETFAIR, BetfairLiveExecClientFactory)
    node.build()

    try:
        await node.run_async()
    except Exception as e:
        print(e)
        print(traceback.format_exc())
    finally:
        return node


if __name__ == "__main__":
    # Update the market ID with something coming up in `Next Races` from
    # https://www.betfair.com.au/exchange/plus/
    # The market ID will appear in the browser query string.
    config = BetfairInstrumentProviderConfig(
        account_currency="AUD",
        market_ids=["1.249237262"],
    )
    node = asyncio.run(main(instrument_config=config, log_level="INFO"))
    node.dispose()
