import asyncio
import traceback
from decimal import Decimal

from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.constants import BETFAIR
from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factories import BetfairLiveExecClientFactory
from nautilus_trader.adapters.betfair.factories import get_cached_betfair_client
from nautilus_trader.adapters.betfair.factories import get_cached_betfair_instrument_provider
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.subscribe import SubscribeStrategy, SubscribeStrategyConfig
from nautilus_trader.live.config import LiveExecEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.adapters.databento.data_utils import data_path
from nautilus_trader.adapters.databento.data_utils import load_catalog
from nautilus_trader.config import StreamingConfig
from pathlib import Path
# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

catalog_base_path = Path("/Users/netomenoci/Documents/data/catalogues")
catalog_folder = "betfair_catalog"
catalog = load_catalog(catalog_folder, base_path=catalog_base_path)

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
        ),
        data_clients={
            BETFAIR: BetfairDataClientConfig(
                account_currency=account.currency_code,
                instrument_config=instrument_config,
                stream_conflate_ms=0,  # Ensures no stream conflation
            ),
        },
        exec_clients={
            BETFAIR: BetfairExecClientConfig(
                account_currency=account.currency_code,
                instrument_config=instrument_config,
                reconcile_market_ids_only=True,
            ),
        },
        streaming = StreamingConfig(
            catalog_path=catalog.path,
            fs_protocol="file",
        ),  

    )
    strategies = [
        SubscribeStrategy(
            config=SubscribeStrategyConfig(
                instrument_id=instrument.id,
                book_type = 2,
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
    config = BetfairInstrumentProviderConfig(
        account_currency="USD",
        market_types = ["MATCH_ODDS"],
    )
    node = asyncio.run(main(instrument_config=config, log_level="INFO"))
    node.dispose()
