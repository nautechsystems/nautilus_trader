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

import time

import pandas as pd

from nautilus_trader.adapters.tardis.common import infer_tardis_exchange_str
from nautilus_trader.adapters.tardis.download import download_file
from nautilus_trader.adapters.tardis.factories import get_tardis_http_client
from nautilus_trader.adapters.tardis.factories import get_tardis_instrument_provider
from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.common.config import LoggingConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model import Currency
from nautilus_trader.model import Venue
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig


def fetch_instruments(
    venues: list[Venue] | None = None,
    instrument_ids: list[InstrumentId] | None = None,
    base_currency: list[Currency] | None = None,
    quote_currency: list[Currency] | None = None,
    instrument_type: list[str] | None = None,
    effective: pd.Timestamp | None = None,
    start: pd.Timestamp | None = None,
    end: pd.Timestamp | None = None,
    active: bool | None = None,
) -> list[Instrument]:
    assert not (venues and instrument_ids), "Only one of venues or instrument_ids can be set"
    assert venues or instrument_ids, "Either venues or instrument_ids must be set"

    if instrument_ids:
        venues_set = {i.venue for i in instrument_ids}
    else:
        venues_set = set()

    filters = {}
    filter_mapping = [
        (venues_set, "venues", lambda x: frozenset(i.value for i in x)),
        (base_currency, "base_currency", lambda x: frozenset(i.code for i in x)),
        (quote_currency, "quote_currency", lambda x: frozenset(i.code for i in x)),
        (instrument_type, "instrument_type", frozenset),
        (effective, "effective", lambda x: x),
        (start, "start", lambda x: x.value if x else x),
        (end, "end", lambda x: x.value if x else x),
        (active, "active", lambda x: x),
    ]

    for value, key, func in filter_mapping:
        if value is not None:
            filters[key] = func(value)

    client = get_tardis_http_client()

    # Override the venues filter to use the correct Tardis exchange names
    if instrument_ids:
        tardis_exchanges = set()
        for instrument_id in instrument_ids:
            venue_str = instrument_id.venue.value.upper().replace("-", "_")
            tardis_exchanges.update(nautilus_pyo3.tardis_exchange_from_venue_str(venue_str))
        filters["venues"] = frozenset(tardis_exchanges)

    config = InstrumentProviderConfig(
        load_all=True,
        load_ids=frozenset(instrument_ids) if instrument_ids else None,
        filters=filters,
    )

    instrument_provider = get_tardis_instrument_provider(client, config=config)

    if instrument_ids:
        instrument_provider.load_ids(instrument_ids, filters=filters)
    else:
        instrument_provider.load_all(filters=filters)

    return instrument_provider.list_all()


def bench_data_streaming_iterators():

    start_time = time.perf_counter()

    date = pd.Timestamp("2025-02-28", tz="UTC")
    # instrument_ids = [
    #     InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
    #     InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
    #     InstrumentId.from_str("SOLUSDT-PERP.BINANCE"),
    #     InstrumentId.from_str("XRPUSDT-PERP.BINANCE"),
    # ]
    instrument_ids = [
        InstrumentId.from_str("XBTUSD.BITMEX"),
    ]

    instruments = fetch_instruments(instrument_ids=instrument_ids)
    assert len(instruments) == len(instrument_ids)

    venues = {i.venue for i in instruments}
    config = BacktestEngineConfig(
        trader_id=TraderId("BACKTEST-001"),
        # data_engine=DataEngineConfig(buffer_deltas=True),  # Buffer individual deltas (.stream_deltas)
        logging=LoggingConfig(bypass_logging=True),
    )

    engine = BacktestEngine(config=config)

    config_actor = DataTesterConfig(
        instrument_ids=instrument_ids,
        subscribe_book_deltas=True,
        manage_book=True,
        # use_pyo3_book=True,
        log_data=False,
    )
    actor = DataTester(config=config_actor)

    engine.trader.add_actor(actor)

    for inst in venues:
        engine.add_venue(
            venue=inst,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,
            starting_balances=[Money(100_000, USDT)],
            book_type=BookType.L2_MBP,
        )

    for inst in instruments:
        loader = TardisCSVDataLoader(
            instrument_id=inst.id,
            size_precision=inst.size_precision,
            price_precision=inst.price_precision,
        )
        tardis_exchange = infer_tardis_exchange_str(inst)
        # Remove -PERP suffix for file naming in Tardis datasets
        symbol_for_file = inst.raw_symbol.value.replace("-PERP", "")
        trades = f"https://datasets.tardis.dev/v1/{tardis_exchange}/trades/{date.strftime('%Y/%m/%d')}/{symbol_for_file}.csv.gz"
        deltas = f"https://datasets.tardis.dev/v1/{tardis_exchange}/incremental_book_L2/{date.strftime('%Y/%m/%d')}/{symbol_for_file}.csv.gz"
        trades_path = download_file(trades)
        deltas_path = download_file(deltas)
        trades_iter = loader.stream_trades(trades_path, limit=1_000_000)
        deltas_iter = loader.stream_batched_deltas(deltas_path, limit=1_000_000)

        engine.add_instrument(inst)
        engine.add_data_iterator(trades, trades_iter)
        engine.add_data_iterator(deltas, deltas_iter)

    print(f"Starting backtest for {instruments}")

    backtest_start = time.perf_counter()
    engine.run(
        streaming=True,
        start=date.value,
        end=(date + pd.Timedelta(24 * 60 + 1, unit="m")).value,
    )
    backtest_time = time.perf_counter() - backtest_start
    engine.end()

    total_time = time.perf_counter() - start_time
    print(f"\n{'='*60}")
    print("PERFORMANCE RESULTS")
    print(f"{'='*60}")
    print(f"Total time (including setup): {total_time:.2f}s")
    print(f"Backtest execution time:      {backtest_time:.2f}s")
    print(f"Setup overhead:               {total_time - backtest_time:.2f}s")


if __name__ == "__main__":
    bench_data_streaming_iterators()
