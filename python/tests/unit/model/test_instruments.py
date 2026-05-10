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

from decimal import Decimal

import pytest

from nautilus_trader.model import AssetClass
from nautilus_trader.model import BettingInstrument
from nautilus_trader.model import BinaryOption
from nautilus_trader.model import Cfd
from nautilus_trader.model import Commodity
from nautilus_trader.model import CryptoFuture
from nautilus_trader.model import CryptoOption
from nautilus_trader.model import CryptoPerpetual
from nautilus_trader.model import Currency
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import Equity
from nautilus_trader.model import FuturesContract
from nautilus_trader.model import FuturesSpread
from nautilus_trader.model import IndexInstrument
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OptionContract
from nautilus_trader.model import OptionKind
from nautilus_trader.model import OptionSpread
from nautilus_trader.model import PerpetualContract
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.model import SyntheticInstrument
from nautilus_trader.model import TokenizedAsset
from nautilus_trader.model import Venue
from tests.providers import TestInstrumentProvider


def test_audusd_sim_construction():
    audusd = TestInstrumentProvider.audusd_sim()

    assert audusd.id == InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
    assert audusd.base_currency == Currency.from_str("AUD")
    assert audusd.quote_currency == Currency.from_str("USD")
    assert audusd.price_precision == 5
    assert audusd.size_precision == 0


def test_usdjpy_sim_construction():
    usdjpy = TestInstrumentProvider.usdjpy_sim()

    assert usdjpy.id == InstrumentId(Symbol("USD/JPY"), Venue("SIM"))
    assert usdjpy.base_currency == Currency.from_str("USD")
    assert usdjpy.quote_currency == Currency.from_str("JPY")
    assert usdjpy.price_precision == 3
    assert usdjpy.size_precision == 0


def test_ethusdt_binance_construction():
    ethusdt = TestInstrumentProvider.ethusdt_binance()

    assert ethusdt.id == InstrumentId(Symbol("ETHUSDT"), Venue("BINANCE"))
    assert ethusdt.base_currency == Currency.from_str("ETH")
    assert ethusdt.quote_currency == Currency.from_str("USDT")
    assert ethusdt.price_precision == 2
    assert ethusdt.size_precision == 5


def test_btcusdt_binance_construction():
    btcusdt = TestInstrumentProvider.btcusdt_binance()

    assert btcusdt.id == InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE"))
    assert btcusdt.base_currency == Currency.from_str("BTC")
    assert btcusdt.quote_currency == Currency.from_str("USDT")
    assert btcusdt.price_precision == 2
    assert btcusdt.size_precision == 6


def test_currency_pair_hash():
    audusd = TestInstrumentProvider.audusd_sim()
    assert isinstance(hash(audusd), int)


def test_currency_pair_type_name():
    audusd = TestInstrumentProvider.audusd_sim()
    assert audusd.type_name == "CurrencyPair"


def test_currency_pair_properties():
    audusd = TestInstrumentProvider.audusd_sim()

    assert audusd.price_increment == Price(1e-05, precision=5)
    assert audusd.size_increment == Quantity.from_int(1)
    assert audusd.lot_size == Quantity.from_str("1000")
    assert audusd.max_quantity == Quantity.from_str("1e7")
    assert audusd.min_quantity == Quantity.from_str("1000")
    assert audusd.margin_init == Decimal("0.03")
    assert audusd.margin_maint == Decimal("0.03")
    assert audusd.maker_fee == Decimal("0.00002")
    assert audusd.taker_fee == Decimal("0.00002")


def test_currency_pair_to_dict_and_from_dict_roundtrip():
    audusd = TestInstrumentProvider.audusd_sim()
    d = audusd.to_dict()
    restored = CurrencyPair.from_dict(d)

    assert restored.id == audusd.id
    assert restored.base_currency == audusd.base_currency
    assert restored.quote_currency == audusd.quote_currency
    assert restored.price_precision == audusd.price_precision
    assert restored.size_precision == audusd.size_precision


