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

import pandas as pd

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport UNIX_EPOCH
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.currency cimport BTC
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.currency cimport ETH
from nautilus_trader.model.currency cimport USD
from nautilus_trader.model.currency cimport USDT
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CSVTickDataLoader:
    """
    Provides a means of loading tick data pandas DataFrames from CSV files.
    """

    @staticmethod
    def load(str file_path) -> pd.DataFrame:
        """
        Return the tick pandas.DataFrame loaded from the given csv file.

        Parameters
        ----------
        file_path : str
            The absolute path to the CSV file.

        Returns
        -------
        pd.DataFrame

        """
        Condition.not_none(file_path, "file_path")

        return pd.read_csv(
            file_path,
            usecols=[1, 2, 3],
            index_col=0,
            header=None,
            parse_dates=True,
        )


cdef class CSVBarDataLoader:
    """
    Provides a means of loading bar data pandas DataFrames from CSV files.
    """

    @staticmethod
    def load(str file_path) -> pd.DataFrame:
        """
        Return the bar pandas.DataFrame loaded from the given csv file.

        Parameters
        ----------
        file_path : str
            The absolute path to the CSV file.

        Returns
        -------
        pd.DataFrame

        """
        Condition.not_none(file_path, "file_path")

        return pd.read_csv(
            file_path,
            index_col="Time (UTC)",
            parse_dates=True,
        )


