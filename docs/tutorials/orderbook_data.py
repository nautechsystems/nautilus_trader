from __future__ import annotations

import json
from collections.abc import Iterable
from collections.abc import Iterator
from decimal import Decimal
from os import PathLike
from zipfile import ZipFile, is_zipfile

import pandas as pd
from nautilus_trader.model import (
    BookAction,
    BookOrder,
    CryptoPerpetual,
    CurrencyPair,
    OrderBookDelta,
    OrderSide,
    Price,
    Quantity,
    RecordFlag,
)


def deltas_from_frame(
    frame: pd.DataFrame,
    instrument: CurrencyPair | CryptoPerpetual,
) -> list[OrderBookDelta]:
    """
    Convert loader rows and preserve snapshot and event boundaries.
    """
    if frame.empty:
        return []

    instrument_id = str(instrument.id)
    if not frame["instrument_id"].eq(instrument_id).all():
        raise ValueError(f"Expected only {instrument_id} order book data")

    rows = list(frame.itertuples())
    deltas: list[OrderBookDelta] = []
    first = rows[0]
    first_action = BookAction.from_str(first.action)
    if int(first.flags) & RecordFlag.F_SNAPSHOT.value and first_action != BookAction.CLEAR:
        ts = int(first.Index.value)
        deltas.append(
            OrderBookDelta(
                instrument_id=instrument.id,
                action=BookAction.CLEAR,
                order=BookOrder(
                    side=OrderSide.from_str(first.side),
                    price=Price.from_decimal_dp(
                        Decimal(str(first.price)),
                        instrument.price_precision,
                    ),
                    size=Quantity.zero(instrument.size_precision),
                    order_id=0,
                ),
                flags=RecordFlag.F_SNAPSHOT.value,
                sequence=int(first.sequence),
                ts_event=ts,
                ts_init=ts,
            ),
        )

    for index, row in enumerate(rows):
        ts = int(row.Index.value)
        next_row = rows[index + 1] if index + 1 < len(rows) else None
        flags = int(row.flags)
        next_starts_snapshot = next_row is not None and next_row.action == "CLEAR"
        snapshot_continues = (
            next_row is not None
            and not next_starts_snapshot
            and flags & RecordFlag.F_SNAPSHOT.value
            and int(next_row.flags) & RecordFlag.F_SNAPSHOT.value
        )
        event_continues = (
            next_row is not None
            and not next_starts_snapshot
            and next_row.Index == row.Index
            and next_row.sequence == row.sequence
        )

        if not snapshot_continues and not event_continues:
            flags |= RecordFlag.F_LAST.value
        deltas.append(
            OrderBookDelta(
                instrument_id=instrument.id,
                action=BookAction.from_str(row.action),
                order=BookOrder(
                    side=OrderSide.from_str(row.side),
                    price=Price.from_decimal_dp(
                        Decimal(str(row.price)),
                        instrument.price_precision,
                    ),
                    size=Quantity.from_decimal_dp(
                        Decimal(str(row.size)),
                        instrument.size_precision,
                    ),
                    order_id=int(row.order_id),
                ),
                flags=flags,
                sequence=int(row.sequence),
                ts_event=ts,
                ts_init=ts,
            ),
        )
    return deltas


def load_bybit_order_book_deltas(
    file_path: str | PathLike[str],
    nrows: int | None = None,
) -> pd.DataFrame:
    if not is_zipfile(file_path):
        raise ValueError("Bybit order book data must be a ZIP archive")

    rows = []
    with ZipFile(file_path) as archive, archive.open(archive.namelist()[0]) as file:
        for event in _bybit_events(file):
            if nrows is not None and len(rows) + len(event) > nrows:
                break
            rows.extend(event)

    columns = [
        "timestamp",
        "instrument_id",
        "action",
        "side",
        "price",
        "size",
        "order_id",
        "flags",
        "sequence",
    ]
    frame = pd.DataFrame(rows, columns=columns).set_index("timestamp")
    return frame.astype({"order_id": int, "flags": int, "sequence": int})


def _bybit_events(lines: Iterable[bytes]) -> Iterator[list[dict[str, object]]]:
    for line in lines:
        message = json.loads(line)
        data = message["data"]
        timestamp = pd.to_datetime(int(message["ts"]) * 1_000_000, unit="ns", utc=True)
        snapshot = message["type"] == "snapshot"
        sides = [(side, data.get(key) or []) for key, side in (("b", "BUY"), ("a", "SELL"))]
        event = []

        if snapshot:
            side, levels = next(((side, levels) for side, levels in sides if levels), ("BUY", []))
            event.append(
                {
                    "timestamp": timestamp,
                    "instrument_id": f"{data['s']}-LINEAR.BYBIT",
                    "action": "CLEAR",
                    "side": side,
                    "price": levels[0][0] if levels else "0",
                    "size": "0",
                    "order_id": 0,
                    "flags": RecordFlag.F_SNAPSHOT.value,
                    "sequence": data["seq"],
                },
            )

        for side, levels in sides:
            for price, size in levels:
                if snapshot:
                    action = "ADD"
                elif Decimal(size) == 0:
                    action = "DELETE"
                else:
                    action = "UPDATE"

                event.append(
                    {
                        "timestamp": timestamp,
                        "instrument_id": f"{data['s']}-LINEAR.BYBIT",
                        "action": action,
                        "side": side,
                        "price": price,
                        "size": size,
                        "order_id": 0,
                        "flags": RecordFlag.F_SNAPSHOT.value if snapshot else 0,
                        "sequence": data["seq"],
                    },
                )

        yield event