def test_currency_pair_direct_construction():
    pair = CurrencyPair(
        instrument_id=InstrumentId(Symbol("TEST/USD"), Venue("SIM")),
        raw_symbol=Symbol("TEST/USD"),
        base_currency=Currency.from_str("BTC"),
        quote_currency=Currency.from_str("USD"),
        price_precision=2,
        size_precision=6,
        price_increment=Price(0.01, precision=2),
        size_increment=Quantity(0.000001, precision=6),
        ts_event=0,
        ts_init=0,
    )

    assert pair.id == InstrumentId(Symbol("TEST/USD"), Venue("SIM"))
    assert pair.price_precision == 2
    assert pair.size_precision == 6


def test_btcusdt_perp_construction():
    perp = TestInstrumentProvider.btcusdt_perp_binance()

    assert perp.id == InstrumentId(Symbol("BTCUSDT-PERP"), Venue("BINANCE"))
    assert perp.base_currency == Currency.from_str("BTC")
    assert perp.quote_currency == Currency.from_str("USDT")
    assert perp.settlement_currency == Currency.from_str("USDT")
    assert perp.is_inverse is False
    assert perp.price_precision == 1
    assert perp.size_precision == 3


def test_crypto_perpetual_type_name():
    perp = TestInstrumentProvider.btcusdt_perp_binance()
    assert perp.type_name == "CryptoPerpetual"


def test_crypto_perpetual_hash():
    perp = TestInstrumentProvider.btcusdt_perp_binance()
    assert isinstance(hash(perp), int)


def test_crypto_perpetual_to_dict_and_from_dict_roundtrip():
    perp = TestInstrumentProvider.btcusdt_perp_binance()
    d = perp.to_dict()
    restored = CryptoPerpetual.from_dict(d)

    assert restored.id == perp.id
    assert restored.base_currency == perp.base_currency
    assert restored.settlement_currency == perp.settlement_currency
    assert restored.is_inverse == perp.is_inverse
    assert restored.price_precision == perp.price_precision
    assert restored.size_precision == perp.size_precision


def test_crypto_perpetual_direct_construction():
    perp = CryptoPerpetual(
        instrument_id=InstrumentId(Symbol("ETHUSDT-PERP"), Venue("BINANCE")),
        raw_symbol=Symbol("ETHUSDT"),
        base_currency=Currency.from_str("ETH"),
        quote_currency=Currency.from_str("USDT"),
        settlement_currency=Currency.from_str("USDT"),
        is_inverse=False,
        price_precision=2,
        size_precision=3,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.001"),
        ts_event=0,
        ts_init=0,
    )

    assert perp.id == InstrumentId(Symbol("ETHUSDT-PERP"), Venue("BINANCE"))
    assert perp.is_inverse is False


def test_equity_direct_construction():
    equity = Equity(
        instrument_id=InstrumentId(Symbol("AAPL"), Venue("NASDAQ")),
        raw_symbol=Symbol("AAPL"),
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        ts_event=0,
        ts_init=0,
        isin="US0378331005",
    )

    assert equity.id == InstrumentId(Symbol("AAPL"), Venue("NASDAQ"))
    assert equity.type_name == "Equity"
    assert equity.quote_currency == Currency.from_str("USD")
    assert equity.price_precision == 2


def test_equity_to_dict_and_from_dict_roundtrip():
    equity = Equity(
        instrument_id=InstrumentId(Symbol("AAPL"), Venue("NASDAQ")),
        raw_symbol=Symbol("AAPL"),
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        ts_event=0,
        ts_init=0,
    )

    d = equity.to_dict()
    restored = Equity.from_dict(d)

    assert restored.id == equity.id
    assert restored.quote_currency == equity.quote_currency
    assert restored.price_precision == equity.price_precision


def test_futures_contract_construction():
    fc = FuturesContract(
        instrument_id=InstrumentId(Symbol("ESZ23"), Venue("XCME")),
        raw_symbol=Symbol("ESZ23"),
        underlying="ES",
        asset_class=AssetClass.INDEX,
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.25"),
        multiplier=Quantity.from_int(50),
        lot_size=Quantity.from_int(1),
        activation_ns=1640390400000000000,
        expiration_ns=1703116800000000000,
        ts_event=0,
        ts_init=0,
    )

    assert fc.id == InstrumentId(Symbol("ESZ23"), Venue("XCME"))
    assert fc.type_name == "FuturesContract"
    assert fc.price_precision == 2


