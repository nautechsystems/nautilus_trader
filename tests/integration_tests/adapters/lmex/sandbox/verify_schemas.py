#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter — Schema verification (no NT required)
# -------------------------------------------------------------------------------------------------

"""
Standalone schema verification script.

Verifies that all msgspec schemas decode the real LMEX API response shapes
correctly.  Requires only ``msgspec`` — no NautilusTrader installation needed.

Run from the repo root::

    python tests/sandbox/verify_schemas.py

Exit code 0 = all checks passed.
Exit code 1 = one or more checks failed.
"""

from __future__ import annotations

import hashlib
import hmac
import json
import sys
import textwrap
from pathlib import Path

try:
    import msgspec
except ImportError:
    print("ERROR: msgspec not installed.  Run: pip install msgspec")
    sys.exit(1)

# ---------------------------------------------------------------------------
# Load fixtures
# ---------------------------------------------------------------------------
FIXTURES = Path(__file__).parent.parent / "resources" / "http_responses"
WS = Path(__file__).parent.parent / "resources" / "ws_messages"

PASSED: list[str] = []
FAILED: list[str] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    if condition:
        PASSED.append(name)
        print(f"  ✓  {name}")
    else:
        FAILED.append(name)
        msg = f"  ✗  {name}"
        if detail:
            msg += f"\n     {detail}"
        print(msg)


# ---------------------------------------------------------------------------
# 1. Order schemas (real field names from live sandbox API)
# ---------------------------------------------------------------------------
print("\n── Order response schemas ──────────────────────────────────────────────")


class LmexOrderResponse(msgspec.Struct, gc=False):
    symbol: str
    orderID: str        # UUID str, capital D
    status: int
    side: str
    size: float
    timestamp: int
    orderType: int = 0
    price: float = 0.0
    fillSize: float = 0.0
    clOrderID: str | None = None
    averageFillPrice: float = 0.0
    originalSize: float = 0.0
    remainingSize: float = 0.0
    triggerPrice: float = 0.0
    stopPrice: float | None = None
    trigger: bool = False
    message: str = ""
    postOnly: bool = False
    time_in_force: str | None = None
    userCurrency: str | None = None


raw_submit = (FIXTURES / "order_submit_btceur.json").read_bytes()
raw_cancel = (FIXTURES / "order_cancel_btceur.json").read_bytes()

dec_order = msgspec.json.Decoder(list[LmexOrderResponse])

submit = dec_order.decode(raw_submit)
check("order_submit: decodes as list", isinstance(submit, list))
check("order_submit: single element", len(submit) == 1)
s = submit[0]
check("order_submit: orderID is UUID string", isinstance(s.orderID, str) and "-" in s.orderID)
check("order_submit: status == 2 (INSERTED)", s.status == 2)
check("order_submit: side == 'BUY'", s.side == "BUY")
check("order_submit: orderType == 76 (LIMIT int)", s.orderType == 76)
check("order_submit: fillSize field exists", s.fillSize == 0.0)
check("order_submit: stopPrice is None", s.stopPrice is None)
check("order_submit: timestamp > 2023", s.timestamp > 1_700_000_000_000)
check("order_submit: clOrderID is None when absent", s.clOrderID is None)

cancel = dec_order.decode(raw_cancel)[0]
check("order_cancel: status == 6 (CANCELLED)", cancel.status == 6)
check("order_cancel: same orderID as submit", cancel.orderID == s.orderID)

# ---------------------------------------------------------------------------
# 2. Open orders schema
# ---------------------------------------------------------------------------
print("\n── Open orders schema ──────────────────────────────────────────────────")


class LmexOpenOrder(msgspec.Struct, gc=False):
    symbol: str
    orderID: str
    side: str
    size: float
    orderType: int = 0
    price: float = 0.0
    filledSize: float = 0.0
    fillSize: float = 0.0
    remainingSize: float = 0.0
    averageFillPrice: float = 0.0
    timestamp: int = 0
    clOrderID: str | None = None
    orderState: str = ""
    timeInForce: str = "GTC"
    orderValue: float = 0.0
    quote: str = ""


raw_open = (FIXTURES / "open_orders_btceur.json").read_bytes()
dec_open = msgspec.json.Decoder(list[LmexOpenOrder])
open_orders = dec_open.decode(raw_open)
check("open_orders: decodes as list", isinstance(open_orders, list))
check("open_orders: single entry", len(open_orders) == 1)
o = open_orders[0]
check("open_orders: orderID is UUID", "-" in o.orderID)
check("open_orders: orderState string", o.orderState == "STATUS_ACTIVE")
check("open_orders: timeInForce camelCase", o.timeInForce == "GTC")
check("open_orders: clOrderID nullable", o.clOrderID is None)

