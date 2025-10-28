# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec
import pyarrow as pa

from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.instruments import Cfd
from nautilus_trader.model.instruments import Commodity
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.instruments import CryptoOption
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import FuturesSpread
from nautilus_trader.model.instruments import IndexInstrument
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.instruments import OptionSpread


SCHEMAS = {
    BettingInstrument: pa.schema(
        {
            "id": pa.string(),
            "venue_name": pa.string(),
            "currency": pa.string(),
            "event_type_id": pa.int64(),
            "event_type_name": pa.string(),
            "competition_id": pa.int64(),
            "competition_name": pa.string(),
            "event_id": pa.int64(),
            "event_name": pa.string(),
            "event_country_code": pa.string(),
            "event_open_date": pa.uint64(),
            "betting_type": pa.string(),
            "market_id": pa.string(),
            "market_name": pa.string(),
            "market_type": pa.string(),
            "market_start_time": pa.uint64(),
            "selection_id": pa.int64(),
            "selection_name": pa.string(),
            "selection_handicap": pa.float64(),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={"type": "BettingInstrument"},
    ),
    BinaryOption: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "asset_class": pa.dictionary(pa.int8(), pa.string()),
            "currency": pa.dictionary(pa.int16(), pa.string()),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "activation_ns": pa.uint64(),
            "expiration_ns": pa.uint64(),
            "maker_fee": pa.string(),
            "taker_fee": pa.string(),
            "max_quantity": pa.dictionary(pa.int16(), pa.string()),
            "min_quantity": pa.dictionary(pa.int16(), pa.string()),
            "outcome": pa.string(),
            "description": pa.string(),
            "info": pa.binary(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={"type": "BinaryOption"},
    ),
    Cfd: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "asset_class": pa.dictionary(pa.int8(), pa.string()),
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
            "multiplier": pa.dictionary(pa.int16(), pa.string()),
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
    CryptoFuture: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "underlying": pa.dictionary(pa.int16(), pa.string()),
            "quote_currency": pa.dictionary(pa.int16(), pa.string()),
            "settlement_currency": pa.dictionary(pa.int16(), pa.string()),
            "is_inverse": pa.bool_(),
            "activation_ns": pa.uint64(),
            "expiration_ns": pa.uint64(),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "multiplier": pa.dictionary(pa.int16(), pa.string()),
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
    CryptoOption: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "underlying": pa.dictionary(pa.int16(), pa.string()),
            "quote_currency": pa.dictionary(pa.int16(), pa.string()),
            "settlement_currency": pa.dictionary(pa.int16(), pa.string()),
            "is_inverse": pa.bool_(),
            "option_kind": pa.dictionary(pa.int8(), pa.string()),
            "strike_price": pa.string(),
            "activation_ns": pa.uint64(),
            "expiration_ns": pa.uint64(),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "multiplier": pa.dictionary(pa.int16(), pa.string()),
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
            "multiplier": pa.dictionary(pa.int16(), pa.string()),
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
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "lot_size": pa.dictionary(pa.int16(), pa.string()),
            "isin": pa.string(),
            "margin_init": pa.string(),
            "margin_maint": pa.string(),
            "maker_fee": pa.string(),
            "taker_fee": pa.string(),
            "info": pa.binary(),
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
            "exchange": pa.dictionary(pa.int16(), pa.string()),
            "currency": pa.dictionary(pa.int16(), pa.string()),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "multiplier": pa.dictionary(pa.int16(), pa.string()),
            "lot_size": pa.dictionary(pa.int16(), pa.string()),
            "activation_ns": pa.uint64(),
            "expiration_ns": pa.uint64(),
            "margin_init": pa.string(),
            "margin_maint": pa.string(),
            "maker_fee": pa.string(),
            "taker_fee": pa.string(),
            "info": pa.binary(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    FuturesSpread: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "underlying": pa.dictionary(pa.int16(), pa.string()),
            "strategy_type": pa.dictionary(pa.int16(), pa.string()),
            "asset_class": pa.dictionary(pa.int8(), pa.string()),
            "exchange": pa.dictionary(pa.int16(), pa.string()),
            "currency": pa.dictionary(pa.int16(), pa.string()),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "multiplier": pa.dictionary(pa.int16(), pa.string()),
            "lot_size": pa.dictionary(pa.int16(), pa.string()),
            "max_quantity": pa.dictionary(pa.int16(), pa.string()),
            "min_quantity": pa.dictionary(pa.int16(), pa.string()),
            "max_price": pa.dictionary(pa.int16(), pa.string()),
            "min_price": pa.dictionary(pa.int16(), pa.string()),
            "activation_ns": pa.uint64(),
            "expiration_ns": pa.uint64(),
            "margin_init": pa.string(),
            "margin_maint": pa.string(),
            "maker_fee": pa.string(),
            "taker_fee": pa.string(),
            "info": pa.binary(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    OptionContract: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "underlying": pa.dictionary(pa.int16(), pa.string()),
            "asset_class": pa.dictionary(pa.int8(), pa.string()),
            "exchange": pa.dictionary(pa.int16(), pa.string()),
            "option_kind": pa.dictionary(pa.int8(), pa.string()),
            "strike_price": pa.dictionary(pa.int64(), pa.string()),
            "currency": pa.dictionary(pa.int16(), pa.string()),
            "activation_ns": pa.uint64(),
            "expiration_ns": pa.uint64(),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "multiplier": pa.dictionary(pa.int16(), pa.string()),
            "lot_size": pa.dictionary(pa.int16(), pa.string()),
            "margin_init": pa.string(),
            "margin_maint": pa.string(),
            "maker_fee": pa.string(),
            "taker_fee": pa.string(),
            "info": pa.binary(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    OptionSpread: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "underlying": pa.dictionary(pa.int16(), pa.string()),
            "strategy_type": pa.dictionary(pa.int16(), pa.string()),
            "asset_class": pa.dictionary(pa.int8(), pa.string()),
            "exchange": pa.dictionary(pa.int16(), pa.string()),
            "currency": pa.dictionary(pa.int16(), pa.string()),
            "price_precision": pa.uint8(),
            "size_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "multiplier": pa.dictionary(pa.int16(), pa.string()),
            "lot_size": pa.dictionary(pa.int16(), pa.string()),
            "max_quantity": pa.dictionary(pa.int16(), pa.string()),
            "min_quantity": pa.dictionary(pa.int16(), pa.string()),
            "max_price": pa.dictionary(pa.int16(), pa.string()),
            "min_price": pa.dictionary(pa.int16(), pa.string()),
            "activation_ns": pa.uint64(),
            "expiration_ns": pa.uint64(),
            "margin_init": pa.string(),
            "margin_maint": pa.string(),
            "maker_fee": pa.string(),
            "taker_fee": pa.string(),
            "info": pa.binary(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    Commodity: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "asset_class": pa.dictionary(pa.int8(), pa.string()),
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
    IndexInstrument: pa.schema(
        {
            "id": pa.dictionary(pa.int64(), pa.string()),
            "raw_symbol": pa.string(),
            "currency": pa.dictionary(pa.int16(), pa.string()),
            "price_precision": pa.uint8(),
            "price_increment": pa.dictionary(pa.int16(), pa.string()),
            "size_precision": pa.uint8(),
            "size_increment": pa.dictionary(pa.int16(), pa.string()),
            "info": pa.binary(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
}


def serialize(obj: Instrument) -> pa.RecordBatch:
    data = obj.to_dict(obj)
    if "info" in data:
        data["info"] = msgspec.json.encode(data["info"])
    schema = SCHEMAS[obj.__class__].with_metadata({"class": obj.__class__.__name__})
    return pa.RecordBatch.from_pylist([data], schema)


def deserialize(batch: pa.RecordBatch) -> list[Instrument]:
    ins_type = batch.schema.metadata.get(b"type") or batch.schema.metadata[b"class"]
    Cls = {
        b"BettingInstrument": BettingInstrument,
        b"BinaryOption": BinaryOption,
        b"Cfd": Cfd,
        b"Commodity": Commodity,
        b"CurrencyPair": CurrencyPair,
        b"CryptoPerpetual": CryptoPerpetual,
        b"CryptoFuture": CryptoFuture,
        b"CryptoOption": CryptoOption,
        b"Equity": Equity,
        b"FuturesContract": FuturesContract,
        b"FuturesSpread": FuturesSpread,
        b"IndexInstrument": IndexInstrument,
        b"OptionContract": OptionContract,
        b"OptionSpread": OptionSpread,
    }[ins_type]

    maps = batch.to_pylist()
    for m in maps:
        info = m.get("info")
        if info is not None:
            m["info"] = msgspec.json.decode(info)
        else:
            m["info"] = None

    return [Cls.from_dict(data) for data in maps]
