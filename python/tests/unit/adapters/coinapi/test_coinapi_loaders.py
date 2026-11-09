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
"""
Test the CoinAPI flat file loader bindings against the committed fixtures.
"""

import datetime as dt
from decimal import ROUND_HALF_EVEN
from decimal import Decimal
from pathlib import Path

import pytest
from tests.providers import TestInstrumentProvider

from nautilus_trader.adapters import coinapi
from nautilus_trader.adapters.coinapi import CoinApiDataLoader
from nautilus_trader.model import FIXED_PRECISION
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import BarType
from nautilus_trader.model import BookAction
from nautilus_trader.model import Currency
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import RecordFlag


FIXTURES = Path(__file__).resolve().parents[5] / "crates/adapters/coinapi/test_data/csv"
BINANCE = InstrumentId.from_str("BTCUSDT.BINANCE")
COINBASE = InstrumentId.from_str("BTC-USD.COINBASE")
BOOK_DATE = dt.date(2025, 9, 1)

# 2025-09-01T00:00:00Z
DAY_START = 1_756_684_800_000_000_000


def _currency_pair(instrument_id: InstrumentId, price_precision: int = 2, size_precision: int = 8):
    return CurrencyPair(
        instrument_id=instrument_id,
        raw_symbol=instrument_id.symbol,
        base_currency=Currency.from_str("SOL" if str(instrument_id.symbol) == "SOLUSD" else "BTC"),
        quote_currency=Currency.from_str("USDT" if instrument_id == BINANCE else "USD"),
        price_precision=price_precision,
        size_precision=size_precision,
        price_increment=Price.from_str("0." + "0" * (price_precision - 1) + "1"),
        size_increment=Quantity.from_str("0." + "0" * (size_precision - 1) + "1"),
        ts_event=0,
        ts_init=0,
    )


@pytest.fixture
def loader() -> CoinApiDataLoader:
    """
    Retain definitions for the captured CoinAPI files.
    """
    instance = CoinApiDataLoader()
    instance.add_instrument(_currency_pair(BINANCE, 2, 5))
    instance.add_instrument(_currency_pair(COINBASE))
    return instance


@pytest.mark.parametrize("to_path", [str, Path])
def test_load_trades_accepts_str_and_path(loader: CoinApiDataLoader, to_path) -> None:
    """
    Test trades load exactly from a str or Path.
    """
    trades = loader.load_trades(
        to_path(FIXTURES / "trades_binance_spot_btc_usdt_20250901.csv"),
        BINANCE,
    )

    assert len(trades) == 50
    assert trades[0].instrument_id == BINANCE
    assert trades[0].price == Price.from_str("108246.36")
    assert trades[0].size == Quantity.from_str("0.00023")
    assert trades[0].aggressor_side == AggressorSide.BUY
    assert str(trades[0].trade_id) == "8a61934b-2ca0-4e94-b4ec-cb5409a28c53"
    assert trades[0].ts_event == DAY_START + 178_000_000
    assert trades[0].ts_init == DAY_START + 299_824_200
    assert {trade.size.precision for trade in trades} == {5}


def test_load_quotes_converts_every_field(loader: CoinApiDataLoader) -> None:
    """
    Test quotes convert every field exactly.
    """
    quotes = loader.load_quotes(FIXTURES / "quotes_coinbase_spot_btc_usd_20250901.csv", COINBASE)

    assert len(quotes) == 50
    assert quotes[0].instrument_id == COINBASE
    assert quotes[0].bid_price == Price.from_str("108247.90")
    assert quotes[0].ask_price == Price.from_str("108247.94")
    assert quotes[0].bid_size == Quantity.from_str("0.18478034")
    assert quotes[0].ask_size == Quantity.from_str("0.19673805")
    assert quotes[0].ts_event == DAY_START + 47_937_000
    assert quotes[0].ts_init == DAY_START + 453_522_400


