# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from itertools import permutations
import os

import pandas as pd

from nautilus_trader import PACKAGE_ROOT

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport basis_points_as_percentage
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.currency cimport currency_from_string
from nautilus_trader.model.c_enums.currency cimport currency_to_string
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport liquidity_side_to_string
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport price_type_to_string
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CommissionModel:
    """
    The base class for all commission models.
    """

    cpdef Money calculate(
            self,
            Symbol symbol,
            Quantity filled_qty,
            Price filled_price,
            double exchange_rate,
            Currency currency,
            LiquiditySide liquidity_side,
    ):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money calculate_for_notional(
            self,
            Symbol symbol,
            Money notional_value,
            LiquiditySide liquidity_side,
    ):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class GenericCommissionModel:
    """
    Provides a generic commission model.
    """

    def __init__(
            self,
            dict rates=None,
            double default_rate_bp=0.20,
            Money minimum=None,
    ):
        """
        Initialize a new instance of the CommissionCalculator class.

        Commission rates are expressed as basis points of notional transaction value.

        Parameters
        ----------
        rates : Dict[Symbol, double]
            The commission rates Dict[Symbol, double].
        default_rate_bp : double
            The default rate if symbol not found in the rates dictionary (>= 0).
        minimum : Money
            The minimum commission fee per transaction.

        Raises
        ------
        TypeError
            If rates contains a key type not of Symbol, or value type not of float.
        ValueError
            If default_rate_bp is negative (< 0).

        """
        if rates is None:
            rates = {}
        if minimum is None:
            minimum = Money(0, Currency.USD)
        Condition.dict_types(rates, Symbol, float, "rates")
        Condition.not_negative(default_rate_bp, "default_rate_bp")
        super().__init__()

        self.rates = rates
        self.default_rate_bp = default_rate_bp
        self.minimum= minimum

    cpdef Money calculate(
            self,
            Symbol symbol,
            Quantity filled_qty,
            Price filled_price,
            double exchange_rate,
            Currency currency,
            LiquiditySide liquidity_side,
    ):
        """
        Return the calculated commission for the given arguments.

        Parameters
        ----------
        symbol : Symbol
            The symbol for calculation.
        filled_qty : Quantity
            The filled quantity.
        filled_price : Price
            The filled price.
        exchange_rate : double
            The exchange rate (symbol quote currency to account base currency).
        currency : Currency
            The currency for the calculation.
        liquidity_side : LiquiditySide
            The liquidity side of the trade.

        Returns
        -------
        Money

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(filled_qty, "filled_qty")
        Condition.not_none(filled_price, "filled_price")
        Condition.positive(exchange_rate, "exchange_rate")

        cdef double commission_rate_percent = basis_points_as_percentage(self.get_rate(symbol))
        cdef double commission = filled_qty.as_double() * filled_price.as_double() * exchange_rate * commission_rate_percent
        cdef double final_commission = max(self.minimum.as_double(), commission)
        return Money(final_commission, currency)

    cpdef Money calculate_for_notional(
            self,
            Symbol symbol,
            Money notional_value,
            LiquiditySide liquidity_side,
    ):
        """
        Return the calculated commission for the given arguments.

        Parameters
        ----------
        symbol : Symbol
            The symbol for calculation.
        notional_value : Money
            The notional value for the transaction.
        liquidity_side : LiquiditySide
            The liquidity side of the trade.

        Returns
        -------
        Money

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(notional_value, "notional_value")

        cdef double commission_rate_percent = basis_points_as_percentage(self.get_rate(symbol))
        cdef double value = max(self.minimum.as_double(), notional_value.as_double() * commission_rate_percent)
        return Money(value, notional_value.currency)

    cpdef double get_rate(self, Symbol symbol) except *:
        """
        Return the commission rate for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the rate.

        Returns
        -------
        double

        """
        rate = self.rates.get(symbol)
        return rate if rate is not None else self.default_rate_bp


