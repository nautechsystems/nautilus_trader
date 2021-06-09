from datetime import datetime


trade_tick_schema = {
    "instrument_id": str,
    "price": float,
    "size": float,
    "aggressor_side": str,
    "match_id": str,
    "ts_event_ns": datetime,
    "ts_recv_ns": datetime,
}


betting_instrument_schema = {
    "venue": str,
    "currency": str,
    "instrument_id": str,
    "event_type_id": str,
    "event_type_name": str,
    "competition_id": str,
    "competition_name": str,
    "event_id": str,
    "event_name": str,
    "event_country_code": str,
    "event_open_date": datetime,
    "betting_type": str,
    "market_id": str,
    "market_name": str,
    "market_start_time": datetime,
    "market_type": str,
    "selection_id": str,
    "selection_name": str,
    "selection_handicap": str,
    "timestamp_ns": int,
    "ts_recv_ns": int,
}

order_book_schema = {
    "instrument_id": str,
    "ts_event_ns": datetime,
    "ts_recv_ns": datetime,
    "side": str,
    "price": float,
    "volume": float,
    "level": int,
}

order_book_updates_schema = {
    "instrument_id": str,
    "ts_event_ns": datetime,
    "ts_recv_ns": datetime,
    "type": str,
    "side": str,
    "price": float,
    "volume": float,
}