def test_futures_contract_to_dict_and_from_dict_roundtrip():
    fc = FuturesContract(
        instrument_id=InstrumentId(Symbol("ESZ23"), Venue("XCME")),
        raw_symbol=Symbol("ESZ23"),
        underlying="ES",
        asset_class=AssetClass.INDEX,
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.25"),
        multiplier=Quantity.from_int(50),
        lot_size=Quantity.from_int(1),
        activation_ns=1640390400000000000,
        expiration_ns=1703116800000000000,
        ts_event=0,
        ts_init=0,
    )

    d = fc.to_dict()
    restored = FuturesContract.from_dict(d)

    assert restored.id == fc.id
    assert restored.price_precision == fc.price_precision


def test_crypto_future_construction():
    cf = CryptoFuture(
        instrument_id=InstrumentId(Symbol("BTCUSDT_220325"), Venue("BINANCE")),
        raw_symbol=Symbol("BTCUSDT"),
        underlying=Currency.from_str("BTC"),
        quote_currency=Currency.from_str("USDT"),
        settlement_currency=Currency.from_str("USDT"),
        is_inverse=False,
        activation_ns=1640390400000000000,
        expiration_ns=1648166400000000000,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        ts_event=0,
        ts_init=0,
    )

    assert cf.id == InstrumentId(Symbol("BTCUSDT_220325"), Venue("BINANCE"))
    assert cf.type_name == "CryptoFuture"
    assert cf.is_inverse is False


def test_crypto_future_to_dict_and_from_dict_roundtrip():
    cf = CryptoFuture(
        instrument_id=InstrumentId(Symbol("BTCUSDT_220325"), Venue("BINANCE")),
        raw_symbol=Symbol("BTCUSDT"),
        underlying=Currency.from_str("BTC"),
        quote_currency=Currency.from_str("USDT"),
        settlement_currency=Currency.from_str("USDT"),
        is_inverse=False,
        activation_ns=1640390400000000000,
        expiration_ns=1648166400000000000,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        ts_event=0,
        ts_init=0,
    )

    d = cf.to_dict()
    restored = CryptoFuture.from_dict(d)

    assert restored.id == cf.id
    assert restored.is_inverse == cf.is_inverse


def test_option_contract_construction():
    oc = OptionContract(
        instrument_id=InstrumentId(Symbol("AAPL231215C00150000"), Venue("OPRA")),
        raw_symbol=Symbol("AAPL231215C00150000"),
        underlying="AAPL",
        asset_class=AssetClass.EQUITY,
        exchange="OPRA",
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        multiplier=Quantity.from_int(100),
        lot_size=Quantity.from_int(1),
        option_kind=OptionKind.CALL,
        strike_price=Price.from_str("150.00"),
        activation_ns=1640390400000000000,
        expiration_ns=1702598400000000000,
        ts_event=0,
        ts_init=0,
    )

    assert oc.id == InstrumentId(Symbol("AAPL231215C00150000"), Venue("OPRA"))
    assert oc.type_name == "OptionContract"
    assert oc.option_kind == OptionKind.CALL
    assert oc.strike_price == Price.from_str("150.00")
    assert oc.underlying == "AAPL"
    assert oc.price_precision == 2


def test_option_contract_to_dict_and_from_dict_roundtrip():
    oc = OptionContract(
        instrument_id=InstrumentId(Symbol("AAPL231215P00145000"), Venue("OPRA")),
        raw_symbol=Symbol("AAPL231215P00145000"),
        underlying="AAPL",
        asset_class=AssetClass.EQUITY,
        exchange="OPRA",
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        multiplier=Quantity.from_int(100),
        lot_size=Quantity.from_int(1),
        option_kind=OptionKind.PUT,
        strike_price=Price.from_str("145.00"),
        activation_ns=1640390400000000000,
        expiration_ns=1702598400000000000,
        ts_event=0,
        ts_init=0,
    )

    d = oc.to_dict()
    restored = OptionContract.from_dict(d)

    assert restored.id == oc.id
    assert restored.option_kind == oc.option_kind
    assert restored.strike_price == oc.strike_price


