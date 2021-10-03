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

from datetime import datetime
from typing import Dict, List, Optional

import numpy as np
import pandas as pd
import quantstats
from numpy import float64

from nautilus_trader.accounting.accounts.base import Account
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.position import Position


class PerformanceAnalyzer:
    """
    Provides a performance analyzer for tracking and generating performance
    metrics and statistics.
    """

    def __init__(self):
        """
        Initialize a new instance of the ``PerformanceAnalyzer`` class.
        """
        self._account_balances_starting = {}  # type: dict[Currency, Money]
        self._account_balances = {}  # type: dict[Currency, Money]
        self._realized_pnls = {}  # type: dict[Currency, pd.Series]
        self._returns = pd.Series(dtype=float64)

    @property
    def currencies(self):
        """
        Return the analyzed currencies.

        Returns
        -------
        List[Currency]

        """
        return list(self._account_balances.keys())

    def calculate_statistics(self, account: Account, positions: List[Position]) -> None:
        """
        Calculate performance metrics from the given data.

        Parameters
        ----------
        account : Account
            The account for the calculations.
        positions : dict[PositionId, Position]
            The positions for the calculations.

        """
        self._account_balances_starting = account.starting_balances()
        self._account_balances = account.balances_total()
        self._realized_pnls = {}
        self._returns = pd.Series(dtype=float64)

        self.add_positions(positions)
        self._returns.sort_index()

    def add_positions(self, positions: List[Position]) -> None:
        """
        Add positions data to the analyzer.

        Parameters
        ----------
        positions : list[Position]
            The positions for analysis.

        """
        for position in positions:
            self.add_trade(position.id, position.realized_pnl)
            self.add_return(unix_nanos_to_dt(position.ts_closed), position.realized_return)

    def add_trade(self, position_id: PositionId, realized_pnl: Money) -> None:
        """
        Add trade data to the analyzer.

        Parameters
        ----------
        position_id : PositionId
            The position ID for the trade.
        realized_pnl : Money
            The realized PnL for the trade.

        """
        currency = realized_pnl.currency
        realized_pnls = self._realized_pnls.get(currency, pd.Series())
        realized_pnls.loc[position_id.value] = realized_pnl.as_double()
        self._realized_pnls[currency] = realized_pnls

    def add_return(self, timestamp: datetime, value: float) -> None:
        """
        Add return data to the analyzer.

        Parameters
        ----------
        timestamp : datetime
            The timestamp for the returns entry.
        value : double
            The return value to add.

        """
        if timestamp not in self._returns:
            self._returns.loc[timestamp] = 0.0
        self._returns.loc[timestamp] += float(value)

    def reset(self) -> None:
        """
        Reset the analyzer.

        All stateful fields are reset to their initial value.
        """
        self._account_balances_starting = {}
        self._account_balances = {}
        self._realized_pnls = {}
        self._returns = pd.DataFrame(dtype=float64)

    def realized_pnls(self, currency: Currency = None) -> Optional[pd.Series]:
        """
        Return the realized PnL for the portfolio.

        For multi-currency portfolios, specify the currency for the result.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the result.

        Returns
        -------
        pd.Series or ``None``

        Raises
        ------
        ValueError
            If currency is ``None`` when analyzing multi-currency portfolios.

        """
        if not self._realized_pnls:
            return None
        if currency is None:
            assert (
                len(self._account_balances) == 1
            ), "currency was None for multi-currency portfolio"
            currency = next(iter(self._account_balances.keys()))

        return self._realized_pnls.get(currency)

    def total_pnl(self, currency: Currency = None) -> float:
        """
        Return the total PnL for the portfolio.

        For multi-currency portfolios, specify the currency for the result.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the result.

        Returns
        -------
        float

        Raises
        ------
        ValueError
            If currency is ``None`` when analyzing multi-currency portfolios.
        ValueError
            If currency is not contained in the tracked account balances.

        """
        if not self._account_balances:
            return 0.0
        if currency is None:
            assert (
                len(self._account_balances) == 1
            ), "currency was None for multi-currency portfolio"
            currency = next(iter(self._account_balances.keys()))
        assert currency in self._account_balances, "currency not found in account_balances"

        account_balance = self._account_balances.get(currency)
        account_balance_starting = self._account_balances_starting.get(currency, Money(0, currency))

        if account_balance is None:
            return 0.0

        return float(account_balance - account_balance_starting)

    def total_pnl_percentage(self, currency: Currency = None) -> float:
        """
        Return the percentage change of the total PnL for the portfolio.

        For multi-currency accounts, specify the currency for the result.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the result.

        Returns
        -------
        float

        Raises
        ------
        ValueError
            If currency is ``None`` when analyzing multi-currency portfolios.
        ValueError
            If currency is not contained in the tracked account balances.

        """
        if not self._account_balances:
            return 0.0
        if currency is None:
            assert (
                len(self._account_balances) == 1
            ), "currency was None for multi-currency portfolio"
            currency = next(iter(self._account_balances.keys()))
        assert currency in self._account_balances, "currency not in account_balances"

        account_balance = self._account_balances.get(currency)
        account_balance_starting = self._account_balances_starting.get(currency, Money(0, currency))

        if account_balance is None:
            return 0.0

        if account_balance_starting.as_decimal() == 0:
            # Protect divide by zero
            return 0.0

        current = account_balance
        starting = account_balance_starting
        difference = current - starting

        return (difference / starting) * 100

    def max_winner(self, currency: Currency = None) -> float:
        """
        Return the maximum winner for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        float

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        return max(realized_pnls)

    def max_loser(self, currency: Currency = None) -> float:
        """
        Return the maximum loser for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        float

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        losers = [x for x in realized_pnls if x < 0.0]
        if realized_pnls is None or not losers:
            return 0.0

        return min(np.asarray(losers, dtype=np.float64))

    def min_winner(self, currency: Currency = None) -> float:
        """
        Return the minimum winner for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        float

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        winners = [x for x in realized_pnls if x > 0.0]
        if realized_pnls is None or not winners:
            return 0.0

        return min(np.asarray(winners, dtype=np.float64))

    def min_loser(self, currency: Currency = None) -> float:
        """
        Return the minimum loser for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        float

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        losers = [x for x in realized_pnls if x <= 0.0]
        if not losers:
            return 0.0

        return max(np.asarray(losers, dtype=np.float64))  # max is least loser

    def avg_winner(self, currency: Currency = None) -> float:
        """
        Return the average winner for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        float

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        pnls = realized_pnls.to_numpy()
        winners = pnls[pnls > 0.0]
        if len(winners) == 0:
            return 0.0
        else:
            return winners.mean()

    def avg_loser(self, currency: Currency = None) -> float:
        """
        Return the average loser for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        float

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        pnls = realized_pnls.to_numpy()
        losers = pnls[pnls <= 0.0]
        if len(losers) == 0:
            return 0.0
        else:
            return losers.mean()

    def win_rate(self, currency: Currency = None) -> float:
        """
        Return the win rate (after commission) for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        float

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        winners = [x for x in realized_pnls if x > 0.0]
        losers = [x for x in realized_pnls if x <= 0.0]

        return len(winners) / float(max(1, (len(winners) + len(losers))))

    def expectancy(self, currency: Currency = None) -> float:
        """
        Return the expectancy for the portfolio.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the analysis.

        Returns
        -------
        float

        """
        realized_pnls = self.realized_pnls(currency)
        if realized_pnls is None or realized_pnls.empty:
            return 0.0

        win_rate = self.win_rate(currency)
        loss_rate = 1.0 - win_rate

        return (self.avg_winner(currency) * win_rate) + (self.avg_loser(currency) * loss_rate)

    def returns(self) -> pd.Series:
        """
        Return raw the returns data.

        Returns
        -------
        pd.Series

        """
        return self._returns

    def returns_avg(self) -> float:
        """
        Return the average of the returns.

        Returns
        -------
        float

        """
        return quantstats.stats.avg_return(returns=self._returns)

    def returns_avg_win(self) -> float:
        """
        Return the average win of the returns.

        Returns
        -------
        float

        """
        return quantstats.stats.avg_win(returns=self._returns)

    def returns_avg_loss(self) -> float:
        """
        Return the average loss of the returns.

        Returns
        -------
        float

        """
        return quantstats.stats.avg_loss(returns=self._returns)

    def returns_annual_volatility(self) -> float:
        """
        Return the mean annual growth rate of the returns.

        Returns
        -------
        float

        Notes
        -----
        This is equivalent to the compound annual growth rate.

        """
        return quantstats.stats.volatility(returns=self._returns)

    def sharpe_ratio(self) -> float:
        """
        Return the Sharpe ratio of the returns.

        Returns
        -------
        float

        """
        return quantstats.stats.sharpe(returns=self._returns)

    def sortino_ratio(self) -> float:
        """
        Return the Sortino ratio of the returns.

        Returns
        -------
        float

        """
        return quantstats.stats.sortino(returns=self._returns)

    def profit_factor(self) -> float:
        """
        Return the profit ratio (win ratio / loss ratio).

        Returns
        -------
        float

        """
        return quantstats.stats.profit_factor(returns=self._returns)

    def profit_ratio(self) -> float:
        """
        Return the profit ratio (win ratio / loss ratio).

        Returns
        -------
        float

        """
        return quantstats.stats.profit_ratio(returns=self._returns)

    def risk_return_ratio(self) -> float:
        """
        Return the return / risk ratio (sharpe ratio without factoring in the risk-free rate).

        Returns
        -------
        float

        """
        return quantstats.stats.risk_return_ratio(returns=self._returns)

    def get_performance_stats_pnls(self, currency: Currency = None) -> Dict[str, float]:
        """
        Return the performance statistics for PnL from the last backtest run.

        Money objects are converted to floats.

        Parameters
        ----------
        currency : Currency
            The currency for the performance.

        Returns
        -------
        dict[str, float]

        """
        return {
            "pnl": self.total_pnl(currency),
            "pnl_%": self.total_pnl_percentage(currency),
            "max_winner": self.max_winner(currency),
            "avg_winner": self.avg_winner(currency),
            "min_winner": self.min_winner(currency),
            "min_loser": self.min_loser(currency),
            "avg_loser": self.avg_loser(currency),
            "max_loser": self.max_loser(currency),
            "win_rate": self.win_rate(currency),
            "expectancy": self.expectancy(currency),
        }

    def get_performance_stats_returns(self) -> Dict[str, float]:
        """
        Return the performance statistics from the last backtest run.

        Returns
        -------
        dict[str, double]

        """
        return {
            "returns_avg": self.returns_avg(),
            "returns_avg_win": self.returns_avg_win(),
            "returns_avg_loss": self.returns_avg_loss(),
            "returns_annual_volatility": self.returns_annual_volatility(),
            "sharpe_ratio": self.sharpe_ratio(),
            "sortino_ratio": self.sortino_ratio(),
            "profit_factor": self.profit_factor(),
            "profit_ratio": self.profit_ratio(),
            "risk_return_ratio": self.risk_return_ratio(),
        }

    def get_performance_stats_pnls_formatted(self, currency: Currency = None) -> List[str]:
        """
        Return the performance statistics from the last backtest run formatted
        for printing in the backtest run footer.

        Parameters
        ----------
        currency : Currency
            The currency for the performance.

        Returns
        -------
        list[str]

        """
        return [
            f"PnL:               {round(self.total_pnl(currency), currency.precision):,} {currency}",
            f"PnL%:              {round(self.total_pnl_percentage(currency), 4)}%",
            f"Max Winner:        {round(self.max_winner(currency), currency.precision):,} {currency}",
            f"Avg Winner:        {round(self.avg_winner(currency), currency.precision):,} {currency}",
            f"Min Winner:        {round(self.min_winner(currency), currency.precision):,} {currency}",
            f"Min Loser:         {round(self.min_loser(currency), currency.precision):,} {currency}",
            f"Avg Loser:         {round(self.avg_loser(currency), currency.precision):,} {currency}",
            f"Max Loser:         {round(self.max_loser(currency), currency.precision):,} {currency}",
            f"Win Rate:          {round(self.win_rate(currency), 4)}",
            f"Expectancy:        {round(self.expectancy(currency), currency.precision):,} {currency}",
        ]

    def get_performance_stats_returns_formatted(self) -> List[str]:
        """
        Return the performance statistics for returns from the last backtest run
        formatted for printing in the backtest run footer.

        Returns
        -------
        list[str]

        """
        return [
            f"Returns Avg:          {round(self.returns_avg() * 100, 2)}%",
            f"Returns Avg win:      {round(self.returns_avg_win() * 100, 2)}%",
            f"Returns Avg loss:     {round(self.returns_avg_loss() * 100, 2)}%",
            f"Volatility (Annual):  {round(self.returns_annual_volatility() * 100, 2)}%",
            f"Sharpe ratio:         {round(self.sharpe_ratio(), 2)}",
            f"Sortino ratio:        {round(self.sortino_ratio(), 2)}",
            f"Profit factor:        {round(self.profit_factor(), 2)}",
            f"Profit ratio:         {round(self.profit_ratio(), 2)}",
            f"Return Risk Ratio:    {round(self.risk_return_ratio(), 2)}",
        ]