cdef class MakerTakerCommissionModel:
    """
    Provides a commission model with separate rates for liquidity takers or
    makers.
    """

    def __init__(
            self,
            dict taker_rates=None,
            dict maker_rates=None,
            double taker_default_rate_bp=7.5,
            double maker_default_rate_bp=-2.5,
    ):
        """
        Initialize a new instance of the CommissionCalculator class.

        Commission rates are expressed as basis points of notional transaction value.

        Parameters
        ----------
        taker_rates : Dict[Symbol, double]
            The taker commission rates in basis points.
        taker_rates : Dict[Symbol, double]
            The maker commission rates in basis points.
        taker_default_rate_bp : double
            The taker default rate if symbol not found in taker_rates dictionary.
        maker_default_rate_bp : double
            The maker default rate if symbol not found in maker_rates dictionary.

        Raises
        ------
        TypeError
            If taker_rates contains a key type not of Symbol, or value type not of float.
        TypeError
            If maker_rates contains a key type not of Symbol, or value type not of float.

        """
        if taker_rates is None:
            taker_rates = {}
        if maker_rates is None:
            maker_rates = {}
        Condition.dict_types(taker_rates, Symbol, float, "taker_rates")
        Condition.dict_types(maker_rates, Symbol, float, "maker_rates")
        super().__init__()

        self.taker_rates = taker_rates
        self.maker_rates = maker_rates
        self.taker_default_rate_bp = taker_default_rate_bp
        self.maker_default_rate_bp = maker_default_rate_bp

    cpdef Money calculate(
            self,
            Symbol symbol,
            Quantity filled_qty,
            Price filled_price,
            double exchange_rate,
            Currency currency,
            LiquiditySide liquidity_side,
    ):
        """
        Return the calculated commission for the given arguments.

        Parameters
        ----------
        symbol : Symbol
            The symbol for calculation.
        filled_qty : Quantity
            The filled quantity.
        filled_price : Price
            The filled price.
        exchange_rate : double
            The exchange rate (symbol quote currency to account base currency).
        currency : Currency
            The currency for the calculation.
        liquidity_side : LiquiditySide
            The liquidity side of the trade.

        Returns
        -------
        Money

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(filled_qty, "filled_qty")
        Condition.not_none(filled_price, "filled_price")
        Condition.positive(exchange_rate, "exchange_rate")

        cdef double commission_rate_percent = basis_points_as_percentage(self.get_rate(symbol, liquidity_side))
        cdef double commission = filled_qty.as_double() * filled_price.as_double() * exchange_rate * commission_rate_percent
        return Money(commission, currency)

    cpdef Money calculate_for_notional(
            self,
            Symbol symbol,
            Money notional_value,
            LiquiditySide liquidity_side,
    ):
        """
        Return the calculated commission for the given arguments.

        Parameters
        ----------
        symbol : Symbol
            The symbol for calculation.
        notional_value : Money
            The notional value for the transaction.
        liquidity_side : LiquiditySide
            The liquidity side of the trade.

        Returns
        -------
        Money

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(notional_value, "notional_value")

        cdef double commission_rate_percent = basis_points_as_percentage(self.get_rate(symbol, liquidity_side))
        cdef double commission = notional_value.as_double() * commission_rate_percent
        return Money(notional_value.as_double() * commission_rate_percent, notional_value.currency)

    cpdef double get_rate(self, Symbol symbol, LiquiditySide liquidity_side) except *:
        """
        Return the commission rate for the given symbol and liquidity side.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the rate.
        liquidity_side : LiquiditySide
            The liquidity side for the rate.

        Returns
        -------
        double

        """
        if liquidity_side == LiquiditySide.TAKER:
            rate = self.taker_rates.get(symbol)
            return rate if rate is not None else self.taker_default_rate_bp
        elif liquidity_side == LiquiditySide.MAKER:
            rate = self.maker_rates.get(symbol)
            return rate if rate is not None else self.maker_default_rate_bp
        else:
            liquidity_side_str = liquidity_side_to_string(liquidity_side)
            raise ValueError(f"Cannot get commission rate (liquidity side was {liquidity_side_str})")