def test_binary_option_construction():
    bo = BinaryOption(
        instrument_id=InstrumentId(Symbol("TRUMP-WIN-2024"), Venue("POLYMARKET")),
        raw_symbol=Symbol("TRUMP-WIN-2024"),
        asset_class=AssetClass.ALTERNATIVE,
        currency=Currency.from_str("USD"),
        price_precision=2,
        size_precision=2,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.01"),
        activation_ns=1640390400000000000,
        expiration_ns=1730419200000000000,
        ts_event=0,
        ts_init=0,
        outcome="Yes",
        description="Will Trump win the 2024 election?",
    )

    assert bo.id == InstrumentId(Symbol("TRUMP-WIN-2024"), Venue("POLYMARKET"))
    assert bo.type_name == "BinaryOption"
    assert bo.outcome == "Yes"
    assert bo.description == "Will Trump win the 2024 election?"
    assert bo.price_precision == 2


def test_binary_option_to_dict_and_from_dict_roundtrip():
    bo = BinaryOption(
        instrument_id=InstrumentId(Symbol("TRUMP-WIN-2024"), Venue("POLYMARKET")),
        raw_symbol=Symbol("TRUMP-WIN-2024"),
        asset_class=AssetClass.ALTERNATIVE,
        currency=Currency.from_str("USD"),
        price_precision=2,
        size_precision=2,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.01"),
        activation_ns=1640390400000000000,
        expiration_ns=1730419200000000000,
        ts_event=0,
        ts_init=0,
        outcome="Yes",
        description="Will Trump win the 2024 election?",
    )

    d = bo.to_dict()
    restored = BinaryOption.from_dict(d)

    assert restored.id == bo.id
    assert restored.outcome == bo.outcome
    assert restored.description == bo.description


def test_perpetual_contract_construction():
    pc = PerpetualContract(
        instrument_id=InstrumentId(Symbol("ETHUSD-PERP"), Venue("DYDX")),
        raw_symbol=Symbol("ETH-USD"),
        underlying="ETH",
        asset_class=AssetClass.CRYPTOCURRENCY,
        quote_currency=Currency.from_str("USD"),
        settlement_currency=Currency.from_str("USD"),
        is_inverse=False,
        price_precision=1,
        size_precision=3,
        price_increment=Price.from_str("0.1"),
        size_increment=Quantity.from_str("0.001"),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_str("0.001"),
        ts_event=0,
        ts_init=0,
    )

    assert pc.id == InstrumentId(Symbol("ETHUSD-PERP"), Venue("DYDX"))
    assert pc.type_name == "PerpetualContract"
    assert pc.is_inverse is False
    assert pc.underlying == "ETH"
    assert pc.price_precision == 1


def test_perpetual_contract_to_dict_and_from_dict_roundtrip():
    pc = PerpetualContract(
        instrument_id=InstrumentId(Symbol("ETHUSD-PERP"), Venue("DYDX")),
        raw_symbol=Symbol("ETH-USD"),
        underlying="ETH",
        asset_class=AssetClass.CRYPTOCURRENCY,
        quote_currency=Currency.from_str("USD"),
        settlement_currency=Currency.from_str("USD"),
        is_inverse=False,
        price_precision=1,
        size_precision=3,
        price_increment=Price.from_str("0.1"),
        size_increment=Quantity.from_str("0.001"),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_str("0.001"),
        ts_event=0,
        ts_init=0,
    )

    d = pc.to_dict()
    restored = PerpetualContract.from_dict(d)

    assert restored.id == pc.id
    assert restored.is_inverse == pc.is_inverse


