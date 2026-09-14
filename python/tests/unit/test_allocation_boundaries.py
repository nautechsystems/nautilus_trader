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
Test Python-controlled allocation boundaries.
"""

import subprocess
import sys
from pathlib import Path

import pytest

from nautilus_trader.adapters.bitmex import BitmexExecutionClientConfig
from nautilus_trader.adapters.bitmex import CancelBroadcaster
from nautilus_trader.adapters.bitmex import SubmitBroadcaster
from nautilus_trader.adapters.tardis import stream_tardis_batched_deltas
from nautilus_trader.adapters.tardis import stream_tardis_deltas
from nautilus_trader.adapters.tardis import stream_tardis_depth10_from_snapshot5
from nautilus_trader.adapters.tardis import stream_tardis_depth10_from_snapshot25
from nautilus_trader.adapters.tardis import stream_tardis_funding_rates
from nautilus_trader.adapters.tardis import stream_tardis_options_chain
from nautilus_trader.adapters.tardis import stream_tardis_quotes
from nautilus_trader.adapters.tardis import stream_tardis_trades
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import CacheConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BarType
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OmsType
from nautilus_trader.model import Quantity
from nautilus_trader.trading import HurstVpinDirectionalConfig
from tests.providers import TestInstrumentProvider


PRICE_LIST_TICKS_MAX = 100_000
TARDIS_CHUNK_SIZE_MAX = 1_000_000
BITMEX_POOL_SIZE_MAX = 16
BACKTEST_CHUNK_SIZE_MAX = 1_000_000
CACHE_DATA_CAPACITY_MAX = 1_000_000
HURST_VPIN_WINDOW_MAX = 16_384
USIZE_MAX = 2 * sys.maxsize + 1

TARDIS_STREAM_FUNCTIONS = (
    stream_tardis_deltas,
    stream_tardis_batched_deltas,
    stream_tardis_quotes,
    stream_tardis_options_chain,
    stream_tardis_trades,
    stream_tardis_depth10_from_snapshot5,
    stream_tardis_depth10_from_snapshot25,
    stream_tardis_funding_rates,
)


def _backtest_config(chunk_size: int) -> BacktestRunConfig:
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    return BacktestRunConfig(venues=[venue], data=[], chunk_size=chunk_size)


def _hurst_vpin_config(
    hurst_window: int = 128,
    vpin_window: int = 50,
) -> HurstVpinDirectionalConfig:
    return HurstVpinDirectionalConfig(
        instrument_id=InstrumentId.from_str("BTCUSDT.BINANCE"),
        bar_type=BarType.from_str("BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
        trade_size=Quantity.from_str("1"),
        hurst_window=hurst_window,
        vpin_window=vpin_window,
    )


@pytest.mark.parametrize("method_name", ["next_bid_prices", "next_ask_prices"])
def test_price_list_boundaries(method_name: str) -> None:
    """
    Test price list allocation boundaries.
    """
    instrument = TestInstrumentProvider.audusd_sim()
    method = getattr(instrument, method_name)

    assert method(1.0, 0) == []
    assert len(method(1.0, 100)) == 100
    assert len(method(1.0, PRICE_LIST_TICKS_MAX)) == PRICE_LIST_TICKS_MAX
    for value in (PRICE_LIST_TICKS_MAX + 1, sys.maxsize, USIZE_MAX):
        with pytest.raises(ValueError, match="num_ticks"):
            method(1.0, value)
    for value in (-1, USIZE_MAX + 1):
        with pytest.raises(OverflowError):
            method(1.0, value)


@pytest.mark.parametrize("stream_function", TARDIS_STREAM_FUNCTIONS)
def test_tardis_stream_rejects_invalid_chunk_sizes(stream_function: object) -> None:
    """
    Test every Tardis stream rejects invalid chunk sizes.
    """
    for value in (0, TARDIS_CHUNK_SIZE_MAX + 1, sys.maxsize, USIZE_MAX):
        with pytest.raises(ValueError, match="chunk_size"):
            stream_function("missing.csv", chunk_size=value)
    for value in (-1, USIZE_MAX + 1):
        with pytest.raises(OverflowError):
            stream_function("missing.csv", chunk_size=value)


@pytest.mark.parametrize("chunk_size", [1, 100_000, TARDIS_CHUNK_SIZE_MAX])
def test_tardis_quote_stream_accepts_supported_chunk_sizes(tmp_path: Path, chunk_size: int) -> None:
    """
    Test the Tardis quote stream accepts supported chunk sizes.
    """
    csv_path = tmp_path / "quotes.csv"
    csv_path.write_text(
        "exchange,symbol,timestamp,local_timestamp,ask_amount,ask_price,bid_price,bid_amount\n"
        "binance,BTCUSDT,1640995200000000,1640995200100000,1.0,50000.0,49999.0,1.5\n",
        encoding="utf-8",
    )

    stream = stream_tardis_quotes(csv_path, chunk_size=chunk_size)

    assert len(next(stream)) == 1


@pytest.mark.parametrize("broadcaster", [SubmitBroadcaster, CancelBroadcaster])
def test_bitmex_broadcaster_pool_boundaries(broadcaster: type) -> None:
    """
    Test direct BitMEX broadcaster pool boundaries.
    """
    minimum = broadcaster(1, api_key="test", api_secret="test")
    normal = broadcaster(8, api_key="test", api_secret="test")
    maximum = broadcaster(BITMEX_POOL_SIZE_MAX, api_key="test", api_secret="test")

    assert minimum.get_metrics()["total_clients"] == 1
    assert normal.get_metrics()["total_clients"] == 8
    assert maximum.get_metrics()["total_clients"] == BITMEX_POOL_SIZE_MAX
    for value in (0, BITMEX_POOL_SIZE_MAX + 1, sys.maxsize, USIZE_MAX):
        with pytest.raises(ValueError, match="pool_size"):
            broadcaster(value)
    for value in (-1, USIZE_MAX + 1):
        with pytest.raises(OverflowError):
            broadcaster(value)


def test_bitmex_execution_pool_boundaries() -> None:
    """
    Test BitMEX execution broadcaster pool boundaries.
    """
    for submitter_pool_size, canceller_pool_size in (
        (1, 1),
        (8, 8),
        (BITMEX_POOL_SIZE_MAX - 1, 1),
        (1, BITMEX_POOL_SIZE_MAX - 1),
    ):
        config = BitmexExecutionClientConfig(
            submitter_pool_size=submitter_pool_size,
            canceller_pool_size=canceller_pool_size,
        )
        assert config.submitter_pool_size == submitter_pool_size
        assert config.canceller_pool_size == canceller_pool_size
    for field in ("submitter_pool_size", "canceller_pool_size"):
        for value in (0, BITMEX_POOL_SIZE_MAX + 1, sys.maxsize, USIZE_MAX):
            with pytest.raises(ValueError, match=field):
                BitmexExecutionClientConfig(**{field: value})
        for value in (-1, USIZE_MAX + 1):
            with pytest.raises(OverflowError):
                BitmexExecutionClientConfig(**{field: value})
    with pytest.raises(ValueError, match="combined_pool_size"):
        BitmexExecutionClientConfig(
            submitter_pool_size=BITMEX_POOL_SIZE_MAX,
            canceller_pool_size=1,
        )


def test_backtest_chunk_size_boundaries() -> None:
    """
    Test backtest streaming chunk size boundaries.
    """
    assert _backtest_config(1).chunk_size == 1
    assert _backtest_config(100_000).chunk_size == 100_000
    assert _backtest_config(BACKTEST_CHUNK_SIZE_MAX).chunk_size == BACKTEST_CHUNK_SIZE_MAX
    for value in (0, BACKTEST_CHUNK_SIZE_MAX + 1, sys.maxsize, USIZE_MAX):
        with pytest.raises(ValueError, match="chunk_size"):
            _backtest_config(value)
    for value in (-1, USIZE_MAX + 1):
        with pytest.raises(OverflowError):
            _backtest_config(value)


@pytest.mark.parametrize("field", ["tick_capacity", "bar_capacity"])
def test_cache_capacity_boundaries(field: str) -> None:
    """
    Test cache tick and bar capacity boundaries.
    """
    assert getattr(CacheConfig(**{field: 1}), field) == 1
    assert getattr(CacheConfig(**{field: 200_000}), field) == 200_000
    assert (
        getattr(CacheConfig(**{field: CACHE_DATA_CAPACITY_MAX}), field) == CACHE_DATA_CAPACITY_MAX
    )

    for value in (0, CACHE_DATA_CAPACITY_MAX + 1, sys.maxsize, USIZE_MAX):
        with pytest.raises(ValueError, match=field):
            CacheConfig(**{field: value})
    for value in (-1, USIZE_MAX + 1):
        with pytest.raises(OverflowError):
            CacheConfig(**{field: value})


@pytest.mark.parametrize("field", ["hurst_window", "vpin_window"])
def test_hurst_vpin_window_boundaries(field: str) -> None:
    """
    Test Hurst and VPIN rolling window boundaries.
    """
    assert getattr(_hurst_vpin_config(**{field: 1}), field) == 1
    assert getattr(_hurst_vpin_config(**{field: 128}), field) == 128
    assert (
        getattr(_hurst_vpin_config(**{field: HURST_VPIN_WINDOW_MAX}), field)
        == HURST_VPIN_WINDOW_MAX
    )

    for value in (0, HURST_VPIN_WINDOW_MAX + 1, sys.maxsize, USIZE_MAX):
        with pytest.raises(ValueError, match=field):
            _hurst_vpin_config(**{field: value})
    for value in (-1, USIZE_MAX + 1):
        with pytest.raises(OverflowError):
            _hurst_vpin_config(**{field: value})


def test_invalid_allocation_sizes_do_not_abort_subprocess() -> None:
    """
    Test invalid allocation sizes do not abort a release subprocess.
    """
    code = """