def test_load_deltas_takes_a_date(loader: CoinApiDataLoader) -> None:
    """
    Test book deltas load with an explicit datetime.date.
    """
    deltas = loader.load_book_deltas(
        FIXTURES / "limitbook_full_coinbase_spot_btc_usd_20250901.csv",
        COINBASE,
        date=BOOK_DATE,
    )

    first = deltas[1]
    assert len(deltas) == 52
    assert deltas[0].action == BookAction.CLEAR
    assert first.instrument_id == COINBASE
    assert first.action == BookAction.ADD
    assert first.order.side == OrderSide.SELL
    assert first.order.price == Price.from_str("108247.94")
    assert first.order.size == Quantity.from_str("0.00923805")
    assert first.order.order_id == 1
    assert first.flags == RecordFlag.F_SNAPSHOT.value
    assert first.sequence == 0
    assert first.ts_event == DAY_START + 1_027_000
    assert first.ts_init == DAY_START + 83_135_700
    assert [delta.action for delta in deltas].count(BookAction.DELETE) == 19


def test_load_deltas_raises_without_a_date_outside_a_partition(loader: CoinApiDataLoader) -> None:
    """
    Test a book file outside a date partition needs a date.
    """
    with pytest.raises(ValueError, match="no date given"):
        loader.load_book_deltas(
            FIXTURES / "limitbook_full_coinbase_spot_btc_usd_20250901.csv",
            COINBASE,
        )


def test_stream_trades_yields_the_loaded_trades_in_chunks(loader: CoinApiDataLoader) -> None:
    """
    Test a trade stream yields the loaded trades in chunks, then stops.
    """
    path = FIXTURES / "trades_binance_spot_btc_usdt_20250901.csv"

    chunks = list(loader.stream_trades(path, BINANCE, chunk_size=7))

    assert [len(chunk) for chunk in chunks] == [7, 7, 7, 7, 7, 7, 7, 1]
    assert [trade for chunk in chunks for trade in chunk] == loader.load_trades(path, BINANCE)


def test_stream_quotes_preserves_fields_limit_chunks_and_eof(
    loader: CoinApiDataLoader,
    tmp_path: Path,
) -> None:
    """
    Test quote streaming binds every field and stops at the requested limit.
    """
    path = tmp_path / "quotes.csv"
    path.write_text(
        "time_exchange;time_coinapi;ask_px;ask_sx;bid_px;bid_sx\n"
        "2025-09-01T00:00:00.1000000;2025-09-01T00:00:00.2000000;101.25;2.5;100.5;1\n"
        "2025-09-01T00:00:01.3000000;2025-09-01T00:00:01.4000000;101.375;2.0625;100.125;0.125\n"
        "2025-09-01T00:00:02;2025-09-01T00:00:03;102;3;101;2\n",
        encoding="utf-8",
    )
    loader.add_instrument(_currency_pair(COINBASE, 3, 4))
    stream = loader.stream_quotes(
        path,
        COINBASE,
        chunk_size=1,
        limit=2,
    )

    first = next(stream)
    second = next(stream)

    assert first == [
        QuoteTick(
            COINBASE,
            Price.from_str("100.500"),
            Price.from_str("101.250"),
            Quantity.from_str("1.0000"),
            Quantity.from_str("2.5000"),
            DAY_START + 100_000_000,
            DAY_START + 200_000_000,
        ),
    ]
    assert second == [
        QuoteTick(
            COINBASE,
            Price.from_str("100.125"),
            Price.from_str("101.375"),
            Quantity.from_str("0.1250"),
            Quantity.from_str("2.0625"),
            DAY_START + 1_300_000_000,
            DAY_START + 1_400_000_000,
        ),
    ]
    assert [
        (q.bid_price.precision, q.ask_price.precision, q.bid_size.precision, q.ask_size.precision)
        for q in first + second
    ] == [(3, 3, 4, 4)] * 2
    with pytest.raises(StopIteration):
        next(stream)
    with pytest.raises(StopIteration):
        next(stream)


def test_stream_quotes_reports_precision_error(loader: CoinApiDataLoader) -> None:
    """
    Test a quote stream exposes a precision failure as ValueError.
    """
    loader.add_instrument(_currency_pair(BINANCE, 1, 5))
    stream = loader.stream_quotes(
        FIXTURES / "quotes_binance_spot_btc_usdt_20250901.csv",
        BINANCE,
        chunk_size=1,
    )

    with pytest.raises(
        ValueError,
        match=r"^bid_px 108246\.35 is finer than instrument precision 1$",
    ):
        next(stream)


