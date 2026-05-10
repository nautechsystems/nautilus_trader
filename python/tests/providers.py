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

from __future__ import annotations

import csv
from datetime import datetime
from decimal import Decimal
from pathlib import Path

from nautilus_trader.model import AggressorSide
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import CryptoPerpetual
from nautilus_trader.model import Currency
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import Symbol
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import Venue


PACKAGE_ROOT = Path(__file__).resolve().parent.parent.parent
TEST_DATA_DIR = PACKAGE_ROOT / "tests" / "test_data"


def _parse_iso_to_ns(value: str) -> int:
    s = value.strip()
    if "+" not in s and not s.endswith("Z"):
        s += "+00:00"
    elif s.endswith("Z"):
        s = s[:-1] + "+00:00"
    return int(datetime.fromisoformat(s).timestamp() * 1_000_000_000)


class TestInstrumentProvider:
    """
    Factory methods for common test instruments.
    """

    @staticmethod
    def default_fx_ccy(symbol: str, venue: Venue | None = None) -> CurrencyPair:
        if venue is None:
            venue = Venue("SIM")

        base_currency = symbol[:3]
        quote_currency = symbol[-3:]

        if quote_currency == "JPY":
            price_precision = 3
        else:
            price_precision = 5

        return CurrencyPair(
            instrument_id=InstrumentId(Symbol(symbol), venue),
            raw_symbol=Symbol(symbol),
            base_currency=Currency.from_str(base_currency),
            quote_currency=Currency.from_str(quote_currency),
            price_precision=price_precision,
            size_precision=0,
            price_increment=Price(1 / 10**price_precision, price_precision),
            size_increment=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
            lot_size=Quantity.from_str("1000"),
            max_quantity=Quantity.from_str("1e7"),
            min_quantity=Quantity.from_str("1000"),
            max_notional=Money(50_000_000.00, Currency.from_str("USD")),
            min_notional=Money(1_000.00, Currency.from_str("USD")),
            margin_init=Decimal("0.03"),
            margin_maint=Decimal("0.03"),
            maker_fee=Decimal("0.00002"),
            taker_fee=Decimal("0.00002"),
        )

    @staticmethod
    def audusd_sim() -> CurrencyPair:
        return TestInstrumentProvider.default_fx_ccy("AUD/USD")

    @staticmethod
    def usdjpy_sim() -> CurrencyPair:
        return TestInstrumentProvider.default_fx_ccy("USD/JPY")

    @staticmethod
    def gbpusd_sim() -> CurrencyPair:
        return TestInstrumentProvider.default_fx_ccy("GBP/USD")

    @staticmethod
    def ethusdt_binance() -> CurrencyPair:
        return CurrencyPair(
            instrument_id=InstrumentId(Symbol("ETHUSDT"), Venue("BINANCE")),
            raw_symbol=Symbol("ETHUSDT"),
            base_currency=Currency.from_str("ETH"),
            quote_currency=Currency.from_str("USDT"),
            price_precision=2,
            size_precision=5,
            price_increment=Price(1e-02, precision=2),
            size_increment=Quantity(1e-05, precision=5),
            ts_event=0,
            ts_init=0,
            max_quantity=Quantity(9000, precision=5),
            min_quantity=Quantity(1e-05, precision=5),
            min_notional=Money(10.00, Currency.from_str("USDT")),
            max_price=Price(1000000, precision=2),
            min_price=Price(0.01, precision=2),
            margin_init=Decimal("1.00"),
            margin_maint=Decimal("0.35"),
            maker_fee=Decimal("0.0001"),
            taker_fee=Decimal("0.0001"),
        )

    @staticmethod
    def btcusdt_binance() -> CurrencyPair:
        return CurrencyPair(
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            raw_symbol=Symbol("BTCUSDT"),
            base_currency=Currency.from_str("BTC"),
            quote_currency=Currency.from_str("USDT"),
            price_precision=2,
            size_precision=6,
            price_increment=Price(1e-02, precision=2),
            size_increment=Quantity(1e-06, precision=6),
            ts_event=0,
            ts_init=0,
            max_quantity=Quantity(9000, precision=6),
            min_quantity=Quantity(1e-06, precision=6),
            min_notional=Money(10.00, Currency.from_str("USDT")),
            max_price=Price(1000000, precision=2),
            min_price=Price(0.01, precision=2),
            margin_init=Decimal(0),
            margin_maint=Decimal(0),
            maker_fee=Decimal("0.001"),
            taker_fee=Decimal("0.001"),
        )

    @staticmethod
    def btcusdt_perp_binance() -> CryptoPerpetual:
        return CryptoPerpetual(
            instrument_id=InstrumentId(Symbol("BTCUSDT-PERP"), Venue("BINANCE")),
            raw_symbol=Symbol("BTCUSDT"),
            base_currency=Currency.from_str("BTC"),
            quote_currency=Currency.from_str("USDT"),
            settlement_currency=Currency.from_str("USDT"),
            is_inverse=False,
            price_precision=1,
            size_precision=3,
            price_increment=Price.from_str("0.1"),
            size_increment=Quantity.from_str("0.001"),
            ts_event=0,
            ts_init=0,
            max_quantity=Quantity.from_str("1000.000"),
            min_quantity=Quantity.from_str("0.001"),
            min_notional=Money(10.00, Currency.from_str("USDT")),
            max_price=Price.from_str("809484.0"),
            min_price=Price.from_str("261.1"),
            margin_init=Decimal("0.0500"),
            margin_maint=Decimal("0.0250"),
            maker_fee=Decimal("0.000200"),
            taker_fee=Decimal("0.000180"),
        )


