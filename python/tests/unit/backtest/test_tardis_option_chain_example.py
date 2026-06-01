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

import importlib.util
from pathlib import Path
from types import ModuleType

import pytest


pytest.importorskip("nautilus_trader._libnautilus")
_model = pytest.importorskip("nautilus_trader.model")
InstrumentId = _model.InstrumentId
Price = _model.Price


def load_example() -> ModuleType:
    example_path = (
        Path(__file__).resolve().parents[4] / "examples" / "backtest" / "tardis_option_chain.py"
    )
    spec = importlib.util.spec_from_file_location("tardis_option_chain_example", example_path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def option_metadata(
    module: ModuleType,
    raw_symbol: str,
    underlying: str,
    settlement_currency: str,
    expiration_ns: int,
    strike: str,
):
    return module.OptionMetadata(
        instrument_id=InstrumentId.from_str(f"{raw_symbol}.DERIBIT"),
        underlying=underlying,
        settlement_currency=settlement_currency,
        expiration_ns=expiration_ns,
        strike=Price.from_str(strike),
    )


def mixed_series_options(module: ModuleType):
    nearest_expiry_ns = 1_704_067_200_000_000_000
    later_expiry_ns = 1_706_745_600_000_000_000
    return [
        option_metadata(
            module,
            "BTC-20240101-45000-C",
            "BTC",
            "BTC",
            nearest_expiry_ns,
            "45000",
        ),
        option_metadata(
            module,
            "BTC-20240101-50000-P",
            "BTC",
            "BTC",
            nearest_expiry_ns,
            "50000",
        ),
        option_metadata(
            module,
            "BTC-20240101-1-C",
            "BTC",
            "USDC",
            nearest_expiry_ns,
            "1",
        ),
        option_metadata(
            module,
            "BTC-20240201-2-C",
            "BTC",
            "BTC",
            later_expiry_ns,
            "2",
        ),
    ]


def test_example_imports() -> None:
    module = load_example()

    assert module.OptionChainBacktest.__name__ == "OptionChainBacktest"
    assert module.OptionChainBacktestConfig.__name__ == "OptionChainBacktestConfig"


def test_nearest_series_filters_strikes_to_selected_series() -> None:
    module = load_example()
    options = mixed_series_options(module)

    selection = module.nearest_series(options)

    assert selection.settlement_currency == "BTC"
    assert selection.instrument_ids == [
        InstrumentId.from_str("BTC-20240101-45000-C.DERIBIT"),
        InstrumentId.from_str("BTC-20240101-50000-P.DERIBIT"),
    ]
    assert selection.strikes == [Price.from_str("45000"), Price.from_str("50000")]


def test_default_strike_uses_selected_series_strikes() -> None:
    module = load_example()
    selection = module.nearest_series(mixed_series_options(module))

    assert module.median_strike(selection.strikes) == Price.from_str("50000")
    assert Price.from_str("1") not in selection.strikes
    assert Price.from_str("2") not in selection.strikes
