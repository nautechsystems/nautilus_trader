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
Test Python and PyO3 memory ownership across repeated backtest lifecycles.
"""

import gc
import weakref

import pytest
from tests.providers import TestInstrumentProvider
from tests.strategies.backtest_surface import QuoteCountActor
from tests.strategies.backtest_surface import QuoteCountActorConfig

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import Venue


pytest.importorskip("pytest_memray")

_RUNS = 128
_INSTRUMENT = TestInstrumentProvider.audusd_sim()
_VENUE = Venue("SIM")
_USD = Currency.from_str("USD")
_ENGINE_CONFIG = BacktestEngineConfig(bypass_logging=True, run_analysis=False)
_QUOTES = [
    QuoteTick(
        instrument_id=_INSTRUMENT.id,
        bid_price=Price.from_str("0.70000"),
        ask_price=Price.from_str("0.70020"),
        bid_size=Quantity.from_int(1_000_000),
        ask_size=Quantity.from_int(1_000_000),
        ts_event=1_600_000_000_000_000_000 + i,
        ts_init=1_600_000_000_000_000_000 + i,
    )
    for i in range(8)
]


@pytest.fixture(scope="module", autouse=True)
def _warm_up_backtest() -> None:
    _assert_backtest_released()


@pytest.mark.limit_leaks("32 KB")
def test_backtest_engine_releases_python_components_each_run() -> None:
    """
    Test repeated backtest runs release their Python and native allocations.
    """
    for _ in range(_RUNS):
        _assert_backtest_released()

    gc.collect()


def _assert_backtest_released() -> None:
    iteration, quote_count, actor_ref = _run_backtest()
    gc.collect()

    assert iteration == len(_QUOTES)
    assert quote_count == len(_QUOTES)
    assert actor_ref() is None


def _run_backtest() -> tuple[int, int, weakref.ReferenceType[QuoteCountActor]]:
    engine = BacktestEngine(_ENGINE_CONFIG)
    actor = QuoteCountActor(
        QuoteCountActorConfig(instrument_id=str(_INSTRUMENT.id), log_events=False),
    )
    try:
        engine.add_venue(
            venue=_VENUE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            starting_balances=[Money(1_000_000.0, _USD)],
            base_currency=_USD,
        )
        engine.add_instrument(_INSTRUMENT)
        engine.add_actor(actor)
        engine.add_data(_QUOTES)
        engine.run()
        return engine.iteration, actor.quote_count, weakref.ref(actor)
    finally:
        engine.dispose()
        QuoteCountActor.reset_observations()
