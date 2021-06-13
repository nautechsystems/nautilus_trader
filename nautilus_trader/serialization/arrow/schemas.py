import pyarrow as pa

from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.tick import TradeTick


TYPE_TO_SCHEMA = {}


TYPE_TO_SCHEMA[TradeTick] = pa.schema(
    {
        "instrument_id": pa.string(),
        "price": pa.string(),
        "size": pa.string(),
        "aggressor_side": pa.string(),
        "match_id": pa.string(),
        "ts_event_ns": pa.uint64(),
        "ts_recv_ns": pa.uint64(),
    },
    metadata={"type": "TradeTick"},
)


TYPE_TO_SCHEMA[BettingInstrument] = pa.schema(
    {
        "venue": pa.string(),
        "currency": pa.string(),
        "instrument_id": pa.string(),
        "event_type_id": pa.string(),
        "event_type_name": pa.string(),
        "competition_id": pa.string(),
        "competition_name": pa.string(),
        "event_id": pa.string(),
        "event_name": pa.string(),
        "event_country_code": pa.string(),
        "event_open_date": pa.date64(),
        "betting_type": pa.string(),
        "market_id": pa.string(),
        "market_name": pa.string(),
        "market_start_time": pa.date64(),
        "market_type": pa.string(),
        "selection_id": pa.string(),
        "selection_name": pa.string(),
        "selection_handicap": pa.string(),
        "ts_recv_ns": pa.uint64(),
        "ts_event_ns": pa.uint64(),
    },
    metadata={"type": "BettingInstrument"},
)


TYPE_TO_SCHEMA[OrderBook] = pa.schema(
    {
        "instrument_id": pa.string(),
        "ts_event_ns": pa.uint64(),
        "ts_recv_ns": pa.uint64(),
        "side": pa.string(),
        "price": pa.float64(),
        "volume": pa.float64(),
        "level": pa.uint8(),
    },
    metadata={"type": "OrderBook"},
)


TYPE_TO_SCHEMA[OrderBookDelta] = pa.schema(
    {
        "instrument_id": pa.string(),
        "ts_event_ns": pa.uint64(),
        "ts_recv_ns": pa.uint64(),
        "type": pa.string(),
        "side": pa.string(),
        "price": pa.float64(),
        "volume": pa.float64(),
    },
    metadata={"type": "OrderBookDelta"},
)


SCHEMA_TO_TYPE = {v.metadata[b"type"]: k for k, v in TYPE_TO_SCHEMA.items()}


def register_schema(cls: type, schema: pa.Schema):
    global TYPE_TO_SCHEMA
    assert isinstance(cls, type)
    assert isinstance(schema, pa.Schema)
    if not schema.metadata:
        schema.add_metadata({"type": cls.__name__})
    TYPE_TO_SCHEMA[cls] = schema
