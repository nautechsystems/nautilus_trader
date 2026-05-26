#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter — Sandbox smoke test (WebSocket)
# -------------------------------------------------------------------------------------------------

"""
Manual smoke test for the LMEX WebSocket feed via the real adapter stack.

Usage
-----
    python tests/sandbox/sandbox_ws.py

Connects to ``wss://ws.lmex.io/ws/spot`` (live endpoint — sandbox WS DNS
returns NXDOMAIN), subscribes to the BTC-USD trade feed, prints 10 messages,
then disconnects.

Findings from live probing (2026-05-26):
  - Trade feed: ``tradeHistoryApi:BTC-USD`` ✓
  - Subscribe ack: ``{"event":"subscribe","channel":["tradeHistoryApi:BTC-USD"]}``
  - Heartbeat: server sends pings automatically; send ``{"op":"ping"}`` manually
  - Orderbook WS: ``orderBookApi:*`` returns empty channel — not publicly available
"""

from __future__ import annotations

import asyncio
import json
import os
import ssl

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
WS_URL = "wss://ws.lmex.io/ws/spot"
TEST_SYMBOL = "BTC-USD"
TRADE_TOPIC = f"tradeHistoryApi:{TEST_SYMBOL}"
MAX_MESSAGES = 10


async def run() -> None:
    try:
        import websockets  # type: ignore[import]
    except ImportError:
        print("websockets package required: pip install websockets")
        return

    ssl_ctx = ssl.create_default_context()
    print(f"Connecting to {WS_URL} ...")

    async with websockets.connect(WS_URL, ssl=ssl_ctx) as ws:
        print(f"Connected.  Subscribing to {TRADE_TOPIC!r} ...")

        await ws.send(json.dumps({"op": "subscribe", "args": [TRADE_TOPIC]}))

        received = 0
        trades_seen = 0

        while received < MAX_MESSAGES:
            try:
                raw = await asyncio.wait_for(ws.recv(), timeout=15)
            except asyncio.TimeoutError:
                print("  [timeout — no message in 15s]")
                break

            msg = json.loads(raw)
            received += 1

            event = msg.get("event")
            topic = msg.get("topic", "")

            if event == "subscribe":
                channel = msg.get("channel", [])
                print(f"  [ack] subscribed to {channel}")
                assert TRADE_TOPIC in channel, f"Expected {TRADE_TOPIC!r} in {channel}"

            elif topic.startswith("tradeHistoryApi:"):
                data = msg.get("data", [])
                trades_seen += len(data)
                for d in data:
                    print(
                        f"  [trade] {d['side']:4s}  price={d['price']:.1f}"
                        f"  size={d['size']}  tradeId={d['tradeId']}"
                        f"  ts={d['timestamp']}"
                    )
                # Validate schema fields
                for d in data:
                    assert isinstance(d["symbol"], str)
                    assert d["side"] in ("BUY", "SELL")
                    assert isinstance(d["price"], float)
                    assert isinstance(d["size"], float)
                    assert isinstance(d["tradeId"], int)
                    assert isinstance(d["timestamp"], int)
                    assert d["timestamp"] > 1_700_000_000_000

            elif event == "pong":
                print("  [pong] heartbeat acknowledged")

            else:
                print(f"  [other] {raw[:120]}")

        print(f"\nReceived {received} messages, {trades_seen} trade ticks")
        assert trades_seen > 0, "No trade ticks received — check WS connection"
        print("✓ WebSocket smoke test passed")


if __name__ == "__main__":
    asyncio.run(run())
