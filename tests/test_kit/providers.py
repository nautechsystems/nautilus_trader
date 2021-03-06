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

from decimal import Decimal

from pandas import DataFrame

from nautilus_trader.backtest.loaders import CSVBarDataLoader
from nautilus_trader.backtest.loaders import CSVTickDataLoader
from nautilus_trader.backtest.loaders import ParquetTickDataLoader
from nautilus_trader.backtest.loaders import TardisQuoteDataLoader
from nautilus_trader.backtest.loaders import TardisTradeDataLoader
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.c_enums.asset_class import AssetClass
from nautilus_trader.model.c_enums.asset_type import AssetType
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instrument import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.stubs import UNIX_EPOCH


class TestDataProvider:

    @staticmethod
    def ethusdt_trades() -> DataFrame:
        return CSVTickDataLoader.load(PACKAGE_ROOT + "/data/binance-ethusdt-trades.csv")

    @staticmethod
    def audusd_ticks() -> DataFrame:
        return CSVTickDataLoader.load(PACKAGE_ROOT + "/data/truefx-audusd-ticks.csv")

    @staticmethod
    def usdjpy_ticks() -> DataFrame:
        return CSVTickDataLoader.load(PACKAGE_ROOT + "/data/truefx-usdjpy-ticks.csv")

    @staticmethod
    def gbpusd_1min_bid() -> DataFrame:
        return CSVBarDataLoader.load(PACKAGE_ROOT + "/data/fxcm-gbpusd-m1-bid-2012.csv")

    @staticmethod
    def gbpusd_1min_ask() -> DataFrame:
        return CSVBarDataLoader.load(PACKAGE_ROOT + "/data/fxcm-gbpusd-m1-ask-2012.csv")

    @staticmethod
    def usdjpy_1min_bid() -> DataFrame:
        return CSVBarDataLoader.load(PACKAGE_ROOT + "/data/fxcm-usdjpy-m1-bid-2013.csv")

    @staticmethod
    def usdjpy_1min_ask() -> DataFrame:
        return CSVBarDataLoader.load(PACKAGE_ROOT + "/data/fxcm-usdjpy-m1-ask-2013.csv")

    @staticmethod
    def tardis_trades() -> DataFrame:
        return TardisTradeDataLoader.load(PACKAGE_ROOT + "/data/tardis_trades.csv")

    @staticmethod
    def tardis_quotes() -> DataFrame:
        return TardisQuoteDataLoader.load(PACKAGE_ROOT + "/data/tardis_quotes.csv")

    @staticmethod
    def parquet_btcusdt_trades() -> DataFrame:
        return ParquetTickDataLoader.load(PACKAGE_ROOT + "/data/binance-btcusdt-trades.parquet")

    @staticmethod
    def parquet_btcusdt_quotes() -> DataFrame:
        return ParquetTickDataLoader.load(PACKAGE_ROOT + "/data/binance-btcusdt-quotes.parquet")


class TestInstrumentProvider:
    """
    Provides instrument template methods for backtesting.
    """

    @staticmethod
    def btcusdt_binance() -> Instrument:
        """
        Return the Binance BTC/USDT instrument for backtesting.

        Returns
        -------
        Instrument

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
            min_notional=Money("10.00000000", USDT),
            max_price=Price("1000000.0"),
            min_price=Price("0.01"),
            margin_init=Decimal(),
            margin_maint=Decimal(),
            maker_fee=Decimal("0.001"),
            taker_fee=Decimal("0.001"),
            financing={},
            timestamp=UNIX_EPOCH,
        )

    @staticmethod
    def ethusdt_binance() -> Instrument:
        """
        Return the Binance ETH/USDT instrument for backtesting.

        Returns
        -------
        Instrument

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
            min_notional=Money("10.00000000", USDT),
            max_price=Price("1000000.0"),
            min_price=Price("0.01"),
            margin_init=Decimal("1.00"),
            margin_maint=Decimal("0.35"),
            maker_fee=Decimal("0.0001"),
            taker_fee=Decimal("0.0001"),
            financing={},
            timestamp=UNIX_EPOCH,
        )

    @staticmethod
    def xbtusd_bitmex(leverage: Decimal=Decimal("1.0")) -> Instrument:
        """
        Return the BitMEX XBT/USD perpetual contract for backtesting.

        Parameters
        ----------
        leverage : Decimal
            The margined leverage for the instrument.

        Returns
        -------
        Instrument

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
            margin_init=Decimal("0.01"),
            margin_maint=Decimal("0.0035"),
            maker_fee=Decimal("-0.00025"),
            taker_fee=Decimal("0.00075"),
            financing={},
            timestamp=UNIX_EPOCH,
        )

    @staticmethod
    def ethusd_bitmex(leverage: Decimal=Decimal("1.0")) -> Instrument:
        """
        Return the BitMEX ETH/USD perpetual contract for backtesting.

        Parameters
        ----------
        leverage : Decimal
            The margined leverage for the instrument.

        Returns
        -------
        Instrument

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
            margin_init=Decimal("0.02"),
            margin_maint=Decimal("0.007"),
            maker_fee=Decimal("-0.00025"),
            taker_fee=Decimal("0.00075"),
            financing={},
            timestamp=UNIX_EPOCH,
        )

    @staticmethod
    def default_fx_ccy(symbol: Symbol, leverage: Decimal=Decimal("50")) -> Instrument:
        """
        Return a default FX currency pair instrument from the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The currency pair symbol.
        leverage : Decimal
            The leverage for the instrument.

        Returns
        -------
        Instrument

        Raises
        ------
        ValueError
            If the symbol.code length is not in range [6, 7].

        """
        PyCondition.not_none(symbol, "symbol")
        PyCondition.in_range_int(len(symbol.code), 6, 7, "len(symbol)")

        base_currency = symbol.code[:3]
        quote_currency = symbol.code[-3:]

        # Check tick precision of quote currency
        if quote_currency == 'JPY':
            price_precision = 3
        else:
            price_precision = 5

        return Instrument(
            symbol=symbol,
            asset_class=AssetClass.FX,
            asset_type=AssetType.SPOT,
            base_currency=Currency.from_str(base_currency),
            quote_currency=Currency.from_str(quote_currency),
            settlement_currency=Currency.from_str(quote_currency),
            is_inverse=False,
            price_precision=price_precision,
            size_precision=0,
            tick_size=Decimal(f"{1 / 10 ** price_precision:.{price_precision}f}"),
            multiplier=Decimal("1"),
            leverage=leverage,
            lot_size=Quantity("1000"),
            max_quantity=Quantity("1e7"),
            min_quantity=Quantity("1000"),
            max_price=None,
            min_price=None,
            max_notional=Money(50000000.00, USD),
            min_notional=Money(1000.00, USD),
            margin_init=Decimal("0.03"),
            margin_maint=Decimal("0.03"),
            maker_fee=Decimal("0.00002"),
            taker_fee=Decimal("0.00002"),
            financing={},
            timestamp=UNIX_EPOCH,
        )
