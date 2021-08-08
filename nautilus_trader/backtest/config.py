import dataclasses
from functools import partial
from typing import List, Optional, Tuple

import pandas as pd
from dask import delayed
from dask.base import normalize_token
from dask.base import tokenize

from nautilus_trader.backtest.data_loader import DataCatalog
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.modules import SimulationModule
from nautilus_trader.core.message import Event
from nautilus_trader.model.c_enums.account_type import AccountTypeParser
from nautilus_trader.model.c_enums.oms_type import OMSTypeParser
from nautilus_trader.model.c_enums.venue_type import VenueTypeParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.orderbook.data import OrderBookDelta


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

    def _check_kwargs(self, kw):
        for k in kw:
            assert k in self.__dataclass_fields__, f"Unknown kwarg: {k}"

    def update(self, **kwargs):
        """Update attributes on this instance"""
        self._check_kwargs(kwargs)
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
class BacktestDataConfig(Partialable):
    catalog_path: str
    data_type: type
    catalog_fs_protocol: str = None
    instrument_id: Optional[str] = None
    start_time: Optional[int] = None
    end_time: Optional[int] = None
    filters: Optional[dict] = None
    client_id: Optional[str] = None

    @property
    def query(self):
        return dict(
            cls=self.data_type,
            instrument_ids=[self.instrument_id] if self.instrument_id else None,
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


def _load(config: BacktestDataConfig):
    catalog = DataCatalog(path=config.catalog_path, fs_protocol=config.catalog_fs_protocol)
    query = config.query
    return {
        "type": query["cls"],
        "data": catalog.query(**query),
        "client_id": config.client_id,
    }


@delayed(pure=True)
def load(config: BacktestDataConfig):
    return _load(config=config)


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
            engine.add_quote_ticks_objects(data=d["data"], instrument_id=instruments[0].id)
        elif d["type"] == TradeTick:
            engine.add_trade_tick_objects(data=d["data"], instrument_id=instruments[0].id)
        elif d["type"] == OrderBookDelta:
            engine.add_order_book_data(data=d["data"])
        elif isinstance(d["data"][0], Event):
            engine.add_events(client_id=d["client_id"], data=d["data"])
        else:
            engine.add_generic_data(client_id=d["client_id"], data=d["data"])

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


def _run_backtest(venues, instruments, data, strategies, name):
    engine = create_backtest_engine(venues=venues, instruments=instruments, data=data)
    results = run_engine(engine=engine, strategies=strategies)
    return name, results


@delayed
def run_backtest(venues, instruments, data, strategies, name):
    return _run_backtest(venues, instruments, data, strategies, name)


def _gather(*results):
    return {k: v for r in results for k, v in r}


@delayed
def gather(*results):
    return _gather(*results)


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


def build_graph(backtest_configs, sync=False):
    backtest_configs = _check_configs(backtest_configs)

    results = []
    for config in backtest_configs:
        config.check(ignore=("name",))  # check all values set
        input_data = []
        for data_config in config.data_config:
            load_func = (
                _load
                if sync
                else partial(load, dask_key_name=f"load-{tokenize(data_config.query)}")
            )
            input_data.append(
                load_func(
                    data_config,
                )
            )
        run_backtest_func = _run_backtest if sync else run_backtest
        results.append(
            run_backtest_func(
                venues=config.venues,
                instruments=config.instruments,
                data=input_data,
                strategies=config.strategies,
                name=config.name or f"backtest-{tokenize(config)}",
            )
        )
    gather_func = _gather if sync else gather
    return gather_func(results)


# Register tokenization methods with dask
for cls in Instrument.__subclasses__():
    normalize_token.register(cls, func=cls.to_dict)
