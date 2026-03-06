def get_polymarket_trades_key(taker_order_id: str, trade_id: str) -> str:
    """
    Return the cache key for a Polymarket orders trades.

    Parameters
    ----------
    taker_order_id : str
        The aggressor Polymarket order ID for the trades.
    trade_id : str
        The trade ID.

    Returns
    -------
    str

    """
    return f"polymarket:trades:{taker_order_id}:{trade_id}"
