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
Generate high-precision catalog fixtures with NautilusTrader 1.231.0.
"""

import json
import sys
from decimal import Decimal
from pathlib import Path

import pyarrow.parquet as pq

from nautilus_trader import __version__
from nautilus_trader.adapters.binance.futures.types import BinanceFuturesMarkPriceUpdate
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog


PRECISION_BYTES = 16

assert __version__ == "1.231.0"
root = Path(sys.argv[1])
root.mkdir(parents=True, exist_ok=True)
catalog = ParquetDataCatalog(str(root))
instrument_id = InstrumentId.from_str("AUD/USD.SIM")
ts = 1_700_000_000_000_000_123
quotes = [
    QuoteTick(
        instrument_id,
        Price.from_str("1.23456"),
        Price.from_str("1.23478"),
        Quantity.from_str("123.000001"),
        Quantity.from_str("456.000002"),
        ts,
        ts + 1,
    ),
    QuoteTick(
        instrument_id,
        Price.from_str("1.34567"),
        Price.from_str("1.34589"),
        Quantity.from_str("789.000003"),
        Quantity.from_str("987.000004"),
        ts + 2,
        ts + 3,
    ),
]
instrument = CurrencyPair(
    instrument_id=instrument_id,
    raw_symbol=Symbol("AUD/USD"),
    base_currency=AUD,
    quote_currency=USD,
    price_precision=5,
    size_precision=0,
    price_increment=Price.from_str("0.00001"),
    size_increment=Quantity.from_str("1"),
    lot_size=Quantity.from_str("1000"),
    max_quantity=Quantity.from_str("10000000"),
    min_quantity=Quantity.from_str("1000"),
    max_notional=Money.from_str("50000000.00 USD"),
    min_notional=Money.from_str("1000.00 USD"),
    margin_init=Decimal("0.03"),
    margin_maint=Decimal("0.03"),
    maker_fee=Decimal("0.00002"),
    taker_fee=Decimal("0.00002"),
    tick_scheme_name="FOREX_5DECIMAL",
    ts_event=ts + 10,
    ts_init=ts + 11,
)
custom = BinanceFuturesMarkPriceUpdate(
    instrument_id,
    Price.from_str("1.45678"),
    Price.from_str("1.56789"),
    Price.from_str("1.67890"),
    Decimal("0.00012345"),
    ts + 100,
    ts + 20,
    ts + 21,
)
catalog.write_data(quotes)
quote_file = next((root / "data" / "quote_tick").rglob("*.parquet"))
precision_bytes = pq.read_schema(quote_file).field("bid_price").type.byte_width
assert precision_bytes == PRECISION_BYTES
catalog.write_data([instrument])
catalog.write_data([custom])
catalog.write_data([], start=ts - 100, end=ts, data_cls=QuoteTick, identifier=str(instrument_id))
expected = {
    "quotes": [QuoteTick.to_dict(q) for q in quotes],
    "instrument": CurrencyPair.to_dict(catalog.instruments()[0]),
    "custom": BinanceFuturesMarkPriceUpdate.to_dict(custom),
}
(root / "expected.json").write_text(json.dumps(expected, indent=2) + "\n")
print(
    precision_bytes,
    sorted(str(f.relative_to(root)) for f in root.rglob("*.parquet")),
)
