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

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport liquidity_side_to_string
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.position_side cimport position_side_to_string
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Quantity


cdef class Instrument:
    """
    Represents a tradeable financial market instrument.
    """

    def __init__(
            self,
            Symbol symbol not None,
            AssetClass asset_class,
            AssetType asset_type,
            Currency base_currency not None,
            Currency quote_currency not None,
            Currency settlement_currency not None,
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
            Decimal settlement_fee not None,
            Decimal funding_rate_long not None,
            Decimal funding_rate_short not None,
            datetime timestamp not None,
            dict info=None,
    ):
        """
        Initialize a new instance of the Instrument class.

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
        settlement_fee : Decimal
            The fee rate for settlements as a percentage of order value.
        funding_rate_long : Decimal
            The funding rate for long positions.
        funding_rate_short : Decimal
            The funding rate for short positions.
        timestamp : datetime
            The timestamp the instrument was created/updated at.
        info : dict, optional
            For more detailed and exchange specific instrument information.

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
        if info is None:
            info = {}
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
        self.is_inverse = info.get("is_inverse", False)
        self.is_quanto = info.get("is_quanto", False)
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.cost_precision = self.settlement_currency.precision
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
        self.settlement_fee = settlement_fee
        self.funding_rate_long = funding_rate_long
        self.funding_rate_short = funding_rate_short
        self.timestamp = timestamp

    def __eq__(self, Instrument other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.symbol == other.symbol

    def __ne__(self, Instrument other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return not self == other

    def __hash__(self) -> int:
        """
        Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash(self.symbol.value)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.__class__.__name__}({self.symbol.value})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{str(self)} object at {id(self)}>"

    cpdef Money calculate_order_margin(self, Quantity quantity, Price price):
        """
        Calculate the order margin from the given parameters.

        Parameters
        ----------
        quantity : Quantity
            The order quantity.
        price : Price
            The order price.

        Returns
        -------
        Money
            In the instrument base currency.

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(price, "price")

        if self.leverage == 1:
            return Money(0, self.base_currency)  # No margin necessary

        cdef Decimal notional = self._calculate_notional(quantity, price)

        cdef Decimal margin = notional / self.leverage * self.margin_initial
        margin += notional * self.taker_fee * 2

        return Money(margin, self.base_currency)

    cpdef Money calculate_position_margin(
            self,
            PositionSide side,
            Quantity quantity,
            QuoteTick last,
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

        Returns
        -------
        Money
            In the base currency for the instrument.

        Raises
        ------
        ValueError
            If last.symbol != self.symbol

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(last, "last")
        Condition.equal(last.symbol, self.symbol, "last.symbol", "self.symbol")

        if self.leverage == 1:
            return Money(0, self.base_currency)  # No margin necessary

        cdef Price close_price = self._get_close_price(side, last)
        cdef Decimal notional = self._calculate_notional(quantity, close_price)

        cdef Decimal margin = notional / self.leverage * self.margin_maintenance
        margin += notional * self.taker_fee

        return Money(margin, self.base_currency)

    cpdef Money calculate_open_value(
        self,
        PositionSide side,
        Quantity quantity,
        QuoteTick last,
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

        Returns
        -------
        Money
            In the instrument base currency.

        Raises
        ------
        ValueError
            If side is UNDEFINED or FLAT.
        ValueError
            If last.symbol != self.symbol

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(last, "last")
        Condition.equal(last.symbol, self.symbol, "last.symbol", "self.symbol")

        cdef Price close_price = self._get_close_price(side, last)
        cdef Decimal notional = self._calculate_notional(quantity, close_price)

        return Money(notional, self.base_currency)

    cpdef Money calculate_unrealized_pnl(
        self,
        PositionSide side,
        Quantity quantity,
        Decimal avg_open,
        QuoteTick last,
    ):
        """
        Calculate the unrealized PNL from the given parameters.

        Parameters
        ----------
        side : PositionSide
            The side of the trade.
        quantity : Quantity
            The quantity
        avg_open : Decimal
            The average open price of the trade.
        last : QuoteTick
            The last quote tick.

        Returns
        -------
        Money
            In the instrument base currency.

        Raises
        ------
        ValueError
            If side is UNDEFINED or FLAT.

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(avg_open, "avg_open")
        Condition.not_none(last, "last")

        cdef Price close_price = self._get_close_price(side, last)

        return self.calculate_pnl(
            side,
            quantity,
            avg_open,
            close_price,
        )

    cpdef Money calculate_pnl(
            self,
            PositionSide side,
            Quantity quantity,
            Decimal avg_open,
            Decimal avg_close,
    ):
        """
        Calculate the unrealized PNL from the given parameters.

        Parameters
        ----------
        side : PositionSide
            The side of the trade.
        quantity : Quantity
            The quantity
        avg_open : Decimal
            The average open price of the trade.
        avg_close : Decimal
            The average close price of the trade.

        Returns
        -------
        Money
            In the instrument base currency.

        Raises
        ------
        ValueError
            If side is UNDEFINED or FLAT.

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(avg_open, "avg_open")
        Condition.not_none(avg_close, "avg_close")

        cdef Decimal notional = self._calculate_notional(quantity, avg_close)

        if side == PositionSide.LONG:
            return_percentage = (avg_close - avg_open) / avg_open
        elif side == PositionSide.SHORT:
            return_percentage = (avg_open - avg_close) / avg_open
        else:
            raise ValueError(f"Cannot calculate PNL "
                             f"(position side was {position_side_to_string(side)}).")

        cdef Decimal pnl = notional * return_percentage

        return Money(pnl, self.base_currency)

    cpdef Money calculate_commission(
        self,
        Quantity quantity,
        Decimal avg_price,
        LiquiditySide liquidity_side,
    ):
        """
        Calculate the commission generated from a transaction with the given
        parameters.

        Parameters
        ----------
        quantity : Quantity
            The quantity for the transaction.
        avg_price : Price
            The average price transaction (only applicable for inverse
            instruments, else ignored).
        liquidity_side : LiquiditySide
            The liquidity side for the transaction.

        Returns
        -------
        Money
            In the base currency for the instrument.

        Raises
        ------
        ValueError
            If liquidity_side is NONE.

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(avg_price, "avg_price")

        cdef Decimal notional = self._calculate_notional(quantity, avg_price)

        if liquidity_side == LiquiditySide.MAKER:
            commission = notional * self.maker_fee
        elif liquidity_side == LiquiditySide.TAKER:
            commission = notional * self.taker_fee
        else:
            raise ValueError(f"Cannot calculate commission "
                             f"(liquidity side was {liquidity_side_to_string(liquidity_side)}).")

        commission += commission * self.settlement_fee

        return Money(commission, self.base_currency)

    cdef inline Decimal _calculate_notional(self, Quantity quantity, Decimal close_price):
        cdef Decimal notional = quantity * self.multiplier

        if self.is_inverse:
            notional *= (1 / close_price)

        return notional

    cdef inline Price _get_close_price(self, PositionSide side, QuoteTick last):
        if side == PositionSide.LONG:
            return last.bid
        elif side == PositionSide.SHORT:
            return last.ask
        else:
            raise ValueError(f"Cannot calculate open value "
                             f"(position side was {position_side_to_string(side)}).")
