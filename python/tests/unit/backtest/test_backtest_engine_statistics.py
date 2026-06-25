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

import math

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import Venue


def _float_maps_equal(a: dict[str, float], b: dict[str, float]) -> bool:
    """
    Return True if two str->float dicts are equal, treating NaN as equal to NaN.
    """
    if a.keys() != b.keys():
        return False
    for key in a:
        va, vb = a[key], b[key]
        if math.isnan(va) and math.isnan(vb):
            continue
        if va != vb:
            return False
    return True


def _nested_float_maps_equal(
    a: dict[str, dict[str, float]],
    b: dict[str, dict[str, float]],
) -> bool:
    """
    Return True if two str->str->float dicts are equal, treating NaN as equal to NaN.
    """
    if a.keys() != b.keys():
        return False
    return all(_float_maps_equal(a[key], b[key]) for key in a)


def _engine_with_account() -> BacktestEngine:
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True))
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=Currency.from_str("USD"),
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
    )
    return engine


def test_engine_exposes_portfolio_statistics():
    engine = _engine_with_account()
    engine.run()
    stats = engine.portfolio.statistics()
    assert isinstance(stats.pnls, dict)
    assert isinstance(stats.returns, dict)
    assert isinstance(stats.general, dict)
    engine.dispose()


def test_engine_portfolio_statistics_equals_result():
    engine = _engine_with_account()
    engine.run()
    stats = engine.portfolio.statistics()
    result = engine.get_result()
    assert _nested_float_maps_equal(stats.pnls, result.stats_pnls)
    assert _float_maps_equal(stats.returns, result.stats_returns)
    assert _float_maps_equal(stats.general, result.stats_general)
    engine.dispose()