cdef class ExchangeRateCalculator:
    """
    Provides exchange rate calculations between currencies. An exchange rate is
    the value of one nation or economic zones currency versus that of another.
    """

    cpdef double get_rate(
            self,
            Currency from_currency,
            Currency to_currency,
            PriceType price_type,
            dict bid_rates,
            dict ask_rates
    ) except *:
        """
        Return the calculated exchange rate for the given quote currency to the
        given base currency for the given price type using the given dictionary of
        bid and ask rates.

        Parameters
        ----------
        from_currency : Currency
            The currency to convert from.
        to_currency : Currency
            The currency to convert to.
        price_type : PriceType
            The price type for conversion.
        bid_rates : dict
            The dictionary of currency pair bid rates Dict[str, double].
        ask_rates : dict
            The dictionary of currency pair ask rates Dict[str, double].

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If bid_rates length is not equal to ask_rates length.
        ValueError
            If price_type is UNDEFINED or LAST.

        """
        Condition.not_none(bid_rates, "bid_rates")
        Condition.not_none(ask_rates, "ask_rates")
        Condition.equal(len(bid_rates), len(ask_rates), "len(bid_rates)", "len(ask_rates)")

        if from_currency == to_currency:
            return 1.0  # No exchange necessary

        if price_type == PriceType.BID:
            calculation_rates = bid_rates
        elif price_type == PriceType.ASK:
            calculation_rates = ask_rates
        elif price_type == PriceType.MID:
            calculation_rates = {}  # type: {str, float}
            for ccy_pair in bid_rates.keys():
                calculation_rates[ccy_pair] = (bid_rates[ccy_pair] + ask_rates[ccy_pair]) / 2.0
        else:
            raise ValueError(f"Cannot calculate exchange rate for price type {price_type_to_string(price_type)}")

        cdef dict exchange_rates = {}
        cdef set symbols = set()
        cdef str symbol_lhs
        cdef str symbol_rhs

        # Add given currency rates
        for ccy_pair, rate in calculation_rates.items():
            # Get currency pair symbols
            symbol_lhs = ccy_pair[:3]
            symbol_rhs = ccy_pair[-3:]
            symbols.add(symbol_lhs)
            symbols.add(symbol_rhs)
            # Add currency dictionaries if they do not already exist
            if symbol_lhs not in exchange_rates:
                exchange_rates[symbol_lhs] = {}
            if symbol_rhs not in exchange_rates:
                exchange_rates[symbol_rhs] = {}
            # Add currency rates
            exchange_rates[symbol_lhs][symbol_lhs] = 1.0
            exchange_rates[symbol_rhs][symbol_rhs] = 1.0
            exchange_rates[symbol_lhs][symbol_rhs] = rate

        # Generate possible currency pairs from all symbols
        cdef list possible_pairs = list(permutations(symbols, 2))

        # Calculate currency inverses
        for ccy_pair in possible_pairs:
            if ccy_pair[0] not in exchange_rates[ccy_pair[1]]:
                # Search for inverse
                if ccy_pair[1] in exchange_rates[ccy_pair[0]]:
                    exchange_rates[ccy_pair[1]][ccy_pair[0]] = 1.0 / exchange_rates[ccy_pair[0]][ccy_pair[1]]
            if ccy_pair[1] not in exchange_rates[ccy_pair[0]]:
                # Search for inverse
                if ccy_pair[0] in exchange_rates[ccy_pair[1]]:
                    exchange_rates[ccy_pair[0]][ccy_pair[1]] = 1.0 / exchange_rates[ccy_pair[1]][ccy_pair[0]]

        cdef str lhs_str = currency_to_string(from_currency)
        cdef str rhs_str = currency_to_string(to_currency)
        cdef double exchange_rate
        try:
            return exchange_rates[lhs_str][rhs_str]
        except KeyError:
            pass  # Exchange rate not yet calculated

        # Continue to calculate remaining currency rates
        cdef double common_ccy1
        cdef double common_ccy2
        for ccy_pair in possible_pairs:
            if ccy_pair[0] not in exchange_rates[ccy_pair[1]]:
                # Search for common currency
                for symbol in symbols:
                    if symbol in exchange_rates[ccy_pair[0]] and symbol in exchange_rates[ccy_pair[1]]:
                        common_ccy1 = exchange_rates[ccy_pair[0]][symbol]
                        common_ccy2 = exchange_rates[ccy_pair[1]][symbol]
                        exchange_rates[ccy_pair[1]][ccy_pair[0]] = common_ccy2 / common_ccy1
                        # Check inverse and calculate if not found
                        if ccy_pair[1] not in exchange_rates[ccy_pair[0]]:
                            exchange_rates[ccy_pair[0]][ccy_pair[1]] = common_ccy1 / common_ccy2
                    elif ccy_pair[0] in exchange_rates[symbol] and ccy_pair[1] in exchange_rates[symbol]:
                        common_ccy1 = exchange_rates[symbol][ccy_pair[0]]
                        common_ccy2 = exchange_rates[symbol][ccy_pair[1]]
                        exchange_rates[ccy_pair[1]][ccy_pair[0]] = common_ccy2 / common_ccy1
                        # Check inverse and calculate if not found
                        if ccy_pair[1] not in exchange_rates[ccy_pair[0]]:
                            exchange_rates[ccy_pair[0]][ccy_pair[1]] = common_ccy1 / common_ccy2
        try:
            return exchange_rates[lhs_str][rhs_str]
        except KeyError:
            raise ValueError(f"Cannot calculate exchange rate for {lhs_str}{rhs_str} or {rhs_str}{lhs_str} "
                             f"(not enough data)")


