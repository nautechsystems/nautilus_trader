import dataclasses
import re
from typing import List, Optional, Tuple

from dask import delayed
import pandas as pd
from pydantic import BaseModel
from pydantic import create_model
from pydantic import validate_model

from nautilus_trader.backtest.data_loader import DataCatalog
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.modules import SimulationModule
from nautilus_trader.model.c_enums.account_type import AccountTypeParser
from nautilus_trader.model.c_enums.oms_type import OMSTypeParser
from nautilus_trader.model.c_enums.venue_type import VenueTypeParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.trading.strategy import TradingStrategy


PARTIAL_SUFFIX = "Partial-"


class Cloneable(BaseModel):
    class Config:
        arbitrary_types_allowed = True

    def is_partial(self):
        cls_name = self.__class__.__name__
        return bool(re.findall(r"\w+Partial-\d+", cls_name))

    def check(self):
        *_, validation_error = validate_model(self._base_class, self.__dict__)
        if validation_error:
            raise validation_error

    def replace(self):
        pass

    @classmethod
    def partial(cls, *_, **kwargs) -> "Cloneable":
        fields = set(cls.__fields__.keys())

        opt = {f: cls.__fields__[f].get_default() for f in fields}
        opt.update(kwargs)

        base_cls = getattr(cls, "_base_class", cls)

        model = create_model(
            cls.__name__,
            __base__=Cloneable,
            _base_class=base_cls,
            **{
                field: (cls.__annotations__[field], opt.get(field, ...))
                for field in fields
            },
        )

        *_, validation_error = validate_model(base_cls, opt)
        if validation_error is None:
            model.__name__ = model.__name__.rsplit(PARTIAL_SUFFIX, maxsplit=1)[0]
            return model(**kwargs)
        model.__name__ += PARTIAL_SUFFIX + str(id(model))
        return model(**kwargs)


class BacktestDataConfig(BaseModel):
    class Config:
        arbitrary_types_allowed = True

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


class BacktestVenueConfig(BaseModel):
    class Config:
        arbitrary_types_allowed = True

    name: str
    venue_type: str
    oms_type: str
    account_type: str
    base_currency: Currency
    starting_balances: List[Money]
    modules: Optional[List[SimulationModule]] = None


@dataclasses.dataclass()
class BacktestConfig:
    """
    Configuration for one specific backtest run (a single set of data / strategies / parameters)
    """

    venues: Optional[List[BacktestVenueConfig]] = None
    instruments: Optional[List[Instrument]] = None
    data_config: Optional[List[BacktestDataConfig]] = None
    strategies: Optional[List[Tuple[type, dict]]] = None

    def create_strategies(self) -> List[TradingStrategy]:
        return [cls(**kw) for cls, kw in self.strategies]

    def missing(self):
        return [x for x in self.__dataclass_fields__ if getattr(self, x) is None]

    def is_partial(self):
        return any(self.missing())

    def check(self):
        missing = self.missing()
        if missing:
            raise ValueError(f"Missing fields: {missing}")

    def update(self, **kwargs):
        """Update attributes on this instance"""
        self.__dict__.update(kwargs)
        return self

    def replace(self, **kwargs):
        """Return a new instance with some attributes replaces"""
        return self.__class__(**{**self.__dict__, **kwargs})

    def __repr__(self):
        r = dataclasses._repr_fn()
        if self.missing():
            return "Partial" + r
        return super().__repr__()


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


@delayed
def gather(*results):
    return list(results)


def build_graph(backtest_configs):
    if isinstance(backtest_configs, BacktestConfig):
        backtest_configs = [backtest_configs]
    results = []
    for config in backtest_configs:
        input_data = []
        for data_config in config.data_config:
            input_data.append(load(data_config.query))
        engine = create_backtest_engine(
            venues=config.venues, instruments=config.instruments, data=input_data
        )
        results.append(run_engine(engine=engine, strategies=config.strategies))
    return gather(results)