def test_cfd_construction_and_roundtrip():
    cfd = Cfd(
        instrument_id=InstrumentId(Symbol("SPX500"), Venue("SIM")),
        raw_symbol=Symbol("SPX500"),
        asset_class=AssetClass.INDEX,
        base_currency=Currency.from_str("USD"),
        quote_currency=Currency.from_str("USD"),
        price_precision=1,
        size_precision=2,
        price_increment=Price.from_str("0.1"),
        size_increment=Quantity.from_str("0.01"),
        ts_event=0,
        ts_init=0,
    )

    assert cfd.id == InstrumentId(Symbol("SPX500"), Venue("SIM"))
    assert cfd.type_name == "Cfd"

    restored = Cfd.from_dict(cfd.to_dict())

    assert restored.id == cfd.id
    assert restored.price_precision == cfd.price_precision


def test_commodity_construction_and_roundtrip():
    com = Commodity(
        instrument_id=InstrumentId(Symbol("GOLD"), Venue("SIM")),
        raw_symbol=Symbol("GOLD"),
        asset_class=AssetClass.COMMODITY,
        quote_currency=Currency.from_str("USD"),
        price_precision=2,
        size_precision=0,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )

    assert com.id == InstrumentId(Symbol("GOLD"), Venue("SIM"))
    assert com.type_name == "Commodity"

    restored = Commodity.from_dict(com.to_dict())

    assert restored.id == com.id
    assert restored.price_precision == com.price_precision


def test_index_instrument_construction_and_roundtrip():
    idx = IndexInstrument(
        instrument_id=InstrumentId(Symbol("SPX"), Venue("CBOE")),
        raw_symbol=Symbol("SPX"),
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        size_precision=0,
        size_increment=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )

    assert idx.id == InstrumentId(Symbol("SPX"), Venue("CBOE"))
    assert idx.type_name == "IndexInstrument"

    restored = IndexInstrument.from_dict(idx.to_dict())

    assert restored.id == idx.id
    assert restored.price_precision == idx.price_precision


def test_tokenized_asset_construction_and_roundtrip():
    ta = TokenizedAsset(
        instrument_id=InstrumentId(Symbol("TSLA-TOKEN"), Venue("FTX")),
        raw_symbol=Symbol("TSLA"),
        asset_class=AssetClass.EQUITY,
        base_currency=Currency.from_str("USD"),
        quote_currency=Currency.from_str("USD"),
        price_precision=2,
        size_precision=4,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.0001"),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_str("0.0001"),
        ts_event=0,
        ts_init=0,
    )

    assert ta.id == InstrumentId(Symbol("TSLA-TOKEN"), Venue("FTX"))
    assert ta.type_name == "TokenizedAsset"

    restored = TokenizedAsset.from_dict(ta.to_dict())

    assert restored.id == ta.id
    assert restored.price_precision == ta.price_precision


def test_futures_spread_construction_and_roundtrip():
    fs = FuturesSpread(
        instrument_id=InstrumentId(Symbol("ES-SPREAD"), Venue("XCME")),
        raw_symbol=Symbol("ES-SPREAD"),
        underlying="ES",
        strategy_type="CALENDAR",
        asset_class=AssetClass.INDEX,
        exchange="XCME",
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.25"),
        multiplier=Quantity.from_int(50),
        lot_size=Quantity.from_int(1),
        activation_ns=1640390400000000000,
        expiration_ns=1703116800000000000,
        ts_event=0,
        ts_init=0,
    )

    assert fs.id == InstrumentId(Symbol("ES-SPREAD"), Venue("XCME"))
    assert fs.type_name == "FuturesSpread"
    assert fs.strategy_type == "CALENDAR"

    restored = FuturesSpread.from_dict(fs.to_dict())

    assert restored.id == fs.id
    assert restored.strategy_type == fs.strategy_type