cdef class RolloverInterestCalculator:
    """
    Provides rollover interest rate calculations.

    If rate_data_csv_path is empty then will default to the included short-term
    interest rate data csv (data since 1956).
    """

    def __init__(self, str short_term_interest_csv_path not None="default"):
        """
        Initialize a new instance of the RolloverInterestCalculator class.

        Parameters
        ----------
        short_term_interest_csv_path : str
            The path to the short term interest rate data csv.

        """
        if short_term_interest_csv_path == "default":
            short_term_interest_csv_path = os.path.join(PACKAGE_ROOT + "/_internal/rates/", "short-term-interest.csv")
        self._exchange_calculator = ExchangeRateCalculator()

        csv_rate_data = pd.read_csv(short_term_interest_csv_path)
        self._rate_data = {
            Currency.AUD: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'AUS'],
            Currency.CAD: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CAN'],
            Currency.CHF: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CHE'],
            Currency.EUR: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'EA19'],
            Currency.USD: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'USA'],
            Currency.JPY: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'JPN'],
            Currency.NZD: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'NZL'],
            Currency.GBP: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'GBR'],
            Currency.RUB: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'RUS'],
            Currency.NOK: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'NOR'],
            Currency.CNY: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CHN'],
            Currency.CNH: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CHN'],
            Currency.MXN: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'MEX'],
            Currency.ZAR: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'ZAF'],
        }

    cpdef object get_rate_data(self):
        """
        Return the short-term interest rate dataframe.

        Returns
        -------
        pd.DataFrame

        """
        return self._rate_data

    cpdef double calc_overnight_rate(self, Symbol symbol, date date) except *:
        """
        Return the rollover interest rate between the given base currency and quote currency.
        Note: 1% = 0.01 bp

        Parameters
        ----------
        symbol : Symbol
            The forex currency symbol for the calculation.
        date : date
            The date for the overnight rate.

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If symbol.code length is not in range [6, 7].

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(date, "timestamp")
        Condition.in_range_int(len(symbol.code), 6, 7, "len(symbol)")

        cdef Currency base_currency = currency_from_string(symbol.code[:3])
        cdef Currency quote_currency = currency_from_string(symbol.code[-3:])

        cdef str time_monthly = f"{date.year}-{str(date.month).zfill(2)}"
        cdef str time_quarter = f"{date.year}-Q{str(int(((date.month - 1) // 3) + 1)).zfill(2)}"

        base_data = self._rate_data[base_currency].loc[self._rate_data[base_currency]['TIME'] == time_monthly]
        if base_data.empty:
            base_data = self._rate_data[base_currency].loc[self._rate_data[base_currency]['TIME'] == time_quarter]

        quote_data = self._rate_data[quote_currency].loc[self._rate_data[quote_currency]['TIME'] == time_monthly]
        if quote_data.empty:
            quote_data = self._rate_data[quote_currency].loc[self._rate_data[quote_currency]['TIME'] == time_quarter]

        if base_data.empty and quote_data.empty:
            raise RuntimeError(f"Cannot find rollover interest rate for {symbol} on {date}.")

        cdef double base_interest = base_data['Value']
        cdef double quote_interest = quote_data['Value']

        return ((base_interest - quote_interest) / 365) / 100
