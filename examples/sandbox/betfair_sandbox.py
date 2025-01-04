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

from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factories import get_cached_betfair_client
from nautilus_trader.adapters.betfair.factories import get_cached_betfair_instrument_provider
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig
from nautilus_trader.adapters.sandbox.config import SandboxExecutionClientConfig
from nautilus_trader.adapters.sandbox.factory import SandboxLiveExecClientFactory
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.live.node import TradingNode


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


async def main(instrument_config: BetfairInstrumentProviderConfig) -> TradingNode:
    # Connect to Betfair client early to load instruments and account currency
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
    print(f"Found instruments:\n{[ins.id for ins in instruments]}")

    # Load account currency
    account_currency = await provider.get_account_currency()

    # Configure trading node
    config = TradingNodeConfig(
        timeout_connection=30.0,
        logging=LoggingConfig(log_level="DEBUG"),
        data_clients={
            "BETFAIR": BetfairDataClientConfig(
                account_currency=account_currency,
                instrument_config=instrument_config,
            ),
        },
        exec_clients={
            "BETFAIR": SandboxExecutionClientConfig(
                venue="BETFAIR",
                base_currency="AUD",
                starting_balances=["10_000 AUD"],
            ),
        },
    )
    strategies = [
        OrderBookImbalance(
            config=OrderBookImbalanceConfig(
                instrument_id=instrument.id,
                max_trade_size=Decimal(10),
                order_id_tag=instrument.selection_id,
            ),
        )
        for instrument in instruments
    ]

    # Set up TradingNode
    node = TradingNode(config=config)

    # Can manually set instruments for sandbox exec client
    for instrument in instruments:
        node.cache.add_instrument(instrument)

    node.trader.add_strategies(strategies)

    # Register your client factories with the node (can take user-defined factories)
    node.add_data_client_factory("BETFAIR", BetfairLiveDataClientFactory)
    node.add_exec_client_factory("BETFAIR", SandboxLiveExecClientFactory)
    node.build()

    try:
        await node.run_async()
    except Exception as e:
        print(e)
        print(traceback.format_exc())
    finally:
        await node.stop_async()
        await asyncio.sleep(1)
        return node


if __name__ == "__main__":
    # Update the market ID with something coming up in `Next Races` from
    # https://www.betfair.com.au/exchange/plus/
    # The market ID will appear in the browser query string.
    config = BetfairInstrumentProviderConfig(
        market_ids=["1.199513161"],
        account_currency="GBP",
    )
    node = asyncio.run(main(config))
    node.dispose()
