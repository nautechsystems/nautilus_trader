import dataclasses
from typing import List, Optional, Tuple, Union

from dask import delayed
import pandas as pd

from nautilus_trader.backtest.data_loader import DataCatalog
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.modules import SimulationModule
from nautilus_trader.model.c_enums.account_type import AccountTypeParser
from nautilus_trader.model.c_enums.oms_type import OMSTypeParser
from nautilus_trader.model.c_enums.venue_type import VenueTypeParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.trading.strategy import TradingStrategy


DataTypes = Union[QuoteTick, TradeTick, OrderBookDelta, GenericData]


@dataclasses.dataclass(frozen=True)
class BacktestDataConfig:
    data_type: DataTypes
    instrument_id: str
    start_time: Optional[int] = None
    end_time: Optional[int] = None
    filters: Optional[dict] = None

    @property
    def query(self):
        return dict(
            cls=self.data_type,
            instrument_ids=[self.instrument_id],
            start=self.start_time,
            end=self.end_time,
            as_nautilus=True,
        )


@dataclasses.dataclass(frozen=True)
class BacktestVenueConfig:
    name: str
    venue_type: str
    oms_type: str
    account_type: str
    base_currency: Currency
    starting_balances: List[Money]
    modules: Optional[List[SimulationModule]] = None


@dataclasses.dataclass(frozen=True)
class BacktestConfig:
    """
    Configuration for one specific backtest run (a single set of data / strategies / parameters)
    """

    venues: List[BacktestVenueConfig]
    instruments: List[Instrument]
    data_config: List[BacktestDataConfig]
    strategies: List[Tuple[TradingStrategy, dict]]

    # TODO (bm) - Implement
    def replace(self, path, value) -> "BacktestConfig":
        pass

    def create_strategies(self) -> List[TradingStrategy]:
        return [cls(**kw) for cls, kw in self.strategies]


@delayed
def load(query):
    catalog = DataCatalog()
    return query["cls"], catalog.query(**query)


@delayed
def create_backtest_engine(venues, instruments, data):
    engine = BacktestEngine(
        bypass_logging=True,
        run_analysis=False,
    )

    # Add Instruments
    for instrument in instruments:
        engine.add_instrument(instrument)

    # Add data
    for kind, vals in data:
        if kind == QuoteTick:
            engine.add_quote_ticks_objects(data=vals, instrument_id=instruments[0].id)

    # Add venues
    for venue in venues:
        engine.add_venue(
            venue=Venue(venue.name),
            venue_type=VenueTypeParser.from_str_py(venue.venue_type),
            oms_type=OMSTypeParser.from_str_py(venue.oms_type),
            account_type=AccountTypeParser.from_str_py(venue.account_type),
            base_currency=venue.base_currency,
            starting_balances=venue.starting_balances,
            modules=venue.modules,
        )
    return engine


@delayed
def run_engine(engine, strategies):
    strategies = [cls(**kw) for cls, kw in strategies]
    engine.run(strategies=strategies)
    return {
        "account": pd.concat(
            [
                engine.trader.generate_account_report(venue).assign(venue=venue)
                for venue in engine.list_venues()
            ]
        ),
        "fills": engine.trader.generate_order_fills_report(),
        "positions": engine.trader.generate_positions_report(),
        "engine": engine,
    }


def build_graph(backtest_configs):
    results = []
    for config in backtest_configs:
        input_data = []
        for data_config in config.data_config:
            input_data.append(load(data_config.query))
        engine = create_backtest_engine(
            venues=config.venues, instruments=config.instruments, data=input_data
        )
        results.append(run_engine(engine=engine, strategies=config.strategies))
    return results