def test_option_spread_construction_and_roundtrip():
    os_ = OptionSpread(
        instrument_id=InstrumentId(Symbol("AAPL-SPREAD"), Venue("OPRA")),
        raw_symbol=Symbol("AAPL-SPREAD"),
        underlying="AAPL",
        strategy_type="VERTICAL",
        asset_class=AssetClass.EQUITY,
        exchange="OPRA",
        currency=Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        multiplier=Quantity.from_int(100),
        lot_size=Quantity.from_int(1),
        activation_ns=1640390400000000000,
        expiration_ns=1702598400000000000,
        ts_event=0,
        ts_init=0,
    )

    assert os_.id == InstrumentId(Symbol("AAPL-SPREAD"), Venue("OPRA"))
    assert os_.type_name == "OptionSpread"
    assert os_.strategy_type == "VERTICAL"

    restored = OptionSpread.from_dict(os_.to_dict())

    assert restored.id == os_.id
    assert restored.strategy_type == os_.strategy_type


def test_betting_instrument_construction_and_roundtrip():
    bi = BettingInstrument(
        instrument_id=InstrumentId(Symbol("1-123456-50214-None"), Venue("BETFAIR")),
        raw_symbol=Symbol("1-123456-50214-None"),
        event_type_id=6423,
        event_type_name="American Football",
        competition_id=12282733,
        competition_name="NFL",
        event_id=29678534,
        event_name="NFL",
        event_country_code="GB",
        event_open_date=1644276600000000000,
        betting_type="ODDS",
        market_id="1-123456",
        market_name="AFC Conference Winner",
        market_type="SPECIAL",
        market_start_time=1644276600000000000,
        selection_id=50214,
        selection_name="Kansas City Chiefs",
        selection_handicap=-9999999.0,
        currency=Currency.from_str("GBP"),
        price_precision=2,
        size_precision=2,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.01"),
        ts_event=0,
        ts_init=0,
    )

    assert bi.id == InstrumentId(Symbol("1-123456-50214-None"), Venue("BETFAIR"))
    assert bi.type_name == "BettingInstrument"
    assert bi.market_type == "SPECIAL"
    assert bi.selection_id == 50214
    assert bi.selection_name == "Kansas City Chiefs"
    assert bi.selection_handicap == -9999999.0
    assert bi.betting_type == "ODDS"

    restored = BettingInstrument.from_dict(bi.to_dict())

    assert restored.id == bi.id
    assert restored.market_type == bi.market_type
    assert restored.selection_name == bi.selection_name


def test_crypto_option_construction():
    co = CryptoOption(
        instrument_id=InstrumentId(Symbol("BTC-20240329-50000-C"), Venue("DERIBIT")),
        raw_symbol=Symbol("BTC-20240329-50000-C"),
        underlying=Currency.from_str("BTC"),
        quote_currency=Currency.from_str("USD"),
        settlement_currency=Currency.from_str("BTC"),
        is_inverse=False,
        option_kind=OptionKind.CALL,
        strike_price=Price.from_str("50000.0"),
        activation_ns=1640390400000000000,
        expiration_ns=1711670400000000000,
        price_precision=4,
        size_precision=1,
        price_increment=Price.from_str("0.0001"),
        size_increment=Quantity.from_str("0.1"),
        ts_event=0,
        ts_init=0,
    )

    assert co.id == InstrumentId(Symbol("BTC-20240329-50000-C"), Venue("DERIBIT"))
    assert co.type_name == "CryptoOption"
    assert co.option_kind == OptionKind.CALL
    assert co.strike_price == Price.from_str("50000.0")
    assert co.is_inverse is False
    assert co.price_precision == 4
    assert co.size_precision == 1


def test_crypto_option_to_dict_and_from_dict_roundtrip():
    co = CryptoOption(
        instrument_id=InstrumentId(Symbol("BTC-20240329-50000-C"), Venue("DERIBIT")),
        raw_symbol=Symbol("BTC-20240329-50000-C"),
        underlying=Currency.from_str("BTC"),
        quote_currency=Currency.from_str("USD"),
        settlement_currency=Currency.from_str("BTC"),
        is_inverse=False,
        option_kind=OptionKind.CALL,
        strike_price=Price.from_str("50000.0"),
        activation_ns=1640390400000000000,
        expiration_ns=1711670400000000000,
        price_precision=4,
        size_precision=1,
        price_increment=Price.from_str("0.0001"),
        size_increment=Quantity.from_str("0.1"),
        ts_event=0,
        ts_init=0,
    )

    d = co.to_dict()
    restored = CryptoOption.from_dict(d)

    assert restored.id == co.id
    assert restored.option_kind == co.option_kind
    assert restored.strike_price == co.strike_price
    assert restored.is_inverse == co.is_inverse