cdef class InstrumentLoader:
    """
    Provides instrument template methods for backtesting.
    """

    @staticmethod
    def btcusdt_binance() -> Instrument:
        """
        Return the Binance BTC/USDT instrument for backtesting.
        """
        return Instrument(
            symbol=Symbol("BTC/USDT", Venue("BINANCE")),
            asset_class=AssetClass.CRYPTO,
            asset_type=AssetType.SPOT,
            base_currency=BTC,
            quote_currency=USDT,
            settlement_currency=USDT,
            is_inverse=False,
            price_precision=2,
            size_precision=6,
            tick_size=Decimal("0.01"),
            multiplier=Decimal("1"),
            leverage=Decimal("1"),
            lot_size=Quantity("1"),
            max_quantity=Quantity("9000.0"),
            min_quantity=Quantity("1e-06"),
            max_notional=None,
            min_notional=Money(10.00, USDT),
            max_price=Price("1000000.0"),
            min_price=Price("0.01"),
            margin_initial=Decimal(),
            margin_maintenance=Decimal(),
            maker_fee=Decimal("0.001"),
            taker_fee=Decimal("0.001"),
            funding_rate_long=Decimal(),
            funding_rate_short=Decimal(),
            timestamp=UNIX_EPOCH,
        )

    @staticmethod
    def ethusdt_binance() -> Instrument:
        """
        Return the Binance ETH/USDT instrument for backtesting.
        """
        return Instrument(
            symbol=Symbol("ETH/USDT", Venue("BINANCE")),
            asset_class=AssetClass.CRYPTO,
            asset_type=AssetType.SPOT,
            base_currency=ETH,
            quote_currency=USDT,
            settlement_currency=USDT,
            is_inverse=False,
            price_precision=2,
            size_precision=5,
            tick_size=Decimal("0.01"),
            multiplier=Decimal("1"),
            leverage=Decimal("1"),
            lot_size=Quantity("1"),
            max_quantity=Quantity("9000"),
            min_quantity=Quantity("1e-05"),
            max_notional=None,
            min_notional=Money(10.00, USDT),
            max_price=Price("1000000.0"),
            min_price=Price("0.01"),
            margin_initial=Decimal("1.00"),
            margin_maintenance=Decimal("0.35"),
            maker_fee=Decimal("0.001"),
            taker_fee=Decimal("0.001"),
            funding_rate_long=Decimal("0"),
            funding_rate_short=Decimal("0"),
            timestamp=UNIX_EPOCH,
        )

    @staticmethod
    def xbtusd_bitmex(leverage: Decimal=Decimal("1.0")) -> Instrument:
        """
        Return the BitMEX XBT/USD perpetual contract for backtesting.
        """
        return Instrument(
            symbol=Symbol("XBT/USD", Venue("BITMEX")),
            asset_class=AssetClass.CRYPTO,
            asset_type=AssetType.SWAP,
            base_currency=BTC,
            quote_currency=USD,
            settlement_currency=BTC,
            is_inverse=True,
            price_precision=1,
            size_precision=0,
            tick_size=Decimal("0.5"),
            multiplier=Decimal("1"),
            leverage=leverage,
            lot_size=Quantity(1),
            max_quantity=None,
            min_quantity=None,
            max_notional=Money("10000000.0", USD),
            min_notional=Money("1.0", USD),
            max_price=Price("1000000.0"),
            min_price=Price("0.5"),
            margin_initial=Decimal("0.01"),
            margin_maintenance=Decimal("0.0035"),
            maker_fee=Decimal("-0.00025"),
            taker_fee=Decimal("0.00075"),
            funding_rate_long=Decimal(),
            funding_rate_short=Decimal("0.003321"),
            timestamp=UNIX_EPOCH,
        )

    @staticmethod
    def ethusd_bitmex(leverage: Decimal=Decimal("1.0")) -> Instrument:
        """
        Return the BitMEX ETH/USD perpetual contract for backtesting.
        """
        return Instrument(
            symbol=Symbol("ETH/USD", Venue("BITMEX")),
            asset_class=AssetClass.CRYPTO,
            asset_type=AssetType.SWAP,
            base_currency=ETH,
            quote_currency=USD,
            settlement_currency=BTC,
            is_inverse=True,
            price_precision=2,
            size_precision=0,
            tick_size=Decimal("0.05"),
            multiplier=Decimal("1"),
            leverage=leverage,
            lot_size=Quantity(1),
            max_quantity=Quantity("10000000.0"),
            min_quantity=Quantity("1.0"),
            max_notional=None,
            min_notional=None,
            max_price=Price("1000000.00"),
            min_price=Price("0.05"),
            margin_initial=Decimal("0.02"),
            margin_maintenance=Decimal("0.007"),
            maker_fee=Decimal("-0.00025"),
            taker_fee=Decimal("0.00075"),
            funding_rate_long=Decimal(),
            funding_rate_short=Decimal("0.000897"),
            timestamp=UNIX_EPOCH,
        )

    @staticmethod
    def ethxbt_bitmex(leverage: Decimal=Decimal("1.0")) -> Instrument:
        """
        Return the BitMEX ETH/XBT perpetual contract for backtesting.
        """
        return Instrument(
            symbol=Symbol("ETH/XBT", Venue("BITMEX")),
            asset_class=AssetClass.CRYPTO,
            asset_type=AssetType.SWAP,
            base_currency=ETH,
            quote_currency=BTC,
            settlement_currency=BTC,
            is_inverse=True,
            price_precision=5,
            size_precision=3,
            tick_size=Decimal("0.00001"),
            multiplier=Decimal("0.00001"),
            leverage=leverage,
            lot_size=Quantity("1"),
            max_quantity=Quantity(""),
            min_quantity=Quantity(1),
            max_notional=None,
            min_notional=Money(1.00, USD),
            max_price=Price("10.00"),
            min_price=Price("0.05"),
            margin_initial=Decimal("1.00"),
            margin_maintenance=Decimal("0.35"),
            maker_fee=Decimal("-0.00025"),
            taker_fee=Decimal("0.00075"),
            funding_rate_long=Decimal(),
            funding_rate_short=Decimal(),
            timestamp=UNIX_EPOCH,
        )

    @staticmethod
    def default_fx_ccy(Symbol symbol) -> Instrument:
        """
        Return a default FX currency pair instrument from the given arguments.

        Parameters
        ----------
        symbol : Symbol
            The currency pair symbol.

        Raises
        ------
        ValueError
            If the symbol.code length is not in range [6, 7].

        """
        Condition.not_none(symbol, "symbol")
        Condition.in_range_int(len(symbol.code), 6, 7, "len(symbol)")

        cdef str base_currency = symbol.code[:3]
        cdef str quote_currency = symbol.code[-3:]

        # Check tick precision of quote currency
        if quote_currency == 'JPY':
            price_precision = 3
        else:
            price_precision = 5

        return Instrument(
            symbol=symbol,
            asset_class=AssetClass.FX,
            asset_type=AssetType.SPOT,
            base_currency=Currency.from_string_c(base_currency),
            quote_currency=Currency.from_string_c(quote_currency),
            settlement_currency=Currency.from_string_c(quote_currency),
            is_inverse=False,
            price_precision=price_precision,
            size_precision=0,
            tick_size=Decimal(1 / (10 ** price_precision), price_precision),
            multiplier=Decimal("1"),
            leverage=Decimal("100"),
            lot_size=Quantity("1000"),
            max_quantity=Quantity("1e7"),
            min_quantity=Quantity("1000"),
            max_price=None,
            min_price=None,
            max_notional=Money(50000000.00, USD),
            min_notional=Money(1000.00, USD),
            margin_initial=Decimal("0.5"),
            margin_maintenance=Decimal("0.1"),
            maker_fee=Decimal("0.00002"),
            taker_fee=Decimal("0.00002"),
            funding_rate_long=Decimal("0.0000"),
            funding_rate_short=Decimal("0.0000"),
            timestamp=UNIX_EPOCH,
        )
