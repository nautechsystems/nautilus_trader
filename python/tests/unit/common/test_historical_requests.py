# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Exercise historical completion through Python actors, strategies, and backtests.
"""

import datetime as dt
from decimal import Decimal
from pathlib import Path

import pytest

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.common import DataActor
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import DataCatalogConfig
from nautilus_trader.core import UUID4
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import BarType
from nautilus_trader.model import ClientId
from nautilus_trader.model import DataType
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.trading import Strategy
from tests.providers import TestInstrumentProvider


@pytest.mark.parametrize("component_type", [DataActor, Strategy])
def test_catalog_request_callbacks_join_and_follow_up(tmp_path: Path, component_type) -> None:
    """
    Deliver catalog data and chain requests from a joined callback.
    """
    instrument = TestInstrumentProvider.audusd_sim()
    quote = QuoteTick(
        instrument_id=instrument.id,
        bid_price=Price.from_str("0.70001"),
        ask_price=Price.from_str("0.70013"),
        bid_size=Quantity.from_str("17000"),
        ask_size=Quantity.from_str("23000"),
        ts_event=1000,
        ts_init=1000,
    )
    catalog = ParquetDataCatalog(str(tmp_path))
    catalog.write_instruments([instrument])
    catalog.write_quote_ticks([quote])
    events = []
    responses = []
    request_time = dt.datetime(1970, 1, 1, 0, 0, 0, 1, tzinfo=dt.UTC)

    class Consumer(component_type):
        def on_start(self):
            self.root = self.request_quotes(
                instrument.id,
                start=request_time,
                end=request_time,
                callback=self.complete,
            )
            self.group = self.request_join([UUID4.from_str(self.root)], callback=self.joined)
            events.append(("pending", self.pending_requests()))

        def on_historical_quotes(self, quotes):
            events.append(("payload", quotes))

        def on_data_response(self, response):
            responses.append(response)
            events.append(("response", response.correlation_id, response.completed))

        def complete(self, request_id):
            events.append(("callback", request_id, self.is_pending_request(request_id)))

        def joined(self, request_id):
            events.append(("joined", request_id, self.pending_requests()))
            self.follow_up = self.request_quotes(
                instrument.id,
                start=request_time,
                end=request_time,
                callback=self.complete,
            )
            self.late = self.request_join([self.root, self.group], callback=self.complete)

    consumer = Consumer()
    engine = BacktestEngine(
        BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
            catalogs=[DataCatalogConfig(path=str(tmp_path))],
        ),
    )
    engine.add_venue(
        instrument.id.venue,
        OmsType.NETTING,
        AccountType.MARGIN,
        [Money.from_str("100000 USD")],
        fee_model=MakerTakerFeeModel(maker_rate=Decimal(0), taker_rate=Decimal(0)),
    )
    engine.add_instrument(instrument)
    engine.add_data(
        [
            QuoteTick(
                instrument_id=instrument.id,
                bid_price=quote.bid_price,
                ask_price=quote.ask_price,
                bid_size=quote.bid_size,
                ask_size=quote.ask_size,
                ts_event=3000,
                ts_init=3000,
            ),
        ],
    )

    if component_type is DataActor:
        engine.add_actor(consumer)
    else:
        engine.add_strategy(consumer)
    try:
        engine.run()
        assert events == [
            ("pending", {consumer.root, consumer.group}),
            ("payload", [quote]),
            ("response", consumer.root, True),
            ("callback", consumer.root, False),
            ("response", consumer.group, True),
            ("joined", consumer.group, set()),
            ("response", consumer.late, True),
            ("callback", consumer.late, False),
            ("payload", [quote]),
            ("response", consumer.follow_up, True),
            ("callback", consumer.follow_up, False),
        ]
        assert [response.failure_reason for response in responses] == [None] * 4
        assert consumer.pending_requests() == set()
        assert consumer.has_pending_requests() is False
    finally:
        engine.dispose()


@pytest.mark.parametrize("component_type", [DataActor, Strategy])
@pytest.mark.parametrize(
    "method",
    [
        "request_data",
        "request_instrument",
        "request_instruments",
        "request_book_snapshot",
        "request_book_deltas",
        "request_book_depth",
        "request_quotes",
        "request_trades",
        "request_funding_rates",
        "request_bars",
        "request_join",
    ],
)
def test_historical_requests_report_borrow_errors_from_python_start(component_type, method) -> None:
    """
    Report a normal Python borrow error when a lifecycle call owns the wrapper.
    """
    instrument_id = TestInstrumentProvider.audusd_sim().id

    if method == "request_data":
        args = (DataType("TestData"), ClientId("SIM"))
    elif method == "request_instruments":
        args = ()
    elif method == "request_bars":
        args = (BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"),)
    elif method == "request_join":
        args = ([UUID4()],)
    else:
        args = (instrument_id,)
    errors = []

    class Consumer(component_type):
        def on_start(self):
            try:
                getattr(self, method)(*args)
            except BaseException as e:  # Include panic exceptions in this boundary check
                errors.append((type(e), str(e)))

    consumer = Consumer()
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    if component_type is DataActor:
        engine.add_actor(consumer)
    else:
        engine.add_strategy(consumer)
    try:
        consumer.start()
        assert errors == [(RuntimeError, "Already mutably borrowed")]
    finally:
        if consumer.is_running():
            consumer.stop()
        engine.dispose()
