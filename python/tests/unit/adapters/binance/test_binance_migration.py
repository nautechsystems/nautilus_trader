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

import asyncio
import json
from collections.abc import Iterator
from contextlib import contextmanager
from decimal import Decimal
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from pathlib import Path
from threading import Thread

import pandas as pd
import pytest

import nautilus_trader.adapters.binance as binance
from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BINANCE_CLIENT_ID
from nautilus_trader.adapters.binance import BINANCE_VENUE
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceInstrumentProviderConfig
from nautilus_trader.adapters.binance import BinanceProductType
from nautilus_trader.adapters.binance import BinanceSpotMarketDataMode
from nautilus_trader.adapters.binance import decode_binance_futures_client_order_id
from nautilus_trader.adapters.binance import decode_binance_spot_client_order_id
from nautilus_trader.adapters.binance import load_binance_instruments
from nautilus_trader.adapters.binance import load_binance_order_book_deltas
from nautilus_trader.model import ClientId
from nautilus_trader.model import Venue


WORKSPACE_ROOT = Path(__file__).parents[5]


@contextmanager
def _serve_exchange_info(payload: bytes, path: str) -> Iterator[tuple[str, list[str]]]:
    requests: list[str] = []

    class ExchangeInfoHandler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            requests.append(self.path)
            if self.path != path:
                self.send_error(404)
                return

            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def log_message(self, format: str, *args: object) -> None:
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), ExchangeInfoHandler)
    thread = Thread(target=server.serve_forever)
    thread.start()
    host, port = server.server_address
    try:
        yield f"http://{host}:{port}", requests
    finally:
        server.shutdown()
        thread.join()
        server.server_close()


def test_binance_migration_exports_runtime_names() -> None:
    assert BINANCE == "BINANCE"
    assert ClientId.from_str("BINANCE") == BINANCE_CLIENT_ID
    assert Venue.from_str("BINANCE") == BINANCE_VENUE
    assert callable(load_binance_instruments)
    assert load_binance_order_book_deltas.__module__ == "nautilus_trader.adapters.binance"
    assert not hasattr(binance, "BinanceOrderBookDeltaDataLoader")


def test_binance_client_order_id_decoders_handle_encoded_and_external_ids() -> None:
    spot_encoded = "x-TD67BGP9-T0000000000000"
    futures_encoded = "x-aHRE4BCj-T0000000000000"
    external = "external-order-42"

    assert decode_binance_spot_client_order_id(spot_encoded) == "O-20200101-000000-000-000-0"
    assert decode_binance_futures_client_order_id(futures_encoded) == "O-20200101-000000-000-000-0"
    assert decode_binance_spot_client_order_id(external) == external
    assert decode_binance_futures_client_order_id(external) == external


def test_load_binance_order_book_deltas_maps_every_output_field(tmp_path: Path) -> None:
    csv_path = tmp_path / "depth.csv"
    csv_path.write_text(
        "symbol,timestamp,first_update_id,last_update_id,side,update_type,price,qty,pu\n"
        "BTCUSDT,1700000000123,101,110,b,snap,40123.5,1.25,100\n"
        "ETHUSDT,1700000001456,202,211,a,update,2345.75,2.50,201\n"
        "BNBUSDT,1700000002789,303,312,b,update,312.25,0.00,302\n",
    )

    result = load_binance_order_book_deltas(csv_path)

    assert result.index.tolist() == [
        pd.Timestamp("2023-11-14 22:13:20.123000+00:00"),
        pd.Timestamp("2023-11-14 22:13:21.456000+00:00"),
        pd.Timestamp("2023-11-14 22:13:22.789000+00:00"),
    ]
    assert result.to_dict(orient="records") == [
        {
            "instrument_id": "BTCUSDT.BINANCE",
            "action": "ADD",
            "side": "BUY",
            "price": 40123.5,
            "size": 1.25,
            "order_id": 0,
            "flags": 32,
            "sequence": 110,
        },
        {
            "instrument_id": "ETHUSDT.BINANCE",
            "action": "UPDATE",
            "side": "SELL",
            "price": 2345.75,
            "size": 2.5,
            "order_id": 0,
            "flags": 0,
            "sequence": 211,
        },
        {
            "instrument_id": "BNBUSDT.BINANCE",
            "action": "DELETE",
            "side": "BUY",
            "price": 312.25,
            "size": 0.0,
            "order_id": 0,
            "flags": 0,
            "sequence": 312,
        },
    ]


def test_load_binance_order_book_deltas_honors_nrows(tmp_path: Path) -> None:
    csv_path = tmp_path / "depth.csv"
    csv_path.write_text(
        "symbol,timestamp,first_update_id,last_update_id,side,update_type,price,qty,pu\n"
        "BTCUSDT,1700000000123,101,110,b,snap,40123.5,1.25,100\n"
        "ETHUSDT,1700000001456,202,211,a,update,2345.75,2.50,201\n",
    )

    result = load_binance_order_book_deltas(csv_path, nrows=1)

    assert result.to_dict(orient="records") == [
        {
            "instrument_id": "BTCUSDT.BINANCE",
            "action": "ADD",
            "side": "BUY",
            "price": 40123.5,
            "size": 1.25,
            "order_id": 0,
            "flags": 32,
            "sequence": 110,
        },
    ]


