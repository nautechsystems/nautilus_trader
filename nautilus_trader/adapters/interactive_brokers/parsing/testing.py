from ib_insync import TickData


def tick_data_to_json(tick_data: TickData):
    return {
        "time": tick_data.time.isoformat(),
        "price": tick_data.price,
        "size": tick_data.size,
        "tickType": tick_data.tickType,
    }
