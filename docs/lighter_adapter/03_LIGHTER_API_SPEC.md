# 03_LIGHTER_API_SPEC.md

## API Spec (working notes)

This markdown summarizes the actionable interface checklist and flags uncertainties to resolve
during the validation spike.

### Latest captures (mainnet)

- WS connects at `wss://mainnet.zklighter.elliot.ai/stream`
- Channels observed: `order_book:1` (snapshot on subscribe + deltas with `offset` and `nonce`),
  `trade:1`, `market_stats:1`
- Private WS requires auth token from signer (`create_auth_token_with_expiry`) plus API key private
  key; wallet key not needed for order flow
- Mainnet auth/sendTx fixtures captured (SDK-free, signer binary + HTTP) under
  `tests/test_data/lighter/http/`:
  - `mainnet_sendtx_create_{btc,eth}.json`, `mainnet_sendtx_cancel_{btc,eth}.json`
  - `mainnet_next_nonce.json`, `mainnet_account_index_659514.json`
  - `mainnet_account_active_orders_market{0,1}.json`
  - `mainnet_orderbook_details_btc.json`
- Fixtures stored under `tests/test_data/lighter/` (public + private samples)

### REST Endpoints (to validate)

| Endpoint | Scope | Auth | Notes/TBD |
|----------|-------|------|-----------|
| `GET /api/v1/orderBooks` | Public | None | Primary source for instrument metadata (price/size decimals, mins) |
| `GET /api/v1/orderBookDetails` | Public | None | Some docs reference this path; confirm canonical endpoint name |
| `GET /api/v1/candlesticks` | Public | None | Historical bars; optional for v1 |
| `GET /api/v1/marketStats` | Public | None | Mark/index/funding reference data; confirm payload schema |
| `GET /api/v1/account` | Private | Token? | Current assumption: token required; earlier notes marked “no auth” — must test |
| `GET /api/v1/accountActiveOrders` | Private | Token? | Use for reconciliation; confirm filters + pagination |
| `POST /api/v1/sendTx` | Private | Signature + Token? | Signing algorithm/payload hashing **TBD** |
| `POST /api/v1/sendTxBatch` | Private | Signature + Token? | Batch limits + error behavior **TBD** |
| `GET /api/v1/nextNonce` (or equivalent) | Private | Token? | Confirm path/name and behavior after failed tx |

### WebSocket Channels

> **IMPORTANT: Channel Delimiter Convention (Verified)**
>
> The Lighter API uses **different delimiters** for requests vs responses:
>
> - **Subscribe requests** use **slashes**: `order_book/{market_index}`
> - **Server responses** use **colons**: `order_book:1`
>
> This is confirmed in the [Lighter WebSocket Reference](https://apidocs.lighter.xyz/docs/websocket-reference).
> The test fixtures (`tests/test_data/lighter/ws/`) contain server responses (colon format),
> which may mislead reviewers into thinking subscriptions should also use colons. They should not.

| Channel | Scope | Notes |
|---------|-------|-------|
| `order_book/{market_index}` | Public | Snapshot on subscribe + deltas with `offset` and `nonce` |
| `trade/{market_index}` | Public | Trade tick feed |
| `market_stats/{market_index}` | Public | Mark/index/funding updates |
| `account_all_orders` (or similar) | Private | Order lifecycle events; payload schema + auth requirement TBD |
| `account_positions` (or similar) | Private | Position/funding events; confirm channel name and fields |

### Capture & Fixture Checklist

- Record HTTP and WS payloads for each endpoint/channel above (testnet), redact secrets, and store
  under `tests/test_data/lighter/{http,ws}/`.
- Verify channel naming and message envelopes (e.g., `{"type":"subscribe"}` vs alternative schema).
- Confirm whether WS sends initial snapshots or requires REST bootstrap only.
- Document the exact signing recipe for `sendTx`/`sendTxBatch`, including nonce rules and hash/curve.
