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

import datetime as dt

import msgspec

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.instruments import CryptoOption
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Instrument


def create_instrument_info(instrument: Instrument) -> nautilus_pyo3.InstrumentMiniInfo:
    return nautilus_pyo3.InstrumentMiniInfo(
        instrument_id=nautilus_pyo3.InstrumentId.from_str(instrument.id.value),
        raw_symbol=instrument.raw_symbol.value,
        exchange=infer_tardis_exchange_str(instrument),
        price_precision=instrument.price_precision,
        size_precision=instrument.size_precision,
    )


def infer_tardis_exchange_str(instrument: Instrument) -> str:  # noqa: C901 (too complex)
    venue = instrument.venue.value

    match venue:
        case "BINANCE":
            if isinstance(instrument, CurrencyPair):
                return "binance"
            elif isinstance(instrument, CryptoOption):
                return "binance-options"
            else:
                return "binance-futures"
        case "BINANCE_US":
            return "binance-us"
        case "BINANCE_DELIVERY":
            return "binance-delivery"
        case "BITFINEX":
            if isinstance(instrument, CurrencyPair):
                return "bitfinex"
            else:
                return "bitfinex-derivatives"
        case "BYBIT":
            if isinstance(instrument, CurrencyPair):
                return "bybit-spot"
            elif isinstance(instrument, CryptoOption):
                return "bybit-options"
            else:
                return "bybit"
        case "CRYPTO_COM":
            if isinstance(instrument, CurrencyPair):
                return "crypto-com"
        case "GATE_IO":
            if isinstance(instrument, CurrencyPair):
                return "gate-io"
            else:
                return "gate-io-futures"
        case "HUOBI":
            if isinstance(instrument, CurrencyPair):
                return "huobi"
            elif isinstance(instrument, CryptoPerpetual):
                return "huobi-dm-linear-swap"
            elif isinstance(instrument, CryptoFuture):
                return "huobi-dm"
            elif isinstance(instrument, CryptoOption):
                return "huobi-dm-options"
        case "HUOBI_DELIVERY":
            return "huobi-dm-swap"
        case "KRAKEN":
            if isinstance(instrument, CurrencyPair):
                return "kraken"
            else:
                return "kraken-futures"
        case "OKEX":
            if isinstance(instrument, CurrencyPair):
                return "okex"
            elif isinstance(instrument, CryptoPerpetual):
                return "okex-swap"
            elif isinstance(instrument, CryptoFuture):
                return "okex-futures"
            elif isinstance(instrument, CryptoOption):
                return "okex-options"

    return venue.lower().replace("_", "-")


def get_ws_client_key(instrument_id: InstrumentId, tardis_data_type: str) -> str:
    return f"{instrument_id}-{tardis_data_type}"


def convert_nautilus_data_type_to_tardis_data_type(data_type: type) -> str:
    if data_type is OrderBookDelta:
        return "book_change"
    elif data_type is OrderBookDepth10:
        return "book_snapshot"
    elif data_type is QuoteTick:
        return "quote"
    elif data_type is TradeTick:
        return "trade"
    else:
        raise ValueError(f"Invalid `data_type` to convert, was {data_type}")


def convert_nautilus_bar_type_to_tardis_data_type(bar_type: BarType) -> str:
    bar_type_pyo3 = nautilus_pyo3.BarType.from_str(str(bar_type))
    return nautilus_pyo3.bar_spec_to_tardis_trade_bar_string(bar_type_pyo3.spec)


def create_replay_normalized_request_options(
    exchange: str,
    symbols: list[str],
    from_date: dt.date,
    to_date: dt.date,
    data_types: list[str],
) -> nautilus_pyo3.ReplayNormalizedRequestOptions:
    PyCondition.not_empty(symbols, "symbols")
    PyCondition.not_empty(data_types, "data_types")

    options = {
        "exchange": exchange,
        "symbols": symbols,
        "from": from_date.isoformat(),
        "to": to_date.isoformat(),
        "data_types": data_types,
        "with_disconnect_messages": True,
    }

    json_options = msgspec.json.encode(options)
    return nautilus_pyo3.ReplayNormalizedRequestOptions.from_json(json_options)


def create_stream_normalized_request_options(
    exchange: str,
    symbols: list[str],
    data_types: list[str],
) -> nautilus_pyo3.StreamNormalizedRequestOptions:
    PyCondition.not_empty(symbols, "symbols")
    PyCondition.not_empty(data_types, "data_types")

    options = {
        "exchange": exchange,
        "symbols": symbols,
        "data_types": data_types,
        "with_disconnect_messages": True,
    }

    json_options = msgspec.json.encode(options)
    return nautilus_pyo3.StreamNormalizedRequestOptions.from_json(json_options)
