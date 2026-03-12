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
Databento market data integration adapter.

This subpackage provides a data client factory, instrument provider,
constants, configurations, and data loaders for connecting to and
interacting with the Databento API, and decoding Databento Binary
Encoding (DBN) format data.

For convenience, the most commonly used symbols are re-exported at the
subpackage's top level, so downstream code can simply import from
``nautilus_trader.adapters.databento``.

"""

from io import BytesIO

import pyarrow as pa

from nautilus_trader.adapters.databento.config import DatabentoDataClientConfig
from nautilus_trader.adapters.databento.constants import ALL_SYMBOLS
from nautilus_trader.adapters.databento.constants import DATABENTO
from nautilus_trader.adapters.databento.constants import DATABENTO_CLIENT_ID
from nautilus_trader.adapters.databento.factories import DatabentoLiveDataClientFactory
from nautilus_trader.adapters.databento.factories import get_cached_databento_http_client
from nautilus_trader.adapters.databento.factories import get_cached_databento_instrument_provider
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.adapters.databento.providers import DatabentoInstrumentProvider
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import DatabentoImbalance
from nautilus_trader.core.nautilus_pyo3 import DatabentoStatistics
from nautilus_trader.serialization.arrow.serializer import register_arrow


def _make_databento_encoder(encode_fn):
    def encoder(data):
        if not isinstance(data, list):
            data = [data]
        batch_bytes = encode_fn(data)
        reader = pa.ipc.open_stream(BytesIO(batch_bytes))
        table = reader.read_all()
        return table.to_batches()[0]

    return encoder


def _make_databento_decoder(decode_fn):
    def decoder(table):
        if isinstance(table, pa.RecordBatch):
            table = pa.Table.from_batches([table])
        sink = pa.BufferOutputStream()
        writer = pa.ipc.new_stream(sink, table.schema)
        for batch in table.to_batches():
            writer.write_batch(batch)
        writer.close()
        ipc_bytes = sink.getvalue().to_pybytes()
        return decode_fn(ipc_bytes)

    return decoder


_PRECISION_BINARY = pa.binary(nautilus_pyo3.PRECISION_BYTES)

_IMBALANCE_SCHEMA = pa.schema(
    [
        pa.field("ref_price", _PRECISION_BINARY, False),
        pa.field("cont_book_clr_price", _PRECISION_BINARY, False),
        pa.field("auct_interest_clr_price", _PRECISION_BINARY, False),
        pa.field("paired_qty", _PRECISION_BINARY, False),
        pa.field("total_imbalance_qty", _PRECISION_BINARY, False),
        pa.field("side", pa.uint8(), False),
        pa.field("significant_imbalance", pa.int8(), False),
        pa.field("ts_event", pa.uint64(), False),
        pa.field("ts_recv", pa.uint64(), False),
        pa.field("ts_init", pa.uint64(), False),
    ],
)

_STATISTICS_SCHEMA = pa.schema(
    [
        pa.field("stat_type", pa.uint8(), False),
        pa.field("update_action", pa.uint8(), False),
        pa.field("price", _PRECISION_BINARY, False),
        pa.field("quantity", _PRECISION_BINARY, False),
        pa.field("channel_id", pa.uint16(), False),
        pa.field("stat_flags", pa.uint8(), False),
        pa.field("sequence", pa.uint32(), False),
        pa.field("ts_ref", pa.uint64(), False),
        pa.field("ts_in_delta", pa.int32(), False),
        pa.field("ts_event", pa.uint64(), False),
        pa.field("ts_recv", pa.uint64(), False),
        pa.field("ts_init", pa.uint64(), False),
    ],
)

_imbalance_encoder = _make_databento_encoder(
    nautilus_pyo3.databento_imbalance_to_arrow_record_batch_bytes,
)
_statistics_encoder = _make_databento_encoder(
    nautilus_pyo3.databento_statistics_to_arrow_record_batch_bytes,
)

register_arrow(
    DatabentoImbalance,
    _IMBALANCE_SCHEMA,
    encoder=_imbalance_encoder,
    decoder=_make_databento_decoder(
        nautilus_pyo3.databento_imbalance_from_arrow_record_batch_bytes,
    ),
    batch_encoder=_imbalance_encoder,
)

register_arrow(
    DatabentoStatistics,
    _STATISTICS_SCHEMA,
    encoder=_statistics_encoder,
    decoder=_make_databento_decoder(
        nautilus_pyo3.databento_statistics_from_arrow_record_batch_bytes,
    ),
    batch_encoder=_statistics_encoder,
)


__all__ = [
    "ALL_SYMBOLS",
    "DATABENTO",
    "DATABENTO_CLIENT_ID",
    "DatabentoDataClientConfig",
    "DatabentoDataLoader",
    "DatabentoImbalance",
    "DatabentoInstrumentProvider",
    "DatabentoLiveDataClientFactory",
    "DatabentoStatistics",
    "get_cached_databento_http_client",
    "get_cached_databento_instrument_provider",
]
