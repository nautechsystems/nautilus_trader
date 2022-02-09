import hashlib

import orjson
import pandas as pd


def _match_id(ts_init: pd.Timestamp, price: str, size: str, symbol: str) -> str:
    hash_values = (symbol, ts_init, price, size)
    h = hashlib.sha256(orjson.dumps(hash_values))
    return h.hexdigest()


# def parse_quote_ticks(quotes: List, details: Dict) -> List[QuoteTick]:
#     if not quotes:
#         return []
#     instrument = ib_data_to_instrument(data=details)
#     wrangler = QuoteTickDataWrangler(instrument=instrument)
#     return wrangler.process(quotes)
#
#
# def parse_trade_ticks(trades: List, inputs: Dict) -> List[TradeTick]:
#     def make_trade_id(r: pd.Series) -> str:
#         return _match_id(
#             ts_init=r.name.isoformat(),
#             price=str(r["price"]),
#             size=str(r["size"]),
#             symbol=r["symbol"],
#         )
#
#     if not trades:
#         return []
#
#     instrument = ib_data_to_instrument(data=inputs)
#     trades.loc[:, "trade_id"] = trades.apply(make_trade_id, axis=1).values
#     wrangler = TradeTickDataWrangler(instrument=instrument)
#     return wrangler.process(trades.rename({"size": "quantity"}, axis=1).assign(side="UNKNOWN"))
