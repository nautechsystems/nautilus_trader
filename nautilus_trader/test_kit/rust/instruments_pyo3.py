# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd
import pytz

from nautilus_trader.core.nautilus_pyo3 import AssetClass
from nautilus_trader.core.nautilus_pyo3 import CryptoFuture
from nautilus_trader.core.nautilus_pyo3 import CryptoPerpetual
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import CurrencyPair
from nautilus_trader.core.nautilus_pyo3 import Equity
from nautilus_trader.core.nautilus_pyo3 import FuturesContract
from nautilus_trader.core.nautilus_pyo3 import FuturesSpread
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OptionKind
from nautilus_trader.core.nautilus_pyo3 import OptionsContract
from nautilus_trader.core.nautilus_pyo3 import OptionsSpread
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.nautilus_pyo3 import Venue
from nautilus_trader.test_kit.rust.types_pyo3 import TestTypesProviderPyo3


_USD = TestTypesProviderPyo3.currency_usd()
_USDT = TestTypesProviderPyo3.currency_usdt()
_BTC = TestTypesProviderPyo3.currency_btc()
_ETH = TestTypesProviderPyo3.currency_eth()


class TestInstrumentProviderPyo3:
    @staticmethod
    def default_fx_ccy(
        symbol: str,
        venue: Venue | None = None,
    ) -> CurrencyPair:
        if venue is None:
            venue = Venue("SIM")
        instrument_id = InstrumentId(Symbol(symbol), venue)
        base_currency = symbol[:3]
        quote_currency = symbol[-3:]

        if quote_currency == "JPY":
            price_precision = 3
        else:
            price_precision = 5

        return CurrencyPair(
            id=instrument_id,
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
            margin_init=Decimal("0.03"),
            margin_maint=Decimal("0.03"),
            maker_fee=Decimal("0.00002"),
            taker_fee=Decimal("0.00002"),
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def audusd_sim():
        return TestInstrumentProviderPyo3.default_fx_ccy("AUD/USD")

    @staticmethod
    def ethusdt_perp_binance() -> CryptoPerpetual:
        return CryptoPerpetual(
            id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
            symbol=Symbol("ETHUSDT-PERP"),
            base_currency=_ETH,
            quote_currency=_USDT,
            settlement_currency=_USDT,
            is_inverse=False,
            price_precision=2,
            size_precision=3,
            price_increment=Price.from_str("0.01"),
            size_increment=Quantity.from_str("0.001"),
            lot_size=None,
            max_quantity=Quantity.from_str("10000"),
            min_quantity=Quantity.from_str("0.001"),
            max_notional=None,
            min_notional=Money(10.0, _USDT),
            max_price=Price.from_str("15000.0"),
            min_price=Price.from_str("1.0"),
            margin_init=Decimal("1.00"),
            margin_maint=Decimal("0.35"),
            maker_fee=Decimal("0.0002"),
            taker_fee=Decimal("0.0004"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def ethusdt_binance() -> CurrencyPair:
        return CurrencyPair(
            id=InstrumentId(
                symbol=Symbol("ETHUSDT"),
                venue=Venue("BINANCE"),
            ),
            raw_symbol=Symbol("ETHUSDT"),
            base_currency=Currency.from_str("ETH"),
            quote_currency=Currency.from_str("USDT"),
            price_precision=2,
            size_precision=5,
            price_increment=Price(1e-02, precision=2),
            size_increment=Quantity(1e-05, precision=5),
            lot_size=None,
            max_quantity=Quantity(9000, precision=5),
            min_quantity=Quantity(1e-05, precision=5),
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
    def xbtusd_bitmex() -> CryptoPerpetual:
        return CryptoPerpetual(
            id=InstrumentId(
                symbol=Symbol("BTC/USD"),
                venue=Venue("BITMEX"),
            ),
            symbol=Symbol("XBTUSD"),
            base_currency=_BTC,
            quote_currency=_USD,
            settlement_currency=_BTC,
            is_inverse=True,
            price_precision=1,
            size_precision=0,
            price_increment=Price.from_str("0.5"),
            size_increment=Quantity.from_int(1),
            max_quantity=None,
            min_quantity=None,
            max_notional=Money(10_000_000.00, _USD),
            min_notional=Money(1.00, _USD),
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
        return CryptoPerpetual(
            id=InstrumentId(
                symbol=Symbol("ETH/USD"),
                venue=Venue("BITMEX"),
            ),
            symbol=Symbol("ETHUSD"),
            base_currency=_ETH,
            quote_currency=_USD,
            settlement_currency=_ETH,
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
    def btcusdt_future_binance(
        activation: pd.Timestamp | None = None,
        expiration: pd.Timestamp | None = None,
    ) -> CryptoFuture:
        if activation is None:
            activation = pd.Timestamp("2021-12-25", tz=pytz.utc)
        if expiration is None:
            expiration = pd.Timestamp("2022-3-25", tz=pytz.utc)
        symbol = f"BTCUSDT_{expiration.strftime('%y%m%d')}"
        instrument_id_str = f"{symbol}.BINANCE"
        return CryptoFuture(
            id=InstrumentId.from_str(instrument_id_str),
            raw_symbol=Symbol(symbol),
            underlying=_BTC,
            quote_currency=_USDT,
            settlement_currency=_USDT,
            activation_ns=activation.value,
            expiration_ns=expiration.value,
            price_precision=2,
            size_precision=6,
            price_increment=Price.from_str("0.01"),
            size_increment=Quantity.from_str("0.000001"),
            maker_fee=Decimal("0"),
            taker_fee=Decimal("0"),
            margin_init=Decimal("0"),
            margin_maint=Decimal("0"),
            lot_size=None,
            max_quantity=Quantity.from_str("9000"),
            min_quantity=Quantity.from_str("0.00001"),
            max_notional=None,
            min_notional=Money(10.0, TestTypesProviderPyo3.currency_usdt()),
            max_price=Price.from_str("1000000.0"),
            min_price=Price.from_str("0.01"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def btcusdt_binance() -> CurrencyPair:
        return CurrencyPair(
            id=InstrumentId.from_str("BTCUSDT.BINANCE"),
            raw_symbol=Symbol("BTCUSDT"),
            base_currency=TestTypesProviderPyo3.currency_btc(),
            quote_currency=_USDT,
            price_precision=2,
            size_precision=6,
            price_increment=Price.from_str("0.01"),
            size_increment=Quantity.from_str("0.000001"),
            lot_size=None,
            max_quantity=Quantity.from_str("9000"),
            min_quantity=Quantity.from_str("0.00001"),
            max_price=Price.from_str("1000000"),
            min_price=Price.from_str("0.01"),
            margin_init=Decimal("0.0500"),
            margin_maint=Decimal("0.0250"),
            maker_fee=Decimal("0.001"),
            taker_fee=Decimal("0.001"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def aapl_option(
        activation: pd.Timestamp | None = None,
        expiration: pd.Timestamp | None = None,
    ) -> OptionsContract:
        if activation is None:
            activation = pd.Timestamp("2021-9-17", tz=pytz.utc)
        if expiration is None:
            expiration = pd.Timestamp("2021-12-17", tz=pytz.utc)
        return OptionsContract(
            id=InstrumentId.from_str("AAPL211217C00150000.OPRA"),
            raw_symbol=Symbol("AAPL211217C00150000"),
            asset_class=AssetClass.EQUITY,
            exchange="GMNI",  # Nasdaq GEMX
            underlying="AAPL",
            option_kind=OptionKind.CALL,
            activation_ns=activation.value,
            expiration_ns=expiration.value,
            strike_price=Price.from_str("149.0"),
            currency=_USDT,
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            max_quantity=None,
            min_quantity=None,
            max_price=None,
            min_price=None,
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def aapl_equity() -> Equity:
        return Equity(
            id=InstrumentId.from_str("AAPL.XNAS"),
            raw_symbol=Symbol("AAPL"),
            isin="US0378331005",
            currency=_USD,
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            lot_size=Quantity.from_int(100),
            max_quantity=None,
            min_quantity=None,
            max_price=None,
            min_price=None,
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def futures_contract_es(
        activation: pd.Timestamp | None = None,
        expiration: pd.Timestamp | None = None,
    ) -> FuturesContract:
        if activation is None:
            activation = pd.Timestamp("2021-9-17", tz=pytz.utc)
        if expiration is None:
            expiration = pd.Timestamp("2021-12-17", tz=pytz.utc)
        return FuturesContract(
            id=InstrumentId.from_str("ESZ1.GLBX"),
            raw_symbol=Symbol("ESZ1"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            underlying="ES",
            activation_ns=activation.value,
            expiration_ns=expiration.value,
            currency=_USD,
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            max_quantity=None,
            min_quantity=None,
            max_price=None,
            min_price=None,
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def futures_spread_es(
        activation: pd.Timestamp | None = None,
        expiration: pd.Timestamp | None = None,
    ) -> FuturesSpread:
        if activation is None:
            activation = pd.Timestamp("2022-6-21T13:30:00", tz=pytz.utc)
        if expiration is None:
            expiration = pd.Timestamp("2024-6-21T13:30:00", tz=pytz.utc)
        return FuturesSpread(
            id=InstrumentId.from_str("ESM4-ESU4.GLBX"),
            raw_symbol=Symbol("ESM4-ESU4"),
            asset_class=AssetClass.INDEX,
            exchange="XCME",
            underlying="ES",
            strategy_type="EQ",
            activation_ns=activation.value,
            expiration_ns=expiration.value,
            currency=_USD,
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            max_quantity=None,
            min_quantity=None,
            max_price=None,
            min_price=None,
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def options_spread(
        activation: pd.Timestamp | None = None,
        expiration: pd.Timestamp | None = None,
    ) -> OptionsSpread:
        if activation is None:
            activation = pd.Timestamp("2023-11-06T20:54:07", tz=pytz.utc)
        if expiration is None:
            expiration = pd.Timestamp("2024-02-23T22:59:00", tz=pytz.utc)
        return OptionsSpread(
            id=InstrumentId.from_str("UD:U$: GN 2534559.GLBX"),
            raw_symbol=Symbol("UD:U$: GN 2534559"),
            asset_class=AssetClass.FX,
            exchange="XCME",
            underlying="SR3",
            strategy_type="GN",
            activation_ns=activation.value,
            expiration_ns=expiration.value,
            currency=_USDT,
            price_precision=2,
            price_increment=Price.from_str("0.01"),
            multiplier=Quantity.from_int(1),
            lot_size=Quantity.from_int(1),
            max_quantity=None,
            min_quantity=None,
            max_price=None,
            min_price=None,
            ts_event=0,
            ts_init=0,
        )
