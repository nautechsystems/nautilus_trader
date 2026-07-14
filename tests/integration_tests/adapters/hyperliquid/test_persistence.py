# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# -------------------------------------------------------------------------------------------------

import pyarrow.parquet as pq

from nautilus_trader.adapters.hyperliquid.data import HyperliquidPublicTrade
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import TestClock
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.funcs import class_to_filename
from nautilus_trader.persistence.writer import StreamingFeatherWriter
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def test_public_trade_streaming_feather_converts_to_catalog_parquet(tmp_path) -> None:
    # Arrange
    catalog = ParquetDataCatalog(str(tmp_path))
    cache = Cache()
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    cache.add_instrument(instrument)
    instance_id = "hyperliquid-public-trades"
    writer = StreamingFeatherWriter(
        path=f"{catalog.path}/backtest/{instance_id}",
        cache=cache,
        clock=TestClock(),
        fs_protocol="file",
        include_types=[HyperliquidPublicTrade],
    )
    trade = HyperliquidPublicTrade(
        instrument_id=instrument.id,
        price=Price.from_str("1.23456"),
        size=Quantity.from_str("1000"),
        aggressor_side=AggressorSide.BUYER,
        trade_id="123456",
        buyer="0xbuyer",
        seller="0xseller",
        hash="0xhash",
        ts_event=1_000,
        ts_init=1_001,
    )

    # Act: this is the exact CustomData envelope a strategy receives from the live client.
    writer.write(
        CustomData(
            DataType(HyperliquidPublicTrade, {"instrument_id": instrument.id.value}),
            trade,
        ),
    )
    writer.close()
    catalog.convert_stream_to_data(instance_id, HyperliquidPublicTrade)

    # Assert: a Feather stream was created and its complete counterparties survive in Parquet.
    table_name = class_to_filename(HyperliquidPublicTrade)
    feather_files = catalog.fs.glob(
        f"{catalog.path}/backtest/{instance_id}/{table_name}/**/*.feather",
    )
    parquet_files = catalog.fs.glob(f"{catalog.path}/data/{table_name}/**/*.parquet")
    assert len(feather_files) == 1
    assert len(parquet_files) == 1

    with catalog.fs.open(parquet_files[0]) as f:
        table = pq.read_table(f)

    assert table.column("trade_id").to_pylist() == ["123456"]
    assert table.column("buyer").to_pylist() == ["0xbuyer"]
    assert table.column("seller").to_pylist() == ["0xseller"]
    assert table.column("hash").to_pylist() == ["0xhash"]

    [loaded] = catalog.custom_data(HyperliquidPublicTrade)
    assert isinstance(loaded, CustomData)
    assert isinstance(loaded.data, HyperliquidPublicTrade)
    assert loaded.data.instrument_id == instrument.id
    assert loaded.data.trade_id == trade.trade_id
    assert loaded.data.buyer == trade.buyer
    assert loaded.data.seller == trade.seller
    assert loaded.data.hash == trade.hash
