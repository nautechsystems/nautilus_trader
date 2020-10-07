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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport basis_points_as_percentage
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport liquidity_side_to_string
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.currency cimport USD
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
            minimum = Money(0, USD)
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
        cdef double commission = filled_qty * filled_price * exchange_rate * commission_rate_percent
        cdef double final_commission = max(self.minimum.as_double(), commission)
        return Money(max(self.minimum.as_double(), commission), currency)

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
        cdef double value = max(self.minimum.as_double(), notional_value * commission_rate_percent)
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
        cdef double commission = filled_qty * filled_price * exchange_rate * commission_rate_percent
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
        cdef double commission = notional_value * commission_rate_percent
        return Money(notional_value * commission_rate_percent, notional_value.currency)

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
