import pathlib

import orjson
import pytest

from nautilus_trader.adapters.betfair.data import on_market_update
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import make_instruments
from nautilus_trader.backtest.data_loader import CSVParser
from nautilus_trader.backtest.data_loader import DataLoader
from nautilus_trader.backtest.data_loader import ParquetParser
from nautilus_trader.backtest.data_loader import TextParser
from tests.test_kit import PACKAGE_ROOT


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))


@pytest.mark.parametrize(
    "glob, num_files",
    [
        ("**.json", 2),
        ("**.txt", 1),
        ("**.parquet", 2),
        ("**.csv", 11),
    ],
)
def test_data_loader_paths(glob, num_files):
    d = DataLoader(path=TEST_DATA_DIR, parser=CSVParser(), glob_pattern=glob)
    assert len(d.path) == num_files


def test_data_loader_json_betting_parser():
    instrument_provider = BetfairInstrumentProvider.from_instruments([])

    def update_instrument_provider(instrument_provider):
        def inner(line):
            data = orjson.loads(line)
            # Find instruments in data
            for mc in data.get("mc", []):
                if "marketDefinition" in mc:
                    market_def = {**mc["marketDefinition"], **{"marketId": mc["id"]}}
                    instruments = make_instruments(
                        market_definition=market_def, currency="GBP"
                    )
                    instrument_provider.add_instruments(instruments)

            # By this point we should always have some instruments loaded from historical data.
            if not instrument_provider.list_instruments():
                # TODO - Need to add historical search
                raise Exception("No instruments found")

        return inner

    parser = TextParser(
        line_parser=lambda x: on_market_update(
            instrument_provider=instrument_provider, update=orjson.loads(x)
        ),
        instrument_provider_update=update_instrument_provider(instrument_provider),
    )
    loader = DataLoader(path=TEST_DATA_DIR, parser=parser, glob_pattern="**.zip")
    assert len(loader.path) == 1

    data = list(loader.run())
    assert len(data) == 19099


def test_data_loader_parquet():
    loader = DataLoader(
        path=TEST_DATA_DIR, parser=ParquetParser(), glob_pattern="**.parquet"
    )
    assert len(loader.path) == 2


# def test_parser():
#     upd = pickle_load(
#         "/Users/bradleymcelroy/projects/dev/nautilus/Tennis||29676224|2020-01-31 08:30:00+00:00|MATCH_ODDS|1.168065827|ODDS|19924823|.BETFAIR.pkl")
#     nautilus_to_parquet(upd)