def test_instruments_equal_by_id():
    audusd1 = TestInstrumentProvider.audusd_sim()
    audusd2 = TestInstrumentProvider.audusd_sim()
    btcusdt = TestInstrumentProvider.btcusdt_binance()

    assert audusd1 == audusd2
    assert audusd1 != btcusdt


def test_instrument_not_equal_to_none():
    audusd = TestInstrumentProvider.audusd_sim()
    assert (audusd == None) is False  # noqa: E711


def test_equal_instruments_have_equal_hashes():
    audusd1 = TestInstrumentProvider.audusd_sim()
    audusd2 = TestInstrumentProvider.audusd_sim()

    assert hash(audusd1) == hash(audusd2)


def test_different_instruments_have_different_hashes():
    audusd = TestInstrumentProvider.audusd_sim()
    btcusdt = TestInstrumentProvider.btcusdt_binance()

    assert hash(audusd) != hash(btcusdt)


@pytest.mark.parametrize(
    ("factory", "expected_type_name", "expected_id_substr"),
    [
        ("audusd_sim", "CurrencyPair", "AUD/USD.SIM"),
        ("btcusdt_perp_binance", "CryptoPerpetual", "BTCUSDT-PERP.BINANCE"),
        ("ethusdt_binance", "CurrencyPair", "ETHUSDT.BINANCE"),
    ],
)
def test_instrument_repr(factory, expected_type_name, expected_id_substr):
    instrument = getattr(TestInstrumentProvider, factory)()
    r = repr(instrument)

    assert r.startswith(expected_type_name + "(")
    assert expected_id_substr in r


def test_currency_pair_roundtrip_all_fields():
    original = TestInstrumentProvider.audusd_sim()
    restored = CurrencyPair.from_dict(original.to_dict())

    assert restored == original
    assert restored.base_currency == original.base_currency
    assert restored.quote_currency == original.quote_currency
    assert restored.price_precision == original.price_precision
    assert restored.size_precision == original.size_precision
    assert restored.price_increment == original.price_increment
    assert restored.size_increment == original.size_increment
    assert restored.lot_size == original.lot_size
    assert restored.margin_init == original.margin_init
    assert restored.margin_maint == original.margin_maint
    assert restored.maker_fee == original.maker_fee
    assert restored.taker_fee == original.taker_fee


def test_crypto_perpetual_roundtrip_all_fields():
    original = TestInstrumentProvider.btcusdt_perp_binance()
    restored = CryptoPerpetual.from_dict(original.to_dict())

    assert restored == original
    assert restored.base_currency == original.base_currency
    assert restored.settlement_currency == original.settlement_currency
    assert restored.is_inverse == original.is_inverse
    assert restored.price_precision == original.price_precision
    assert restored.size_precision == original.size_precision
    assert restored.price_increment == original.price_increment
    assert restored.size_increment == original.size_increment


def test_make_price_uses_instrument_precision():
    audusd = TestInstrumentProvider.audusd_sim()
    price = audusd.make_price(1.234567890)

    assert price.precision == audusd.price_precision
    assert price == Price.from_str("1.23457")


def test_make_qty_uses_instrument_precision():
    audusd = TestInstrumentProvider.audusd_sim()
    qty = audusd.make_qty(1000)

    assert qty.precision == audusd.size_precision
    assert qty == Quantity.from_int(1000)


def test_make_qty_round_down():
    ethusdt = TestInstrumentProvider.ethusdt_binance()
    qty = ethusdt.make_qty(1.999999, round_down=True)

    assert qty.precision == ethusdt.size_precision
    assert qty == Quantity.from_str("1.99999")


