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
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OptionKind
from nautilus_trader.core.nautilus_pyo3 import OptionsContract
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.nautilus_pyo3 import Venue
from nautilus_trader.test_kit.rust.types_pyo3 import TestTypesProviderPyo3


USD = TestTypesProviderPyo3.currency_usd()
USDT = TestTypesProviderPyo3.currency_usdt()
BTC = TestTypesProviderPyo3.currency_btc()
ETH = TestTypesProviderPyo3.currency_eth()


class TestInstrumentProviderPyo3:
    @staticmethod
    def ethusdt_perp_binance() -> CryptoPerpetual:
        return CryptoPerpetual(
            id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
            symbol=Symbol("ETHUSDT"),
            base_currency=ETH,
            quote_currency=USDT,
            settlement_currency=USDT,
            is_inverse=False,
            price_precision=2,
            size_precision=0,
            price_increment=Price.from_str("0.01"),
            size_increment=Quantity.from_str("0.001"),
            lot_size=None,
            max_quantity=Quantity.from_str("10000"),
            min_quantity=Quantity.from_str("0.001"),
            max_notional=None,
            min_notional=Money(10.0, USDT),
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
    def xbtusd_bitmex() -> CryptoPerpetual:
        return CryptoPerpetual(
            id=InstrumentId(
                symbol=Symbol("BTC/USD"),
                venue=Venue("BITMEX"),
            ),
            symbol=Symbol("XBTUSD"),
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
    def btcusdt_future_binance(
        activation: pd.Timestamp | None = None,
        expiration: pd.Timestamp | None = None,
    ) -> CryptoFuture:
        if activation is None:
            activation = pd.Timestamp("2021-12-25", tz=pytz.utc)
        if expiration is None:
            expiration = pd.Timestamp("2022-3-25", tz=pytz.utc)

        instrument_id_str = f"BTCUSDT_{expiration.strftime('%y%m%d')}.BINANCE"
        return CryptoFuture(
            id=InstrumentId.from_str(instrument_id_str),
            raw_symbol=Symbol("BTCUSDT"),
            underlying=BTC,
            quote_currency=USDT,
            settlement_currency=USDT,
            activation_ns=activation.value,
            expiration_ns=expiration.value,
            price_precision=2,
            size_precision=6,
            price_increment=Price.from_str("0.01"),
            size_increment=Quantity.from_str("0.000001"),
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
            quote_currency=USDT,
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
            maker_fee=Decimal("0.000200"),
            taker_fee=Decimal("0.000180"),
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
            underlying="AAPL",
            option_kind=OptionKind.CALL,
            activation_ns=activation.value,
            expiration_ns=expiration.value,
            strike_price=Price.from_str("149.0"),
            currency=USDT,
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
            currency=USD,
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
            id=InstrumentId.from_str("ESZ21.CME"),
            raw_symbol=Symbol("ESZ21"),
            asset_class=AssetClass.INDEX,
            underlying="ES",
            activation_ns=activation.value,
            expiration_ns=expiration.value,
            currency=USD,
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
