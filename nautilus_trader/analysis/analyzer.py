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

from datetime import datetime
from decimal import Decimal
from typing import Any

import pandas as pd
from numpy import float64

from nautilus_trader.accounting.accounts.base import Account
from nautilus_trader.analysis.statistic import PortfolioStatistic
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.position import Position


_NAUTILUS_PYO3_MODULE = ("nautilus_trader", "core", "nautilus_pyo3")


class PortfolioAnalyzer:
    """
    Provides a portfolio performance analyzer for tracking and generating performance
    metrics and statistics.
    """

    def __init__(self, cache: Any | None = None) -> None:
        self._statistics: dict[str, PortfolioStatistic] = {}
        self._cache = cache

        # Data
        self._account_balances_starting: dict[Currency, Money] = {}
        self._account_balances: dict[Currency, Money] = {}
        self._positions: list[Position] = []
        self._realized_pnls: dict[Currency, pd.Series] = {}
        self._position_returns: pd.Series = self._empty_returns()
        self._portfolio_returns: pd.Series = self._empty_returns()
        self._returns: pd.Series = self._empty_returns()
        self._venue = None

    def register_statistic(self, statistic: PortfolioStatistic) -> None:
        """
        Register the given statistic with the analyzer.

        Parameters
        ----------
        statistic : PortfolioStatistic
            The statistic to register.

        """
        PyCondition.not_none(statistic, "statistic")

        self._statistics[statistic.name] = statistic

    def deregister_statistic(self, statistic: PortfolioStatistic) -> None:
        """
        Deregister a statistic from the analyzer.
        """
        self._statistics.pop(statistic.name, None)

    def deregister_statistics(self) -> None:
        """
        Deregister all statistics from the analyzer.
        """
        self._statistics.clear()

    def reset(self) -> None:
        """
        Reset the analyzer.

        All stateful fields are reset to their initial value.

        """
        self._account_balances_starting = {}
        self._account_balances = {}
        self._positions = []
        self._realized_pnls = {}
        self._position_returns = self._empty_returns()
        self._portfolio_returns = self._empty_returns()
        self._returns = self._empty_returns()
        self._venue = None

    def _get_max_length_name(self) -> int:
        max_length = 0
        for stat_name in self._statistics:
            max_length = max(max_length, len(stat_name))

        return max_length

    @property
    def currencies(self) -> list[Currency]:
        """
        Return the analyzed currencies.

        Returns
        -------
        list[Currency]

        """
        return list(self._account_balances.keys())

    def statistic(self, name: str) -> PortfolioStatistic | None:
        """
        Return the statistic with the given name (if found).

        Returns
        -------
        PortfolioStatistic or ``None``

        """
        return self._statistics.get(name)

    def returns(self) -> pd.Series:
        """
        Return the primary returns series for backward compatibility.

        This returns portfolio returns when available, otherwise it falls back
        to position returns.

        Returns
        -------
        pd.Series

        """
        return self._returns

    def position_returns(self) -> pd.Series:
        """
        Return the per-position returns series.

        Returns
        -------
        pd.Series

        """
        return self._position_returns

    def portfolio_returns(self) -> pd.Series:
        """
        Return the portfolio returns series derived from account balances.

        Returns
        -------
        pd.Series

        """
        return self._portfolio_returns

    def calculate_statistics(self, account: Account, positions: list[Position]) -> None:
        """
        Calculate performance metrics from the given data.

        Parameters
        ----------
        account : Account
            The account for the calculations.
        positions : list[Position]
            The positions for the calculations.

        """
        self._account_balances_starting = account.starting_balances()
        self._account_balances = account.balances_total()
        self._positions = []
        self._realized_pnls = {}
        self._position_returns = self._empty_returns()
        self._portfolio_returns = self._empty_returns()
        self._returns = self._empty_returns()
        self._venue = positions[0].instrument_id.venue if positions else None

        self.add_positions(positions)
        self._position_returns = self._position_returns.sort_index()
        self._portfolio_returns = self._calculate_portfolio_returns(account)
        self._sync_returns_alias()

    def add_positions(self, positions: list[Position]) -> None:
        """
        Add positions data to the analyzer.

        Parameters
        ----------
        positions : list[Position]
            The positions for analysis.

        """
        self._positions += positions

        for position in positions:
            if position.realized_pnl is None:
                continue  # Skip empty shell positions

            self.add_trade(position.id, position.realized_pnl)

            if position.ts_closed > 0:
                self.add_position_return(
                    unix_nanos_to_dt(position.ts_closed),
                    position.realized_return,
                )

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
        realized_pnls = self._realized_pnls.get(currency, pd.Series(dtype=float64))
        realized_pnls.loc[position_id.value] = realized_pnl.as_double()
        self._realized_pnls[currency] = realized_pnls

    def add_position_return(self, timestamp: datetime, value: float) -> None:
        """
        Add position return data to the analyzer.

        Parameters
        ----------
        timestamp : datetime
            The timestamp for the position returns entry.
        value : double
            The position return value to add.

        """
        if timestamp not in self._position_returns:
            self._position_returns.loc[timestamp] = 0.0
        self._position_returns.loc[timestamp] += float(value)

        self._sync_returns_alias()

    def add_return(self, timestamp: datetime, value: float) -> None:
        """
        Add return data to the analyzer.

        This is a backward-compatible alias for adding position returns.

        Parameters
        ----------
        timestamp : datetime
            The timestamp for the position returns entry.
        value : double
            The position return value to add.

        """
        self.add_position_return(timestamp, value)

    def realized_pnls(self, currency: Currency | None = None) -> pd.Series | None:
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
            If `currency` is ``None`` when analyzing multi-currency portfolios.

        """
        if not self._realized_pnls:
            return None
        if currency is None:
            if len(self._account_balances) > 1:
                raise ValueError("`currency` was `None` for multi-currency portfolio")
            currency = next(iter(self._account_balances.keys()))

        return self._realized_pnls.get(currency)

    def total_pnl(
        self,
        currency: Currency | None = None,
        unrealized_pnl: Money | None = None,
    ) -> float:
        """
        Return the total PnL for the portfolio.

        For multi-currency portfolios, specify the currency for the result.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the result.
        unrealized_pnl : Money, optional
            The unrealized PnL for the given currency.

        Returns
        -------
        float

        Raises
        ------
        ValueError
            If `currency` is ``None`` when analyzing multi-currency portfolios.
        ValueError
            If `currency` is not contained in the tracked account balances.
        ValueError
            If `unrealized_pnl` is not ``None`` and currency is not equal to the given currency.

        """
        if not self._account_balances:
            return 0.0

        if currency is None:
            if len(self._account_balances) > 1:
                raise ValueError("`currency` was `None` for multi-currency portfolio")
            currency = next(iter(self._account_balances.keys()))

        if unrealized_pnl is not None and unrealized_pnl.currency != currency:
            raise ValueError(f"unrealized PnL currency is not {currency}")

        account_balance = self._account_balances.get(currency)
        account_balance_starting = self._account_balances_starting.get(currency, Money(0, currency))

        if account_balance is None:
            return 0.0

        balance_difference = self._calculate_balance_difference(currency)
        if balance_difference is not None:
            return balance_difference + self._effective_unrealized_pnl(currency, unrealized_pnl)

        unrealized_pnl_f64 = 0.0 if unrealized_pnl is None else unrealized_pnl.as_double()
        return float(account_balance - account_balance_starting) + unrealized_pnl_f64

    def total_pnl_percentage(
        self,
        currency: Currency | None = None,
        unrealized_pnl: Money | None = None,
    ) -> float:
        """
        Return the percentage change of the total PnL for the portfolio.

        For multi-currency accounts, specify the currency for the result.

        Parameters
        ----------
        currency : Currency, optional
            The currency for the result.
        unrealized_pnl : Money, optional
            The unrealized PnL for the given currency.

        Returns
        -------
        float

        Raises
        ------
        ValueError
            If `currency` is ``None`` when analyzing multi-currency portfolios.
        ValueError
            If `currency` is not contained in the tracked account balances.
        ValueError
            If `unrealized_pnl` is not ``None`` and currency is not equal to the given currency.

        """
        if not self._account_balances:
            return 0.0

        if currency is None:
            if len(self._account_balances) != 1:
                raise ValueError("currency was None for multi-currency portfolio")
            currency = next(iter(self._account_balances.keys()))

        if unrealized_pnl is not None and unrealized_pnl.currency != currency:
            raise ValueError(f"unrealized PnL currency is not {currency}")

        account_balance = self._account_balances.get(currency)
        account_balance_starting = self._account_balances_starting.get(currency, Money(0, currency))

        if account_balance is None:
            return 0.0

        starting_equity = self._calculate_portfolio_value(self._account_balances_starting, currency)
        if starting_equity is not None:
            if starting_equity == 0:
                return 0.0

            difference = self.total_pnl(currency, unrealized_pnl)
            return float((difference / starting_equity) * 100)

        if account_balance_starting.as_decimal() == 0:
            # Protect divide by zero
            return 0.0

        unrealized_pnl_f64 = 0.0 if unrealized_pnl is None else unrealized_pnl.as_double()

        # Calculate percentage
        current = account_balance + unrealized_pnl_f64
        starting = account_balance_starting
        difference = current - starting

        return float((difference / starting) * 100)

    def _calculate_balance_difference(self, currency: Currency) -> float | None:
        ending_equity = self._calculate_portfolio_value(self._account_balances, currency)
        if ending_equity is None:
            return None

        starting_equity = self._calculate_portfolio_value(self._account_balances_starting, currency)
        if starting_equity is None:
            return None

        return ending_equity - starting_equity

    def _calculate_portfolio_value(
        self,
        balances: dict[Currency, Money],
        target_currency: Currency,
    ) -> float | None:
        if not balances:
            return 0.0
        if self._cache is None:
            return None

        total = 0.0

        for money in balances.values():
            converted = self._convert_money(money, target_currency)
            if converted is None:
                return None
            total += converted

        return total

    def _effective_unrealized_pnl(
        self,
        currency: Currency,
        unrealized_pnl: Money | None,
    ) -> float:
        calculated = self._calculate_open_unrealized_pnl(currency, only_off_balance=True)
        if calculated is not None:
            return calculated

        return 0.0 if unrealized_pnl is None else unrealized_pnl.as_double()

    def _calculate_open_unrealized_pnl(
        self,
        currency: Currency,
        *,
        only_off_balance: bool,
    ) -> float | None:
        if self._cache is None:
            return None

        total = 0.0

        for position in self._positions:
            if not position.is_open:
                continue
            if only_off_balance and self._position_is_balance_backed(position):
                continue

            price = self._position_price(position)
            if price is None:
                return None

            unrealized = position.unrealized_pnl(price)
            converted = self._convert_money(unrealized, currency)
            if converted is None:
                return None

            total += converted

        return total

    def _position_is_balance_backed(self, position: Position) -> bool:
        return (
            position.base_currency is not None and position.base_currency in self._account_balances
        )

    def _position_price(self, position: Position):
        if self._cache is None:
            return None

        if position.is_long:
            price_type = PriceType.BID
        elif position.is_short:
            price_type = PriceType.ASK
        else:
            price_type = PriceType.LAST

        price = self._cache.price(position.instrument_id, price_type)
        if price is not None:
            return price

        return self._cache.price(position.instrument_id, PriceType.LAST) or self._cache.price(
            position.instrument_id,
            PriceType.MID,
        )

    def _convert_money(self, money: Money, target_currency: Currency) -> float | None:
        if money.currency == target_currency:
            return money.as_double()
        if self._cache is None:
            return None

        venue = self._resolve_conversion_venue(money.currency, target_currency)
        if venue is None:
            return None

        xrate = self._cache.get_xrate(
            venue=venue,
            from_currency=money.currency,
            to_currency=target_currency,
            price_type=PriceType.MID,
        )

        if xrate is None or xrate <= 0:
            return None

        return money.as_double() * xrate

    def _resolve_conversion_venue(self, from_currency: Currency, to_currency: Currency):
        if self._venue is not None:
            return self._venue
        if self._cache is None:
            return None

        for position in self._positions:
            if (
                position.quote_currency == from_currency
                or position.quote_currency == to_currency
                or position.base_currency == from_currency
                or position.base_currency == to_currency
                or position.settlement_currency == from_currency
                or position.settlement_currency == to_currency
            ):
                return position.instrument_id.venue

        for instrument in self._cache.instruments():
            base_currency = instrument.get_base_currency()
            if (
                (base_currency == from_currency and instrument.quote_currency == to_currency)
                or (base_currency == to_currency and instrument.quote_currency == from_currency)
                or (
                    instrument.quote_currency == from_currency
                    and instrument.get_settlement_currency() == to_currency
                )
                or (
                    instrument.quote_currency == to_currency
                    and instrument.get_settlement_currency() == from_currency
                )
            ):
                return instrument.id.venue

        return None

    def get_performance_stats_pnls(
        self,
        currency: Currency | None = None,
        unrealized_pnl: Money | None = None,
    ) -> dict[str, float]:
        """
        Return the 'PnL' (profit and loss) performance statistics, optionally includes
        the unrealized PnL.

        Money objects are converted to floats.

        Parameters
        ----------
        currency : Currency
            The currency for the performance.
        unrealized_pnl : Money, optional
            The unrealized PnL for the performance.

        Returns
        -------
        dict[str, Any]

        """
        realized_pnls = self.realized_pnls(currency)

        output: dict[str, Any] = {
            "PnL (total)": self.total_pnl(currency, unrealized_pnl),
            "PnL% (total)": self.total_pnl_percentage(currency, unrealized_pnl),
        }

        for name, stat in self._statistics.items():
            if _is_pyo3_statistic(stat):
                pnls_list = realized_pnls.tolist() if realized_pnls is not None else []
                value = stat.calculate_from_realized_pnls(pnls_list)
            else:
                value = stat.calculate_from_realized_pnls(realized_pnls)

            if value is None:
                continue  # Not implemented

            if not isinstance(value, int | float | str | bool):
                value = str(value)

            output[name] = value

        return output

    def get_performance_stats_returns(self) -> dict[str, Any]:
        """
        Return the primary `returns` performance statistics values.

        This uses portfolio returns when available, otherwise it falls back
        to position returns.

        Returns
        -------
        dict[str, Any]

        """
        return self._calculate_returns_stats(self._returns)

    def get_performance_stats_position_returns(self) -> dict[str, Any]:
        """
        Return the per-position returns performance statistics values.

        Returns
        -------
        dict[str, Any]

        """
        return self._calculate_returns_stats(self._position_returns)

    def get_performance_stats_portfolio_returns(self) -> dict[str, Any]:
        """
        Return the portfolio returns performance statistics values.

        Returns
        -------
        dict[str, Any]

        """
        return self._calculate_returns_stats(self._portfolio_returns)

    def get_performance_stats_general(self) -> dict[str, Any]:
        """
        Return the `general` performance statistics.

        Returns
        -------
        dict[str, Any]

        """
        output: dict[str, Any] = {}

        for name, stat in self._statistics.items():
            value = stat.calculate_from_positions(self._positions)
            if value is None:
                continue  # Not implemented

            if not isinstance(value, int | float | str | bool):
                value = str(value)

            output[name] = value

        return output

    def get_stats_pnls_formatted(
        self,
        currency: Currency | None = None,
        unrealized_pnl: Money | None = None,
    ) -> list[str]:
        """
        Return the performance statistics from the last backtest run formatted for
        printing in the backtest run footer.

        Parameters
        ----------
        currency : Currency
            The currency for the performance.
        unrealized_pnl : Money, optional
            The unrealized PnL for the performance.

        Returns
        -------
        list[str]

        """
        max_length: int = self._get_max_length_name()
        stats = self.get_performance_stats_pnls(currency, unrealized_pnl)

        output: list[str] = []

        for k, v in stats.items():
            padding = max_length - len(k) + 1
            output.append(f"{k}: {' ' * padding}{v:_}")

        return output

    def get_stats_returns_formatted(self) -> list[str]:
        """
        Return the performance statistics for primary returns from the last backtest run
        formatted for printing in the backtest run footer.

        Returns
        -------
        list[str]

        """
        return self._format_stats(self.get_performance_stats_returns())

    def get_stats_position_returns_formatted(self) -> list[str]:
        """
        Return the performance statistics for per-position returns from the last
        backtest run formatted for printing in the backtest run footer.

        Returns
        -------
        list[str]

        """
        return self._format_stats(self.get_performance_stats_position_returns())

    def get_stats_portfolio_returns_formatted(self) -> list[str]:
        """
        Return the performance statistics for portfolio returns from the last backtest
        run formatted for printing in the backtest run footer.

        Returns
        -------
        list[str]

        """
        return self._format_stats(self.get_performance_stats_portfolio_returns())

    def get_stats_general_formatted(self) -> list[str]:
        """
        Return the performance statistics for returns from the last backtest run
        formatted for printing in the backtest run footer.

        Returns
        -------
        list[str]

        """
        max_length: int = self._get_max_length_name()
        stats = self.get_performance_stats_general()

        output: list[str] = []

        for k, v in stats.items():
            padding = max_length - len(k) + 1
            v_formatted = f"{v:_}" if isinstance(v, int | float | Decimal) else str(v)
            output.append(f"{k}: {' ' * padding}{v_formatted}")

        return output

    def _calculate_portfolio_returns(self, account: Account) -> pd.Series:
        """
        Compute daily portfolio returns from account balance snapshots.

        Returns an empty series (falling back to per-position returns) when:
        - Fewer than two account state events exist.
        - Any event carries multiple balance currencies.
        - The balance currency changes between events.
        - Fewer than two distinct calendar days have balance data.

        Multi-currency accounts are not yet supported; the caller silently
        receives per-position returns in that case.

        """
        states = sorted(account.events, key=lambda state: state.ts_event)
        if len(states) < 2:
            return self._empty_returns()

        currency = None
        daily_balances: dict[pd.Timestamp, float] = {}

        for state in states:
            if len(state.balances) != 1:
                return self._empty_returns()

            balance = state.balances[0]
            if currency is not None and balance.currency != currency:
                return self._empty_returns()
            currency = balance.currency

            day = pd.Timestamp(unix_nanos_to_dt(state.ts_event)).normalize()
            daily_balances[day] = balance.total.as_double()

        if len(daily_balances) < 2:
            return self._empty_returns()

        total_balance = pd.Series(daily_balances, dtype=float64).sort_index()
        account_returns = (
            total_balance.resample("D")
            .last()
            .ffill()
            .pct_change()
            .replace(
                [float("inf"), float("-inf")],
                float("nan"),
            )
        ).dropna()

        if account_returns.empty:
            return self._empty_returns()

        return account_returns.astype(float64)

    def _calculate_returns_stats(self, returns: pd.Series) -> dict[str, Any]:
        output: dict[str, Any] = {}

        for name, stat in self._statistics.items():
            if _is_pyo3_statistic(stat):
                returns_dict: dict[int, float] = {}

                if not returns.empty:
                    for timestamp, value in returns.items():
                        returns_dict[timestamp.value] = float(value)

                value = stat.calculate_from_returns(returns_dict)
            else:
                value = stat.calculate_from_returns(returns)

            if value is None:
                continue

            if not isinstance(value, int | float | str | bool):
                value = str(value)

            output[name] = value

        return output

    def _format_stats(self, stats: dict[str, Any]) -> list[str]:
        max_length: int = self._get_max_length_name()
        output: list[str] = []

        for key, value in stats.items():
            padding = max_length - len(key) + 1
            output.append(f"{key}: {' ' * padding}{value:_}")

        return output

    def _sync_returns_alias(self) -> None:
        if not self._portfolio_returns.empty:
            self._returns = self._portfolio_returns
            return

        self._returns = self._position_returns

    def _empty_returns(self) -> pd.Series:
        return pd.Series(dtype=float64)


def _is_pyo3_statistic(stat: Any) -> bool:
    module = getattr(type(stat), "__module__", None)
    if not isinstance(module, str):
        return False

    return tuple(module.split(".")[: len(_NAUTILUS_PYO3_MODULE)]) == _NAUTILUS_PYO3_MODULE
