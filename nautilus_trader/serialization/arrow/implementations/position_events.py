import pyarrow as pa

from nautilus_trader.model.events import PositionChanged
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionEvent
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.objects import Money


def try_float(x):
    if x == "None" or x is None:
        return None
    return float(x)


def serialize(event: PositionEvent):
    data = {k: v for k, v in event.to_dict(event).items() if k not in ("order_fill",)}
    caster = {
        "signed_qty": float,
        "quantity": float,
        "peak_qty": float,
        "avg_px_open": float,
        "last_qty": float,
        "last_px": float,
        "avg_px_close": try_float,
        "realized_return": try_float,
    }
    values = {k: caster[k](v) if k in caster else v for k, v in data.items()}  # type: ignore
    if "realized_pnl" in values:
        realized = Money.from_str(values["realized_pnl"])
        values["realized_pnl"] = realized.as_double()
    if "unrealized_pnl" in values:
        unrealized = Money.from_str(values["unrealized_pnl"])
        values["unrealized_pnl"] = unrealized.as_double()
    return pa.RecordBatch.from_pylist([values], schema=SCHEMAS[type(event)])


def deserialize(cls):
    def inner(batch: pa.RecordBatch) -> PositionOpened | (PositionChanged | PositionClosed):
        def parse(data):
            for k in ("quantity", "last_qty", "peak_qty", "last_px"):
                if k in data:
                    data[k] = str(data[k])
            if "realized_pnl" in data:
                data["realized_pnl"] = f"{data['realized_pnl']} {data['currency']}"
            if "unrealized_pnl" in data:
                data["unrealized_pnl"] = f"{data['unrealized_pnl']} {data['currency']}"
            return data

        return [cls.from_dict(parse(d)) for d in batch.to_pylist()]

    return inner


SCHEMAS: dict[PositionEvent, pa.Schema] = {
    PositionOpened: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "position_id": pa.string(),
            "opening_order_id": pa.string(),
            "entry": pa.string(),
            "side": pa.string(),
            "signed_qty": pa.float64(),
            "quantity": pa.float64(),
            "peak_qty": pa.float64(),
            "last_qty": pa.float64(),
            "last_px": pa.float64(),
            "currency": pa.string(),
            "avg_px_open": pa.float64(),
            "realized_pnl": pa.float64(),
            "event_id": pa.string(),
            "duration_ns": pa.uint64(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    PositionChanged: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "position_id": pa.string(),
            "opening_order_id": pa.string(),
            "entry": pa.string(),
            "side": pa.string(),
            "signed_qty": pa.float64(),
            "quantity": pa.float64(),
            "peak_qty": pa.float64(),
            "last_qty": pa.float64(),
            "last_px": pa.float64(),
            "currency": pa.string(),
            "avg_px_open": pa.float64(),
            "avg_px_close": pa.float64(),
            "realized_return": pa.float64(),
            "realized_pnl": pa.float64(),
            "unrealized_pnl": pa.float64(),
            "event_id": pa.string(),
            "ts_opened": pa.uint64(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
    PositionClosed: pa.schema(
        {
            "trader_id": pa.dictionary(pa.int16(), pa.string()),
            "account_id": pa.dictionary(pa.int16(), pa.string()),
            "strategy_id": pa.dictionary(pa.int16(), pa.string()),
            "instrument_id": pa.dictionary(pa.int64(), pa.string()),
            "position_id": pa.string(),
            "opening_order_id": pa.string(),
            "closing_order_id": pa.string(),
            "entry": pa.string(),
            "side": pa.string(),
            "signed_qty": pa.float64(),
            "quantity": pa.float64(),
            "peak_qty": pa.float64(),
            "last_qty": pa.float64(),
            "last_px": pa.float64(),
            "currency": pa.string(),
            "avg_px_open": pa.float64(),
            "avg_px_close": pa.float64(),
            "realized_return": pa.float64(),
            "realized_pnl": pa.float64(),
            "event_id": pa.string(),
            "ts_opened": pa.uint64(),
            "ts_closed": pa.uint64(),
            "duration_ns": pa.uint64(),
            "ts_init": pa.uint64(),
        },
    ),
}