def test_stream_deltas_ends_each_chunk_at_an_event_boundary(loader: CoinApiDataLoader) -> None:
    """
    Test a book stream yields the loaded deltas and ends every chunk with F_LAST.
    """
    path = FIXTURES / "limitbook_full_coinbase_spot_btc_usd_20250901.csv"

    chunks = list(loader.stream_book_deltas(path, COINBASE, date=BOOK_DATE, chunk_size=5))

    assert len(chunks[0]) == 14
    assert all(chunk[-1].flags & RecordFlag.F_LAST.value for chunk in chunks)
    assert [delta for chunk in chunks for delta in chunk] == loader.load_book_deltas(
        path,
        COINBASE,
        date=BOOK_DATE,
    )


def test_stream_raises_for_a_value_finer_than_its_precision(loader: CoinApiDataLoader) -> None:
    """
    Test a stream raises ValueError for a value finer than its precision.
    """
    loader.add_instrument(_currency_pair(BINANCE, 1, 5))
    chunks = loader.stream_trades(
        FIXTURES / "trades_binance_spot_btc_usdt_20250901.csv",
        BINANCE,
        chunk_size=10,
    )

    with pytest.raises(
        ValueError,
        match=r"^price 108246\.36 is finer than instrument precision 1$",
    ):
        next(chunks)


@pytest.mark.parametrize("chunk_size", [0, 1_000_001])
def test_stream_rejects_chunk_size_out_of_range(loader: CoinApiDataLoader, chunk_size: int) -> None:
    """
    Test a stream rejects a chunk size outside its range.
    """
    with pytest.raises(ValueError, match="chunk_size"):
        loader.stream_trades(
            FIXTURES / "trades_binance_spot_btc_usdt_20250901.csv",
            BINANCE,
            chunk_size=chunk_size,
        )


def test_ohlcv_loader_maps_symbol_ids_to_instruments(loader: CoinApiDataLoader) -> None:
    """
    Test the OHLCV loader maps numeric symbol IDs and skips the rest.
    """
    loader.add_instrument(
        _currency_pair(InstrumentId.from_str("SOLUSD.BITSTAMP")),
        symbol_id_int=138492,
    )

    bars = loader.load_bars(FIXTURES / "ohlcv_bitstamp_1day_20250911.csv", period_id="1DAY")

    assert len(bars) == 1
    assert bars[0].bar_type == BarType.from_str("SOLUSD.BITSTAMP-1-DAY-LAST-EXTERNAL")

    # The captured volume carries a numeric tail, rounded at the build's precision
    volume = Decimal("689.5507347100003").quantize(
        Decimal(1).scaleb(-FIXED_PRECISION),
        rounding=ROUND_HALF_EVEN,
    )
    assert bars[0].open == Price.from_str("496.11")
    assert bars[0].high == Price.from_str("515.47")
    assert bars[0].low == Price.from_str("493.23")
    assert bars[0].close == Price.from_str("508.00")
    assert bars[0].volume.as_decimal() == volume
    assert bars[0].ts_event == 1_757_635_200_000_000_000
    assert bars[0].ts_init == 1_757_635_200_000_000_000


@pytest.mark.parametrize("column", ["px_open", "px_high", "px_low", "px_close"])
def test_ohlcv_loader_rejects_finer_price_without_partial_bars(
    loader: CoinApiDataLoader,
    tmp_path: Path,
    column: str,
) -> None:
    """
    Reject a later symbol's over-precise price without returning earlier bars.
    """
    header, valid = (
        (FIXTURES / "ohlcv_bitstamp_1day_20250911.csv").read_text(encoding="utf-8").splitlines()[:2]
    )
    invalid = valid.split(";")
    invalid[0] = "27576"
    invalid[header.split(";").index(column)] = "1.234"
    path = tmp_path / "TP-1DAY.csv"
    path.write_text(f"{header}\n{valid}\n{';'.join(invalid)}\n", encoding="utf-8")
    loader.add_instrument(
        _currency_pair(InstrumentId.from_str("SOLUSD.BITSTAMP")),
        symbol_id_int=138492,
    )
    loader.add_instrument(_currency_pair(COINBASE), symbol_id_int=27576)

    with pytest.raises(ValueError, match=rf"^{column} ") as error:
        loader.load_bars(path)

    assert str(error.value) == f"{column} 1.234 is finer than instrument precision 2"


