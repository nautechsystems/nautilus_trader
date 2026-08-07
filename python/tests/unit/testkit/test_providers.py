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

from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.testkit.providers import TestInstrumentProvider


def test_xbtusd_bitmex_matches_rust_fixture() -> None:
    # Mirrors the `xbtusd_bitmex` fixture in `crates/model/src/instruments/stubs.rs`
    instrument = TestInstrumentProvider.xbtusd_bitmex()

    assert instrument.id == InstrumentId.from_str("BTCUSDT.BITMEX")
    assert instrument.raw_symbol == Symbol("XBTUSD")
    assert instrument.base_currency == Currency.from_str("BTC")
    assert instrument.quote_currency == Currency.from_str("USD")
    assert instrument.settlement_currency == Currency.from_str("BTC")
    assert instrument.is_inverse
    assert instrument.price_precision == 1
    assert instrument.size_precision == 0
    assert instrument.price_increment == Price.from_str("0.5")
    assert instrument.size_increment == Quantity.from_str("1")
    assert instrument.max_price == Price.from_str("10000000")
    assert instrument.min_price == Price.from_str("0.01")
    assert instrument.margin_init == Decimal("0.01")
    assert instrument.margin_maint == Decimal("0.0035")
    assert instrument.maker_fee == Decimal("-0.00025")
    assert instrument.taker_fee == Decimal("0.00075")
