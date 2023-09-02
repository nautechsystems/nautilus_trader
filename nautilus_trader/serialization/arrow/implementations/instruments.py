# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import pyarrow as pa

from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import OptionsContract


SCHEMAS = {
    BettingInstrument: pa.schema(
        {
            "venue_name": pa.string(),
            "currency": pa.string(),
            "id": pa.string(),
            "event_type_id": pa.string(),
            "event_type_name": pa.string(),
            "competition_id": pa.string(),
            "competition_name": pa.string(),
            "event_id": pa.string(),
            "event_name": pa.string(),
            "event_country_code": pa.string(),
            "event_open_date": pa.string(),
            "betting_type": pa.string(),
            "market_id": pa.string(),
            "market_name": pa.string(),
            "market_start_time": pa.string(),
            "market_type": pa.string(),
            "selection_id": pa.string(),
            "selection_name": pa.string(),
            "selection_handicap": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={"type": "BettingInstrument"},
    ),
    CurrencyPair: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "base_currency": pa.dictionary(pa.int16(), pa.string()),
            "quote_currency": pa.dictionary(pa.int16(), pa.string()),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "lot_size": pa.dictionary(pa.int16(), pa.string()),
            "max_quantity": pa.dictionary(pa.int16(), pa.string()),
            "min_quantity": pa.dictionary(pa.int16(), pa.string()),
            "max_notional": pa.dictionary(pa.int16(), pa.string()),
            "min_notional": pa.dictionary(pa.int16(), pa.string()),
            "max_price": pa.dictionary(pa.int16(), pa.string()),
            "min_price": pa.dictionary(pa.int16(), pa.string()),
            "margin_init": pa.string(),
            "margin_maint": pa.string(),
            "maker_fee": pa.string(),
            "taker_fee": pa.string(),
            "info": pa.binary(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    CryptoPerpetual: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "base_currency": pa.dictionary(pa.int16(), pa.string()),
            "quote_currency": pa.dictionary(pa.int16(), pa.string()),
            "settlement_currency": pa.dictionary(pa.int16(), pa.string()),
            "is_inverse": pa.bool_(),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "max_quantity": pa.dictionary(pa.int16(), pa.string()),
            "min_quantity": pa.dictionary(pa.int16(), pa.string()),
            "max_notional": pa.dictionary(pa.int16(), pa.string()),
            "min_notional": pa.dictionary(pa.int16(), pa.string()),
            "max_price": pa.dictionary(pa.int16(), pa.string()),
            "min_price": pa.dictionary(pa.int16(), pa.string()),
            "margin_init": pa.string(),
            "margin_maint": pa.string(),
            "maker_fee": pa.string(),
            "taker_fee": pa.string(),
            "info": pa.binary(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    CryptoFuture: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "underlying": pa.dictionary(pa.int16(), pa.string()),
            "quote_currency": pa.dictionary(pa.int16(), pa.string()),
            "settlement_currency": pa.dictionary(pa.int16(), pa.string()),
            "expiry_date": pa.dictionary(pa.int16(), pa.string()),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "max_quantity": pa.dictionary(pa.int16(), pa.string()),
            "min_quantity": pa.dictionary(pa.int16(), pa.string()),
            "max_notional": pa.dictionary(pa.int16(), pa.string()),
            "min_notional": pa.dictionary(pa.int16(), pa.string()),
            "max_price": pa.dictionary(pa.int16(), pa.string()),
            "min_price": pa.dictionary(pa.int16(), pa.string()),
            "margin_init": pa.string(),
            "margin_maint": pa.string(),
            "maker_fee": pa.string(),
            "taker_fee": pa.string(),
            "info": pa.binary(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    Equity: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "currency": pa.dictionary(pa.int16(), pa.string()),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "multiplier": pa.dictionary(pa.int16(), pa.string()),
            "lot_size": pa.dictionary(pa.int16(), pa.string()),
            "isin": pa.string(),
            "margin_init": pa.string(),
            "margin_maint": pa.string(),
            "maker_fee": pa.string(),
            "taker_fee": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    FuturesContract: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "underlying": pa.dictionary(pa.int16(), pa.string()),
            "asset_class": pa.dictionary(pa.int8(), pa.string()),
            "currency": pa.dictionary(pa.int16(), pa.string()),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "multiplier": pa.dictionary(pa.int16(), pa.string()),
            "lot_size": pa.dictionary(pa.int16(), pa.string()),
            "expiry_date": pa.dictionary(pa.int16(), pa.string()),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    OptionsContract: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "underlying": pa.dictionary(pa.int16(), pa.string()),
            "asset_class": pa.dictionary(pa.int8(), pa.string()),
            "currency": pa.dictionary(pa.int16(), pa.string()),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "multiplier": pa.dictionary(pa.int16(), pa.string()),
            "lot_size": pa.dictionary(pa.int16(), pa.string()),
            "expiry_date": pa.dictionary(pa.int64(), pa.string()),
            "strike_price": pa.dictionary(pa.int64(), pa.string()),
            "kind": pa.dictionary(pa.int8(), pa.string()),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
}


def serialize(obj) -> pa.RecordBatch:
    data = obj.to_dict(obj)
    schema = SCHEMAS[obj.__class__].with_metadata({"class": obj.__class__.__name__})
    return pa.RecordBatch.from_pylist([data], schema)


def deserialize(batch: pa.RecordBatch) -> list[Instrument]:
    ins_type = batch.schema.metadata.get(b"type") or batch.schema.metadata[b"class"]
    Cls = {
        b"BettingInstrument": BettingInstrument,
        b"CurrencyPair": CurrencyPair,
        b"CryptoPerpetual": CryptoPerpetual,
        b"CryptoFuture": CryptoFuture,
        b"Equity": Equity,
        b"FuturesContract": FuturesContract,
        b"OptionsContract": OptionsContract,
    }[ins_type]
    return [Cls.from_dict(data) for data in batch.to_pylist()]