def test_load_binance_order_book_deltas_rejects_unknown_side(tmp_path: Path) -> None:
    csv_path = tmp_path / "depth.csv"
    csv_path.write_text(
        "symbol,timestamp,first_update_id,last_update_id,side,update_type,price,qty,pu\n"
        "BTCUSDT,1700000000123,101,110,x,update,40123.5,1.25,100\n",
    )

    with pytest.raises(RuntimeError, match="unrecognized side 'x'"):
        load_binance_order_book_deltas(csv_path)


def test_load_binance_order_book_deltas_rejects_missing_file(tmp_path: Path) -> None:
    csv_path = tmp_path / "missing.csv"

    with pytest.raises(RuntimeError, match="failed to open Binance order book CSV"):
        load_binance_order_book_deltas(csv_path)


def test_load_binance_order_book_deltas_rejects_malformed_row(tmp_path: Path) -> None:
    csv_path = tmp_path / "depth.csv"
    csv_path.write_text(
        "symbol,timestamp,first_update_id,last_update_id,side,update_type,price,qty,pu\n"
        "BTCUSDT,1700000000123,101,110,b,update,not-a-price,1.25,100\n",
    )

    with pytest.raises(RuntimeError, match="CSV deserialize error"):
        load_binance_order_book_deltas(csv_path)


def test_load_binance_instruments_uses_provider_config() -> None:
    payload = (
        WORKSPACE_ROOT
        / "crates/adapters/binance/test_data/futures/http_json/exchange_info_usdm.json"
    ).read_bytes()

    with _serve_exchange_info(payload, "/fapi/v1/exchangeInfo") as (base_url, requests):
        config = BinanceDataClientConfig(
            product_type=BinanceProductType.USD_M,
            base_url_http=base_url,
            instrument_provider=BinanceInstrumentProviderConfig(
                load_all=False,
                load_ids=["BTCUSDT-PERP.BINANCE"],
                log_warnings=False,
            ),
        )

        instruments = asyncio.run(load_binance_instruments(config))

    assert requests == ["/fapi/v1/exchangeInfo"]
    assert len(instruments) == 1
    instrument = instruments[0]
    assert str(instrument.id) == "BTCUSDT-PERP.BINANCE"
    assert str(instrument.raw_symbol) == "BTCUSDT"
    assert instrument.price_precision == 2
    assert instrument.size_precision == 3
    assert instrument.maker_fee == Decimal("0.000200")
    assert instrument.taker_fee == Decimal("0.000500")


def test_load_binance_spot_us_instruments_uses_public_json_without_credentials() -> None:
    payload = json.loads(
        (
            WORKSPACE_ROOT
            / "crates/adapters/binance/test_data/spot/http_json/exchange_info_response.json"
        ).read_text(),
    )
    payload["symbols"][0]["filters"] = [
        {
            "filterType": "PRICE_FILTER",
            "minPrice": "0.00000100",
            "maxPrice": "100000.00000000",
            "tickSize": "0.00000100",
        },
        {
            "filterType": "LOT_SIZE",
            "minQty": "0.00100000",
            "maxQty": "100000.00000000",
            "stepSize": "0.00100000",
        },
    ]

    with _serve_exchange_info(
        json.dumps(payload).encode(),
        "/api/v3/exchangeInfo",
    ) as (base_url, requests):
        config = BinanceDataClientConfig(
            product_type=BinanceProductType.SPOT,
            base_url_http=base_url,
            spot_market_data_mode=BinanceSpotMarketDataMode.Json,
            us=True,
            instrument_provider=BinanceInstrumentProviderConfig(
                load_all=False,
                load_ids=["ETHBTC.BINANCE"],
                log_warnings=False,
            ),
        )

        instruments = asyncio.run(load_binance_instruments(config))

    assert requests == ["/api/v3/exchangeInfo"]
    assert len(instruments) == 1
    instrument = instruments[0]
    assert str(instrument.id) == "ETHBTC.BINANCE"
    assert str(instrument.raw_symbol) == "ETHBTC"
    assert instrument.price_precision == 6
    assert instrument.size_precision == 3
    assert instrument.maker_fee == Decimal("0.001")
    assert instrument.taker_fee == Decimal("0.001")


def test_load_binance_instruments_rejects_unsupported_product() -> None:
    config = BinanceDataClientConfig(product_type=BinanceProductType.MARGIN)

    with pytest.raises(ValueError, match="supports Spot, UsdM, or CoinM, was Margin"):
        asyncio.run(load_binance_instruments(config))
