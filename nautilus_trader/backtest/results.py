# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from dataclasses import dataclass
from typing import List

import pandas as pd

from nautilus_trader.backtest.engine import BacktestEngine


@dataclass
class BacktestResult:
    id: str
    account_balances: pd.DataFrame
    fill_report: pd.DataFrame
    positions: pd.DataFrame

    @classmethod
    def from_engine(cls, backtest_id: str, engine: BacktestEngine):
        account_balances = pd.concat(
            [
                engine.trader.generate_account_report(venue).assign(venue=venue)
                for venue in engine.list_venues()
            ]
        )
        return BacktestResult(
            id=backtest_id,
            account_balances=account_balances,
            fill_report=engine.trader.generate_order_fills_report(),
            positions=engine.trader.generate_positions_report(),
        )

    def final_balances(self):
        return self.account_balances.groupby(["venue", "currency"])["total"].last()

    def __repr__(self):
        def repr_balance():
            items = [
                (venue, currency, balance)
                for (venue, currency), balance in self.final_balances().items()
            ]
            return ",".join([f"{v.value}[{c}]={b}" for (v, c, b) in items])

        return f"{self.__class__.__name__}({self.id}, {repr_balance()})"


def ensure_plotting(func):
    """
    Decorate a function that require a plotting library

    Ensures library is installed and providers a better error about how to install if not found
    """

    def inner(*args, **kwargs):
        try:
            import hvplot.pandas

            assert hvplot.pandas
        except ImportError:
            raise ImportError(
                "Failed to import plotting library - install in notebook via `%pip install hvplot`"
            )
        return func(*args, **kwargs)

    return inner


@dataclass()
class BacktestRunResults:
    results: List[BacktestResult]

    def final_balances(self):
        return pd.concat(r.final_balances().to_frame().assign(id=r.id) for r in self.results)

    @ensure_plotting
    def plot_balances(self):
        df = self.final_balances()
        df = df.reset_index().set_index("id").astype({"venue": str, "total": float})
        return df.hvplot.bar(y="total", rot=45, by=["venue", "currency"])
