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
from itertools import combinations
from typing import List

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd

from nautilus_trader.analysis.statistic import PortfolioStatistic
from nautilus_trader.analysis.statistics.sharpe_ratio import SharpeRatio


try:
    import statsmodels.api as sm
    from statsmodels.distributions.empirical_distribution import ECDF
except ImportError:
    raise ImportError(
        "The statsmodels package is not installed. "
        "Please install via pip or poetry install statsmodels",
    )


statistics_instance = SharpeRatio()


@dataclass
class CSCV:
    """
    Combinatorially symmetric cross-validation algorithm.

    Parameters
    ----------
    n_sub_matrices :
    statistics: PortfolioStatistic
        function for  in sample(IS) and out of sample(OOS) return benchmark algorithm.
    returns: pd.DataFrame
        The returns of N strategies.

    References
    ----------
    Bailey et al (2015) "The Probability of Backtest Overfitting"
    https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2326253
    https://github.com/finlab-python/finlab_crypto/blob/master/finlab_crypto/overfitting.py
    """

    def __init__(
        self,
        returns: pd.DataFrame,
        n_sub_matrices: int = 10,
        statistics: PortfolioStatistic = statistics_instance,
    ):
        self.n_sub_matrices: int = n_sub_matrices
        self.statistics: PortfolioStatistic = statistics
        self.returns: pd.DataFrame = returns
        self.comb: List = [
            set(x) for x in combinations(range(self.n_sub_matrices), int(self.n_sub_matrices / 2))
        ]
        self.Rs: List[pd.Series] = [pd.Series(dtype=float) for i in range(len(self.comb))]
        self.R_bars: List[pd.Series] = [pd.Series(dtype=float) for i in range(len(self.comb))]
        bin_size = int(len(self.returns) / self.n_sub_matrices)
        return_bins = [
            self.returns[i * bin_size : (i + 1) * bin_size] for i in range(self.n_sub_matrices)
        ]
        for set_id, IS_set in enumerate(self.comb):
            OOS_set = set(range(self.n_sub_matrices)) - IS_set
            IS_returns = pd.concat([return_bins[idx] for idx in IS_set])
            OOS_returns = pd.concat([return_bins[idx] for idx in OOS_set])
            IS_metrics = IS_returns.apply(self.statistics.calculate_from_returns, axis=0)
            OOS_metrics = OOS_returns.apply(self.statistics.calculate_from_returns, axis=0)
            self.Rs[set_id] = pd.concat([self.Rs[set_id], IS_metrics])
            self.R_bars[set_id] = pd.concat([self.R_bars[set_id], OOS_metrics])

    def estimate(self):
        """
        Evaluate backtest overfitting,performance_degradation and  the second order stochastic dominance.
        """
        R_df = pd.DataFrame(self.Rs)
        R_bar_df = pd.DataFrame(self.R_bars)

        # calculate ranking of the strategies
        R_rank_df = R_df.rank(axis=1, ascending=False, method="first")
        R_bar_rank_df = R_bar_df.rank(axis=1, ascending=False, method="first")

        # find best ranking IS strategy's performance in IS
        self.r_star_series = (R_df * (R_rank_df == 1)).sum(axis=1)
        # fina best ranking IS stratgy's performance in OOS
        self.r_bar_star_series = (R_bar_df * (R_rank_df == 1)).sum(axis=1)
        # find relative rank
        r_bar_rank_series = (R_bar_rank_df * (R_rank_df == 1)).sum(axis=1)
        # estimate logits of OOS rankings
        self.logits = (1 - ((r_bar_rank_series) / (len(R_df.columns) + 1))).map(
            lambda p: np.log(p / (1 - p))
        )
        self.pbo = (self.logits < 0).sum() / len(self.logits)
        y = np.linspace(
            min(self.r_bar_star_series), max(self.r_bar_star_series), endpoint=True, num=1000
        )
        R_bar_n_star_cdf = ECDF(self.r_bar_star_series.values)
        optimized = R_bar_n_star_cdf(y)

        # build CDF performance of average candidate in IS
        R_bar_mean_cdf = ECDF(R_bar_df.median(axis=1).values)
        non_optimized = R_bar_mean_cdf(y)
        self.dom_df = pd.DataFrame(
            dict(optimized_IS=optimized, non_optimized_OOS=non_optimized), index=y
        )
        self.dom_df["SD2"] = (self.dom_df.non_optimized_OOS - self.dom_df.optimized_IS).cumsum()

    def plot_pbo(self):
        """
        Plot the probability of backtest overfitting (PBO).
        """
        plt.figure(figsize=(15, 4))
        plt.hist(
            x=[item for item in self.logits if item > -10000],
            bins="auto",
            label=f"Prob Overfit = {self.pbo}",
        )
        plt.title("Histogram of Rank Logits", fontsize=20)
        plt.xlabel("Logits")
        plt.ylabel("Frequency")
        plt.legend()
        plt.show()

    def plot_performance_degradation(self):
        """
        Plot the performance degradation.
        """
        X, Y = self.r_star_series.to_list(), self.r_bar_star_series.to_list()
        model = sm.OLS(Y, sm.add_constant(X), hasconst=True)
        results = model.fit()

        x1 = np.linspace(np.min(X), np.max(X), 20)
        y1 = results.params[0] + results.params[1] * x1
        plt.figure(figsize=(15, 4))
        plt.scatter(X, Y)
        plt.plot(
            x1,
            y1,
            color="red",
            ls="--",
            label=f"Regression slope: {str(np.round(results.params[1], 2))} ; "
            f"P-value: {str(np.round(results.f_pvalue, 2))}",
        )
        plt.title("Out of Sample Performance Degradation", fontsize=20)
        plt.xlabel("Performance IS")
        plt.ylabel("Performance OOS")
        plt.grid(False)
        plt.legend()
        plt.show()

    def plot_stochastic_dominance(self):
        """
        Plot the second order stochastic dominance(SD).
        """
        # first and second Stochastic dominance
        plt.figure(figsize=(15, 4))
        self.dom_df.plot(secondary_y=["SD2"])
        plt.title("Stochastic dominance")
        plt.xlabel("Performance optimized vs non-optimized")
        plt.ylabel("Frequency")
        plt.show()