def test_notional_value_currency_pair():
    audusd = TestInstrumentProvider.audusd_sim()
    notional = audusd.notional_value(
        quantity=Quantity.from_str("100000"),
        price=Price.from_str("0.75000"),
    )

    assert notional.currency == audusd.quote_currency
    assert notional.as_double() == pytest.approx(75_000.0)


def test_synthetic_instrument_construction():
    btcusdt_id = InstrumentId.from_str("BTCUSDT.BINANCE")
    ethusdt_id = InstrumentId.from_str("ETHUSDT.BINANCE")

    synth = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[btcusdt_id, ethusdt_id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    assert synth.id == InstrumentId(Symbol("BTC-ETH"), Venue("SYNTH"))
    assert synth.price_precision == 8
    assert len(synth.components) == 2
    assert synth.formula == "(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2"


def test_synthetic_instrument_calculate():
    btcusdt_id = InstrumentId.from_str("BTCUSDT.BINANCE")
    ethusdt_id = InstrumentId.from_str("ETHUSDT.BINANCE")

    synth = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=2,
        components=[btcusdt_id, ethusdt_id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    result = synth.calculate([50_000.0, 3_000.0])

    assert result.precision == 2
    assert result.as_double() == pytest.approx(26_500.0)


def test_synthetic_instrument_change_formula():
    btcusdt_id = InstrumentId.from_str("BTCUSDT.BINANCE")
    ethusdt_id = InstrumentId.from_str("ETHUSDT.BINANCE")

    synth = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[btcusdt_id, ethusdt_id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    synth.change_formula("BTCUSDT.BINANCE - ETHUSDT.BINANCE")

    assert synth.formula == "BTCUSDT.BINANCE - ETHUSDT.BINANCE"


def test_synthetic_instrument_is_valid_formula():
    btcusdt_id = InstrumentId.from_str("BTCUSDT.BINANCE")
    ethusdt_id = InstrumentId.from_str("ETHUSDT.BINANCE")

    synth = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[btcusdt_id, ethusdt_id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=0,
        ts_init=0,
    )

    assert synth.is_valid_formula("BTCUSDT.BINANCE + ETHUSDT.BINANCE")
    assert not synth.is_valid_formula("BTCUSDT.BINANCE + XRPUSDT.BINANCE")


def test_synthetic_instrument_calculate_from_map():
    btcusdt_id = InstrumentId.from_str("BTCUSDT.BINANCE")
    ethusdt_id = InstrumentId.from_str("ETHUSDT.BINANCE")

    synth = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=4,
        components=[btcusdt_id, ethusdt_id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=1,
        ts_init=2,
    )

    result = synth.calculate_from_map(
        {
            "BTCUSDT.BINANCE": 50_000.0,
            "ETHUSDT.BINANCE": 3_000.0,
        },
    )

    assert result == Price.from_str("26500.0000")


def test_synthetic_instrument_basic_properties():
    btcusdt_id = InstrumentId.from_str("BTCUSDT.BINANCE")
    ethusdt_id = InstrumentId.from_str("ETHUSDT.BINANCE")

    synth = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=4,
        components=[btcusdt_id, ethusdt_id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=1,
        ts_init=2,
    )

    assert synth.price_increment == Price.from_str("0.0001")
    assert synth.id == InstrumentId(Symbol("BTC-ETH"), Venue("SYNTH"))
    assert synth.ts_event == 1
    assert synth.ts_init == 2


def test_synthetic_instrument_calculate_from_map_missing_component_raises():
    btcusdt_id = InstrumentId.from_str("BTCUSDT.BINANCE")
    ethusdt_id = InstrumentId.from_str("ETHUSDT.BINANCE")

    synth = SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=4,
        components=[btcusdt_id, ethusdt_id],
        formula="(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2",
        ts_event=1,
        ts_init=2,
    )

    with pytest.raises(ValueError, match=r"Missing price for component: ETHUSDT\.BINANCE"):
        synth.calculate_from_map({"BTCUSDT.BINANCE": 50_000.0})
