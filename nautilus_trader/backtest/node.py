# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from functools import partial
from typing import List

import pandas as pd
from dask import delayed
from dask.base import normalize_token
from dask.base import tokenize

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.trading.config import StrategyFactory


# Register tokenization methods with dask
# TODO(cs): Possible move this somewhere else?

for cls in Instrument.__subclasses__():
    normalize_token.register(cls, func=cls.to_dict)


class BacktestNode:
    """
    Provides a node for managing backtest runs.
    """

    def __init__(self):
        """
        Initialize a new instance of the ``BacktestNode`` class.
        """
        pass

    @delayed(pure=True)
    def load(self, config: BacktestDataConfig):
        return self._load(config=config)

    def _load(self, config: BacktestDataConfig):
        catalog = DataCatalog(path=config.catalog_path, fs_protocol=config.catalog_fs_protocol)
        query = config.query
        return {
            "type": query["cls"],
            "data": catalog.query(**query),
            "instrument": catalog.instruments(
                instrument_ids=config.instrument_id, as_nautilus=True
            )[0],
            "client_id": config.client_id,
        }

    def create_backtest_engine(self, venues, data):
        # Configure backtest engine
        config = BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=True,
        )
        # Build the backtest engine
        engine = BacktestEngine(config=config)

        # Add data
        for d in data:
            instrument = d["instrument"]
            if instrument is not None:
                engine.add_instrument(instrument)

            if d["type"] == QuoteTick:
                engine.add_quote_ticks_objects(data=d["data"], instrument_id=d["instrument"].id)
            elif d["type"] == TradeTick:
                engine.add_trade_tick_objects(data=d["data"], instrument_id=d["instrument"].id)
            elif d["type"] == OrderBookDelta:
                engine.add_order_book_data(data=d["data"])
            # TODO(cs): Unsure if we should allow adding events to the engine directly in this way?
            # elif isinstance(d["data"][0], Event):
            #     engine.add_events(client_id=d["client_id"], data=d["data"])
            else:
                engine.add_generic_data(client_id=d["client_id"], data=d["data"])

        # Add venues
        for venue in venues:
            engine.add_venue(
                venue=Venue(venue.name),
                venue_type=VenueType[venue.venue_type],
                oms_type=OMSType[venue.oms_type],
                account_type=AccountType[venue.account_type],
                base_currency=Currency.from_str(venue.base_currency),
                starting_balances=[Money.from_str(m) for m in venue.starting_balances],
                # modules=venue.modules,  # TODO(cs): Implement next iteration
            )
        return engine

    def run_engine(self, engine, strategies):
        strategies = [StrategyFactory.create(config) for config in strategies]
        engine.run(strategies=strategies)
        data = {
            "account": pd.concat(
                [
                    engine.trader.generate_account_report(venue).assign(venue=venue)
                    for venue in engine.list_venues()
                ]
            ),
            "fills": engine.trader.generate_order_fills_report(),
            "positions": engine.trader.generate_positions_report(),
        }
        engine.dispose()
        return data

    def _run_backtest(self, venues, data, strategies, name):
        engine = self.create_backtest_engine(venues=venues, data=data)
        results = self.run_engine(engine=engine, strategies=strategies)
        return name, results

    @delayed
    def run_backtest(self, venues, data, strategies, name):
        return self._run_backtest(venues, data, strategies, name)

    def _gather(self, *results):
        return {k: v for r in results for k, v in r}

    @delayed
    def gather(self, *results):
        return self._gather(*results)

    def _check_configs(self, configs):
        if isinstance(configs, BacktestRunConfig):
            configs = [configs]

        for config in configs:
            if not isinstance(config.strategies, list):
                config.strategies = [config.strategies]
            for strategy in config.strategies:
                err = "strategy argument must be tuple of (TradingStrategy, TradingStrategyConfig)"
                assert isinstance(strategy, ImportableStrategyConfig), err

        return configs

    def build_graph(self, backtest_configs: List[BacktestRunConfig], sync=False):
        backtest_configs = self._check_configs(backtest_configs)

        results = []
        for config in backtest_configs:
            config.check(ignore=("name",))  # check all values set
            input_data = []
            for data_config in config.data:
                load_func = (
                    self._load
                    if sync
                    else partial(self.load, dask_key_name=f"load-{tokenize(data_config.query)}")
                )
                input_data.append(
                    load_func(  # type: ignore
                        data_config,
                    )
                )
            run_backtest_func = self._run_backtest if sync else self.run_backtest
            results.append(
                run_backtest_func(
                    venues=config.venues,
                    data=input_data,
                    strategies=config.strategies,
                    name=config.name or f"backtest-{tokenize(config)}",
                )
            )
        gather_func = self._gather if sync else self.gather
        return gather_func(results)
