# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import datetime as dt
import pathlib
import random
from decimal import Decimal
from pathlib import Path
from typing import Any

import fsspec
import numpy as np
import pandas as pd
import pytz
from fsspec.implementations.local import LocalFileSystem

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import ADA
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.currencies import XRP
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.instruments import Cfd
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.instruments import SyntheticInstrument
from nautilus_trader.model.instruments.betting import null_handicap
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.loaders import CSVBarDataLoader
from nautilus_trader.persistence.loaders import CSVTickDataLoader
from nautilus_trader.persistence.loaders import ParquetBarDataLoader
from nautilus_trader.persistence.loaders import ParquetTickDataLoader


# Constants
NANOSECONDS_IN_SECOND = 1_000_000_000  # 1 billion nanoseconds in a second


class TestInstrumentProvider:
    """
    Provides instrument template methods for backtesting.
    """

    __test__ = False  # Prevents pytest from collecting this as a test class

    @staticmethod
    def adabtc_binance() -> CurrencyPair:
        """
        Return the Binance Spot ADA/BTC instrument for backtesting.

        Returns
        -------
        CurrencyPair

        """
        return CurrencyPair(
            instrument_id=InstrumentId(
                symbol=Symbol("ADABTC"),
                venue=Venue("BINANCE"),
            ),
            raw_symbol=Symbol("ADABTC"),
            base_currency=ADA,
            quote_currency=BTC,
            price_precision=8,
            size_precision=8,
            price_increment=Price(1e-08, precision=8),
            size_increment=Quantity(1e-08, precision=8),
            lot_size=None,
            max_quantity=Quantity.from_int(90_000_000),
            min_quantity=Quantity.from_int(1),
            max_notional=None,
            min_notional=Money(0.00010000, BTC),
            max_price=Price(1000, precision=8),
            min_price=Price(1e-8, precision=8),
            margin_init=Decimal("0"),
            margin_maint=Decimal("0"),
            maker_fee=Decimal("0.0010"),
            taker_fee=Decimal("0.0010"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def adausdt_binance() -> CurrencyPair:
        """
        Return the Binance Spot ADA/USDT instrument for backtesting.

        Returns
        -------
        CurrencyPair

        """
        return CurrencyPair(
            instrument_id=InstrumentId(
                symbol=Symbol("ADAUSDT"),
                venue=Venue("BINANCE"),
            ),
            raw_symbol=Symbol("ADAUSDT"),
            base_currency=ADA,
            quote_currency=USDT,
            price_precision=4,
            size_precision=1,
            price_increment=Price(0.0001, precision=4),
            size_increment=Quantity(0.1, precision=1),
            lot_size=Quantity(0.1, precision=1),
            max_quantity=Quantity(900_000, precision=1),
            min_quantity=Quantity(0.1, precision=1),
            max_notional=None,
            min_notional=Money(0.00010000, BTC),
            max_price=Price(1000, precision=4),
            min_price=Price(1e-8, precision=4),
            margin_init=Decimal("0"),
            margin_maint=Decimal("0"),
            maker_fee=Decimal("0.0010"),
            taker_fee=Decimal("0.0010"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def btcusdt_binance() -> CurrencyPair:
        """
        Return the Binance Spot BTCUSDT instrument for backtesting.

        Returns
        -------
        CurrencyPair

        """
        return CurrencyPair(
            instrument_id=InstrumentId(
                symbol=Symbol("BTCUSDT"),
                venue=Venue("BINANCE"),
            ),
            raw_symbol=Symbol("BTCUSDT"),
            base_currency=BTC,
            quote_currency=USDT,
            price_precision=2,
            size_precision=6,
            price_increment=Price(1e-02, precision=2),
            size_increment=Quantity(1e-06, precision=6),
            lot_size=None,
            max_quantity=Quantity(9000, precision=6),
            min_quantity=Quantity(1e-06, precision=6),
            max_notional=None,
            min_notional=Money(10.00000000, USDT),
            max_price=Price(1000000, precision=2),
            min_price=Price(0.01, precision=2),
            margin_init=Decimal(0),
            margin_maint=Decimal(0),
            maker_fee=Decimal("0.001"),
            taker_fee=Decimal("0.001"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def btcusdt_perp_binance() -> CryptoPerpetual:
        """
        Return the Binance Futures BTCUSDT instrument for backtesting.

        Returns
        -------
        CryptoPerpetual

        """
        return CryptoPerpetual(
            instrument_id=InstrumentId(
                symbol=Symbol("BTCUSDT-PERP"),
                venue=Venue("BINANCE"),
            ),
            raw_symbol=Symbol("BTCUSDT"),
            base_currency=BTC,
            quote_currency=USDT,
            settlement_currency=USDT,
            is_inverse=False,
            price_precision=1,
            price_increment=Price.from_str("0.1"),
            size_precision=3,
            size_increment=Quantity.from_str("0.001"),
            max_quantity=Quantity.from_str("1000.000"),
            min_quantity=Quantity.from_str("0.001"),
            max_notional=None,
            min_notional=Money(10.00, USDT),
            max_price=Price.from_str("809484.0"),
            min_price=Price.from_str("261.1"),
            margin_init=Decimal("0.0500"),
            margin_maint=Decimal("0.0250"),
            maker_fee=Decimal("0.000200"),
            taker_fee=Decimal("0.000180"),
            ts_event=1646199312128000000,
            ts_init=1646199342953849862,
        )

    @staticmethod
    def ethusdt_binance() -> CurrencyPair:
        """
        Return the Binance Spot ETHUSDT instrument for backtesting.

        Returns
        -------
        CurrencyPair

        """
        return CurrencyPair(
            instrument_id=InstrumentId(
                symbol=Symbol("ETHUSDT"),
                venue=Venue("BINANCE"),
            ),
            raw_symbol=Symbol("ETHUSDT"),
            base_currency=ETH,
            quote_currency=USDT,
            price_precision=2,
            size_precision=5,
            price_increment=Price(1e-02, precision=2),
            size_increment=Quantity(1e-05, precision=5),
            lot_size=None,
            max_quantity=Quantity(9000, precision=5),
            min_quantity=Quantity(1e-05, precision=5),
            max_notional=None,
            min_notional=Money(10.00, USDT),
            max_price=Price(1000000, precision=2),
            min_price=Price(0.01, precision=2),
            margin_init=Decimal("1.00"),
            margin_maint=Decimal("0.35"),
            maker_fee=Decimal("0.0001"),
            taker_fee=Decimal("0.0001"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def ethusdt_perp_binance() -> CryptoPerpetual:
        """
        Return the Binance Futures ETHUSDT-PERP instrument for backtesting.

        Returns
        -------
        CryptoPerpetual

        """
        return CryptoPerpetual(
            instrument_id=InstrumentId(
                symbol=Symbol("ETHUSDT-PERP"),
                venue=Venue("BINANCE"),
            ),
            raw_symbol=Symbol("ETHUSDT"),
            base_currency=ETH,
            quote_currency=USDT,
            settlement_currency=USDT,
            is_inverse=False,
            price_precision=2,
            size_precision=3,
            price_increment=Price.from_str("0.01"),
            size_increment=Quantity.from_str("0.001"),
            max_quantity=Quantity.from_str("10000.000"),
            min_quantity=Quantity.from_str("0.001"),
            max_notional=None,
            min_notional=Money(10.00, USDT),
            max_price=Price.from_str("152588.43"),
            min_price=Price.from_str("29.91"),
            margin_init=Decimal("1.00"),
            margin_maint=Decimal("0.35"),
            maker_fee=Decimal("0.0002"),
            taker_fee=Decimal("0.0004"),
            ts_event=1646199312128000000,
            ts_init=1646199342953849862,
        )

    @staticmethod
    def btcusdt_future_binance(
        activation: pd.Timestamp | None = None,
        expiration: pd.Timestamp | None = None,
    ) -> CryptoFuture:
        """
        Return the Binance Futures BTCUSDT instrument for backtesting.

        Parameters
        ----------
        activation : pd.Timestamp, optional
            The activation (UTC) for the contract.
        expiration : pd.Timestamp, optional
            The expiration (UTC) for the contract.

        Returns
        -------
        CryptoFuture

        """
        if activation is None:
            activation = pd.Timestamp("2021-12-25", tz=pytz.utc)
        if expiration is None:
            expiration = pd.Timestamp("2022-3-25", tz=pytz.utc)
        return CryptoFuture(
            instrument_id=InstrumentId(
                symbol=Symbol(f"BTCUSDT_{expiration.strftime('%y%m%d')}"),
                venue=Venue("BINANCE"),
            ),
            raw_symbol=Symbol("BTCUSDT"),
            underlying=BTC,
            quote_currency=USDT,
            settlement_currency=USDT,
            is_inverse=False,
            activation_ns=activation.value,
            expiration_ns=expiration.value,
            price_precision=2,
            size_precision=6,
            price_increment=Price(1e-02, precision=2),
            size_increment=Quantity(1e-06, precision=6),
            max_quantity=Quantity(9000, precision=6),
            min_quantity=Quantity(1e-06, precision=6),
            max_notional=None,
            min_notional=Money(10.00000000, USDT),
            max_price=Price(1000000, precision=2),
            min_price=Price(0.01, precision=2),
            margin_init=Decimal(0),
            margin_maint=Decimal(0),
            maker_fee=Decimal("0.001"),
            taker_fee=Decimal("0.001"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def xrpusdt_linear_bybit() -> CryptoPerpetual:
        """
        Return the ByBit Linear XRPUSDT instrument for backtesting.

        Returns
        -------
        CryptoPerpetual

        """
        return CryptoPerpetual(
            instrument_id=InstrumentId(
                symbol=Symbol("XRPUSDT-LINEAR"),
                venue=Venue("BYBIT"),
            ),
            raw_symbol=Symbol("XRPUSDT"),
            base_currency=XRP,
            quote_currency=USDT,
            settlement_currency=USDT,
            is_inverse=False,
            price_precision=4,
            price_increment=Price.from_str("0.0001"),
            size_precision=0,
            size_increment=Quantity.from_str("1"),
            max_quantity=Quantity.from_str("10965300"),
            min_quantity=Quantity.from_str("1"),
            max_notional=None,
            min_notional=Money(1.0, USDT),
            max_price=Price.from_str("199.9998"),
            min_price=Price.from_str("0.0001"),
            margin_init=Decimal("0.1"),
            margin_maint=Decimal("0.1"),
            maker_fee=Decimal("0.0002"),
            taker_fee=Decimal("0.00055"),
            ts_event=1646199312128000000,
            ts_init=1646199342953849862,
        )

    @staticmethod
    def xbtusd_bitmex() -> CryptoPerpetual:
        """
        Return the BitMEX XBT/USD perpetual contract for backtesting.

        Returns
        -------
        CryptoPerpetual

        """
        return CryptoPerpetual(
            instrument_id=InstrumentId(
                symbol=Symbol("BTC/USD"),
                venue=Venue("BITMEX"),
            ),
            raw_symbol=Symbol("XBTUSD"),
            base_currency=BTC,
            quote_currency=USD,
            settlement_currency=BTC,
            is_inverse=True,
            price_precision=1,
            size_precision=0,
            price_increment=Price.from_str("0.5"),
            size_increment=Quantity.from_int(1),
            max_quantity=None,
            min_quantity=None,
            max_notional=Money(10_000_000.00, USD),
            min_notional=Money(1.00, USD),
            max_price=Price.from_str("1000000.0"),
            min_price=Price(0.5, precision=1),
            margin_init=Decimal("0.01"),
            margin_maint=Decimal("0.0035"),
            maker_fee=Decimal("-0.00025"),
            taker_fee=Decimal("0.00075"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def ethusd_bitmex() -> CryptoPerpetual:
        """
        Return the BitMEX ETH/USD perpetual swap contract for backtesting.

        Returns
        -------
        CryptoPerpetual

        """
        return CryptoPerpetual(
            instrument_id=InstrumentId(
                symbol=Symbol("ETH/USD"),
                venue=Venue("BITMEX"),
            ),
            raw_symbol=Symbol("ETHUSD"),
            base_currency=ETH,
            quote_currency=USD,
            settlement_currency=BTC,
            is_inverse=True,
            price_precision=2,
            size_precision=0,
            price_increment=Price.from_str("0.05"),
            size_increment=Quantity.from_int(1),
            max_quantity=Quantity.from_int(10_000_000),
            min_quantity=Quantity.from_int(1),
            max_notional=None,
            min_notional=None,
            max_price=Price.from_str("1000000.00"),
            min_price=Price.from_str("0.05"),
            margin_init=Decimal("0.02"),
            margin_maint=Decimal("0.007"),
            maker_fee=Decimal("-0.00025"),
            taker_fee=Decimal("0.00075"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def onethousandrats_perp_binance() -> CryptoPerpetual:
        """
        Return the Binance 1000RATSUSDT perpetual contract for backtesting.

        Returns
        -------
        CryptoPerpetual

        """
        return CryptoPerpetual(
            instrument_id=InstrumentId(
                symbol=Symbol("1000RATSUSDT-PERP"),
                venue=Venue("BINANCE"),
            ),
            raw_symbol=Symbol("1000RATSUSDT"),
            base_currency=Currency.from_str("1000RATS"),
            quote_currency=USDT,
            settlement_currency=USDT,
            is_inverse=False,
            price_precision=7,
            size_precision=0,
            price_increment=Price.from_str("0.0000100"),
            size_increment=Quantity.from_int(1),
            max_quantity=Quantity.from_int(1_000_000),
            min_quantity=Quantity.from_int(1),
            max_notional=None,
            min_notional=None,
            max_price=Price.from_str("1000000.00"),
            min_price=Price.from_str("0.0"),
            margin_init=Decimal("0.0500"),
            margin_maint=Decimal("0.0250"),
            maker_fee=Decimal("0.000200"),
            taker_fee=Decimal("0.000500"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def default_fx_ccy(symbol: str, venue: Venue | None = None) -> CurrencyPair:
        """
        Return a default FX currency pair instrument from the given symbol and venue.

        Parameters
        ----------
        symbol : str
            The currency pair symbol.
        venue : Venue
            The currency pair venue.

        Returns
        -------
        CurrencyPair

        Raises
        ------
        ValueError
            If `symbol` length is not in range [6, 7].

        """
        if venue is None:
            venue = Venue("SIM")
        PyCondition.valid_string(symbol, "symbol")
        PyCondition.in_range_int(len(symbol), 6, 7, "len(symbol)")

        instrument_id = InstrumentId(
            symbol=Symbol(symbol),
            venue=venue,
        )

        base_currency = symbol[:3]
        quote_currency = symbol[-3:]

        # Check tick precision of quote currency
        if quote_currency == "JPY":
            price_precision = 3
            tick_scheme_name = "FOREX_3DECIMAL"
        else:
            price_precision = 5
            tick_scheme_name = "FOREX_5DECIMAL"

        return CurrencyPair(
            instrument_id=instrument_id,
            raw_symbol=Symbol(symbol),
            base_currency=Currency.from_str(base_currency),
            quote_currency=Currency.from_str(quote_currency),
            price_precision=price_precision,
            size_precision=0,
            price_increment=Price(1 / 10**price_precision, price_precision),
            size_increment=Quantity.from_int(1),
            lot_size=Quantity.from_str("1000"),
            max_quantity=Quantity.from_str("1e7"),
            min_quantity=Quantity.from_str("1000"),
            max_price=None,
            min_price=None,
            max_notional=Money(50_000_000.00, USD),
            min_notional=Money(1_000.00, USD),
            margin_init=Decimal("0.03"),
            margin_maint=Decimal("0.03"),
            maker_fee=Decimal("0.00002"),
            taker_fee=Decimal("0.00002"),
            tick_scheme_name=tick_scheme_name,
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def audusd_cfd() -> Cfd:
        return Cfd(
            instrument_id=InstrumentId.from_str("AUDUSD.OANDA"),
            raw_symbol=Symbol("AUD/USD"),
            asset_class=AssetClass.FX,
            base_currency=AUD,
            quote_currency=USD,
            price_precision=5,
            price_increment=Price.from_str("0.00001"),
            size_precision=0,
            size_increment=Quantity.from_int(1),
            lot_size=Quantity.from_int(1000),
            margin_init=Decimal("0.03"),
            margin_maint=Decimal("0.03"),
            maker_fee=Decimal("0.00002"),
            taker_fee=Decimal("0.00002"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def equity(symbol: str = "AAPL", venue: str = "XNAS") -> Equity:
        return Equity(
            instrument_id=InstrumentId(symbol=Symbol(symbol), venue=Venue(venue)),
            raw_symbol=Symbol(symbol),
            currency=USD,
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            lot_size=Quantity.from_int(100),
            isin="US0378331005",
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def es_future(
        expiry_year: int,
        expiry_month: int,
    ) -> FuturesContract:
        activation_date = first_friday_two_years_six_months_ago(expiry_year, expiry_month)
        expiration_date = third_friday_of_month(expiry_year, expiry_month)

        activation_time = pd.Timedelta(hours=21, minutes=30)
        expiration_time = pd.Timedelta(hours=14, minutes=30)
        activation_utc = pd.Timestamp(activation_date, tz=pytz.utc) + activation_time
        expiration_utc = pd.Timestamp(expiration_date, tz=pytz.utc) + expiration_time

        raw_symbol = f"ES{get_contract_month_code(expiry_month)}{expiry_year % 10}"

        return FuturesContract(
            instrument_id=InstrumentId(symbol=Symbol(raw_symbol), venue=Venue("GLBX")),
            raw_symbol=Symbol(raw_symbol),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            currency=USD,
            price_precision=2,
            price_increment=Price.from_str("0.25"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying="ES",
            activation_ns=activation_utc.value,
            expiration_ns=expiration_utc.value,
            ts_event=activation_utc.value,
            ts_init=activation_utc.value,
        )

    @staticmethod
    def eurusd_future(
        expiry_year: int,
        expiry_month: int,
        venue_name: str = "XCME",
    ) -> FuturesContract:
        activation_date = first_friday_two_years_six_months_ago(expiry_year, expiry_month)
        expiration_date = third_friday_of_month(expiry_year, expiry_month)

        activation_time = pd.Timedelta(hours=21, minutes=30)
        expiration_time = pd.Timedelta(hours=14, minutes=30)
        activation_utc = pd.Timestamp(activation_date, tz=pytz.utc) + activation_time
        expiration_utc = pd.Timestamp(expiration_date, tz=pytz.utc) + expiration_time

        base_symbol = "6E"
        raw_symbol = f"{base_symbol}{get_contract_month_code(expiry_month)}{expiry_year % 10}"

        return FuturesContract(
            instrument_id=InstrumentId(symbol=Symbol(raw_symbol), venue=Venue(venue_name)),
            raw_symbol=Symbol(raw_symbol),
            asset_class=AssetClass.FX,
            exchange=venue_name,
            currency=USD,
            price_precision=5,
            price_increment=Price.from_str("0.00005"),
            multiplier=Quantity.from_int(125000),
            lot_size=Quantity.from_int(1),
            underlying=base_symbol,
            activation_ns=activation_utc.value,
            expiration_ns=expiration_utc.value,
            ts_event=activation_utc.value,
            ts_init=activation_utc.value,
        )

    @staticmethod
    def future(
        symbol: str = "ESZ1",
        underlying: str = "ES",
        venue: str = "GLBX",
        exchange: str = "XCME",
    ) -> FuturesContract:
        return FuturesContract(
            instrument_id=InstrumentId(symbol=Symbol(symbol), venue=Venue(venue)),
            raw_symbol=Symbol(symbol),
            asset_class=AssetClass.INDEX,
            exchange=exchange,
            currency=USD,
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            underlying=underlying,
            activation_ns=1616160600000000000,
            expiration_ns=1639751400000000000,
            ts_event=1638133151389539971,
            ts_init=1638316800000000000,
        )

    @staticmethod
    def aapl_option() -> OptionContract:
        return OptionContract(
            instrument_id=InstrumentId(symbol=Symbol("AAPL211217C00150000"), venue=Venue("OPRA")),
            raw_symbol=Symbol("AAPL211217C00150000"),
            asset_class=AssetClass.EQUITY,
            exchange="GMNI",
            currency=USD,
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            underlying="AAPL",
            option_kind=OptionKind.CALL,
            strike_price=Price.from_str("149.00"),
            activation_ns=pd.Timestamp("2021-9-17", tz=pytz.utc).value,
            expiration_ns=pd.Timestamp("2021-12-17", tz=pytz.utc).value,
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def synthetic_instrument() -> SyntheticInstrument:
        return SyntheticInstrument(
            symbol=Symbol("BTC-ETH"),
            price_precision=8,
            components=[
                TestInstrumentProvider.btcusdt_binance().id,
                TestInstrumentProvider.ethusdt_binance().id,
            ],
            formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def betting_instrument(venue: str | None = None) -> BettingInstrument:
        return BettingInstrument(
            venue_name=venue or "BETFAIR",
            betting_type="ODDS",
            competition_id=12282733,
            competition_name="NFL",
            event_country_code="GB",
            event_id=29678534,
            event_name="NFL",
            event_open_date=pd.Timestamp("2022-02-07 23:30:00+00:00"),
            event_type_id=6423,
            event_type_name="American Football",
            market_id="1-123456789",
            market_name="AFC Conference Winner",
            market_start_time=pd.Timestamp("2022-02-07 23:30:00+00:00"),
            market_type="SPECIAL",
            selection_handicap=null_handicap(),
            selection_id=50214,
            selection_name="Kansas City Chiefs",
            currency="GBP",
            price_precision=2,  # BETFAIR_PRICE_PRECISION,
            size_precision=2,  # BETFAIR_QUANTITY_PRECISION,
            min_notional=Money(1, GBP),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def binary_option() -> BinaryOption:
        raw_symbol = Symbol(
            "0x12a0cb60174abc437bf1178367c72d11f069e1a3add20b148fb0ab4279b772b2-92544998123698303655208967887569360731013655782348975589292031774495159624905",
        )
        price_increment = Price.from_str("0.001")
        size_increment = Quantity.from_str("0.01")
        return BinaryOption(
            instrument_id=InstrumentId(symbol=raw_symbol, venue=Venue("POLYMARKET")),
            raw_symbol=raw_symbol,
            outcome="Yes",
            description="Will the outcome of this market be 'Yes'?",
            asset_class=AssetClass.ALTERNATIVE,
            currency=USDC,
            price_precision=price_increment.precision,
            price_increment=price_increment,
            size_precision=size_increment.precision,
            size_increment=size_increment,
            activation_ns=0,
            expiration_ns=pd.Timestamp("2024-01-01", tz="UTC").value,
            max_quantity=None,
            min_quantity=Quantity.from_int(5),
            maker_fee=Decimal(0),  # TBD
            taker_fee=Decimal(0),  # TBD
            ts_event=0,
            ts_init=0,
        )


def first_friday_two_years_six_months_ago(year: int, month: int) -> dt.date:
    target_year = year - 2
    target_month = month - 6

    # Adjust the year and month if necessary
    if target_month <= 0:
        target_year -= 1
        target_month += 12

    first_day = dt.date(target_year, target_month, 1)
    first_day_weekday = first_day.weekday()

    days_to_add = (4 - first_day_weekday + 7) % 7
    first_friday = first_day + dt.timedelta(days=days_to_add)

    return first_friday


def third_friday_of_month(year: int, month: int) -> dt.date:
    first_day = dt.date(year, month, 1)
    first_day_weekday = first_day.weekday()

    days_to_add = (4 - first_day_weekday + 7) % 7 + 14
    third_friday = first_day + dt.timedelta(days=days_to_add)

    return third_friday


def get_contract_month_code(expiry_month: int) -> str:  # noqa: C901 (too complex)
    match expiry_month:
        case 1:
            return "F"
        case 2:
            return "G"
        case 3:
            return "H"
        case 4:
            return "J"
        case 5:
            return "K"
        case 6:
            return "M"
        case 7:
            return "N"
        case 8:
            return "Q"
        case 9:
            return "U"
        case 10:
            return "V"
        case 11:
            return "X"
        case 12:
            return "Z"
        case _:
            raise ValueError(f"invalid `expiry_month`, was {expiry_month}. Use [1, 12].")


class TestDataProvider:
    """
    Provides an API to load data from either the 'test/' directory or the projects
    GitHub repo.

    Parameters
    ----------
    branch : str
        The NautilusTrader GitHub branch for the path.

    """

    __test__ = False  # Prevents pytest from collecting this as a test class

    def __init__(self, branch: str = "develop") -> None:
        self.fs: fsspec.AbstractFileSystem | None = None
        self.root: str | None = None
        self._determine_filesystem()
        self.branch = branch

    @staticmethod
    def _test_data_directory() -> str | None:
        # Determine if the test data directory exists (i.e. this is a checkout of the source code).
        source_root = pathlib.Path(__file__).parent.parent
        assert source_root.stem == "nautilus_trader"
        test_data_dir = source_root.parent.joinpath("tests", "test_data")
        if test_data_dir.exists():
            return str(test_data_dir)
        else:
            return None

    def _determine_filesystem(self) -> None:
        test_data_dir = TestDataProvider._test_data_directory()
        if test_data_dir:
            self.root = test_data_dir
            self.fs = fsspec.filesystem("file")
        else:
            print("Couldn't find test data directory, test data will be pulled from GitHub")
            self.root = "tests/test_data"
            self.fs = fsspec.filesystem("github", org="nautechsystems", repo="nautilus_trader")

    def _make_uri(self, path: str) -> str:
        # Moved here from top level import because GithubFileSystem has extra deps we may not have installed.
        from fsspec.implementations.github import GithubFileSystem

        if isinstance(self.fs, LocalFileSystem):
            return f"file://{self.root}/{path}"
        elif isinstance(self.fs, GithubFileSystem):
            return f"github://{self.fs.org}:{self.fs.repo}@{self.branch}/{self.root}/{path}"
        else:
            raise ValueError(f"Unsupported file system {self.fs}")

    def read(self, path: str) -> fsspec.core.OpenFile:
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return f.read()

    def read_csv(self, path: str, **kwargs: Any) -> pd.DataFrame:
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return pd.read_csv(f, **kwargs)

    def read_csv_ticks(self, path: str) -> pd.DataFrame:
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return CSVTickDataLoader.load(file_path=f)

    def read_csv_bars(self, path: str) -> pd.DataFrame:
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return CSVBarDataLoader.load(file_path=f)

    def read_parquet_ticks(self, path: str, timestamp_column: str = "timestamp") -> pd.DataFrame:
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return ParquetTickDataLoader.load(file_path=f, timestamp_column=timestamp_column)

    def read_parquet_bars(self, path: str) -> pd.DataFrame:
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return ParquetBarDataLoader.load(file_path=f)


class TestDataGenerator:

    __test__ = False  # Prevents pytest from collecting this as a test class

    @staticmethod
    def simulate_value_diffs(
        count: int,
        max_diff: float = 10,
        prob_increase: float = 0.25,
        prob_decrease: float = 0.25,
    ) -> pd.Series:
        gen = np.random.default_rng()

        def sim():
            if random.random() <= prob_increase:  # noqa: S311
                return gen.uniform(0, max_diff)
            elif random.random() <= prob_decrease:  # noqa: S311
                return -gen.uniform(0, max_diff)
            else:
                return 0

        return pd.Series([sim() for _ in range(count)])

    @staticmethod
    def generate_time_series_index(
        start_timestamp: str = "2020-01-01",
        max_freq: str = "1s",
        count: int = 100_000,
    ) -> pd.DatetimeIndex:
        gen = np.random.default_rng()
        start = dt_to_unix_nanos(pd.Timestamp(start_timestamp))
        freq_in_nanos = secs_to_nanos(pd.Timedelta(max_freq).total_seconds())
        diffs = gen.uniform(0, freq_in_nanos, size=count - 1)
        srs = pd.Series([start, *diffs.tolist()])
        return pd.to_datetime(srs.cumsum(), unit="us")

    @staticmethod
    def generate_time_series(
        start_timestamp: str = "2020-01-01",
        start_price: float = 100.0,
        default_quantity: int = 10,
        max_freq: str = "1s",
        count: int = 100_000,
    ) -> pd.DataFrame:
        gen = np.random.default_rng()
        price_diffs = gen.uniform(-1, 1, size=count - 1)
        prices = pd.Series([start_price, *price_diffs.tolist()]).cumsum()

        quantity_diffs = TestDataGenerator.simulate_value_diffs(count)
        quantity = pd.Series(default_quantity + quantity_diffs).astype(int)

        index = TestDataGenerator.generate_time_series_index(start_timestamp, max_freq, count)
        return pd.DataFrame(
            index=index,
            data={"price": prices.to_numpy(), "quantity": quantity.to_numpy()},
        )

    @staticmethod
    def generate_quote_ticks(
        instrument_id: str,
        price_prec: int = 4,
        quantity_prec: int = 4,
        **kwargs: Any,
    ) -> list[QuoteTick]:
        df: pd.DataFrame = TestDataGenerator.generate_time_series(**kwargs)
        return [
            QuoteTick(
                InstrumentId.from_str(instrument_id),
                Price(row["price"] + 1, price_prec),
                Price(row["price"] - 1, price_prec),
                Quantity(row["quantity"], quantity_prec),
                Quantity(row["quantity"], quantity_prec),
                dt_to_unix_nanos(idx),
                dt_to_unix_nanos(idx),
            )
            for idx, row in df.iterrows()
        ]

    @staticmethod
    def generate_trade_ticks(
        instrument_id: str,
        price_prec: int = 4,
        quantity_prec: int = 4,
        **kwargs: Any,
    ) -> list[TradeTick]:
        df: pd.DataFrame = TestDataGenerator.generate_time_series(**kwargs)
        return [
            TradeTick(
                InstrumentId.from_str(instrument_id),
                Price(row["price"], price_prec),
                Quantity(row["quantity"], quantity_prec),
                AggressorSide.NO_AGGRESSOR,
                TradeId(UUID4().value),
                dt_to_unix_nanos(idx),
                dt_to_unix_nanos(idx),
            )
            for idx, row in df.iterrows()
        ]

    @staticmethod
    def generate_monotonic_bars(
        instrument: Instrument,
        first_bar: Bar,
        bar_count: int = 20,
        time_change_nanos: int = 60 * NANOSECONDS_IN_SECOND,  # Default to 1 minute
        price_change_ticks: int = 10,
        increasing_series: bool = True,
    ) -> list[Bar]:
        """
        Generate a sequence of bars with monotonic price progression.

        This function creates a series of bars with consistent price progression
        based on the specified parameters. Each subsequent bar's prices change
        by a constant amount of ticks in the direction specified by `increasing_series`.

        Parameters
        ----------
        instrument : Instrument
            The instrument for which to generate bars
        bar_type : BarType
            The bar type specification for the generated bars
        first_bar : Bar
            The first bar in the sequence with initial OHLCV values
        bar_count : int
            The total number of bars to generate (including first_bar)
        time_change_nanos : int
            The time increment between consecutive bars in nanoseconds
        price_change_ticks : int
            The price increment between consecutive bars in ticks
        increasing_series : bool
            If True, generates a price series with increasing prices;
            if False, generates a series with decreasing prices

        Returns
        -------
        list[Bar]
            The list of generated bars with the specified progression

        """
        # Calculate price change
        tick_size = instrument.price_increment.as_double()
        price_change = tick_size * price_change_ticks

        # Increasing or decreasing series
        if not increasing_series:
            price_change = -price_change

        # Collection of all generated artificial bars
        bars = [first_bar]

        # Generate subsequent bars
        for i in range(bar_count - 1):  # -1 because we already have the first bar
            prev_bar = bars[-1]  # Get the last bar
            ts_event = prev_bar.ts_event + time_change_nanos
            ts_init = prev_bar.ts_init + time_change_nanos
            next_bar = Bar(
                bar_type=first_bar.bar_type,
                open=instrument.make_price(prev_bar.open + price_change),
                high=instrument.make_price(prev_bar.high + price_change),
                low=instrument.make_price(prev_bar.low + price_change),
                close=instrument.make_price(prev_bar.close + price_change),
                volume=prev_bar.volume,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            bars.append(next_bar)

        return bars


def get_test_data_large_path() -> Path:
    return (PACKAGE_ROOT / "tests" / "test_data" / "large").resolve()


def get_test_data_large_checksums_filepath() -> Path:
    return (get_test_data_large_path() / "checksums.json").resolve()


def ensure_test_data_exists(filename: str, url: str) -> Path:
    filepath = (get_test_data_large_path() / filename).resolve()
    checksums_filepath = get_test_data_large_checksums_filepath()
    nautilus_pyo3.ensure_file_exists_or_download_http(str(filepath), url, str(checksums_filepath))
    return filepath


def ensure_data_exists_tardis_deribit_book_l2() -> Path:
    filename = "tardis_deribit_incremental_book_L2_2020-04-01_BTC-PERPETUAL.csv.gz"
    base_url = "https://datasets.tardis.dev"
    url = f"{base_url}/v1/deribit/incremental_book_L2/2020/04/01/BTC-PERPETUAL.csv.gz"
    return ensure_test_data_exists(filename, url)


def ensure_data_exists_tardis_binance_snapshot5() -> Path:
    filename = "tardis_binance-futures_book_snapshot_5_2020-09-01_BTCUSDT.csv.gz"
    base_url = "https://datasets.tardis.dev"
    url = f"{base_url}/v1/binance-futures/book_snapshot_5/2020/09/01/BTCUSDT.csv.gz"
    return ensure_test_data_exists(filename, url)


def ensure_data_exists_tardis_binance_snapshot25() -> Path:
    filename = "tardis_binance-futures_book_snapshot_25_2020-09-01_BTCUSDT.csv.gz"
    base_url = "https://datasets.tardis.dev"
    url = f"{base_url}/v1/binance-futures/book_snapshot_25/2020/09/01/BTCUSDT.csv.gz"
    return ensure_test_data_exists(filename, url)


def ensure_data_exists_tardis_huobi_quotes() -> Path:
    filename = "tardis_huobi-dm-swap_quotes_2020-05-01_BTC-USD.csv.gz"
    base_url = "https://datasets.tardis.dev"
    url = f"{base_url}/v1/huobi-dm-swap/quotes/2020/05/01/BTC-USD.csv.gz"
    return ensure_test_data_exists(filename, url)


def ensure_data_exists_tardis_bitmex_trades() -> Path:
    filename = "tardis_bitmex_trades_2020-03-01_XBTUSD.csv.gz"
    base_url = "https://datasets.tardis.dev"
    url = f"{base_url}/v1/bitmex/trades/2020/03/01/XBTUSD.csv.gz"
    return ensure_test_data_exists(filename, url)
