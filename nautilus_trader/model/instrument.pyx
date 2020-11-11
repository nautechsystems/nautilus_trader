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

from decimal import ROUND_HALF_EVEN

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.position_side cimport PositionSideParser
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Quantity


cdef class CostSpecification:
    """
    Represents a standard instruments cost specification for accurate
    calculation of PNLs.
    """

    def __init__(self, Currency quote_currency, str rounding not None=ROUND_HALF_EVEN):
        """
        Initialize a new instance of the `CostSpecification` class.

        Parameters
        ----------
        quote_currency : Currency
            The instruments quote currency.

        """
        self.quote_currency = quote_currency
        self.settlement_currency = quote_currency
        self.rounding = rounding


cdef class InverseCostSpecification(CostSpecification):
    """
    Represents an inverse instruments cost specification for accurate
    calculation of PNLs.
    """

    def __init__(
            self,
            Currency base_currency not None,
            Currency quote_currency not None,
            str rounding not None=ROUND_HALF_EVEN,
    ):
        """
        Initialize a new instance of the `InverseCostSpecification` class.

        Parameters
        ----------
        base_currency : Currency
            The instruments base currency.
        quote_currency : Currency
            The instruments quote currency.

        """
        super().__init__(quote_currency=quote_currency, rounding=rounding)

        self.base_currency = base_currency
        self.settlement_currency = base_currency
        self.is_inverse = True


cdef class QuantoCostSpecification(CostSpecification):
    """
    Represents a quanto instruments cost specification for accurate
    calculation of PNLs.
    """

    def __init__(
            self,
            Currency base_currency not None,
            Currency quote_currency not None,
            Currency settlement_currency not None,
            bint is_inverse,
            Decimal xrate not None,
            str rounding not None=ROUND_HALF_EVEN,
    ):
        """
        Initialize a new instance of the `QuantoCostSpecification` class.

        Parameters
        ----------
        base_currency : Currency
            The instruments base currency.
        quote_currency : Currency
            The instruments quote currency.
        settlement_currency : Currency
            The instruments settlement currency.
        is_inverse : bool
            If the instrument is inverse.
        xrate : Decimal
            The current exchange rate between base and settlement currencies.

        """
        super().__init__(quote_currency=quote_currency, rounding=rounding)

        self.base_currency = base_currency
        self.settlement_currency = settlement_currency
        self.is_inverse = is_inverse
        self.is_quanto = True
        self.xrate = xrate