import sys
from nautilus_trader.adapters.bitmex import BitmexExecutionClientConfig
from nautilus_trader.adapters.tardis import stream_tardis_quotes
from nautilus_trader.config import BacktestRunConfig, CacheConfig
from nautilus_trader.model import BarType, InstrumentId, Quantity
from nautilus_trader.testkit.providers import TestInstrumentProvider
from nautilus_trader.trading import HurstVpinDirectionalConfig

calls = (
    lambda: TestInstrumentProvider.audusd_sim().next_bid_prices(1.0, sys.maxsize),
    lambda: stream_tardis_quotes('missing.csv', chunk_size=sys.maxsize),
    lambda: BitmexExecutionClientConfig(submitter_pool_size=sys.maxsize),
    lambda: BacktestRunConfig(venues=[], data=[], chunk_size=sys.maxsize),
    lambda: CacheConfig(tick_capacity=sys.maxsize),
    lambda: HurstVpinDirectionalConfig(
        instrument_id=InstrumentId.from_str('BTCUSDT.BINANCE'),
        bar_type=BarType.from_str('BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL'),
        trade_size=Quantity.from_str('1'),
        hurst_window=sys.maxsize,
    ),
)

for call in calls:
    try:
        call()
    except ValueError:
        pass
    else:
        raise AssertionError('expected ValueError')
print('allocation boundaries passed')
"""
    result = subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )

    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "allocation boundaries passed"
    assert result.stderr == ""