class TestDataProvider:
    """
    Provide test data for acceptance tests.

    Includes synthetic generators (deterministic sine-wave price patterns that create
    EMA crossovers) and CSV readers for the shared `tests/test_data` directory.

    """

    @staticmethod
    def quotes_from_fxcm_bars(
        instrument: CurrencyPair,
        bid_csv: str,
        ask_csv: str,
        max_rows: int | None = None,
    ) -> list[QuoteTick]:
        """
        Build QuoteTicks from a pair of FXCM 1-minute OHLC CSV files.

        For each bid/ask bar, emits four ticks in OHLC order with the bar timestamp.

        """
        bid_rows = TestDataProvider._read_ohlc_rows(TEST_DATA_DIR / bid_csv, max_rows)
        ask_rows = TestDataProvider._read_ohlc_rows(TEST_DATA_DIR / ask_csv, max_rows)
        precision = instrument.price_precision
        size = Quantity.from_str("1000000")
        ticks: list[QuoteTick] = []

        for bid_row, ask_row in zip(bid_rows, ask_rows, strict=True):
            ts_ns = _parse_iso_to_ns(bid_row[0])

            for column in (1, 2, 3, 4):  # open, high, low, close
                bid_price = Price(float(bid_row[column]), precision=precision)
                ask_price = Price(float(ask_row[column]), precision=precision)
                ticks.append(
                    QuoteTick(
                        instrument_id=instrument.id,
                        bid_price=bid_price,
                        ask_price=ask_price,
                        bid_size=size,
                        ask_size=size,
                        ts_event=ts_ns,
                        ts_init=ts_ns,
                    ),
                )

        return ticks

    @staticmethod
    def bars_from_fxcm_bars(
        instrument: CurrencyPair,
        bar_type: BarType,
        bid_or_ask_csv: str,
        max_rows: int | None = None,
    ) -> list[Bar]:
        rows = TestDataProvider._read_ohlc_rows(TEST_DATA_DIR / bid_or_ask_csv, max_rows)
        precision = instrument.price_precision
        bars: list[Bar] = []

        for row in rows:
            ts_ns = _parse_iso_to_ns(row[0])
            bars.append(
                Bar(
                    bar_type=bar_type,
                    open=Price(float(row[1]), precision=precision),
                    high=Price(float(row[2]), precision=precision),
                    low=Price(float(row[3]), precision=precision),
                    close=Price(float(row[4]), precision=precision),
                    volume=Quantity.from_str("1000000"),
                    ts_event=ts_ns,
                    ts_init=ts_ns,
                ),
            )

        return bars

    @staticmethod
    def quotes_from_truefx_csv(
        instrument: CurrencyPair,
        csv_name: str,
        max_rows: int | None = None,
    ) -> list[QuoteTick]:
        path = TEST_DATA_DIR / csv_name
        precision = instrument.price_precision
        size = Quantity.from_str("1000000")
        ticks: list[QuoteTick] = []

        with path.open("r") as f:
            reader = csv.reader(f)
            header = next(reader)
            assert header[:3] == ["timestamp", "bid", "ask"]

            for i, row in enumerate(reader):
                if max_rows is not None and i >= max_rows:
                    break
                ts_ns = _parse_iso_to_ns(row[0])
                ticks.append(
                    QuoteTick(
                        instrument_id=instrument.id,
                        bid_price=Price(float(row[1]), precision=precision),
                        ask_price=Price(float(row[2]), precision=precision),
                        bid_size=size,
                        ask_size=size,
                        ts_event=ts_ns,
                        ts_init=ts_ns,
                    ),
                )

        return ticks

    @staticmethod
    def trades_from_binance_csv(
        instrument: CurrencyPair,
        csv_name: str,
        max_rows: int | None = None,
    ) -> list[TradeTick]:
        path = TEST_DATA_DIR / csv_name
        price_precision = instrument.price_precision
        size_precision = instrument.size_precision
        trades: list[TradeTick] = []

        with path.open("r") as f:
            reader = csv.reader(f)
            header = next(reader)
            assert header[:5] == [
                "timestamp",
                "trade_id",
                "price",
                "quantity",
                "buyer_maker",
            ]

            for i, row in enumerate(reader):
                if max_rows is not None and i >= max_rows:
                    break
                ts_ns = _parse_iso_to_ns(row[0])
                buyer_maker = row[4].strip().lower() == "true"
                aggressor = AggressorSide.SELLER if buyer_maker else AggressorSide.BUYER
                trades.append(
                    TradeTick(
                        instrument_id=instrument.id,
                        price=Price(float(row[2]), precision=price_precision),
                        size=Quantity(float(row[3]), precision=size_precision),
                        aggressor_side=aggressor,
                        trade_id=TradeId(row[1]),
                        ts_event=ts_ns,
                        ts_init=ts_ns,
                    ),
                )

        return trades

    @staticmethod
    def bars_from_binance_csv(
        instrument: CurrencyPair,
        bar_type: BarType,
        csv_name: str,
        max_rows: int | None = None,
    ) -> list[Bar]:
        path = TEST_DATA_DIR / csv_name
        price_precision = instrument.price_precision
        size_precision = instrument.size_precision
        bars: list[Bar] = []

        with path.open("r") as f:
            reader = csv.reader(f)
            header = next(reader)
            assert header[:6] == ["timestamp", "open", "high", "low", "close", "volume"]

            for i, row in enumerate(reader):
                if max_rows is not None and i >= max_rows:
                    break
                ts_ns = _parse_iso_to_ns(row[0])
                bars.append(
                    Bar(
                        bar_type=bar_type,
                        open=Price(float(row[1]), precision=price_precision),
                        high=Price(float(row[2]), precision=price_precision),
                        low=Price(float(row[3]), precision=price_precision),
                        close=Price(float(row[4]), precision=price_precision),
                        volume=Quantity(float(row[5]), precision=size_precision),
                        ts_event=ts_ns,
                        ts_init=ts_ns,
                    ),
                )

        return bars

    @staticmethod
    def _read_ohlc_rows(path: Path, max_rows: int | None) -> list[list[str]]:
        rows: list[list[str]] = []

        with path.open("r") as f:
            reader = csv.reader(f)
            header = next(reader)
            assert header[:5] == ["timestamp", "open", "high", "low", "close"]

            for i, row in enumerate(reader):
                if max_rows is not None and i >= max_rows:
                    break
                rows.append(row)

        return rows

    @staticmethod
    def usdjpy_quotes(count: int = 10_000) -> list[QuoteTick]:
        """
        Generate USD/JPY quote ticks with a sine-wave bid pattern.
        """
        import math

        instrument_id = InstrumentId(Symbol("USD/JPY"), Venue("SIM"))
        base_ns = 1_546_383_600_000_000_000  # 2019-01-02 00:00:00 UTC

        ticks = []

        for i in range(count):
            ts = base_ns + i * 1_000_000_000
            bid = 109.500 + 0.500 * math.sin(i / 500.0)
            ask = bid + 0.010
            ticks.append(
                QuoteTick(
                    instrument_id=instrument_id,
                    bid_price=Price(bid, precision=3),
                    ask_price=Price(ask, precision=3),
                    bid_size=Quantity.from_int(1_000_000),
                    ask_size=Quantity.from_int(1_000_000),
                    ts_event=ts,
                    ts_init=ts,
                ),
            )
        return ticks

    @staticmethod
    def audusd_quotes(count: int = 3_000) -> list[QuoteTick]:
        """
        Generate AUD/USD quote ticks with a sine-wave bid pattern.
        """
        import math

        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
        base_ns = 1_546_383_600_000_000_000

        ticks = []

        for i in range(count):
            ts = base_ns + i * 1_000_000_000
            bid = 0.71000 + 0.00500 * math.sin(i / 300.0)
            ask = bid + 0.00010
            ticks.append(
                QuoteTick(
                    instrument_id=instrument_id,
                    bid_price=Price(bid, precision=5),
                    ask_price=Price(ask, precision=5),
                    bid_size=Quantity.from_int(1_000_000),
                    ask_size=Quantity.from_int(1_000_000),
                    ts_event=ts,
                    ts_init=ts,
                ),
            )
        return ticks