@pytest.mark.parametrize("negative", [False, True])
def test_ohlcv_loader_reports_unrepresentable_volume(
    loader: CoinApiDataLoader,
    tmp_path: Path,
    negative: bool,
) -> None:
    """
    Test invalid volume errors propagate through the loader binding.
    """
    volume = Decimal("0.25").scaleb(-FIXED_PRECISION)
    if negative:
        volume = -volume
    source = format(volume, "f")
    header, row = (
        (FIXTURES / "ohlcv_bitstamp_1day_20250911.csv").read_text(encoding="utf-8").splitlines()[:2]
    )
    path = tmp_path / "TP-1DAY.csv"
    path.write_text(f"{header}\n{row.rsplit(';', 1)[0]};{source}\n", encoding="utf-8")
    loader.add_instrument(
        _currency_pair(InstrumentId.from_str("SOLUSD.BITSTAMP")),
        symbol_id_int=138492,
    )
    expected = (
        f"sx_sum must be non-negative: {source}"
        if negative
        else f"sx_sum {source} rounds to zero at precision {FIXED_PRECISION}"
    )

    with pytest.raises(ValueError, match=r"^sx_sum ") as exc_info:
        loader.load_bars(path)

    assert str(exc_info.value) == expected


def test_data_loader_uses_cached_instrument_precision_without_credentials() -> None:
    """
    Reuse registered precision for eager and streamed local data.
    """
    loader = CoinApiDataLoader()
    instrument = TestInstrumentProvider.btcusdt_binance()
    loader.add_instrument(instrument)
    path = FIXTURES / "trades_binance_spot_btc_usdt_20250901.csv"

    trades = loader.load_trades(path, instrument.id, limit=2)
    chunks = list(loader.stream_trades(path, instrument.id, chunk_size=1, limit=2))

    assert loader.instrument(instrument.id) == instrument
    assert len(trades) == 2
    assert trades == [record for chunk in chunks for record in chunk]
    assert trades[0].price.as_decimal() == Decimal("108246.36")
    assert trades[0].size.as_decimal() == Decimal("0.00023")
    assert trades[0].price.precision == instrument.price_precision
    assert trades[0].size.precision == instrument.size_precision


def test_data_loader_rejects_missing_metadata_before_opening_files() -> None:
    """
    Require definitions before opening a data file.
    """
    loader = CoinApiDataLoader()

    with pytest.raises(ValueError, match=r"No loaded instrument BTCUSDT\.BINANCE"):
        loader.load_trades("missing.csv", BINANCE)


def test_data_loader_book_stream_owns_definition_snapshot() -> None:
    """
    Retain the stream definition after the loader is dropped.
    """
    loader = CoinApiDataLoader()
    instrument = TestInstrumentProvider.btcusdt_binance()
    loader.add_instrument(instrument)
    path = FIXTURES / "limitbook_full_binance_spot_btc_usdt_20250901.csv"
    stream = loader.stream_book_deltas(path, instrument.id, date=BOOK_DATE, chunk_size=5)
    expected = loader.load_book_deltas(path, instrument.id, date=BOOK_DATE)
    del loader

    chunks = list(stream)

    assert [record for chunk in chunks for record in chunk] == expected
    assert all(RecordFlag.F_LAST.matches(chunk[-1].flags) for chunk in chunks)


def test_data_loader_is_the_public_ingestion_interface() -> None:
    """
    Keep provider ingestion on the stateful loader.
    """
    removed = {
        "CoinApiOhlcvLoader",
        "load_coinapi_deltas",
        "load_coinapi_quotes",
        "load_coinapi_trades",
        "stream_coinapi_deltas",
        "stream_coinapi_quotes",
        "stream_coinapi_trades",
    }

    assert not removed.intersection(vars(coinapi))
    assert "CoinApiDataLoader" in coinapi.__all__
