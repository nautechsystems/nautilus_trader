import inspect
import sys

import numpy as np
import orjson
import pandas as pd
import pytest

from nautilus_trader.adapters.betfair.parsing import on_market_update
from nautilus_trader.data.wrangling import QuoteTickDataWrangler
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


def parse_text(x):
    # Mock actual parsing
    yield TradeTick(
        instrument_id=TestStubs.audusd_id(),
        price=Price.from_int(1),
        size=Quantity.from_int(1),
        aggressor_side=AggressorSide.BUY,
        match_id="1",
        ts_event=0,
        ts_init=0,
    )


def parse_csv_quotes(data):
    if data is None:
        return
    data.loc[:, "timestamp"] = pd.to_datetime(data["timestamp"])
    wrangler = QuoteTickDataWrangler(
        instrument=TestInstrumentProvider.default_fx_ccy(
            "AUD/USD"
        ),  # Normally we would properly parse this
        data_quotes=data.set_index("timestamp"),
    )
    wrangler.pre_process(0)
    yield from wrangler.build_ticks()


def parse_json_bytes(data):
    yield data


def parse_betfair(line, instrument_provider):
    yield from on_market_update(instrument_provider=instrument_provider, update=orjson.loads(line))


# def parse_csv_bars(data):
#     if data is None:
#         return
#     data.loc[:, "timestamp"] = pd.to_datetime(data["timestamp"])
#     wrangler = BarDataWrangler(
#         BarType(
#             instrument_id=TestInstrumentProvider.default_fx_ccy("AUD/USD").id,
#             spec=BarSpecification()
#         ),
#         2,
#         2,
#         data=data.set_index("timestamp"),
#     )
#     yield from wrangler.build_bars_all()


@pytest.fixture()
def get_parser():
    def inner(name):
        mappings = {
            name: obj
            for name, obj in inspect.getmembers(sys.modules[__name__])
            if inspect.isfunction(obj)
        }
        if name in mappings:
            return mappings[name]
        raise KeyError

    return inner


@pytest.fixture()
def parser(request, get_parser):
    return get_parser(request.param)


@pytest.fixture()
def sample_df():
    return pd.DataFrame({"value": np.random.random(5), "instrument_id": ["a", "a", "a", "b", "b"]})