cdef class Instrument:
    """
    Represents a tradeable financial market instrument.
    """

    def __init__(
            self,
            Symbol symbol not None,
            AssetClass asset_class,
            AssetType asset_type,
            Currency base_currency,  # Can be None
            Currency quote_currency not None,
            Currency settlement_currency not None,
            bint is_inverse,
            int price_precision,
            int size_precision,
            Decimal tick_size not None,
            Decimal multiplier not None,
            Decimal leverage not None,
            Quantity lot_size not None,
            Quantity max_quantity,  # Can be None
            Quantity min_quantity,  # Can be None
            Money max_notional,     # Can be None
            Money min_notional,     # Can be None
            Price max_price,        # Can be None
            Price min_price,        # Can be None
            Decimal margin_initial not None,
            Decimal margin_maintenance not None,
            Decimal maker_fee not None,
            Decimal taker_fee not None,
            Decimal funding_rate_long not None,
            Decimal funding_rate_short not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the `Instrument` class.

        Parameters
        ----------
        symbol : Symbol
            The symbol.
        asset_type : AssetClass
            The asset class.
        asset_type : AssetType
            The asset type.
        base_currency : Currency
            The base currency.
        quote_currency : Currency
            The quote currency.
        settlement_currency : Currency
            The settlement currency.
        is_inverse : Currency
            If the instrument costing is inverse (quantity expressed in quote currency units).
        price_precision : int
            The price decimal precision.
        size_precision : int
            The trading size decimal precision.
        tick_size : Decimal
            The tick size.
        multiplier : Decimal
            The contract value multiplier.
        leverage : Decimal
            The current leverage for the instrument.
        lot_size : Quantity
            The rounded lot unit size.
        max_quantity : Quantity
            The maximum possible order quantity.
        min_quantity : Quantity
            The minimum possible order quantity.
        max_notional : Money
            The maximum possible order notional value.
        min_notional : Money
            The minimum possible order notional value.
        max_price : Price
            The maximum possible printed price.
        min_price : Price
            The minimum possible printed price.
        margin_initial : Decimal
            The initial margin requirement in percentage of order value.
        margin_maintenance : Decimal
            The maintenance margin in percentage of position value.
        maker_fee : Decimal
            The fee rate for liquidity makers as a percentage of order value.
        taker_fee : Decimal
            The fee rate for liquidity takers as a percentage of order value.
        funding_rate_long : Decimal
            The funding rate for long positions.
        funding_rate_short : Decimal
            The funding rate for short positions.
        timestamp : datetime
            The timestamp the instrument was created/updated at.

        Raises
        ------
        ValueError
            If asset_class is UNDEFINED.
        ValueError
            If asset_type is UNDEFINED.
        ValueError
            If price_precision is negative (< 0).
        ValueError
            If size_precision is negative (< 0).
        ValueError
            If tick_size is not positive (> 0).
        ValueError
            If multiplier is not positive (> 0).
        ValueError
            If leverage is not positive (> 0).
        ValueError
            If lot size is not positive (> 0).
        ValueError
            If max_quantity is not positive (> 0).
        ValueError
            If min_quantity is negative (< 0).
        ValueError
            If max_notional is not positive (> 0).
        ValueError
            If min_notional is negative (< 0).
        ValueError
            If max_price is not positive (> 0).
        ValueError
            If min_price is negative (< 0).

        """
        Condition.not_equal(asset_class, AssetClass.UNDEFINED, 'asset_class', 'UNDEFINED')
        Condition.not_equal(asset_type, AssetType.UNDEFINED, 'asset_type', 'UNDEFINED')
        Condition.not_negative_int(price_precision, 'price_precision')
        Condition.not_negative_int(size_precision, 'volume_precision')
        Condition.positive(tick_size, "tick_size")
        Condition.positive(multiplier, "multiplier")
        Condition.positive(leverage, "leverage")
        Condition.positive(lot_size, "lot_size")
        if max_quantity:
            Condition.positive(max_quantity, "max_quantity")
        if min_quantity:
            Condition.not_negative(min_quantity, "min_quantity")
        if max_notional:
            Condition.positive(max_notional, "max_notional")
        if min_notional:
            Condition.not_negative(min_notional, "min_notional")
        if max_price:
            Condition.positive(max_price, "max_price")
        if min_price:
            Condition.not_negative(min_price, "min_price")
        Condition.not_negative(margin_initial, "margin_initial")
        Condition.not_negative(margin_maintenance, "margin_maintenance")
        Condition.not_negative(margin_maintenance, "margin_maintenance")

        self.symbol = symbol
        self.asset_class = asset_class
        self.asset_type = asset_type
        self.base_currency = base_currency
        self.quote_currency = quote_currency
        self.settlement_currency = settlement_currency
        self.is_inverse = is_inverse
        self.is_quanto = settlement_currency not in (base_currency, quote_currency)
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.tick_size = tick_size
        self.multiplier = multiplier
        self.leverage = leverage
        self.lot_size = lot_size
        self.max_quantity = max_quantity
        self.min_quantity = min_quantity
        self.max_notional = max_notional
        self.min_notional = min_notional
        self.max_price = max_price
        self.min_price = min_price
        self.margin_initial = margin_initial
        self.margin_maintenance = margin_maintenance
        self.maker_fee = maker_fee
        self.taker_fee = taker_fee
        self.funding_rate_long = funding_rate_long
        self.funding_rate_short = funding_rate_short
        self.timestamp = timestamp

    def __eq__(self, Instrument other) -> bool:
        return self.symbol.value == other.symbol.value

    def __ne__(self, Instrument other) -> bool:
        return self.symbol.value != other.symbol.value

    def __hash__(self) -> int:
        return hash(self.symbol.value)

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.symbol.value}')"

    cpdef void set_rounding(self, str rounding) except *:
        """
        Set the rounding rule for the `CostSpecification`.

        Parameters
        ----------
        rounding : str
            The rounding rule (constant from the decimal module).

        """
        Condition.valid_string(rounding, "rounding")

        self.cost_spec.rounding = rounding

    cpdef CostSpecification get_cost_spec(self, Decimal xrate=None):
        """
        Return the `CostSpecification` for the instrument.

        Parameters
        ----------
        xrate : Decimal, optional
            Only applicable to quanto instruments, otherwise ignored.

        Raises
        ------
        ValueError
            If is_quanto and xrate is None.

        """
        if self.is_quanto:
            Condition.not_none(xrate, "xrate")

            return QuantoCostSpecification(
                base_currency=self.base_currency,
                quote_currency=self.quote_currency,
                settlement_currency=self.settlement_currency,
                is_inverse=self.is_inverse,
                xrate=xrate,
            )

        if self.is_inverse:
            return InverseCostSpecification(
                base_currency=self.base_currency,
                quote_currency=self.quote_currency,
            )

        return CostSpecification(self.quote_currency)

    cpdef Money calculate_notional(
            self,
            Quantity quantity,
            Decimal close_price,
            Decimal xrate=None,
    ):
        """
        Calculate the notional value from the given parameters.

        Parameters
        ----------
        quantity : Quantity
            The total quantity.
        close_price : Decimal
            The closing price.
        xrate : Decimal, optional
            The exchange rate between cost and settlement currencies. Applicable
            to quanto instruments only, otherwise ignored.

        Returns
        -------
        Money
            In the settlement currency.

        Raises
        ------
        ValueError
            If is_quanto and xrate is None.

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(close_price, "close_price")
        if self.is_quanto:
            Condition.not_none(xrate, "xrate")

        if self.is_inverse:
            close_price = 1 / close_price

        cdef Decimal notional = quantity * close_price * self.multiplier

        if self.is_quanto:
            notional *= xrate

        return Money(notional, self.settlement_currency)

    cpdef Money calculate_order_margin(
            self,
            Quantity quantity,
            Price price,
            Decimal xrate=None,
    ):
        """
        Calculate the order margin from the given parameters.

        Parameters
        ----------
        quantity : Quantity
            The order quantity.
        price : Price
            The order price.
        xrate : Decimal, optional
            The exchange rate between cost and settlement currencies. Applicable
            to quanto instruments only, otherwise ignored.

        Returns
        -------
        Money
            In the settlement currency.

        Raises
        ------
        ValueError
            If is_quanto and xrate is None.

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(price, "price")
        # xrate checked in calculate_notional

        if self.leverage == 1:
            return Money(0, self.settlement_currency)  # No margin necessary

        cdef Decimal notional = self.calculate_notional(quantity, price, xrate)
        cdef Decimal margin = notional / self.leverage * self.margin_initial
        margin += notional * self.taker_fee * 2

        return Money(margin, self.settlement_currency)

    cpdef Money calculate_position_margin(
            self,
            PositionSide side,
            Quantity quantity,
            QuoteTick last,
            Decimal xrate=None,
    ):
        """
        Calculate the position margin from the given parameters.
        Parameters
        ----------
        side : PositionSide
            The currency position side.
        quantity : Quantity
            The currency position quantity.
        last : QuoteTick
            The last quote tick.
        xrate : Decimal, optional
            The exchange rate between cost and settlement currencies. Applicable
            to quanto instruments only, otherwise ignored.

        Returns
        -------
        Money
            In the settlement currency.

        Raises
        ------
        ValueError
            If last.symbol != self.symbol.
        ValueError
            If is_quanto and xrate is None.

        """
        # side checked in _get_close_price
        Condition.not_none(quantity, "quantity")
        Condition.not_none(last, "last")
        Condition.equal(last.symbol, self.symbol, "last.symbol", "self.symbol")
        # xrate checked in calculate_notional

        if self.leverage == 1:
            return Money(0, self.settlement_currency)  # No margin necessary

        cdef Price close_price = self._get_close_price(side, last)
        cdef Decimal notional = self.calculate_notional(quantity, close_price, xrate)
        cdef Decimal margin = notional / self.leverage * self.margin_maintenance
        margin += notional * self.taker_fee

        return Money(margin, self.settlement_currency)

    cpdef Money calculate_open_value(
            self,
            PositionSide side,
            Quantity quantity,
            QuoteTick last,
            Decimal xrate=None,
    ):
        """
        Parameters
        ----------
        side : PositionSide
            The currency position side.
        quantity : Quantity
            The open quantity.
        last : QuoteTick
            The last quote tick.
        xrate : Decimal, optional
            The exchange rate between cost and settlement currencies. Applicable
            to quanto instruments only, otherwise ignored.

        Returns
        -------
        Money
            In the settlement currency.

        Raises
        ------
        ValueError
            If side is UNDEFINED or FLAT.
        ValueError
            If last.symbol != self.symbol.
        ValueError
            If is_quanto and xrate is None.

        """
        # side checked in _get_close_price
        Condition.not_none(quantity, "quantity")
        Condition.not_none(last, "last")
        Condition.equal(last.symbol, self.symbol, "last.symbol", "self.symbol")

        cdef Price close_price = self._get_close_price(side, last)
        cdef Decimal notional = self.calculate_notional(quantity, close_price, xrate)

        return Money(notional, self.settlement_currency)

    cpdef Money calculate_commission(
        self,
        Quantity quantity,
        Decimal avg_price,
        LiquiditySide liquidity_side,
        Decimal xrate=None,
    ):
        """
        Calculate the commissions generated from a transaction with the given
        parameters.

        Parameters
        ----------
        quantity : Quantity
            The quantity for the transaction.
        avg_price : Price
            The average transaction price (only applicable for inverse
            instruments, else ignored).
        liquidity_side : LiquiditySide
            The liquidity side for the transaction.
        xrate : Decimal, optional
            The exchange rate between cost and settlement currencies. Applicable
            to quanto instruments only, otherwise ignored.

        Returns
        -------
        Money
            In the settlement currency.

        Raises
        ------
        ValueError
            If liquidity_side is NONE.
        ValueError
            If is_quanto and xrate is None.

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(avg_price, "avg_price")
        Condition.not_equal(liquidity_side, LiquiditySide.NONE, "liquidity_side", "NONE")
        if self.is_quanto:
            Condition.not_none(xrate, "xrate")

        cdef Decimal notional = self.calculate_notional(quantity, avg_price, xrate)

        cdef Decimal commission
        if liquidity_side == LiquiditySide.MAKER:
            commission = notional * self.maker_fee
        elif liquidity_side == LiquiditySide.TAKER:
            commission = notional * self.taker_fee
        else:
            raise RuntimeError(f"invalid LiquiditySide, "
                               f"was {LiquiditySideParser.to_string(liquidity_side)}")

        return Money(commission, self.settlement_currency)

    cdef inline Decimal _get_close_price(self, PositionSide side, QuoteTick last):
        if side == PositionSide.LONG:
            return last.bid
        elif side == PositionSide.SHORT:
            return last.ask
        else:
            raise RuntimeError(f"invalid PositionSide, "
                               f"was {PositionSideParser.to_string(side)}")