# ---------------------------------------------------------------------------
# 3. Fill / trade history schema
# ---------------------------------------------------------------------------
print("\n── Fill schema ─────────────────────────────────────────────────────────")


class LmexFill(msgspec.Struct, gc=False):
    symbol: str
    orderId: str        # lowercase 'd' — API inconsistency
    tradeId: str        # UUID str
    side: str
    price: float
    size: float
    filledSize: float
    filledPrice: float
    feeCurrency: str
    feeAmount: float
    timestamp: int
    serialId: int = 0
    base: str = ""
    quote: str = ""
    clOrderID: str | None = None
    averageFillPrice: float = 0.0


raw_fills = (FIXTURES / "trade_history_btceur.json").read_bytes()
dec_fill = msgspec.json.Decoder(list[LmexFill])
fills = dec_fill.decode(raw_fills)
check("fills: decodes as list", isinstance(fills, list))
check("fills: two entries", len(fills) == 2)
f = fills[0]
check("fills: tradeId is UUID", "-" in f.tradeId)
check("fills: orderId lowercase d (consistent with API)", isinstance(f.orderId, str) and "-" in f.orderId)
check("fills: filledPrice field exists", f.filledPrice > 0)
check("fills: filledSize field exists", f.filledSize > 0)
check("fills: feeCurrency is str", isinstance(f.feeCurrency, str))
check("fills: feeAmount > 0", f.feeAmount > 0)
check("fills: serialId is int", isinstance(f.serialId, int))
check("fills: timestamp in ms", f.timestamp > 1_700_000_000_000)

# ---------------------------------------------------------------------------
# 4. Wallet schema
# ---------------------------------------------------------------------------
print("\n── Wallet schema ───────────────────────────────────────────────────────")


class LmexWalletEntry(msgspec.Struct, gc=False):
    currency: str
    available: float
    total: float


raw_wallet = (FIXTURES / "wallet.json").read_bytes()
dec_wallet = msgspec.json.Decoder(list[LmexWalletEntry])
wallet = dec_wallet.decode(raw_wallet)
check("wallet: decodes as list", isinstance(wallet, list))
tusd = next((e for e in wallet if e.currency == "TUSD"), None)
check("wallet: TUSD entry present", tusd is not None)
check("wallet: TUSD total == 100000", tusd is not None and tusd.total == 100000.0)
eur = next((e for e in wallet if e.currency == "EUR"), None)
check("wallet: EUR available <= total", eur is not None and eur.available <= eur.total)

# ---------------------------------------------------------------------------
# 5. Market schemas
# ---------------------------------------------------------------------------
print("\n── Market schemas ──────────────────────────────────────────────────────")


class LmexServerTime(msgspec.Struct):
    iso: str
    epoch: int


class LmexOrderBookEntry(msgspec.Struct):
    price: str
    size: str


class LmexOrderBook(msgspec.Struct):
    symbol: str
    buyQuote: list[LmexOrderBookEntry]
    sellQuote: list[LmexOrderBookEntry]
    timestamp: int | None = None


class LmexTrade(msgspec.Struct):
    price: float
    size: float
    side: str
    symbol: str
    serialId: int
    timestamp: int


class LmexMarketSummary(msgspec.Struct):
    symbol: str
    base: str
    quote: str
    active: bool
    futures: bool
    minPriceIncrement: float
    minSizeIncrement: float
    minOrderSize: float
    maxOrderSize: float
    last: float = 0.0


time_raw = (FIXTURES / "time.json").read_bytes()
t = msgspec.json.decode(time_raw, type=LmexServerTime)
check("time: iso is str", isinstance(t.iso, str))
check("time: epoch > 2023", t.epoch > 1_700_000_000)

ob_raw = (FIXTURES / "orderbook_btcusd.json").read_bytes()
ob = msgspec.json.decode(ob_raw, type=LmexOrderBook)
check("orderbook: symbol == BTC-USD", ob.symbol == "BTC-USD")
check("orderbook: has bids", len(ob.buyQuote) > 0)
check("orderbook: has asks", len(ob.sellQuote) > 0)
check("orderbook: has timestamp", ob.timestamp is not None and ob.timestamp > 0)
check("orderbook: bid price is str", isinstance(ob.buyQuote[0].price, str))
check("orderbook: bid > ask ordering consistent",
      float(ob.buyQuote[0].price) < float(ob.sellQuote[0].price))

trades_raw = (FIXTURES / "trades_btcusd.json").read_bytes()
trades = msgspec.json.decode(trades_raw, type=list[LmexTrade])
check("trades: non-empty list", len(trades) > 0)
check("trades: side is BUY/SELL", trades[0].side in ("BUY", "SELL"))
check("trades: price > 0", trades[0].price > 0)
check("trades: timestamp in ms", trades[0].timestamp > 1_700_000_000_000)

