# Validation Spike Notes (Public WS)

## Findings (Mainnet, 2025-12-08)
- **WS endpoint:** `wss://mainnet.zklighter.elliot.ai/stream`
- **Channel format (public):** slash-delimited
  - Order books: `{"type":"subscribe","channel":"order_book/{market_id}"}`
  - Trades: `{"type":"subscribe","channel":"trade/{market_id}"}`
  - Market stats (mark/index/funding): `{"type":"subscribe","channel":"market_stats/{market_id}"}`
- **Sample capture:** Stored at `tests/test_data/lighter/ws/public_mainnet.json` (order_book/trade/market_stats for market_id=1).
- **REST orderBooks:** Captured mainnet response at `tests/test_data/lighter/http/orderbooks_mainnet.json` (119 markets; `market_id` populated, includes fee fields and `status`).

## Actions taken
- Updated WS client to use slash channels (backward-compatible parsing still supports colon).
- Added parsing test to confirm slash/colon channel parsing.
- Added new fixtures under `tests/test_data/lighter/{http,ws}/`.

## Still open (private/auth)
- Private WS/REST (`sendTx`, `account`, `accountActiveOrders`, `nextNonce`) not validated here.
- Need the signing/auth recipe for `LIGHTER_API_KEY`/`LIGHTER_API_SECRET` to derive auth tokens/headers.
- Once provided, capture private REST/WS payloads and update fixtures/parsers accordingly.

## How to reproduce public WS capture
```bash
python - <<'PY'
import asyncio, json, websockets
channels = ["order_book/1","trade/1","market_stats/1"]
async def main():
    msgs=[]
    async with websockets.connect("wss://mainnet.zklighter.elliot.ai/stream") as ws:
        for ch in channels:
            await ws.send(json.dumps({"type":"subscribe","channel":ch}))
        for _ in range(20):
            try:
                msg = await asyncio.wait_for(ws.recv(), timeout=2)
                msgs.append(json.loads(msg))
            except Exception:
                break
    print(len(msgs))
    print(msgs[:2])
asyncio.run(main())
PY
```
