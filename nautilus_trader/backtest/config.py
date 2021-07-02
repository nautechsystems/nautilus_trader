import dataclasses
from typing import List, Optional, Tuple

from dask.base import normalize_token
from dask.base import tokenize
from dask import delayed
import pandas as pd

from nautilus_trader.backtest.data_loader import DataCatalog
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.modules import SimulationModule
from nautilus_trader.model.c_enums.account_type import AccountTypeParser
from nautilus_trader.model.c_enums.oms_type import OMSTypeParser
from nautilus_trader.model.c_enums.venue_type import VenueTypeParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.tick import QuoteTick


PARTIAL_SUFFIX = "Partial-"


class Partialable:
    def missing(self):
        return [x for x in self.__dataclass_fields__ if getattr(self, x) is None]

    def is_partial(self):
        return any(self.missing())

    def check(self, ignore=None):
        missing = [m for m in self.missing() if m not in (ignore or {})]
        if missing:
            raise AssertionError(f"Missing fields: {missing}")

    def update(self, **kwargs):
        """Update attributes on this instance"""
        self.__dict__.update(kwargs)
        return self

    def replace(self, **kwargs):
        """Return a new instance with some attributes replaces"""
        return self.__class__(
            **{**{k: getattr(self, k) for k in self.__dataclass_fields__}, **kwargs}
        )

    def __repr__(self):
        dataclass_repr_func = dataclasses._repr_fn(
            fields=list(self.__dataclass_fields__.values()), globals=self.__dict__
        )
        r = dataclass_repr_func(self)
        if self.missing():
            return "Partial-" + r
        return r


@dataclasses.dataclass()
class BacktestDataConfig:
    data_type: type
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


@dataclasses.dataclass()
class BacktestVenueConfig:
    name: str
    venue_type: str
    oms_type: str
    account_type: str
    base_currency: Currency
    starting_balances: List[Money]
    fill_model: Optional[FillModel] = None
    modules: Optional[List[SimulationModule]] = None

    def __dask_tokenize__(self):
        values = [
            self.name,
            self.venue_type,
            self.oms_type,
            self.account_type,
            self.base_currency.code,
            ",".join(sorted([balance.to_str() for balance in self.starting_balances])),
            self.modules,
        ]
        return tuple(values)


@dataclasses.dataclass(repr=False)
class BacktestConfig(Partialable):
    """
    Configuration for one specific backtest run (a single set of data / strategies / parameters)
    """

    venues: Optional[List[BacktestVenueConfig]] = None
    instruments: Optional[List[Instrument]] = None
    data_config: Optional[List[BacktestDataConfig]] = None
    strategies: Optional[List[Tuple[type, dict]]] = None
    name: Optional[str] = None
    # data_catalog_path: Optional[str] = None


@delayed(pure=True)
def load(query):
    catalog = DataCatalog()
    return {"type": query["cls"], "data": catalog.query(**query)}


# @delayed(pure=True)
def create_backtest_engine(venues, instruments, data):
    engine = BacktestEngine(
        bypass_logging=True,
        run_analysis=False,
    )

    # Add Instruments
    for instrument in instruments:
        engine.add_instrument(instrument)

    # Add data
    for d in data:
        if d["type"] == QuoteTick:
            engine.add_quote_ticks_objects(
                data=d["data"], instrument_id=instruments[0].id
            )

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


# @delayed(pure=True)
def run_engine(engine, strategies):
    strategies = [cls(**kw) for cls, kw in strategies]
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


@delayed
def run_backtest(venues, instruments, data, strategies, name):
    engine = create_backtest_engine(venues=venues, instruments=instruments, data=data)
    results = run_engine(engine=engine, strategies=strategies)
    return name, results


@delayed
def gather(*results):
    return {k: v for r in results for k, v in r}


def _check_configs(configs):
    if isinstance(configs, BacktestConfig):
        configs = [configs]

    for config in configs:
        if not isinstance(config.strategies, list):
            config.strategies = [config.strategies]
        for strategy in config.strategies:
            err = "strategy argument must be tuple of (TradingStrategy class, kwargs dict)"
            assert (
                isinstance(strategy, tuple)
                and isinstance(strategy[0], type)
                and isinstance(strategy[1], dict)
            ), err

    return configs


def build_graph(backtest_configs):
    backtest_configs = _check_configs(backtest_configs)

    _ = (
        DataCatalog()
    )  # Ensure we can instantiate a DataCatalog before we try a computation

    results = []
    for config in backtest_configs:
        config.check(ignore=("name",))  # check all values set
        input_data = []
        for data_config in config.data_config:
            input_data.append(
                load(
                    data_config.query,
                    dask_key_name=f"load-{tokenize(data_config.query)}",
                )
            )
        results.append(
            run_backtest(
                venues=config.venues,
                instruments=config.instruments,
                data=input_data,
                strategies=config.strategies,
                name=config.name or f"backtest-{tokenize(config)}",
            )
        )
        # engine = create_backtest_engine(
        #
        #     dask_key_name=f"create_backtest_engine-{tokenize(config.venues, config.instruments, input_data)}",
        # )
        # # Ensure run_engine gets run on the same worker as create engine
        # with dask.annotate(resources={engine.key: 1}):
        #     results.append(
        #         run_engine(
        #             engine=engine,
        #             strategies=config.strategies,
        #             dask_key_name=f"run_engine-{tokenize(engine, config.strategies)}",
        #         )
        #     )
    return gather(results)


# Register tokenization methods with dask
for cls in Instrument.__subclasses__():
    normalize_token.register(cls, func=cls.to_dict)