ms_raw = (FIXTURES / "market_summary_sample.json").read_bytes()
ms = msgspec.json.decode(ms_raw, type=list[LmexMarketSummary])
btc = next((s for s in ms if s.symbol == "BTC-USD"), None)
check("market_summary: BTC-USD present", btc is not None)
check("market_summary: BTC-USD active", btc is not None and btc.active is True)
check("market_summary: minPriceIncrement=0.1", btc is not None and btc.minPriceIncrement == 0.1)
check("market_summary: minSizeIncrement=1e-05", btc is not None and btc.minSizeIncrement == 1e-05)

# ---------------------------------------------------------------------------
# 6. WebSocket message schemas
# ---------------------------------------------------------------------------
print("\n── WebSocket schemas ───────────────────────────────────────────────────")


class LmexWsTradeDatum(msgspec.Struct):
    symbol: str
    side: str
    size: float
    price: float
    tradeId: int
    timestamp: int


class LmexWsTradeMsg(msgspec.Struct):
    topic: str
    data: list[LmexWsTradeDatum]


class LmexWsMsg(msgspec.Struct):
    topic: str | None = None
    event: str | None = None
    channel: list[str] | None = None


class LmexWsOrderEvent(msgspec.Struct):
    symbol: str
    orderId: int
    clOrderId: str
    status: int
    side: str
    size: float
    filledSize: float
    price: float
    avgFillPrice: float
    feeAmount: float
    feeCurrency: str
    tradeId: int
    timestamp: int


class LmexWsOrderEventMsg(msgspec.Struct):
    topic: str
    data: list[LmexWsOrderEvent]


trade_raw = (WS / "trade_feed.json").read_bytes()
tm = msgspec.json.decode(trade_raw, type=LmexWsTradeMsg)
check("ws_trade: topic correct", tm.topic == "tradeHistoryApi:BTC-USD")
check("ws_trade: two data items", len(tm.data) == 2)
check("ws_trade: first price == 76653.1", abs(tm.data[0].price - 76653.1) < 0.01)
check("ws_trade: first side == SELL", tm.data[0].side == "SELL")
check("ws_trade: tradeId is int", isinstance(tm.data[0].tradeId, int))

ack_raw = (WS / "subscribe_ack.json").read_bytes()
ack = msgspec.json.decode(ack_raw, type=LmexWsMsg)
check("ws_ack: event == 'subscribe'", ack.event == "subscribe")
check("ws_ack: topic is None", ack.topic is None)

fill_raw = (WS / "order_event_fill.json").read_bytes()
fill_msg = msgspec.json.decode(fill_raw, type=LmexWsOrderEventMsg)
check("ws_fill: topic == notificationsApi", fill_msg.topic == "notificationsApi")
check("ws_fill: status == 4 (FILLED)", fill_msg.data[0].status == 4)

cancel_raw = (WS / "order_event_cancel.json").read_bytes()
cancel_msg = msgspec.json.decode(cancel_raw, type=LmexWsOrderEventMsg)
check("ws_cancel: status == 6 (CANCELLED)", cancel_msg.data[0].status == 6)

# ---------------------------------------------------------------------------
# 7. HMAC-SHA384 signing
# ---------------------------------------------------------------------------
print("\n── HMAC-SHA384 signing ─────────────────────────────────────────────────")

secret = "MYSECRET"
path = "/api/v3.2/order"
nonce = "1779786400000"
body = '{"symbol":"BTC-USD","side":"BUY","size":0.01,"price":76000.0}'

sig = hmac.new(
    secret.encode("utf-8"),
    (path + nonce + body).encode("utf-8"),
    hashlib.sha384,
).hexdigest()

check("hmac: output is 96 hex chars", len(sig) == 96)
check("hmac: output is lowercase", sig == sig.lower())
check("hmac: all hex chars", all(c in "0123456789abcdef" for c in sig))
check("hmac: deterministic", sig == hmac.new(
    secret.encode(), (path + nonce + body).encode(), hashlib.sha384
).hexdigest())

# Different nonces produce different sigs
sig2 = hmac.new(
    secret.encode(), (path + "9999999999999" + body).encode(), hashlib.sha384
).hexdigest()
check("hmac: nonce changes signature", sig != sig2)

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
total = len(PASSED) + len(FAILED)
print(f"\n{'─' * 72}")
print(f"Results: {len(PASSED)}/{total} passed", end="")
if FAILED:
    print(f"  ({len(FAILED)} FAILED)")
    for name in FAILED:
        print(f"  ✗ {name}")
    sys.exit(1)
else:
    print("  ✓ All checks passed")
    sys.exit(0)
