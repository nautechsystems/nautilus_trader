# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Dict, Optional


@dataclass
class BacktestResult:
    """
    Represents the results of a single complete backtest run.
    """

    trader_id: str
    machine_id: str
    run_config_id: Optional[str]
    instance_id: str
    run_id: str
    run_started: Optional[int]
    run_finished: Optional[int]
    backtest_start: Optional[int]
    backtest_end: Optional[int]
    elapsed_time: float
    iterations: int
    total_events: int
    total_orders: int
    total_positions: int
    stats_pnls: Dict[str, Dict[str, float]]
    stats_returns: Dict[str, float]

    # account_balances: pd.DataFrame
    # fills_report: pd.DataFrame
    # positions: pd.DataFrame
    #
    # def final_balances(self):
    #     return self.account_balances.groupby(["venue", "currency"])["total"].last()
    #
    # def __repr__(self) -> str:
    #     def repr_balance():
    #         items = [
    #             (venue, currency, balance)
    #             for (venue, currency), balance in self.final_balances().items()
    #         ]
    #         return ",".join([f"{v.value}[{c}]={b}" for (v, c, b) in items])
    #
    #     return f"{self.__class__.__name__}({self.run_id}, {repr_balance()})"


def ensure_plotting(func):
    """
    Decorate a function that require a plotting library.

    Ensures library is installed and providers a better error about how to install if not found.
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


# @dataclass()
# class BacktestRunResults:
#     results: List[BacktestResult]
#
#     def final_balances(self):
#         return pd.concat(r.final_balances().to_frame().assign(id=r.id) for r in self.results)
#
#     @ensure_plotting
#     def plot_balances(self):
#         df = self.final_balances()
#         df = df.reset_index().set_index("id").astype({"venue": str, "total": float})
#         return df.hvplot.bar(y="total", rot=45, by=["venue", "currency"])
